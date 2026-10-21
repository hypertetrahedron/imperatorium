"""What a session is doing, what it has done, and what keeps failing.

Two sources, and the window uses whichever it has:

1. Tool hooks (`PreToolUse`, `PostToolUse`, `PostToolUseFailure`), when
   installed. These are live and timestamped: the only way to say "running
   `npm test` for 40s" rather than "last ran `npm test`". Kept in memory -
   they arrive once per tool call per session, and the store rewrites its
   whole file on every write.
2. The conversation itself, read through the SDK. Always available, never
   timestamped (the SDK's messages carry no times), and exact about which
   calls failed, because every tool result says whether it was an error.

Everything that turns messages into a timeline or a tally is pure and works
on `conversation.to_items` output, so it is tested without a session.
"""
import collections
import re
import threading
import time

MAX_EVENTS = 200
ACTIVITY_CHARS = 140

TOOL_EVENTS = {"PreToolUse", "PostToolUse", "PostToolUseFailure"}


# -- from the conversation ------------------------------------------------
def timeline(items):
    """[{tool, summary, ok, error}] in order, from render-ready items.

    A call whose result is not in view (still running, or cut off by the
    read limit) has ok=None.
    """
    calls, by_id = [], {}
    for item in items:
        for b in item.get("blocks", ()):
            if b.get("kind") == "tool":
                entry = {"tool": b.get("name"), "summary": b.get("summary") or "",
                         "ok": None, "error": None}
                calls.append(entry)
                if b.get("id"):
                    by_id[b["id"]] = entry
            elif b.get("kind") == "result":
                entry = by_id.get(b.get("id"))
                if entry is None:
                    continue
                entry["ok"] = not b.get("is_error")
                if b.get("is_error"):
                    entry["error"] = first_line(b.get("text"), 200)
    return calls


def first_line(text, limit=ACTIVITY_CHARS):
    for line in str(text or "").splitlines():
        line = line.strip()
        if line:
            return line[:limit] + ("…" if len(line) > limit else "")
    return ""


def doing(items):
    """One line on what the session is doing now, from its conversation.

    The newest thing wins: a tool call that has not returned is what it is
    doing; otherwise the last thing Claude said, which after a finished turn
    is its report on it.
    """
    for item in reversed(items):
        for b in reversed(item.get("blocks", ())):
            kind = b.get("kind")
            if kind == "tool":
                return "%s %s" % (b.get("name"), first_line(b.get("summary")))
            if kind == "text" and item.get("speaker") == "claude":
                return first_line(b.get("text"))
            if kind == "text" and item.get("speaker") == "you":
                return "asked: " + first_line(b.get("text"))
    return ""


# Error text -> a category worth counting. Order matters: first match wins.
CATEGORIES = [
    ("rejected by user", re.compile(r"user (doesn't want|rejected|denied)|"
                                    r"permission (was )?denied|was blocked", re.I)),
    ("file not found", re.compile(r"(does not exist|no such file|not found|"
                                  r"cannot find the (file|path))", re.I)),
    ("edit did not match", re.compile(r"string to replace|old_string|"
                                      r"not unique|found \d+ matches", re.I)),
    ("file not read first", re.compile(r"(has not been read|read it first|"
                                       r"must read)", re.I)),
    ("file changed on disk", re.compile(r"modified since|changed on disk", re.I)),
    ("timed out", re.compile(r"timed? ?out|timeout", re.I)),
    ("command failed", re.compile(r"exit code [1-9]|exited with|returned non-zero|"
                                  r"error:", re.I)),
    ("network", re.compile(r"ECONN|network|fetch failed|429|503|getaddrinfo", re.I)),
]


def categorise(text):
    for name, pattern in CATEGORIES:
        if pattern.search(text or ""):
            return name
    return "other"


def tally(calls):
    """Counts for one timeline: {calls, errors, by_tool:{tool:[calls,errors]},
    by_category:{category: n}, examples:{category: text}}."""
    by_tool = collections.defaultdict(lambda: [0, 0])
    by_cat = collections.Counter()
    examples = {}
    errors = 0
    for c in calls:
        by_tool[c["tool"]][0] += 1
        if c["ok"] is False:
            errors += 1
            by_tool[c["tool"]][1] += 1
            cat = categorise(c.get("error"))
            by_cat[cat] += 1
            examples.setdefault(cat, "%s: %s" % (c["tool"], c.get("error") or ""))
    return {"calls": len(calls), "errors": errors,
            "by_tool": {k: v for k, v in by_tool.items()},
            "by_category": dict(by_cat), "examples": examples}


def merge_tallies(tallies):
    out = {"calls": 0, "errors": 0, "by_tool": {}, "by_category": {}, "examples": {}}
    for t in tallies:
        out["calls"] += t["calls"]
        out["errors"] += t["errors"]
        for tool, (n, e) in t["by_tool"].items():
            cur = out["by_tool"].setdefault(tool, [0, 0])
            cur[0] += n
            cur[1] += e
        for cat, n in t["by_category"].items():
            out["by_category"][cat] = out["by_category"].get(cat, 0) + n
        for cat, ex in t["examples"].items():
            out["examples"].setdefault(cat, ex)
    return out


# -- from tool hooks ------------------------------------------------------
def summarise_input(tool_input):
    """Same idea as conversation._summarise_input, kept local to stay pure."""
    if not isinstance(tool_input, dict):
        return str(tool_input or "")[:200]
    for key in ("command", "file_path", "path", "pattern", "url", "prompt",
                "query", "description"):
        if tool_input.get(key):
            return str(tool_input[key])[:200]
    return ", ".join(sorted(tool_input)[:6])


class Live:
    """Per-session tool events from hooks, newest last, bounded."""

    def __init__(self, max_events=MAX_EVENTS):
        self._lock = threading.Lock()
        self._events = collections.defaultdict(
            lambda: collections.deque(maxlen=max_events))

    def ingest(self, key, payload, now=None):
        event = payload.get("hook_event_name")
        if event not in TOOL_EVENTS:
            return False
        # A subagent's calls are its own story; the session line is about
        # the main conversation.
        if payload.get("agent_id"):
            return True
        now = time.time() if now is None else now
        tid = payload.get("tool_use_id")
        with self._lock:
            events = self._events[key]
            if event == "PreToolUse":
                events.append({"id": tid, "tool": payload.get("tool_name"),
                               "summary": summarise_input(payload.get("tool_input")),
                               "start": now, "end": None, "ok": None,
                               "error": None})
                return True
            for e in reversed(events):
                if e["id"] == tid:
                    break
            else:
                # The Pre was missed (dispatcher restarted mid-call).
                e = {"id": tid, "tool": payload.get("tool_name"),
                     "summary": summarise_input(payload.get("tool_input")),
                     "start": None, "end": None, "ok": None, "error": None}
                events.append(e)
            e["end"] = now
            e["ok"] = event == "PostToolUse"
            if not e["ok"]:
                e["error"] = first_line(payload.get("error")
                                        or payload.get("tool_response"), 200)
        return True

    def events(self, key):
        with self._lock:
            return [dict(e) for e in self._events.get(key, ())]

    def current(self, key, now=None):
        """The call still running, as a line with its age, or None."""
        now = time.time() if now is None else now
        with self._lock:
            events = self._events.get(key)
            if not events:
                return None
            last = events[-1]
            if last["end"] is not None or last["start"] is None:
                return None
            # A Pre with no Post for ten minutes is a call whose Post was
            # lost, not a ten-minute command - say nothing rather than lie.
            if now - last["start"] > 600:
                return None
            return {"tool": last["tool"], "summary": last["summary"],
                    "since": last["start"]}

    def forget(self, key):
        with self._lock:
            self._events.pop(key, None)
