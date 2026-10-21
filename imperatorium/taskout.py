"""What the commands a session is waiting on are printing.

A session blocked on a long build, or idle until a `Monitor` fires, looks
stuck: the board can say it is waiting on two shells, but not what they are
doing. Claude Code already writes each one's output to a file as it runs:

    <temp>/claude/<project slug>/<session id>/tasks/<task id>.output

for a backgrounded Bash or PowerShell call (`toolUseResult.backgroundTaskId`)
and a `Monitor` (`taskId`) alike, and for a call still in the foreground too,
under an id the transcript only learns if the call is later backgrounded. A
foreground call's file goes when the call returns; a background one's stays.

Reading those files takes nothing from the session. Python opens them with
sharing on, the writer keeps writing, and the harness keeps reading its own
copy. Nothing here writes, locks, truncates or deletes.

A subagent's file there is empty: its progress is its own transcript,

    <projects>/<project slug>/<session id>/subagents/agent-<agent id>.jsonl

beside a `.meta.json` naming the `toolUseId` of the call that started it. A
background subagent is known by its agent id (`toolUseResult.agentId`), one
in the foreground only by that call id, so the meta file is what ties the
two. Its transcript is read the same way and told as lines of text: what it
said, the tools it called, and which of them failed.

Which tasks are still running comes from `sessionstat`; this only finds
their files and reads the end of each.
"""
import glob
import json
import os
import re
import tempfile

from . import activity

TAIL_BYTES = 16 * 1024
# A transcript line carries the whole message as JSON, so more of the file
# makes the same few lines of story.
AGENT_TAIL_BYTES = 256 * 1024
# A foreground call's file is made a moment after the call is written to the
# transcript, but clocks and flushes are not exact.
SLACK = 3.0
ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-_]")


def temp_roots():
    """Where Claude Code keeps its temp folder: `CLAUDE_CODE_TMPDIR` if set,
    else the system temp directory."""
    roots = []
    for base in (os.environ.get("CLAUDE_CODE_TMPDIR"), tempfile.gettempdir()):
        if base:
            path = os.path.join(base, "claude")
            if path not in roots:
                roots.append(path)
    return roots


def tasks_dir(session_id, roots=None):
    """The session's tasks folder, or None. A session id is unique, so the
    project slug - whose spelling of the drive letter varies - is not needed."""
    if not re.fullmatch(r"[0-9a-fA-F-]{36}", session_id or ""):
        return None
    for root in temp_roots() if roots is None else roots:
        hits = [h for h in glob.glob(os.path.join(root, "*", session_id, "tasks"))
                if os.path.isdir(h)]
        if hits:
            return max(hits, key=os.path.getmtime)
    return None


def clean(text):
    """Terminal output as plain lines: colour codes gone, and a line redrawn
    with carriage returns (a progress bar) kept as its last drawing."""
    text = ANSI.sub("", text.replace("\r\n", "\n"))
    return "\n".join(line.rsplit("\r", 1)[-1] for line in text.split("\n"))


def tail(path, limit=TAIL_BYTES):
    """The last `limit` bytes of `path` as text, starting at a line boundary
    when it had to cut. Returns (text, size, mtime); ("", 0, None) if gone."""
    try:
        with open(path, "rb") as fh:
            st = os.fstat(fh.fileno())
            start = max(0, st.st_size - limit)
            fh.seek(start)
            data = fh.read(st.st_size - start)
    except OSError:
        return "", 0, None
    if start:
        cut = data.find(b"\n")
        data = data[cut + 1:] if cut >= 0 else data
    return clean(data.decode("utf-8", "replace")), st.st_size, st.st_mtime


def subagents_dir(transcript):
    """The subagents folder beside a session's transcript, or None."""
    if not transcript or not transcript.endswith(".jsonl"):
        return None
    path = os.path.join(transcript[:-len(".jsonl")], "subagents")
    return path if os.path.isdir(path) else None


def agent_files(folder):
    """{id: transcript path} for each subagent in `folder`, under its agent id
    and under the id of the tool call that started it."""
    out = {}
    if not folder:
        return out
    for path in glob.glob(os.path.join(folder, "agent-*.jsonl")):
        agent_id = os.path.basename(path)[len("agent-"):-len(".jsonl")]
        out[agent_id] = path
        try:
            with open(path[:-len(".jsonl")] + ".meta.json", encoding="utf-8") as fh:
                call = json.load(fh).get("toolUseId")
        except (OSError, ValueError, AttributeError):
            call = None
        if call:
            out[call] = path
    return out


def _first_line(text, limit=200):
    line = str(text or "").strip().split("\n", 1)[0]
    return line if len(line) <= limit else line[:limit - 1] + "…"


def story(entries):
    """A subagent's transcript entries as plain lines: its text, a `> Tool
    what` line per call and an `x` line for each call that failed. The task
    it was given is left out; the tab's label already says it."""
    lines = []
    for entry in entries:
        if entry.get("type") not in ("assistant", "user") or entry.get("parentUuid") is None:
            continue
        content = (entry.get("message") or {}).get("content")
        if not isinstance(content, list):
            continue
        for block in content:
            if not isinstance(block, dict):
                continue
            kind = block.get("type")
            if kind == "text" and entry["type"] == "assistant" and block.get("text", "").strip():
                lines.append(block["text"].strip())
            elif kind == "tool_use":
                lines.append("> %s %s" % (block.get("name") or "?",
                                          _first_line(activity.summarise_input(block.get("input")))))
            elif kind == "tool_result" and block.get("is_error"):
                body = block.get("content")
                if isinstance(body, list):
                    body = " ".join(b.get("text", "") for b in body if isinstance(b, dict))
                lines.append("  x " + _first_line(body))
    return "\n".join(lines) + ("\n" if lines else "")


def agent_tail(path, limit=AGENT_TAIL_BYTES):
    """The end of a subagent's transcript as told by `story`. Returns
    (text, size, mtime); ("", 0, None) if gone. A line still being written
    is left for the next read."""
    try:
        with open(path, "rb") as fh:
            st = os.fstat(fh.fileno())
            start = max(0, st.st_size - limit)
            fh.seek(start)
            data = fh.read(st.st_size - start)
    except OSError:
        return "", 0, None
    if start:
        cut = data.find(b"\n")
        data = data[cut + 1:] if cut >= 0 else b""
    entries = []
    for line in data.decode("utf-8", "replace").split("\n"):
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if isinstance(entry, dict):
            entries.append(entry)
    return story(entries), st.st_size, st.st_mtime


def _created(path):
    try:
        st = os.stat(path)
    except OSError:
        return None
    # st_ctime is creation time on Windows; elsewhere the inode change time,
    # which for a file only ever appended to is close enough.
    return getattr(st, "st_birthtime", None) or st.st_ctime


def awaited(stat, folder, limit=TAIL_BYTES, agents=None):
    """[{id, kind, label, foreground, started, size, mtime, text}] for each
    running shell, monitor and subagent in `stat` (a `sessionstat` summary),
    oldest first. `folder` is the session's tasks folder and `agents` its
    subagents folder, either None when there is none."""
    out = []
    files = {}
    if folder:
        for path in glob.glob(os.path.join(folder, "*.output")):
            files[os.path.basename(path)[:-len(".output")]] = path
    known = set(stat.get("known_tasks") or ())

    def add(task_id, item, kind, path):
        if not path:
            text, size, mtime = "", 0, None
        elif kind == "agent":
            text, size, mtime = agent_tail(path)
        else:
            text, size, mtime = tail(path, limit)
        out.append({"id": task_id, "kind": kind, "label": item.get("label") or "",
                    "foreground": bool(item.get("foreground")),
                    "started": item.get("started"), "size": size,
                    "mtime": mtime, "text": text})

    foreground = []
    for kind, name in (("shells", "shell"), ("monitors", "monitor")):
        for item in stat.get(kind) or ():
            if item.get("foreground"):
                foreground.append(item)
            elif item.get("id"):
                add(item["id"], item, name, files.get(item["id"]))

    transcripts = agent_files(agents)
    for item in stat.get("agents") or ():
        if item.get("id"):
            add(item["id"], item, "agent", transcripts.get(item["id"]))

    # A foreground call is matched to a file no transcript record names that
    # appeared after the call was made, in the order both happened.
    if foreground:
        foreground.sort(key=lambda i: i.get("started") or 0)
        earliest = min((i.get("started") or 0) for i in foreground) - SLACK
        loose = sorted((_created(p) or 0, tid, p) for tid, p in files.items()
                       if tid not in known and not tid.startswith("a"))
        loose = [x for x in loose if x[0] >= earliest]
        # More files than calls means the oldest are leftovers; keep the newest.
        loose = loose[-len(foreground):]
        for i, item in enumerate(foreground):
            match = loose[i] if i < len(loose) else None
            add(match[1] if match else item.get("id"), item, "shell",
                match[2] if match else None)

    out.sort(key=lambda t: t.get("started") or 0)
    return out
