r"""Open the control window.

Chromeless if possible: `--app=` gives a browser window with no tabs, no
address bar and its own taskbar entry, which is the difference between "a
page I have open somewhere" and "the window I work in". Edge ships with
Windows 11, so there is normally nothing to install.

Falls back to the default browser, which works fine - it just looks like a
tab, because it is one.

    python -m ccontrol.window
"""
import os
import shutil
import subprocess
import sys

URL = os.environ.get("CCONTROL_URL", "http://127.0.0.1:8792/")

# Order matters: the first one present wins.
CANDIDATES = (
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
)


def find_browser():
    for path in CANDIDATES:
        if os.path.isfile(path):
            return path
    for name in ("msedge", "chrome", "chromium"):
        found = shutil.which(name)
        if found:
            return found
    return None


def open_window(url=URL, size="1280,820"):
    """Returns the browser used, or None if it fell back to the default."""
    browser = find_browser()
    if browser:
        # A separate user-data-dir would isolate the window but also lose the
        # session cookie jar and start a second browser process tree; the
        # page is local and unauthenticated, so there is nothing to isolate.
        subprocess.Popen(
            [browser, "--app=" + url, "--window-size=" + size],
            creationflags=getattr(subprocess, "DETACHED_PROCESS", 0)
            if sys.platform == "win32" else 0,
        )
        return browser
    import webbrowser

    webbrowser.open(url)
    return None


if __name__ == "__main__":
    used = open_window()
    print("opened %s in %s" % (URL, os.path.basename(used) if used
                               else "the default browser"))
