r"""The global hotkey. Raises the control window, or opens it.

All that is left of the palette. The palette existed because there was
nowhere else to type; now the window has a prompt box, a session list and a
microphone, so the only thing a resident process still needs to do is put
that window in front of you from anywhere.

    .venv\Scripts\pythonw.exe hotkey.py

No window of its own, so nothing to see. RegisterHotKey binds to the calling
thread, so this process is essentially one message loop.
"""
import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ccontrol import window as window_mod  # noqa: E402
from ccontrol import winfocus  # noqa: E402

CONFIG = os.path.join(os.path.dirname(os.path.abspath(__file__)), "config.json")

# Conflicts are the norm: ctrl+alt+space and win+k were both already taken on
# this machine, so the first candidate Windows grants wins.
DEFAULT_HOTKEYS = ["ctrl+alt+backquote", "ctrl+alt+j", "ctrl+shift+space",
                   "ctrl+shift+f12"]

MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN = 0x0001, 0x0002, 0x0004, 0x0008
WM_HOTKEY = 0x0312
VK = {"space": 0x20, "enter": 0x0D, "tab": 0x09, "escape": 0x1B,
      "backquote": 0xC0, "`": 0xC0}

# The window's <title>. Chromium puts it in the frame title for --app windows.
WINDOW_TITLE = "Imperatorium"


def parse_hotkey(spec):
    """'ctrl+alt+space' -> (modifiers, virtual key code)."""
    mods, key = 0, None
    for part in (spec or "").lower().split("+"):
        part = part.strip()
        if part in ("ctrl", "control"):
            mods |= MOD_CONTROL
        elif part == "alt":
            mods |= MOD_ALT
        elif part == "shift":
            mods |= MOD_SHIFT
        elif part in ("win", "super"):
            mods |= MOD_WIN
        elif part:
            key = part
    if not key:
        raise ValueError("no key in hotkey %r" % (spec,))
    if key in VK:
        return mods, VK[key]
    if len(key) == 1:
        return mods, ord(key.upper())
    if key.startswith("f") and key[1:].isdigit():
        return mods, 0x6F + int(key[1:])  # VK_F1 is 0x70
    raise ValueError("unrecognised key %r" % (key,))


def candidates(override=None):
    if override:
        return [override]
    try:
        with open(CONFIG, encoding="utf-8") as fh:
            configured = (json.load(fh).get("palette") or {}).get("hotkey")
    except (OSError, ValueError):
        configured = None
    if isinstance(configured, str):
        return [configured]
    if isinstance(configured, list) and configured:
        return configured
    return DEFAULT_HOTKEYS


def find_window():
    """HWND of the control window, by title. None when it is not open."""
    if not winfocus.WINDOWS:
        return None
    import ctypes

    found = []

    def cb(hwnd, _lparam):
        if not winfocus._user32.IsWindowVisible(hwnd):
            return True
        length = winfocus._user32.GetWindowTextLengthW(hwnd)
        if not length:
            return True
        buf = ctypes.create_unicode_buffer(length + 1)
        winfocus._user32.GetWindowTextW(hwnd, buf, length + 1)
        if WINDOW_TITLE.lower() in buf.value.lower():
            found.append(hwnd)
        return True

    winfocus._user32.EnumWindows(winfocus._ENUM_PROC(cb), 0)
    return found[0] if found else None


def raise_or_open():
    """Front the window if it exists, otherwise start it."""
    hwnd = find_window()
    if hwnd:
        winfocus._user32.ShowWindow(hwnd, winfocus.SW_RESTORE)
        if winfocus._user32.SetForegroundWindow(hwnd):
            return "raised"
    window_mod.open_window()
    return "opened"


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--hotkey", default=None)
    ap.add_argument("--once", action="store_true",
                    help="raise or open the window and exit")
    args = ap.parse_args()

    if args.once:
        print(raise_or_open())
        return 0
    if sys.platform != "win32":
        print("global hotkeys are Windows-only here")
        return 1

    import ctypes
    from ctypes import wintypes

    user32 = ctypes.WinDLL("user32", use_last_error=True)
    bound = None
    for spec in candidates(args.hotkey):
        try:
            mods, vk = parse_hotkey(spec)
        except ValueError:
            continue
        if user32.RegisterHotKey(None, 1, mods, vk):
            bound = spec
            break
    if not bound:
        print("no hotkey could be registered; open the window with "
              "python -m ccontrol.window")
        return 1
    print("hotkey %s raises the Imperatorium window" % bound)

    msg = wintypes.MSG()
    while user32.GetMessageW(ctypes.byref(msg), None, 0, 0) != 0:
        if msg.message == WM_HOTKEY:
            try:
                raise_or_open()
            except Exception:
                # A hotkey daemon that dies on one bad press is worse than
                # one that occasionally does nothing.
                pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
