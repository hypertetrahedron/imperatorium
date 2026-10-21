r"""Open a project in Claude Code with a prompt pre-filled.

`claude-cli://open?cwd=<abs>&q=<prompt>` opens a session in the right
directory with the prompt typed but NOT sent. That last part is the feature:
it is the confirmation step a voice front-end needs, and it is what makes
launching safe when the project match was merely probable.

The handler is registered on Windows at the first prompt of an interactive
session (HKCU\Software\Classes\claude-cli).
"""
import os
import subprocess
import sys
from urllib.parse import quote

MAX_Q = 5000
WINDOWS = sys.platform == "win32"


class DeepLinkError(RuntimeError):
    pass


def build(cwd, prompt=""):
    """A claude-cli://open URL for `cwd`, with `prompt` pre-filled."""
    if not cwd:
        raise DeepLinkError("cwd is required")
    path = os.path.abspath(cwd)
    # Deep links reject UNC/network paths outright, and at least one project
    # on this machine has been opened from one. Fail here with a reason
    # rather than letting the handler swallow it silently.
    if path.startswith("\\\\") or path.startswith("//"):
        raise DeepLinkError("deep links reject UNC paths: %s" % path)
    if ".." in path.replace("\\", "/").split("/"):
        raise DeepLinkError("deep links reject '..' segments: %s" % path)
    if len(prompt) > MAX_Q:
        raise DeepLinkError("prompt is %d chars; the cap is %d" % (len(prompt), MAX_Q))
    url = "claude-cli://open?cwd=" + quote(path, safe="")
    if prompt:
        url += "&q=" + quote(prompt, safe="")
    return url


def open_url(url):
    """Hand the URL to the OS handler."""
    if WINDOWS:
        # start via cmd so the shell resolves the protocol handler; the empty
        # title argument is required or a quoted URL is read as the title.
        subprocess.run(
            ["cmd", "/c", "start", "", url],
            check=False,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
    elif sys.platform == "darwin":
        subprocess.run(["open", url], check=False)
    else:
        subprocess.run(["xdg-open", url], check=False)


def launch(cwd, prompt=""):
    url = build(cwd, prompt)
    open_url(url)
    return url


if __name__ == "__main__":
    print(build(sys.argv[1], " ".join(sys.argv[2:])))
