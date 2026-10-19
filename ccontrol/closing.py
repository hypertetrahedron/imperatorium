"""End a local session the way a person at its terminal would.

Killing `claude` ends the session but leaves its Windows Terminal tab open on
"[process exited with code 1]": the terminal closes a tab by itself only
when what runs in it exits cleanly. So the session is asked to leave first,
by typing into its console - Ctrl+C (clears a draft, or interrupts a turn),
then `/exit` and Enter - and only killed if it is still there afterwards.

A session started by hand from a shell would leave that shell behind in its
tab, so when the shell sits directly under the terminal it is sent `exit`
too, once `claude` has gone.

Typing means attaching to the session's console, and a process can only be
attached to one: the dispatcher may have a console of its own (`impctl run`),
so the typing is done by a short-lived child (`python -m ccontrol.closing`).
"""
import os
import signal
import subprocess
import sys
import time

from . import procs

WINDOWS = sys.platform == "win32"

# What hosts a tab whose shell may be closed along with the session. Never a
# shell under another program (an IDE's terminal, a script), which belongs
# to someone else.
TERMINALS = {"windowsterminal"}
SHELLS = {"pwsh", "powershell", "cmd", "bash", "sh", "zsh", "fish"}

CTRL_C = "\x03"
ENTER = "\r"
# A control character does not survive a command line, so the child is
# handed these names instead.
KEYS = {CTRL_C: "<ctrl-c>", ENTER: "<enter>"}


def _alive(pid, table=None):
    table = procs.process_table() if table is None else table
    return any(p == pid for p, _pp, _e, _a in table)


def _wait_gone(pid, wait, alive):
    end = time.time() + wait
    while time.time() < end:
        if not alive(pid):
            return True
        time.sleep(0.25)
    return not alive(pid)


def _type(pid, *chunks):
    """Type `chunks` into `pid`'s console, pausing between them. True on success."""
    try:
        r = subprocess.run([sys.executable, "-m", "ccontrol.closing", str(pid)]
                           + [KEYS.get(c, c) for c in chunks],
                           cwd=os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                           capture_output=True, timeout=15,
                           creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    except (OSError, subprocess.SubprocessError):
        return False
    return r.returncode == 0


def _kill(pid):
    if WINDOWS:
        subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"],
                       capture_output=True, timeout=15,
                       creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    else:
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass


def tab_shell(pid, table=None):
    """The shell `pid` was started from, when that shell is a terminal tab's own."""
    table = procs.process_table() if table is None else table
    by_pid = {p: (pp, e) for p, pp, e, _a in table}
    parent = by_pid.get(pid, (None, None))[0]
    if parent not in by_pid or by_pid[parent][1] not in SHELLS:
        return None
    host = by_pid.get(by_pid[parent][0], (None, None))[1]
    return parent if host in TERMINALS else None


def close(pid, wait=8.0, typer=None, killer=None, alive=None, table=None):
    """End the session whose `claude` process is `pid`.

    Returns {ok, how: "exit" | "killed", tab_closed}. `how` says whether it
    left by itself (its tab closes) or had to be killed (its tab stays, on
    the exit message). The other arguments replace the real calls in tests.
    """
    typer, killer = typer or _type, killer or _kill
    alive = alive or _alive
    try:
        pid = int(pid)
    except (TypeError, ValueError):
        return {"ok": False, "error": "no process to close"}
    if not alive(pid):
        return {"ok": True, "how": "gone", "tab_closed": False}

    if WINDOWS:
        shell = tab_shell(pid, table)
        typer(pid, CTRL_C, "/exit", ENTER)
        if _wait_gone(pid, wait, alive):
            closed = True
            if shell and alive(shell):
                closed = typer(shell, "exit", ENTER) and _wait_gone(shell, 3.0, alive)
            return {"ok": True, "how": "exit", "tab_closed": closed}
    else:
        try:
            os.kill(pid, signal.SIGTERM)
        except OSError:
            pass
        if _wait_gone(pid, wait, alive):
            return {"ok": True, "how": "exit", "tab_closed": False}

    killer(pid)
    if _wait_gone(pid, 3.0, alive):
        return {"ok": True, "how": "killed", "tab_closed": False}
    return {"ok": False, "error": "it would not exit (pid %d)" % pid}


def _type_into(pid, chunks):
    """Runs in the child: attach to `pid`'s console and type `chunks`."""
    import ctypes
    from ctypes import wintypes

    class KEY(ctypes.Structure):
        _fields_ = [("bKeyDown", wintypes.BOOL), ("wRepeatCount", wintypes.WORD),
                    ("wVirtualKeyCode", wintypes.WORD),
                    ("wVirtualScanCode", wintypes.WORD),
                    ("uChar", wintypes.WCHAR), ("dwControlKeyState", wintypes.DWORD)]

    class RECORD(ctypes.Structure):
        class _U(ctypes.Union):
            _fields_ = [("KeyEvent", KEY), ("_pad", ctypes.c_byte * 16)]
        _fields_ = [("EventType", wintypes.WORD), ("Event", _U)]

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateFileW.restype = wintypes.HANDLE
    k32.FreeConsole()
    if not k32.AttachConsole(pid):
        return 2
    try:
        conin = k32.CreateFileW("CONIN$", 0xC0000000, 3, None, 3, 0, None)
        if not conin or conin == wintypes.HANDLE(-1).value:
            return 3
        try:
            for i, chunk in enumerate(chunks):
                if i:
                    time.sleep(0.4)
                records = []
                # One key event per UTF-16 unit: a character outside the
                # BMP (an emoji) is typed as its surrogate pair.
                data = chunk.encode("utf-16-le", "surrogatepass")
                for ch in (chr(int.from_bytes(data[j:j + 2], "little"))
                           for j in range(0, len(data), 2)):
                    vk, state = 0, 0
                    if ch == ENTER:
                        vk = 0x0D
                    elif ch == CTRL_C:
                        vk, state = 0x43, 0x0008  # 'C', LEFT_CTRL_PRESSED
                    for down in (1, 0):
                        r = RECORD()
                        r.EventType = 1  # KEY_EVENT
                        r.Event.KeyEvent = KEY(down, 1, vk, 0, ch, state)
                        records.append(r)
                n = wintypes.DWORD()
                arr = (RECORD * len(records))(*records)
                if not k32.WriteConsoleInputW(conin, arr, len(records),
                                              ctypes.byref(n)):
                    return 4
        finally:
            k32.CloseHandle(conin)
    finally:
        k32.FreeConsole()
    return 0


if __name__ == "__main__":
    names = {v: k for k, v in KEYS.items()}
    sys.exit(_type_into(int(sys.argv[1]), [names.get(a, a) for a in sys.argv[2:]]))
