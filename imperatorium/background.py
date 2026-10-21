"""Hand work to a background session instead of opening a window.

`claude --bg "<prompt>"` starts a session with no terminal, prints a short id,
and returns. Claude Code isolates it in a git worktree under `.claude/` before
it edits anything, and on delete it commits and pushes rather than losing
work. None of that is ours to build.

This is the right shape for "project: laser ledger, implement the new feature
as recommended" - work you want done, not a window you want to sit in. The
deep-link path stays for the other intent, where you do want to watch.

A background session that hits a permission prompt shows up on the board as
blocked, exactly like an interactive one, because the same hooks report it.
"""
import os
import re
import subprocess
import sys

WINDOWS = sys.platform == "win32"

# `claude --bg` prints a short id; keep this loose, since the surrounding
# wording is not a contract.
ID_PATTERN = re.compile(r"\b([0-9a-f]{6,12})\b")

DEFAULT_TIMEOUT = 90.0


class BackgroundError(RuntimeError):
    pass


def short_name(prompt, limit=28):
    """A readable session name from the first few words of a prompt.

    Without one, Claude Code names a background session after the whole
    prompt, which is unreadable on the board and unusable on a Stream Deck
    key. Letters, digits and hyphens only, so it stays a usable handle.
    """
    words = re.findall(r"[A-Za-z0-9]+", prompt or "")
    name = "-".join(words[:4]).lower()[:limit].strip("-")
    return name or None


def build_command(prompt, model=None, permission_mode=None, agent=None,
                  name=None, extra=None):
    cmd = ["claude", "--bg"]
    # None derives a name from the prompt; "" means keep the session's own,
    # which is what resuming one wants.
    name = short_name(prompt) if name is None else name
    if name:
        cmd += ["--name", name]
    if model:
        cmd += ["--model", model]
    if permission_mode:
        cmd += ["--permission-mode", permission_mode]
    if agent:
        cmd += ["--agent", agent]
    cmd += list(extra or ())
    cmd.append(prompt)
    return cmd


def parse_id(stdout):
    """The session id `claude --bg` printed, or None.

    Takes the last match rather than the first: the id is what the command
    ends with, and any preamble may contain hex-looking noise.
    """
    matches = ID_PATTERN.findall(stdout or "")
    return matches[-1] if matches else None


OPTION_KEYS = ("model", "permission_mode", "agent", "name", "extra")


def start(prompt, cwd, timeout=DEFAULT_TIMEOUT, **options):
    """Start a background session in `cwd`. Returns {id, command, output}."""
    if not prompt or not prompt.strip():
        raise BackgroundError("prompt is empty")
    if not cwd or not os.path.isdir(cwd):
        raise BackgroundError("not a directory: %s" % (cwd,))

    # These come from config.json, which is hand-edited and carries comment
    # keys. An unknown key is not worth failing a dispatch over.
    cmd = build_command(prompt, **{k: v for k, v in options.items()
                                   if k in OPTION_KEYS})
    try:
        done = subprocess.run(
            cmd,
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=timeout,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0) if WINDOWS else 0,
        )
    except FileNotFoundError as exc:
        raise BackgroundError("claude is not on PATH") from exc
    except subprocess.TimeoutExpired as exc:
        raise BackgroundError("claude --bg did not return within %gs" % timeout) from exc

    output = ((done.stdout or "") + (done.stderr or "")).strip()
    if done.returncode != 0:
        raise BackgroundError(output[:400] or "claude --bg exited %d" % done.returncode)

    session = parse_id(done.stdout or "")
    if not session:
        # It may well have started; we just cannot name it. Say so rather than
        # claiming a failure that would tempt a retry and start a second one.
        raise BackgroundError(
            "started, but no session id was printed: %s" % output[:200]
        )
    return {"id": session, "command": cmd, "output": output[:400]}
