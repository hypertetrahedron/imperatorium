"""Bring a session's window to the front, given its process id.

This is the one piece of the system that still cares where a window is. It
deliberately does not touch virtual desktops: the API that can enumerate and
switch them is undocumented and breaks on Windows feature updates, whereas
SetForegroundWindow on a window that lives on another desktop makes Windows
switch to it anyway.

ctypes only - no dependencies, and it degrades to False rather than raising
when there is nothing to focus.
"""
import re
import sys

WINDOWS = sys.platform == "win32"

if WINDOWS:
    import ctypes
    from ctypes import wintypes

    _user32 = ctypes.WinDLL("user32", use_last_error=True)
    _kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    _ENUM_PROC = ctypes.WINFUNCTYPE(
        wintypes.BOOL, wintypes.HWND, wintypes.LPARAM
    )
    _user32.EnumWindows.argtypes = [_ENUM_PROC, wintypes.LPARAM]
    _user32.GetWindowTextW.argtypes = [
        wintypes.HWND, wintypes.LPWSTR, ctypes.c_int
    ]
    _user32.IsWindowVisible.argtypes = [wintypes.HWND]
    _user32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
    _user32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
    _user32.SetForegroundWindow.argtypes = [wintypes.HWND]
    _user32.GetWindowThreadProcessId.argtypes = [
        wintypes.HWND,
        ctypes.POINTER(wintypes.DWORD),
    ]
    _user32.GetWindowThreadProcessId.restype = wintypes.DWORD
    _user32.GetForegroundWindow.restype = wintypes.HWND
    _user32.IsWindow.argtypes = [wintypes.HWND]
    _user32.BringWindowToTop.argtypes = [wintypes.HWND]
    _user32.AttachThreadInput.argtypes = [
        wintypes.DWORD, wintypes.DWORD, wintypes.BOOL
    ]
    _user32.AllowSetForegroundWindow.argtypes = [wintypes.DWORD]
    _kernel32.GetCurrentThreadId.restype = wintypes.DWORD

    SW_RESTORE = 9

# Ancestors that are never the terminal a person means.
STOP_AT = {"explorer.exe", "svchost.exe", "services.exe", "winlogon.exe",
           "sihost.exe", "wininit.exe", "csrss.exe"}

# Claude Code prefixes the terminal title with a status glyph that changes as
# the session works, so the title is not stable and cannot be compared raw.
# Stripping every leading non-alphanumeric leaves the part it actually set.
_LEADING_GLYPH = re.compile(r"^[^0-9A-Za-z]+")


def normalise_title(text):
    """A title reduced to what can be compared between two sources.

    The window carries a glyph the session does not know it has, and the
    session's own summary carries none, so the two only line up once the
    decoration is gone.
    """
    if not text:
        return ""
    return " ".join(_LEADING_GLYPH.sub("", str(text)).split()).casefold()


def match_title(summary, windows):
    """The hwnd whose title is this session's, or None.

    `windows` is [(hwnd, title)]. Exact match first, because two sessions in
    one project can have similar summaries and guessing between them would
    focus the wrong one - worse than focusing nothing, since the whole point
    is to stop doing that. A lone prefix match is accepted only when it is
    unambiguous, which covers a title the terminal elided to fit its tab.
    """
    want = normalise_title(summary)
    if not want:
        return None
    pairs = [(hwnd, normalise_title(title)) for hwnd, title in windows]
    exact = [hwnd for hwnd, title in pairs if title == want]
    if len(exact) == 1:
        return exact[0]
    if exact:
        return None
    partial = [hwnd for hwnd, title in pairs
               if title and (want.startswith(title) or title.startswith(want))]
    return partial[0] if len(partial) == 1 else None


def _windows_for_pid(pid):
    found = []

    def cb(hwnd, _lparam):
        owner = wintypes.DWORD()
        _user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and _user32.IsWindowVisible(hwnd):
            # A visible window with no title is usually a helper surface, not
            # the terminal a person means.
            if _user32.GetWindowTextLengthW(hwnd) > 0:
                found.append(hwnd)
        return True

    _user32.EnumWindows(_ENUM_PROC(cb), 0)
    return found


def titled_windows(pid=None):
    """[(hwnd, title)] for visible, titled windows, optionally one process."""
    found = []
    if not WINDOWS:
        return found

    def cb(hwnd, _lparam):
        owner = wintypes.DWORD()
        _user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if pid is not None and owner.value != pid:
            return True
        if not _user32.IsWindowVisible(hwnd):
            return True
        length = _user32.GetWindowTextLengthW(hwnd)
        if length <= 0:
            return True
        buf = ctypes.create_unicode_buffer(length + 1)
        _user32.GetWindowTextW(hwnd, buf, length + 1)
        found.append((hwnd, buf.value))
        return True

    _user32.EnumWindows(_ENUM_PROC(cb), 0)
    return found


def owner_pid(hwnd):
    """Which process owns this window, or None."""
    if not WINDOWS or not hwnd:
        return None
    owner = wintypes.DWORD()
    _user32.GetWindowThreadProcessId(wintypes.HWND(hwnd), ctypes.byref(owner))
    return owner.value or None


def _raise(hwnd):
    """SetForegroundWindow, including from a process that is not in front.

    Windows only lets the process that already owns the foreground window
    change it. The dispatcher is a background service and never does, so the
    call was simply refused and focus fell through to opening a new window -
    which is how a focus request ended up launching a second session.

    Borrowing the foreground thread's input queue for the duration is the
    long-standing way to be allowed. It is undone immediately: leaving two
    threads attached couples their input state for good.
    """
    _user32.ShowWindow(hwnd, SW_RESTORE)
    _user32.BringWindowToTop(hwnd)
    if _user32.SetForegroundWindow(hwnd):
        return True

    front = _user32.GetForegroundWindow()
    if not front:
        return False
    theirs = _user32.GetWindowThreadProcessId(front, None)
    mine = _kernel32.GetCurrentThreadId()
    if not theirs or theirs == mine:
        return False

    owner = wintypes.DWORD()
    _user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
    _user32.AllowSetForegroundWindow(owner)
    if not _user32.AttachThreadInput(mine, theirs, True):
        return False
    try:
        _user32.ShowWindow(hwnd, SW_RESTORE)
        _user32.BringWindowToTop(hwnd)
        _user32.SetForegroundWindow(hwnd)
    finally:
        _user32.AttachThreadInput(mine, theirs, False)
    return _user32.GetForegroundWindow() == hwnd.value


def focus_hwnd(hwnd):
    """Raise one specific window. True when it came forward."""
    if not WINDOWS or not hwnd:
        return False
    hwnd = wintypes.HWND(hwnd)
    if not _user32.IsWindow(hwnd):
        return False
    return _raise(hwnd)


def terminal_for(pid):
    """The pid of the terminal hosting this session, or None.

    Several sessions share one terminal process, so this identifies the
    host, never the window - which is exactly why it is not enough on its own.
    """
    if not WINDOWS or not pid:
        return None
    for candidate, exe in _ancestors(int(pid)):
        if exe and exe.lower() in STOP_AT:
            return None
        if _windows_for_pid(candidate):
            return candidate
    return None


def focus_pid(pid):
    """Raise a window belonging to `pid`. True when one was raised.

    A console session's window often belongs to the terminal host rather than
    to the `claude` process itself, so this walks up to the parent when the
    process owns no window of its own.
    """
    if not WINDOWS or not pid:
        return False
    try:
        pid = int(pid)
    except (TypeError, ValueError):
        return False

    for candidate, exe in [(pid, None)] + _ancestors(pid):
        # Walking too far lands on the shell, and focusing explorer.exe means
        # showing the desktop - visibly worse than doing nothing.
        if exe and exe.lower() in STOP_AT:
            break
        for hwnd in _windows_for_pid(candidate):
            if _raise(wintypes.HWND(hwnd)):
                return True
    return False


def _ancestors(pid, depth=4):
    """[(pid, exe_name)] for parents, nearest first, via a toolhelp snapshot."""
    if not WINDOWS:
        return []
    TH32CS_SNAPPROCESS = 0x00000002

    class PROCESSENTRY32(ctypes.Structure):
        _fields_ = [
            ("dwSize", wintypes.DWORD),
            ("cntUsage", wintypes.DWORD),
            ("th32ProcessID", wintypes.DWORD),
            ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
            ("th32ModuleID", wintypes.DWORD),
            ("cntThreads", wintypes.DWORD),
            ("th32ParentProcessID", wintypes.DWORD),
            ("pcPriClassBase", ctypes.c_long),
            ("dwFlags", wintypes.DWORD),
            ("szExeFile", ctypes.c_char * 260),
        ]

    parents = {}
    names = {}
    snap = _kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if snap == -1:
        return []
    try:
        entry = PROCESSENTRY32()
        entry.dwSize = ctypes.sizeof(PROCESSENTRY32)
        if not _kernel32.Process32First(snap, ctypes.byref(entry)):
            return []
        while True:
            parents[entry.th32ProcessID] = entry.th32ParentProcessID
            names[entry.th32ProcessID] = entry.szExeFile.decode(
                "ascii", "replace"
            )
            if not _kernel32.Process32Next(snap, ctypes.byref(entry)):
                break
    finally:
        _kernel32.CloseHandle(snap)

    out = []
    seen = {pid}
    current = pid
    for _ in range(depth):
        current = parents.get(current)
        if not current or current in seen:
            break
        seen.add(current)
        out.append((current, names.get(current, "")))
    return out
