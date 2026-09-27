"""What a session actually said, ready to render.

This is the piece the first design was missing. A board that says
"laser-ledger is blocked" is useless without the question it is blocked on,
and a prompt box that shows nothing is worse - you are answering blind.

Read through the Agent SDK's `get_session_messages`, which is the supported
reader and works on a session that is live in another process: 1032 messages
came back in 0.03s from a session running at the time. The transcript files
are deliberately not parsed; the docs say their format is internal and changes
between versions.

`to_items` is pure, so the shape the window renders can be tested without a
session, an SDK, or a model.
"""
TEXT, TOOL, RESULT, THINKING = "text", "tool", "result", "thinking"

# Who the window should name as the speaker. The wire `role` cannot answer
# this: the API returns every tool result inside a message whose role is
# "user", because that is the transport convention for handing a result back
# to the model, not a statement about who wrote it. Rendering that role
# verbatim put a person's name on the machine's own output - 599 of the 627
# user-role messages across three real sessions here were tool results. So
# the speaker is derived from what the message actually contains.
YOU, CLAUDE, TOOLS = "you", "claude", "tool"

# Long tool output is for reassurance, not reading. Enough to recognise, and
# the window can ask for the rest.
RESULT_CHARS = 1200
INPUT_CHARS = 600


def available():
    try:
        import claude_agent_sdk  # noqa: F401
    except Exception:
        return False
    return True


def _text_of(value):
    """Tool results arrive as a string, a list of blocks, or neither."""
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        parts = []
        for block in value:
            if isinstance(block, dict):
                if block.get("type") == "text":
                    parts.append(str(block.get("text") or ""))
                elif block.get("type") == "image":
                    parts.append("[image]")
            elif block is not None:
                parts.append(str(block))
        return "\n".join(p for p in parts if p)
    if value is None:
        return ""
    return str(value)


def _summarise_input(name, tool_input):
    """The one line that says what a tool call is about to do."""
    if not isinstance(tool_input, dict):
        return str(tool_input or "")[:INPUT_CHARS]
    for key in ("command", "file_path", "path", "pattern", "url", "prompt",
                "query", "description"):
        value = tool_input.get(key)
        if value:
            return str(value)[:INPUT_CHARS]
    if not tool_input:
        return ""
    # Nothing recognisable: show the keys rather than a wall of JSON.
    return ", ".join(sorted(tool_input)[:6])[:INPUT_CHARS]


def to_items(messages, limit=60, include_subagents=False):
    """Normalise SDK messages into render-ready items, oldest first.

    Each item is {speaker, role, blocks:[...]}, where a block is one of:
      {kind: "text",     text}
      {kind: "thinking", text}
      {kind: "tool",     name, summary, id}
      {kind: "result",   text, truncated, is_error, id}
    """
    items = []
    for entry in messages:
        parent_tool = getattr(entry, "parent_tool_use_id", None)
        parent_agent = getattr(entry, "parent_agent_id", None)
        if not include_subagents and (parent_tool or parent_agent):
            continue
        raw = getattr(entry, "message", None)
        if not isinstance(raw, dict):
            continue
        role = raw.get("role") or getattr(entry, "type", None) or "assistant"
        content = raw.get("content")
        if isinstance(content, str):
            content = [{"type": "text", "text": content}]
        if not isinstance(content, list):
            continue

        blocks = []
        for block in content:
            if not isinstance(block, dict):
                continue
            kind = block.get("type")
            if kind == "text":
                text = str(block.get("text") or "").strip()
                if text:
                    blocks.append({"kind": TEXT, "text": text})
            elif kind == "thinking":
                text = str(block.get("thinking") or "").strip()
                if text:
                    blocks.append({"kind": THINKING, "text": text})
            elif kind == "tool_use":
                blocks.append({
                    "kind": TOOL,
                    "name": block.get("name") or "tool",
                    "summary": _summarise_input(block.get("name"),
                                                block.get("input")),
                    "id": block.get("id"),
                })
            elif kind == "tool_result":
                text = _text_of(block.get("content"))
                blocks.append({
                    "kind": RESULT,
                    "text": text[:RESULT_CHARS],
                    "truncated": len(text) > RESULT_CHARS,
                    "is_error": bool(block.get("is_error")),
                    "id": block.get("tool_use_id"),
                })
        if blocks:
            items.append({"role": role, "speaker": speaker_of(role, blocks),
                          "blocks": blocks})

    # Limit first, then merge: the cap has to stay a cap on how much is read
    # off the wire, or a tool-heavy turn would collapse into one item and
    # drag the whole conversation into every 1.5s poll with it.
    return merge_turns(items[-limit:] if limit else items)


def speaker_of(role, blocks):
    """Who is talking, given what they said.

    A user-role message is the person only when it carries something a person
    could have typed. A message that is nothing but tool results is the
    machine reporting back on its own work.
    """
    if role == "assistant":
        return CLAUDE
    if any(block["kind"] != RESULT for block in blocks):
        return YOU
    return TOOLS


def merge_turns(items):
    """One item per turn, so the window prints one name per turn.

    Two joins, both because the wire shape is finer-grained than a
    conversation. A single turn's thinking, prose and tool calls arrive as
    separate assistant messages, and each tool call's result comes back as a
    message of its own. Neither is a change of speaker, and a header on each
    one is noise at best and wrong at worst.

    A tool result with no call above it - the limit cut the turn in half -
    keeps its own header rather than being attached to the wrong speaker.
    """
    merged = []
    for item in items:
        prev = merged[-1] if merged else None
        joins = prev is not None and (
            item["speaker"] == prev["speaker"]
            or (item["speaker"] == TOOLS and prev["speaker"] == CLAUDE)
        )
        if joins:
            prev["blocks"].extend(item["blocks"])
        else:
            merged.append(dict(item, blocks=list(item["blocks"])))
    return merged


def last_assistant_text(items):
    """The most recent thing Claude actually said, for a sidebar preview."""
    for item in reversed(items):
        if item["speaker"] != CLAUDE:
            continue
        for block in reversed(item["blocks"]):
            if block["kind"] == TEXT:
                return block["text"]
    return ""


def title(session_id):
    """The title this session is showing in its terminal, or "".

    This is the only thing that identifies a session *on screen*. Neither
    `claude agents --json` nor the session file carries it, and the tile's
    name (`central-control-70`, derived from the folder) is not what the
    window says, which is why a window could not be found from a tile. The
    SDK's `summary` is the same string Claude Code puts in the title bar.

    Read through the SDK rather than the transcript: the summary is in the
    JSONL as an `ai-title` record, but that file's format is internal.
    """
    try:
        import claude_agent_sdk as sdk
    except Exception:
        return ""
    try:
        info = sdk.get_session_info(session_id)
    except Exception:
        return ""
    return getattr(info, "custom_title", None) or getattr(info, "summary", None) or ""


def read(session_id, limit=60, include_subagents=False, directory=None):
    """Items for one session. Raises RuntimeError when the SDK is absent."""
    try:
        import claude_agent_sdk as sdk
    except Exception as exc:
        raise RuntimeError("claude-agent-sdk is not installed: %s" % exc) from exc
    kwargs = {"directory": directory} if directory else {}
    messages = sdk.get_session_messages(session_id, **kwargs)
    return to_items(messages, limit=limit, include_subagents=include_subagents)
