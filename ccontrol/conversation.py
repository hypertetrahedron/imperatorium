"""What a session actually said, ready to render.

This is the piece the first design was missing. A board that says
"laser-ledger is blocked" is useless without the question it is blocked on,
and a prompt box that shows nothing is worse - you are answering blind.

Read through the Agent SDK's `get_session_messages`, which is the supported
reader and works on a session that is live in another process: 1032 messages
came back in 0.03s from a session running at the time. The docs say the
transcript format is internal and changes between versions, so nothing here
parses it directly.

Three exceptions, all through the SDK's own parser rather than around it.
`read` walks the same chain the SDK builds and adds back what its reader
drops, falling back to the public reader when the SDK's internals have moved:

- prompts sent from the window. They reach the session through its inbox,
  and Claude Code records them as meta messages from a peer, which
  `get_session_messages` drops - so everything typed into the window
  vanished from the conversation it was typed into;
- prompts that landed mid-turn, which Claude Code folds into the running
  turn as `queued_command` attachments rather than user messages;
- everything before a compaction, which starts a fresh chain linked to the
  old one only by `logicalParentUuid` (`with_compacted`).

`to_items` is pure, so the shape the window renders can be tested without a
session, an SDK, or a model.
"""
import re

TEXT, TOOL, RESULT, THINKING = "text", "tool", "result", "thinking"

# Who the window should name as the speaker. The wire `role` cannot answer
# this: the API returns every tool result inside a message whose role is
# "user", because that is the transport convention for handing a result back
# to the model, not a statement about who wrote it. Rendering that role
# verbatim put a person's name on the machine's own output - 599 of the 627
# user-role messages across three real sessions here were tool results. So
# the speaker is derived from what the message actually contains.
YOU, CLAUDE, TOOLS = "you", "claude", "tool"

# Claude Code also writes its own messages into the user role: a background
# task finishing, a slash command's output, the summary that replaces a
# compacted conversation. They are plain text, so they looked typed - 116
# task notifications across the transcripts here were captioned "you". The
# transcript entry says where each came from (`origin.kind`,
# `isCompactSummary`), which the SDK drops; `_messages` carries it across as
# `system`, and the text prefixes below catch what arrives without it.
SYSTEM = "system"
NOTE_TASK = "task finished"
NOTE_COMMAND = "command output"
NOTE_SUMMARY = "earlier conversation, summarised"
NOTE_SUBAGENT = "subagent report"
SYSTEM_PREFIXES = (
    ("<task-notification>", NOTE_TASK),
    ("<local-command-stdout>", NOTE_COMMAND),
    ("<local-command-stderr>", NOTE_COMMAND),
)
TASK_SUMMARY = re.compile(r"<summary>(.*?)</summary>", re.S)
COMMAND_TAGS = re.compile(r"</?local-command-std(?:out|err)>")

# Long tool output is for reassurance, not reading. Enough to recognise, and
# the window can ask for the rest.
RESULT_CHARS = 1200
INPUT_CHARS = 600

# What Claude Code wraps around an inbox prompt before the model sees it. The
# window shows what was typed, not the wrapper.
PEER_HEAD = "sent a message:"
PEER_TAIL = "This came from another Claude session"

# Claude Code presents everything that arrives through an inbox as "Another
# Claude session sent a message", and a prompt from the dispatcher carries no
# sender. So the receiving agent believed a peer had asked, went looking for
# one to answer, and guessed: with two sessions open in hai-proof, a prompt
# typed into the window for one was answered by SendMessage to the other,
# which replied back. This line, put in front of every dispatched prompt,
# says what the wrapper cannot. The window strips it again before showing the
# prompt (`strip_origin`). It lives here, not in inbox.py, because remote
# hosts run this file on its own and it cannot import a sibling.
ORIGIN_NOTE = (
    "[Delivered by Imperatorium, the session dispatcher on this machine - "
    "there is no sending session behind it. Reply here, in this session's own "
    "conversation, as you would to a typed prompt; do not use SendMessage to "
    "answer it.]"
)


def with_origin(content):
    """`content` with ORIGIN_NOTE in front, once."""
    if content.startswith(ORIGIN_NOTE):
        return content
    return ORIGIN_NOTE + "\n\n" + content


def strip_origin(text):
    """What was typed, without ORIGIN_NOTE."""
    text = str(text or "")
    head = text.lstrip()
    if head.startswith(ORIGIN_NOTE):
        return head[len(ORIGIN_NOTE):].lstrip()
    return text


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


def to_items(messages, limit=0, include_subagents=False):
    """Normalise SDK messages into render-ready items, oldest first.

    Each item is {speaker, role, blocks:[...]}, where a block is one of:
      {kind: "text",     text}
      {kind: "thinking", text}
      {kind: "tool",     name, summary, id}
      {kind: "result",   text, truncated, is_error, id}

    A prompt that arrived through the inbox carries `via: "inbox"`.
    limit=0, the default, keeps the whole chain, as a terminal does.
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
        note = getattr(entry, "system", None) or system_note(role, blocks)
        if note:
            blocks = [tidy_system(note, b) for b in blocks]
        if blocks:
            item = {"role": role, "speaker": SYSTEM if note else speaker_of(role, blocks),
                    "blocks": blocks}
            if note:
                item["note"] = note
            if getattr(entry, "via", None) and not note:
                item["via"] = entry.via
            items.append(item)

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


def system_note(role, blocks):
    """What Claude Code wrote this as, judged by its text; None if a person."""
    if role != "user" or not blocks or blocks[0]["kind"] != TEXT:
        return None
    head = blocks[0]["text"].lstrip()
    for prefix, note in SYSTEM_PREFIXES:
        if head.startswith(prefix):
            return note
    return None


def tidy_system(note, block):
    """The readable part of a system message: a task's one-line summary, a
    command's output without its wrapper."""
    if block["kind"] != TEXT:
        return block
    text = block["text"]
    if note == NOTE_TASK:
        found = TASK_SUMMARY.search(text)
        text = found.group(1).strip() if found else text
    elif note == NOTE_COMMAND:
        text = COMMAND_TAGS.sub("", text).strip()
    return dict(block, text=text)


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
            (item["speaker"] == prev["speaker"]
             and item.get("via") == prev.get("via")
             and item.get("note") == prev.get("note"))
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


def unwrap_peer(text):
    """The prompt as typed, without the wrapper Claude Code put round it."""
    text = str(text or "")
    first, sep, rest = text.partition("\n")
    if sep and first.rstrip().endswith(PEER_HEAD):
        text = rest
    cut = text.rfind(PEER_TAIL)
    if cut > 0:
        text = text[:cut]
    return strip_origin(text.strip()).strip()


class _Sent:
    """A peer prompt dressed as a SessionMessage, for `to_items`."""
    parent_tool_use_id = None
    parent_agent_id = None
    type = "user"
    via = "inbox"
    system = None

    def __init__(self, entry, content=None, origin=None):
        if content is None:
            content = (entry.get("message") or {}).get("content")
        if isinstance(content, list):
            content = _text_of(content)
        self.uuid = entry.get("uuid", "")
        self.message = {"role": "user", "content": unwrap_peer(content)}
        # A subagent handing back its report arrives through the same inbox,
        # but it is model output, not anything a person sent.
        origin = origin if origin is not None else entry.get("origin")
        if isinstance(origin, dict) and origin.get("senderTaskId"):
            self.system, self.via = NOTE_SUBAGENT, None


class _Queued(_Sent):
    """A prompt that arrived mid-turn.

    Claude Code folds anything that lands while a turn is running into that
    turn as a `queued_command` attachment, never as a user entry - so a prompt
    sent from the window to a busy session was in the transcript but not in
    the chat. Task notifications that land mid-turn arrive the same way.
    """

    def __init__(self, entry):
        att = entry.get("attachment") or {}
        origin = att.get("origin")
        super().__init__(entry, att.get("prompt") or "", origin)
        kind = origin.get("kind") if isinstance(origin, dict) else None
        if kind == "task-notification" or att.get("commandMode") == "task-notification":
            self.system, self.via = NOTE_TASK, None


def _is_peer_prompt(entry):
    origin = entry.get("origin")
    return (entry.get("type") == "user" and entry.get("isMeta")
            and not entry.get("isSidechain")
            and isinstance(origin, dict) and origin.get("kind") == "peer")


def _is_queued_prompt(entry):
    att = entry.get("attachment")
    return (entry.get("type") == "attachment" and not entry.get("isSidechain")
            and isinstance(att, dict) and att.get("type") == "queued_command"
            and att.get("prompt"))


def with_compacted(entries, chain):
    """`chain` with everything before each compaction put back in front.

    A compaction - including one Claude Code runs on its own while the
    session sits idle - starts a fresh chain: the `compact_boundary` entry
    has no parent, and links to what came before only through
    `logicalParentUuid`. The SDK stops there, as the terminal does, so the
    window lost the whole conversation every time a session compacted. The
    file still holds all of it; follow the link back and prepend, once per
    compaction. Nothing is duplicated: the messages a compaction keeps are
    referenced from the new chain, not rewritten into it.
    """
    by_uuid = {e["uuid"]: e for e in entries if e.get("uuid")}
    seen = {e["uuid"] for e in chain}
    while chain and chain[0].get("subtype") == "compact_boundary":
        cur = by_uuid.get(chain[0].get("logicalParentUuid") or "")
        earlier = []
        while cur is not None and cur["uuid"] not in seen:
            seen.add(cur["uuid"])
            earlier.append(cur)
            cur = by_uuid.get(cur.get("parentUuid") or "")
        if not earlier:
            break
        earlier.reverse()
        chain = earlier + chain
    return chain


def _system_origin(entry):
    """The note for an entry Claude Code wrote itself, from its metadata."""
    if entry.get("isCompactSummary"):
        return NOTE_SUMMARY
    origin = entry.get("origin")
    if isinstance(origin, dict) and origin.get("kind") == "task-notification":
        return NOTE_TASK
    return None


def _messages(sdk, session_id, directory):
    """The SDK's visible messages, plus the peer prompts it hides.

    Uses the SDK's private chain builder so the order is exactly the SDK's,
    then puts back what each compaction cut off. Any change in those internals falls back to the
    public reader: the inbox prompts go missing again, nothing else does.
    """
    try:
        from claude_agent_sdk._internal import sessions as s
        if not s._validate_uuid(session_id):
            return []
        content = s._read_session_file(session_id, directory)
        if not content:
            return []
        entries = s._parse_transcript_entries(content)
        chain = with_compacted(entries, s._build_conversation_chain(entries))
        out = []
        for entry in chain:
            if s._is_visible_message(entry):
                msg = s._to_session_message(entry)
                note = _system_origin(entry)
                if note:
                    msg.system = note
                out.append(msg)
            elif _is_peer_prompt(entry):
                out.append(_Sent(entry))
            elif _is_queued_prompt(entry):
                out.append(_Queued(entry))
        return out
    except Exception:
        kwargs = {"directory": directory} if directory else {}
        return sdk.get_session_messages(session_id, **kwargs)


def read(session_id, limit=0, include_subagents=False, directory=None):
    """Items for one session. Raises RuntimeError when the SDK is absent."""
    try:
        import claude_agent_sdk as sdk
    except Exception as exc:
        raise RuntimeError("claude-agent-sdk is not installed: %s" % exc) from exc
    messages = _messages(sdk, session_id, directory)
    return to_items(messages, limit=limit, include_subagents=include_subagents)
