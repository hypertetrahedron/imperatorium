"""Is an idle session still waiting on something it started?

A session that hands a command to the background - a build, a test run, a
`Monitor` - goes `idle` the moment its turn ends, and `claude agents --json`
says so. But it has not finished: it will wake when the command exits. The
board showed that as idle, which reads as "done, give it more work".

Claude Code runs every Bash and PowerShell tool call as a shell process under
its own `claude` process, and an idle session has no shells under it unless
one of them is still running. So the count of shells beneath a session's pid
is the count of things it is waiting on. Hooks and the statusline also run
through a shell, but briefly; `MIN_AGE` keeps them from making a tile flicker.

A background subagent runs inside the `claude` process and is invisible here.
"""
import os
import subprocess
import sys
import time

WINDOWS = sys.platform == "win32"

SHELLS = {"bash", "sh", "zsh", "dash", "fish", "pwsh", "powershell"}

# Long enough that a hook or statusline render (both bounded by Claude Code's
# own timeouts, and in practice well under a second) never counts.
MIN_AGE = 5.0


def _exe(name):
    base = os.path.basename(name or "").lower()
    return base[:-4] if base.endswith(".exe") else base


def process_table():
    """[(pid, ppid, exe name, started_at or None)] for every process."""
    try:
        return _table_windows() if WINDOWS else _table_posix()
    except Exception:
        return []


def _table_posix():
    out = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,etimes=,comm="],
                         capture_output=True, text=True, timeout=10).stdout
    now, rows = time.time(), []
    for line in out.splitlines():
        parts = line.split(None, 3)
        if len(parts) == 4 and parts[0].isdigit() and parts[1].isdigit():
            age = float(parts[2]) if parts[2].isdigit() else None
            rows.append((int(parts[0]), int(parts[1]), _exe(parts[3]),
                         now - age if age is not None else None))
    return rows


def _table_windows():
    import ctypes
    from ctypes import wintypes

    class ENTRY(ctypes.Structure):
        _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD),
                    ("th32ProcessID", wintypes.DWORD),
                    ("th32DefaultHeapID", ctypes.c_size_t),
                    ("th32ModuleID", wintypes.DWORD), ("cntThreads", wintypes.DWORD),
                    ("th32ParentProcessID", wintypes.DWORD),
                    ("pcPriClassBase", ctypes.c_long), ("dwFlags", wintypes.DWORD),
                    ("szExeFile", ctypes.c_wchar * 260)]

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    k32.OpenProcess.restype = wintypes.HANDLE
    snap = k32.CreateToolhelp32Snapshot(0x2, 0)  # TH32CS_SNAPPROCESS
    if not snap or snap == wintypes.HANDLE(-1).value:
        return []
    rows = []
    try:
        e = ENTRY()
        e.dwSize = ctypes.sizeof(ENTRY)
        ok = k32.Process32FirstW(snap, ctypes.byref(e))
        while ok:
            rows.append((e.th32ProcessID, e.th32ParentProcessID,
                         _exe(e.szExeFile), None))
            ok = k32.Process32NextW(snap, ctypes.byref(e))
    finally:
        k32.CloseHandle(snap)
    return rows


def _started_windows(pid):
    """Creation time of `pid` as a Unix timestamp, or None."""
    import ctypes
    from ctypes import wintypes
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.OpenProcess.restype = wintypes.HANDLE
    h = k32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    if not h:
        return None
    try:
        times = [wintypes.FILETIME() for _ in range(4)]
        if not k32.GetProcessTimes(h, *[ctypes.byref(t) for t in times]):
            return None
        ft = (times[0].dwHighDateTime << 32) | times[0].dwLowDateTime
        return ft / 1e7 - 11644473600.0
    finally:
        k32.CloseHandle(h)


def waiting_shells(pids, table=None, now=None, started=None):
    """{pid: number of shells beneath it that have outlived MIN_AGE}.

    A shell inside a shell (Git Bash re-execs itself) is one task, so only the
    topmost shell on each branch is counted.
    """
    table = process_table() if table is None else table
    now = time.time() if now is None else now
    started = started or (_started_windows if WINDOWS else (lambda pid: None))
    children = {}
    for pid, ppid, exe, at in table:
        children.setdefault(ppid, []).append((pid, exe, at))
    out = {}
    for root in pids:
        count, stack, seen = 0, list(children.get(root, ())), {root}
        while stack:
            pid, exe, at = stack.pop()
            if pid in seen:
                continue
            seen.add(pid)
            if exe in SHELLS:
                at = at if at is not None else started(pid)
                if at is None or now - at >= MIN_AGE:
                    count += 1
                continue
            stack.extend(children.get(pid, ()))
        if count:
            out[root] = count
    return out


def annotate(rows):
    """Set `shell_processes` on every live local row (for its tile), and
    `background_tasks` on idle ones still waiting on a shell. One process
    table read for all of them."""
    live = {r["pid"]: r for r in rows
            if r.get("status") and isinstance(r.get("pid"), int)}
    if not live:
        return rows
    counts = waiting_shells(live)
    for pid, row in live.items():
        n = counts.get(pid, 0)
        row["shell_processes"] = n
        if n and row.get("status") == "idle":
            row["background_tasks"] = n
    return rows
