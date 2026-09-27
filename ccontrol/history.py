"""Past sessions: finding one, and getting back into it.

"Which session did I fix the tunnel in?" has no answer in the sidebar, which
only shows what is running. The SDK's `list_sessions` knows every session on
this machine with its title, first prompt and directory, and
`get_session_messages` reads any of them, so search is those two and a
cache: titles are always searched, and full text is searched from a per-
session text cache that is rebuilt only when the transcript's size changes.

Getting back in is one of three things, all Claude Code's own:
- resume in a window: `claude --resume <id>` in a new Windows Terminal tab;
- resume in the background: `claude --bg --resume <id>`;
- fork: `--fork-session` with either, so the original stays as it was.

A dead session - one that ended, or whose process is gone - is just a past
session that is still on screen, so it is resumed the same way.
"""
import os
import shutil
import subprocess
import sys
import threading
import time

from . import background, conversation

WINDOWS = sys.platform == "win32"
MAX_SESSIONS = 400
SNIPPET = 90


def _sdk():
    try:
        import claude_agent_sdk as sdk
    except Exception as exc:
        raise RuntimeError("claude-agent-sdk is not installed: %s" % exc) from exc
    return sdk


def info_dict(info):
    return {
        "session_id": info.session_id,
        "title": getattr(info, "custom_title", None) or getattr(info, "summary", None)
        or "",
        "first_prompt": (getattr(info, "first_prompt", None) or "")[:300],
        "cwd": getattr(info, "cwd", None),
        "branch": getattr(info, "git_branch", None),
        "modified": (getattr(info, "last_modified", 0) or 0) / 1000.0,
        "created": (getattr(info, "created_at", 0) or 0) / 1000.0,
        "size": getattr(info, "file_size", 0) or 0,
    }


def plain_text(items):
    """Everything a person or Claude wrote in a session, one string. Pure."""
    parts = []
    for item in items:
        for b in item.get("blocks", ()):
            if b.get("kind") in ("text", "tool"):
                parts.append(b.get("text") or "%s %s" % (b.get("name"), b.get("summary")))
    return "\n".join(parts)


def snippet(text, query, width=SNIPPET):
    """The match in context, on one line."""
    at = text.lower().find(query.lower())
    if at < 0:
        return ""
    start = max(0, at - width // 2)
    end = min(len(text), at + len(query) + width // 2)
    s = " ".join(text[start:end].split())
    return ("…" if start else "") + s + ("…" if end < len(text) else "")


def matches(query, record, text=""):
    """Where `query` appears: 'title', 'prompt', 'text', or None. Pure.

    Every word must appear somewhere, in any order - "tunnel pi" finds the
    session titled "Raspberry Pi tunnel fix".
    """
    words = [w for w in query.lower().split() if w]
    if not words:
        return None
    title = (record.get("title") or "").lower()
    prompt = (record.get("first_prompt") or "").lower()
    where = (record.get("cwd") or "").lower()
    hay = "\n".join((title, prompt, where, text.lower()))
    if not all(w in hay for w in words):
        return None
    if all(w in title or w in where for w in words):
        return "title"
    if all(w in title or w in prompt or w in where for w in words):
        return "prompt"
    return "text"


class Index:
    """Session list plus a lazily built full-text cache."""

    def __init__(self, reader=None, lister=None):
        self._lock = threading.Lock()
        self._text = {}  # session id -> (size, text)
        self._reader = reader
        self._lister = lister
        self._listed = {"at": 0.0, "rows": []}

    def list(self, max_age=30.0):
        with self._lock:
            if time.time() - self._listed["at"] < max_age:
                return list(self._listed["rows"])
        lister = self._lister or (lambda: _sdk().list_sessions())
        rows = [info_dict(i) for i in lister()]
        rows.sort(key=lambda r: r["modified"], reverse=True)
        rows = rows[:MAX_SESSIONS]
        with self._lock:
            self._listed = {"at": time.time(), "rows": rows}
        return list(rows)

    def text(self, record):
        sid, size = record["session_id"], record.get("size")
        with self._lock:
            hit = self._text.get(sid)
            if hit and hit[0] == size:
                return hit[1]
        reader = self._reader or (lambda s: conversation.read(s, limit=0))
        try:
            text = plain_text(reader(sid))
        except Exception:
            text = ""
        with self._lock:
            self._text[sid] = (size, text)
        return text

    def search(self, query, limit=40, full_text=True, budget=8.0):
        """Newest first. Full text is read until `budget` seconds are spent,
        so a first search over hundreds of sessions answers rather than
        hanging; the cache makes the next one complete."""
        query = (query or "").strip()
        rows = self.list()
        if not query:
            return {"results": [dict(r, where=None, snippet="") for r in rows[:limit]],
                    "complete": True}
        out, deadline, complete = [], time.time() + budget, True
        for r in rows:
            where = matches(query, r)
            text = ""
            if where is None and full_text:
                if time.time() > deadline:
                    complete = False
                    continue
                text = self.text(r)
                where = matches(query, r, text)
            if where:
                out.append(dict(r, where=where, snippet=snippet(
                    text or r.get("first_prompt") or r.get("title") or "",
                    query.split()[0])))
                if len(out) >= limit:
                    break
        return {"results": out, "complete": complete}


# -- getting back in ------------------------------------------------------
class ResumeError(RuntimeError):
    pass


def resume_command(session_id, fork=False):
    """The command a person would type. Also what the window shows to copy."""
    cmd = ["claude", "--resume", session_id]
    if fork:
        cmd.append("--fork-session")
    return cmd


def terminal_command(cwd, cmd, title=None):
    """Windows Terminal: a new tab, in the most recent window, in `cwd`."""
    wt = shutil.which("wt") or shutil.which("wt.exe")
    if not wt:
        raise ResumeError("Windows Terminal (wt.exe) is not on PATH")
    out = [wt, "-w", "0", "new-tab", "-d", cwd]
    if title:
        out += ["--title", title]
    return out + list(cmd)


def resume(session_id, cwd, where="window", fork=False, prompt=None,
           options=None, popen=None, bg_start=None):
    """Resume (or fork) a session in a new tab or in the background.

    Returns {ok, action, command, id?}. `popen` and `bg_start` replace the
    real process calls in tests.
    """
    if not session_id:
        raise ResumeError("session id is required")
    if not cwd or not os.path.isdir(cwd):
        raise ResumeError("its directory is gone: %s" % (cwd,))
    if where == "window":
        cmd = terminal_command(cwd, resume_command(session_id, fork))
        (popen or _popen)(cmd, cwd)
        return {"ok": True, "action": "fork" if fork else "resume",
                "where": "window", "command": " ".join(resume_command(session_id, fork))}
    if where == "background":
        extra = ["--resume", session_id] + (["--fork-session"] if fork else [])
        opts = dict(options or {}, name="")
        opts["extra"] = list(opts.get("extra") or ()) + extra
        text = prompt or "Continue where you left off."
        try:
            started = (bg_start or background.start)(text, cwd, **opts)
        except background.BackgroundError as exc:
            raise ResumeError(str(exc)) from exc
        return {"ok": True, "action": "fork" if fork else "resume",
                "where": "background", "id": started["id"],
                "command": " ".join(["claude", "--bg"] + extra)}
    raise ResumeError("where must be window or background")


def _popen(cmd, cwd):
    subprocess.Popen(cmd, cwd=cwd, close_fds=True,
                     creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)
                     if WINDOWS else 0)
