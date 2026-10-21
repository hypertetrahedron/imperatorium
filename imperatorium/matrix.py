"""The chat matrix: every running session at once, each cut down to what fits.

A tile cannot hold a conversation, so it holds two things: what Claude said
to you, and what it is doing right now. The prompts, the tool results and
every finished step in between are dropped. A tool call shows while it runs
and goes when its result comes back; a burst of thinking shows until the
next thing arrives. What is left is the replies, with at most one live step
under them.

Pure functions of the conversation items `conversation.to_items` makes, so
the window's poll costs one walk over a cached list per session.
"""
from . import conversation

REPLIES = 12        # replies kept per tile; the tile shows the newest that fit
REPLY_CHARS = 2000  # a reply longer than this keeps only its end
LINE_CHARS = 200    # a step's one-line summary

ENDED = ("done", "failed", "stopped")


def active(row):
    """Whether a session gets a tile: its process is up and it has not ended."""
    return bool(row.get("status")) and row.get("state") not in ENDED


def working(row):
    """Whether the session is in a turn, so a trailing thought is still live."""
    return row.get("status") in ("busy", "shell") or row.get("state") == "working"


def focus(items, live=True, keep=REPLIES):
    """{replies, current} from a conversation's items, oldest first.

    `replies` are Claude's text blocks, each with the number of the turn it
    belongs to, so a tile can rule a line between turns. `current` is the
    step in flight - a tool call with no result yet, or the thought Claude is
    in the middle of - or None.

    Not every call gets a result written down: a parallel call cancelled when
    a sibling fails, a turn interrupted. So a reply or a new prompt closes
    whatever is still open, and a session that is not `live` - not in a turn
    and not waiting on you - has nothing in flight at all.
    """
    replies, open_steps, turn = [], [], 0
    for item in items:
        speaker = item.get("speaker")
        blocks = item.get("blocks") or []
        if speaker not in (conversation.CLAUDE, conversation.TOOLS):
            # A tool result that comes back with text attached (a reminder, a
            # note that a file changed) reads as a message from you; it still
            # ends its call. Only a message with no results is a new prompt.
            results = [b for b in blocks if b.get("kind") == conversation.RESULT]
            if results:
                ids = {b.get("id") for b in results}
                open_steps = [s for s in open_steps
                              if s["kind"] != "tool" or s["id"] not in ids]
            elif speaker == conversation.YOU:
                turn += 1
                open_steps = []
            continue
        for block in blocks:
            kind = block.get("kind")
            # Any new block ends a thought: Claude has moved on from it.
            open_steps = [s for s in open_steps if s["kind"] != conversation.THINKING]
            if kind == conversation.TEXT:
                open_steps = []
                text = block.get("text") or ""
                # The tile shows the bottom of a reply and lets the top run
                # off, and a reply's conclusion is at its end: keep the end.
                cut = len(text) > REPLY_CHARS
                replies.append({"text": ("…" if cut else "") + text[-REPLY_CHARS:],
                                "turn": turn})
            elif kind == conversation.TOOL:
                open_steps.append({"kind": "tool", "id": block.get("id"),
                                   "name": block.get("name") or "tool",
                                   "summary": _line(block.get("summary"))})
            elif kind == conversation.THINKING:
                open_steps.append({"kind": "thinking", "text": _line(block.get("text"))})
            elif kind == conversation.RESULT:
                open_steps = [s for s in open_steps
                              if s["kind"] != "tool" or s["id"] != block.get("id")]
    if not live:
        open_steps = []
    current = open_steps[-1] if open_steps else None
    if current:
        current = {k: v for k, v in current.items() if k != "id"}
        # Parallel calls finish in any order; say how many are still out.
        tools = sum(1 for s in open_steps if s["kind"] == "tool")
        if tools > 1:
            current["more"] = tools - 1
    return {"replies": replies[-keep:] if keep else replies, "current": current}


def _line(text):
    """The first non-blank line, cut to fit one line of a tile."""
    for line in str(text or "").splitlines():
        line = line.strip()
        if line:
            return line[:LINE_CHARS] + ("…" if len(line) > LINE_CHARS else "")
    return ""


def spend(tiles, limits=None):
    """The status tile: what the sessions on screen have cost, in total and
    each, and how far through its usage windows the account is."""
    tokens = {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0}
    cost, costed, each = 0.0, 0, []
    for tile in tiles:
        got = tile.get("tokens") or {}
        for name in tokens:
            value = got.get(name)
            if isinstance(value, (int, float)):
                tokens[name] += value
        usd = tile.get("cost_usd")
        if isinstance(usd, (int, float)):
            cost += usd
            costed += 1
        each.append({"name": tile.get("name"), "cost_usd": usd,
                     "tokens": sum(v for v in got.values() if isinstance(v, (int, float)))})
    each.sort(key=lambda e: -(e["cost_usd"] or 0))
    return {"cost_usd": cost, "costed": costed, "sessions": len(tiles),
            "tokens": tokens, "total_tokens": sum(tokens.values()),
            "each": each, "limits": limits or {}}
