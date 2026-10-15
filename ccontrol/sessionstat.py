"""One session in numbers, read off its transcript.

The window's status panel wants what the terminal knows but `claude agents`
does not say: what the session left running in the background, whether its
prompt cache is still warm, and what it has spent. All of it is in the JSONL
Claude Code writes, as records rather than prose:

- a backgrounded command's result carries `toolUseResult.backgroundTaskId`;
  a `Monitor` carries `taskId` and `timeoutMs`; a background subagent carries
  `status: "async_launched"` and `agentId`;
- each of those ends with a `<task-notification>` whose `<status>` is
  completed, failed, killed or stopped, or with a `TaskStop` result. A
  Monitor also ends when its timeout runs out, which may say nothing;
- every assistant message carries the API's `usage`, repeated on each
  content block of the same message, so it is counted once per message id.
  `cache_creation` says whether what was written lives 5 minutes or an hour.

Read straight from the file, not through the SDK: these are the entries the
SDK's message view leaves out. The format is Claude Code's own and unversioned,
so everything here is optional and a missing field is simply not shown.
"""
import glob
import json
import os
import re
import threading
from datetime import datetime

TERMINAL = {"completed", "failed", "killed", "stopped"}
CACHE_5M, CACHE_1H = 300.0, 3600.0
NOTE = re.compile(r"<task-notification>(.*?)</task-notification>", re.S)
TASK_ID = re.compile(r"<task-id>(.*?)</task-id>")
STATUS = re.compile(r"<status>(.*?)</status>")
SHELL_TOOLS = {"Bash", "PowerShell"}
AGENT_TOOLS = {"Agent", "Task"}
LABEL_CHARS = 80


def _when(stamp):
    """ISO timestamp -> Unix seconds, or None."""
    if not isinstance(stamp, str):
        return None
    try:
        return datetime.fromisoformat(stamp.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def _num(value):
    return value if isinstance(value, (int, float)) and not isinstance(value, bool) else 0


def _texts(entry):
    """Every piece of plain text a user-side entry carries."""
    msg = entry.get("message") or {}
    content = msg.get("content")
    if isinstance(content, str):
        yield content
    elif isinstance(content, list):
        for block in content:
            if isinstance(block, dict) and block.get("type") == "text":
                yield str(block.get("text") or "")
    att = entry.get("attachment")
    if isinstance(att, dict) and isinstance(att.get("prompt"), str):
        yield att["prompt"]


def _label(tool_input):
    if not isinstance(tool_input, dict):
        return ""
    for key in ("description", "command", "prompt"):
        if tool_input.get(key):
            return " ".join(str(tool_input[key]).split())[:LABEL_CHARS]
    return ""


def summarise(entries, now):
    """The panel's numbers from parsed transcript entries. Pure.

    `tasks` lists what was started in the background and has not been seen
    to end. Whether any of it can still be running depends on the session
    process being alive, which the caller knows and this does not.
    """
    return Fold().feed(entries).summary(now)


class Fold:
    """`summarise`, one entry at a time, keeping only what the answer needs.

    The tiles want every session's numbers every couple of seconds, so a
    transcript is folded as it grows rather than kept and walked again:
    a call is forgotten once its result arrives.
    """

    def __init__(self):
        self.calls = {}         # unanswered tool_use id -> (name, input)
        self.tasks = {}         # task id -> {kind, label, started, expires}
        self.ended = set()      # task ids seen to end
        self.seen_msgs = set()
        self.tokens = {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0}
        self.prompts = set()
        self.last = None        # the newest main-chain assistant message
        self.ttl = CACHE_5M
        self.out = {"compactions": 0, "turns": 0}

    def feed(self, entries):
        for entry in entries:
            self._one(entry)
        return self

    def _one(self, entry):
        out = self.out
        kind = entry.get("type")
        side = entry.get("isSidechain")
        at = _when(entry.get("timestamp"))
        if at and "started" not in out:
            out["started"] = at
        for field, name in (("version", "version"), ("gitBranch", "branch")):
            if entry.get(field):
                out[name] = entry[field]
        if kind == "permission-mode" and entry.get("permissionMode"):
            out["permission_mode"] = entry["permissionMode"]
        elif kind == "system":
            sub = entry.get("subtype")
            if sub == "compact_boundary":
                out["compactions"] += 1
            elif sub == "turn_duration" and not side:
                out["turns"] += 1
                if isinstance(entry.get("durationMs"), (int, float)):
                    out["last_turn_ms"] = entry["durationMs"]
        msg = entry.get("message") or {}
        content = msg.get("content")
        if kind == "assistant":
            mid = msg.get("id")
            usage = msg.get("usage") or {}
            if usage and mid not in self.seen_msgs:
                self.seen_msgs.add(mid)
                tokens = self.tokens
                tokens["input"] += _num(usage.get("input_tokens"))
                tokens["output"] += _num(usage.get("output_tokens"))
                tokens["cache_read"] += _num(usage.get("cache_read_input_tokens"))
                tokens["cache_write"] += _num(usage.get("cache_creation_input_tokens"))
                if not side:
                    made = usage.get("cache_creation") or {}
                    if _num(made.get("ephemeral_1h_input_tokens")):
                        self.ttl = CACHE_1H
                    elif _num(made.get("ephemeral_5m_input_tokens")):
                        self.ttl = CACHE_5M
                    self.last = (at, usage, msg.get("model"))
            if isinstance(content, list):
                for block in content:
                    if isinstance(block, dict) and block.get("type") == "tool_use":
                        self.calls[block.get("id")] = (block.get("name"), block.get("input"))
        elif kind in ("user", "attachment"):
            if kind == "user" and not side and entry.get("promptId"):
                self.prompts.add(entry["promptId"])
            if isinstance(content, list):
                for block in content:
                    if isinstance(block, dict) and block.get("type") == "tool_result":
                        cid = block.get("tool_use_id")
                        _started(self.tasks, self.ended, entry, cid, self.calls, at)
                        self.calls.pop(cid, None)
            for text in _texts(entry):
                for note in NOTE.findall(text):
                    status = STATUS.search(note)
                    if status and status.group(1).strip() in TERMINAL:
                        self.ended.update(t.strip() for t in TASK_ID.findall(note))

    def summary(self, now):
        out = dict(self.out)
        running = {"agents": [], "shells": [], "monitors": []}
        for tid, task in self.tasks.items():
            if tid in self.ended or (task.get("expires") and task["expires"] <= now):
                continue
            running[task["kind"]].append(dict(task, id=tid))
        # A call still waiting on its result is running in the foreground - a
        # subagent or a command the turn is blocked on.
        for cid, (name, tool_input) in self.calls.items():
            item = {"id": cid, "label": _label(tool_input), "foreground": True}
            if name in AGENT_TOOLS:
                running["agents"].append(dict(item, kind="agents"))
            elif name in SHELL_TOOLS:
                running["shells"].append(dict(item, kind="shells"))

        out.update(running)
        out["tokens"] = dict(self.tokens)
        out["prompts"] = len(self.prompts)
        if self.last:
            at, usage, model = self.last
            ttl = self.ttl
            context = (_num(usage.get("input_tokens"))
                       + _num(usage.get("cache_read_input_tokens"))
                       + _num(usage.get("cache_creation_input_tokens")))
            out["model_id"] = model
            out["last_reply_at"] = at
            out["context_tokens"] = context
            out["cache"] = {
                "ttl": ttl,
                "expires_at": at + ttl if at else None,
                "warm": bool(at) and now < at + ttl,
                "hit": (_num(usage.get("cache_read_input_tokens")) / context)
                if context else None,
            }
        return out


def _started(tasks, ended, entry, call_id, calls, at):
    """Record what one tool result says it put in the background."""
    result = entry.get("toolUseResult")
    if not isinstance(result, dict):
        return
    name, tool_input = calls.get(call_id, (None, None))
    label = _label(tool_input)
    if result.get("backgroundTaskId"):
        tasks[result["backgroundTaskId"]] = {"kind": "shells", "label": label, "started": at}
    elif result.get("taskId") and name == "Monitor":
        timeout = result.get("timeoutMs")
        expires = at + timeout / 1000.0 if at and isinstance(timeout, (int, float)) \
            and not result.get("persistent") else None
        tasks[result["taskId"]] = {"kind": "monitors", "label": label, "started": at,
                                   "expires": expires}
    elif result.get("status") == "async_launched" and result.get("agentId"):
        tasks[result["agentId"]] = {"kind": "agents", "label": result.get("description") or label,
                                    "started": at, "model": result.get("resolvedModel")}
    if name == "TaskStop" and result.get("task_id"):
        ended.add(result["task_id"])


def projects_dir():
    base = os.environ.get("CLAUDE_CONFIG_DIR") or os.path.join(os.path.expanduser("~"), ".claude")
    return os.path.join(base, "projects")


def transcript_path(session_id):
    if not re.fullmatch(r"[0-9a-fA-F-]{36}", session_id or ""):
        return None
    hits = glob.glob(os.path.join(projects_dir(), "*", session_id + ".jsonl"))
    return max(hits, key=os.path.getmtime) if hits else None


def parse(text):
    out = []
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if isinstance(entry, dict):
            out.append(entry)
    return out


class Reader:
    """`summarise` per session, reading only what was appended since last time.

    The window asks every few seconds and a long transcript is megabytes - a
    full parse of one here took 200 ms - but Claude Code only ever appends.
    So each session keeps a `Fold` with the byte offset it reached, and a
    file that grew is read from there; one that shrank is read again whole.
    The summary is redone each time, because it depends on the clock (cache
    expiry, Monitor timeouts). A fold is small, so every tile's session fits.
    """

    KEEP = 64

    def __init__(self):
        self._lock = threading.Lock()
        self._cache = {}     # session id -> [path, offset, fold]

    def read(self, session_id, now):
        with self._lock:
            hit = self._cache.get(session_id)
        # The path is found once; globbing every project folder per tile per
        # refresh would be the expensive part.
        path = hit[0] if hit and os.path.exists(hit[0]) else transcript_path(session_id)
        if not path:
            return None
        with self._lock:
            hit = self._cache.pop(session_id, None)
            if not hit or hit[0] != path:
                hit = [path, 0, Fold()]
            self._cache[session_id] = hit
            while len(self._cache) > self.KEEP:
                self._cache.pop(next(iter(self._cache)))
            try:
                size = os.path.getsize(path)
                if size < hit[1]:
                    hit[1], hit[2] = 0, Fold()
                if size > hit[1]:
                    with open(path, "rb") as fh:
                        fh.seek(hit[1])
                        chunk = fh.read(size - hit[1])
                    # Stop at the last complete line; a line still being
                    # written is picked up next time.
                    end = chunk.rfind(b"\n") + 1
                    hit[2].feed(parse(chunk[:end].decode("utf-8", "replace")))
                    hit[1] += end
            except OSError:
                return None
            return hit[2].summary(now)
