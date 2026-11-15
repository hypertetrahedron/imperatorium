//! What a session actually said, ready to render: a port of
//! `imperatorium/conversation.py` (classification of the chain, `to_items`,
//! `merge_turns`, `tail_blocks`), checked against it on every transcript by
//! `rust/conformance/items_compare.py`.
//!
//! Python semantics kept where the output depends on them: truthiness,
//! `str()` of non-strings, slicing by code point, Python's whitespace for
//! `strip()`, and the key order of every item and block.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::pyish::{or, py_str, strip, take, truthy};

pub const TEXT: &str = "text";
pub const TOOL: &str = "tool";
pub const RESULT: &str = "result";
pub const THINKING: &str = "thinking";
pub const YOU: &str = "you";
pub const CLAUDE: &str = "claude";
pub const TOOLS: &str = "tool";
pub const SYSTEM: &str = "system";
pub const NOTE_TASK: &str = "task finished";
pub const NOTE_COMMAND: &str = "command output";
pub const NOTE_SUMMARY: &str = "earlier conversation, summarised";
pub const NOTE_SUBAGENT: &str = "subagent report";

const RESULT_CHARS: usize = 1200;
const INPUT_CHARS: usize = 600;
const DIFF_CHARS: usize = 3000;
const PEER_HEAD: &str = "sent a message:";
const PEER_TAIL: &str = "This came from another Claude session";
pub const ORIGIN_NOTE: &str =
    "[Delivered by Imperatorium, the session dispatcher on this machine - \
there is no sending session behind it. Reply here, in this session's own \
conversation, as you would to a typed prompt; do not use SendMessage to \
answer it.]";

/// One entry of the conversation, as conversation.py's message objects
/// (`_Message`, `_Sent`, `_Queued`) carry it to `to_items`.
#[derive(Debug, Clone)]
pub struct Message {
    pub kind: &'static str, // "user" | "assistant"
    pub message: Value,
    pub system: Option<&'static str>,
    pub via: Option<&'static str>,
}

fn s<'a>(e: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    e.get(k).and_then(Value::as_str)
}

// -- classification (conversation._native_messages) ---------------------------

fn is_visible(e: &Map<String, Value>) -> bool {
    matches!(s(e, "type"), Some("user") | Some("assistant"))
        && !truthy(e.get("isMeta"))
        && !truthy(e.get("isSidechain"))
        && !truthy(e.get("teamName"))
}

fn origin_kind(origin: Option<&Value>) -> Option<&str> {
    origin.and_then(Value::as_object).and_then(|o| s(o, "kind"))
}

fn is_peer_prompt(e: &Map<String, Value>) -> bool {
    s(e, "type") == Some("user")
        && truthy(e.get("isMeta"))
        && !truthy(e.get("isSidechain"))
        && origin_kind(e.get("origin")) == Some("peer")
}

fn is_queued_prompt(e: &Map<String, Value>) -> bool {
    let att = e.get("attachment").and_then(Value::as_object);
    s(e, "type") == Some("attachment")
        && !truthy(e.get("isSidechain"))
        && att.is_some_and(|a| s(a, "type") == Some("queued_command") && truthy(a.get("prompt")))
}

fn system_origin(e: &Map<String, Value>) -> Option<&'static str> {
    if truthy(e.get("isCompactSummary")) {
        return Some(NOTE_SUMMARY);
    }
    if origin_kind(e.get("origin")) == Some("task-notification") {
        return Some(NOTE_TASK);
    }
    None
}

pub fn strip_origin(text: &str) -> String {
    let head = text.trim_start_matches(crate::pyish::is_space);
    match head.strip_prefix(ORIGIN_NOTE) {
        Some(rest) => rest.trim_start_matches(crate::pyish::is_space).to_string(),
        None => text.to_string(),
    }
}

/// The prompt as typed, without the wrapper Claude Code put round it.
pub fn unwrap_peer(content: Option<&Value>) -> String {
    let empty = Value::String(String::new());
    let mut text = py_str(or(content, Some(&empty)).unwrap());
    if let Some((first, rest)) = text.split_once('\n') {
        if first
            .trim_end_matches(crate::pyish::is_space)
            .ends_with(PEER_HEAD)
        {
            text = rest.to_string();
        }
    }
    if let Some(cut) = text.rfind(PEER_TAIL) {
        if cut > 0 {
            text.truncate(cut);
        }
    }
    strip(&strip_origin(strip(&text))).to_string()
}

/// `_Sent(entry, content, origin)`: a prompt that arrived through the inbox.
fn sent(content: Option<&Value>, origin: Option<&Value>) -> Message {
    let content = match content {
        Some(Value::Array(_)) => Some(Value::String(text_of(content))),
        other => other.cloned(),
    };
    let message = json!({"role": "user", "content": unwrap_peer(content.as_ref())});
    let from_task = origin
        .and_then(Value::as_object)
        .is_some_and(|o| truthy(o.get("senderTaskId")));
    if from_task {
        Message {
            kind: "user",
            message,
            system: Some(NOTE_SUBAGENT),
            via: None,
        }
    } else {
        Message {
            kind: "user",
            message,
            system: None,
            via: Some("inbox"),
        }
    }
}

/// The chain (as `imperatorium sessions chain` gives it) as messages.
pub fn messages(chain: &[Value]) -> Vec<Message> {
    let mut out = Vec::new();
    for entry in chain {
        let Some(e) = entry.as_object() else { continue };
        if is_visible(e) {
            let kind = if s(e, "type") == Some("user") {
                "user"
            } else {
                "assistant"
            };
            out.push(Message {
                kind,
                message: e.get("message").cloned().unwrap_or(Value::Null),
                system: system_origin(e),
                via: None,
            });
        } else if is_peer_prompt(e) {
            let content = e
                .get("message")
                .and_then(Value::as_object)
                .and_then(|m| m.get("content"));
            out.push(sent(content, e.get("origin")));
        } else if is_queued_prompt(e) {
            let att = e["attachment"].as_object().unwrap();
            let empty = Value::String(String::new());
            let prompt = or(att.get("prompt"), Some(&empty));
            let origin = att.get("origin");
            let mut m = sent(prompt, origin);
            if origin_kind(origin) == Some("task-notification")
                || s(att, "commandMode") == Some("task-notification")
            {
                m.system = Some(NOTE_TASK);
                m.via = None;
            }
            out.push(m);
        }
    }
    out
}

// -- to_items -------------------------------------------------------------------

/// Tool results arrive as a string, a list of blocks, or neither.
pub fn text_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(t)) => t.clone(),
        Some(Value::Array(blocks)) => {
            let mut parts: Vec<String> = Vec::new();
            for b in blocks {
                match b {
                    Value::Object(o) => match s(o, "type") {
                        Some("text") => {
                            let empty = Value::String(String::new());
                            parts.push(py_str(or(o.get("text"), Some(&empty)).unwrap()));
                        }
                        Some("image") => parts.push("[image]".into()),
                        _ => {}
                    },
                    Value::Null => {}
                    other => parts.push(py_str(other)),
                }
            }
            parts
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        }
        None | Some(Value::Null) => String::new(),
        Some(other) => py_str(other),
    }
}

/// The one line that says what a tool call is about to do.
pub fn summarise_input(input: Option<&Value>) -> String {
    let Some(Value::Object(ti)) = input else {
        let empty = Value::String(String::new());
        return take(&py_str(or(input, Some(&empty)).unwrap()), INPUT_CHARS);
    };
    if let Some(Value::Array(qs)) = ti.get("questions") {
        if !qs.is_empty() {
            let parts: Vec<String> = qs
                .iter()
                .filter_map(Value::as_object)
                .map(|q| {
                    let empty = Value::String(String::new());
                    py_str(or(q.get("question"), Some(&empty)).unwrap())
                })
                .collect();
            return take(&parts.join(" / "), INPUT_CHARS);
        }
    }
    for key in [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "prompt",
        "query",
        "description",
    ] {
        if let Some(v) = ti.get(key).filter(|v| truthy(Some(v))) {
            return take(&py_str(v), INPUT_CHARS);
        }
    }
    if ti.is_empty() {
        return String::new();
    }
    let mut keys: Vec<&String> = ti.keys().collect();
    keys.sort();
    let shown: Vec<&str> = keys.iter().take(6).map(|k| k.as_str()).collect();
    take(&shown.join(", "), INPUT_CHARS)
}

/// An Edit or MultiEdit call's change as diff lines, or "".
pub fn edit_diff(name: Option<&Value>, input: Option<&Value>) -> String {
    let name = name.and_then(Value::as_str);
    if !matches!(name, Some("Edit") | Some("MultiEdit")) {
        return String::new();
    }
    let Some(Value::Object(ti)) = input else {
        return String::new();
    };
    let edits: Vec<&Value> = if name == Some("MultiEdit") {
        match ti.get("edits") {
            Some(Value::Array(a)) => a.iter().collect(),
            _ => vec![],
        }
    } else {
        vec![input.unwrap()]
    };
    let mut hunks: Vec<String> = Vec::new();
    for edit in edits {
        let Some(e) = edit.as_object() else { continue };
        let empty = Value::String(String::new());
        let old = py_str(or(e.get("old_string"), Some(&empty)).unwrap());
        let new = py_str(or(e.get("new_string"), Some(&empty)).unwrap());
        if old.is_empty() && new.is_empty() {
            continue;
        }
        let mut lines: Vec<String> = Vec::new();
        if !old.is_empty() {
            lines.extend(old.split('\n').map(|l| format!("-{l}")));
        }
        if !new.is_empty() {
            lines.extend(new.split('\n').map(|l| format!("+{l}")));
        }
        hunks.push(lines.join("\n"));
    }
    let text = hunks.join("\n@@\n");
    if text.chars().count() > DIFF_CHARS {
        let cut = take(&text, DIFF_CHARS);
        let head = match cut.rfind('\n') {
            Some(i) => cut[..i].to_string(),
            None => cut,
        };
        return format!("{head}\n\u{2026} (cut short)");
    }
    text
}

fn speaker_of(role: &Value, blocks: &[Value]) -> &'static str {
    if role.as_str() == Some("assistant") {
        return CLAUDE;
    }
    if blocks.iter().any(|b| b["kind"].as_str() != Some(RESULT)) {
        return YOU;
    }
    TOOLS
}

fn system_note(role: &Value, blocks: &[Value]) -> Option<&'static str> {
    if role.as_str() != Some("user")
        || blocks.is_empty()
        || blocks[0]["kind"].as_str() != Some(TEXT)
    {
        return None;
    }
    let head = blocks[0]["text"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches(crate::pyish::is_space);
    for (prefix, note) in [
        ("<task-notification>", NOTE_TASK),
        ("<local-command-stdout>", NOTE_COMMAND),
        ("<local-command-stderr>", NOTE_COMMAND),
    ] {
        if head.starts_with(prefix) {
            return Some(note);
        }
    }
    None
}

fn tidy_system(note: &str, block: Value) -> Value {
    static RE: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (summary, tags) = RE.get_or_init(|| {
        (
            Regex::new(r"(?s)<summary>(.*?)</summary>").unwrap(),
            Regex::new(r"</?local-command-std(?:out|err)>").unwrap(),
        )
    });
    if block["kind"].as_str() != Some(TEXT) {
        return block;
    }
    let text = block["text"].as_str().unwrap_or("").to_string();
    let text = match note {
        NOTE_TASK => match summary.captures(&text) {
            Some(c) => strip(&c[1]).to_string(),
            None => text,
        },
        NOTE_COMMAND => strip(&tags.replace_all(&text, "")).to_string(),
        _ => text,
    };
    let mut b = block.as_object().unwrap().clone();
    b.insert("text".into(), Value::String(text));
    Value::Object(b)
}

/// `to_items(messages, limit, include_subagents=False, merge)`.
pub fn to_items(messages: &[Message], limit: usize, merge: bool) -> Vec<Value> {
    let mut items: Vec<Value> = Vec::new();
    for entry in messages {
        let Some(raw) = entry.message.as_object() else {
            continue;
        };
        let fallback = Value::String(entry.kind.to_string());
        let role = or(raw.get("role"), Some(&fallback)).unwrap().clone();
        let content: Vec<Value> = match raw.get("content") {
            Some(Value::String(t)) => vec![json!({"type": "text", "text": t})],
            Some(Value::Array(a)) => a.clone(),
            _ => continue,
        };
        let mut blocks: Vec<Value> = Vec::new();
        for block in &content {
            let Some(b) = block.as_object() else { continue };
            let empty = Value::String(String::new());
            match s(b, "type") {
                Some("text") => {
                    let t = py_str(or(b.get("text"), Some(&empty)).unwrap());
                    let t = strip(&t);
                    if !t.is_empty() {
                        blocks.push(json!({"kind": TEXT, "text": t}));
                    }
                }
                Some("thinking") => {
                    let t = py_str(or(b.get("thinking"), Some(&empty)).unwrap());
                    let t = strip(&t);
                    if !t.is_empty() {
                        blocks.push(json!({"kind": THINKING, "text": t}));
                    }
                }
                Some("tool_use") => {
                    let tool_name = Value::String("tool".into());
                    let mut tool = Map::new();
                    tool.insert("kind".into(), json!(TOOL));
                    tool.insert(
                        "name".into(),
                        or(b.get("name"), Some(&tool_name)).unwrap().clone(),
                    );
                    tool.insert("summary".into(), json!(summarise_input(b.get("input"))));
                    tool.insert("id".into(), b.get("id").cloned().unwrap_or(Value::Null));
                    let diff = edit_diff(b.get("name"), b.get("input"));
                    if !diff.is_empty() {
                        tool.insert("diff".into(), json!(diff));
                    }
                    if b.get("name").and_then(Value::as_str) == Some(crate::pending::QUESTION_TOOL) {
                        // The choices, so the chat shows what the terminal asks
                        // even after the window's own offer of it has expired.
                        let mut req = Map::new();
                        req.insert("tool_input".into(), b.get("input").cloned().unwrap_or(Value::Null));
                        if let Some(qs) = crate::pending::questions(&req) {
                            tool.insert("questions".into(), Value::Array(qs));
                        }
                    }
                    blocks.push(Value::Object(tool));
                }
                Some("tool_result") => {
                    let t = text_of(b.get("content"));
                    blocks.push(json!({
                        "kind": RESULT,
                        "text": take(&t, RESULT_CHARS),
                        "truncated": t.chars().count() > RESULT_CHARS,
                        "is_error": truthy(b.get("is_error")),
                        "id": b.get("tool_use_id").cloned().unwrap_or(Value::Null),
                    }));
                }
                _ => {}
            }
        }
        let note = entry.system.or_else(|| system_note(&role, &blocks));
        if let Some(n) = note {
            blocks = blocks.into_iter().map(|b| tidy_system(n, b)).collect();
        }
        if blocks.is_empty() {
            continue;
        }
        let speaker = if note.is_some() {
            SYSTEM
        } else {
            speaker_of(&role, &blocks)
        };
        let mut item = Map::new();
        item.insert("role".into(), role);
        item.insert("speaker".into(), json!(speaker));
        item.insert("blocks".into(), Value::Array(blocks));
        if let Some(n) = note {
            item.insert("note".into(), json!(n));
        }
        if let (Some(v), None) = (entry.via, note) {
            item.insert("via".into(), json!(v));
        }
        items.push(Value::Object(item));
    }
    if limit > 0 && items.len() > limit {
        items.drain(..items.len() - limit);
    }
    if merge {
        merge_turns(items)
    } else {
        items
    }
}

/// One item per turn, so the window prints one name per turn.
pub fn merge_turns(items: Vec<Value>) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();
    for item in items {
        let joins = match merged.last() {
            Some(prev) => {
                (item["speaker"] == prev["speaker"]
                    && item.get("via") == prev.get("via")
                    && item.get("note") == prev.get("note"))
                    || (item["speaker"] == TOOLS && prev["speaker"] == CLAUDE)
            }
            None => false,
        };
        if joins {
            let extra = item["blocks"].as_array().cloned().unwrap_or_default();
            merged.last_mut().unwrap()["blocks"]
                .as_array_mut()
                .unwrap()
                .extend(extra);
        } else {
            merged.push(item);
        }
    }
    merged
}

/// merge_turns of borrowed items: each item copied once, into its place.
pub fn merge_turns_ref(items: &[Value]) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();
    for item in items {
        let joins = match merged.last() {
            Some(prev) => {
                (item["speaker"] == prev["speaker"]
                    && item.get("via") == prev.get("via")
                    && item.get("note") == prev.get("note"))
                    || (item["speaker"] == TOOLS && prev["speaker"] == CLAUDE)
            }
            None => false,
        };
        let into = if joins {
            merged
                .last_mut()
                .and_then(|m| m.get_mut("blocks"))
                .and_then(Value::as_array_mut)
        } else {
            None
        };
        match into {
            Some(blocks) => {
                if let Some(extra) = item["blocks"].as_array() {
                    blocks.extend(extra.iter().cloned());
                }
            }
            None if joins => {} // a previous item without blocks (merge_turns would panic)
            None => merged.push(item.clone()),
        }
    }
    merged
}

/// The newest `blocks` blocks of merged items: (items, start, first_block).
pub fn tail_blocks(items: &[Value], blocks: i64) -> (Vec<Value>, usize, usize) {
    if blocks <= 0 {
        return (items.to_vec(), 0, 0);
    }
    let mut left = blocks;
    let mut i = items.len();
    while i > 0 && left > 0 {
        i -= 1;
        let n = items[i]["blocks"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0)
            .max(1) as i64;
        left -= n;
    }
    let mut out: Vec<Value> = items[i..].to_vec();
    let mut first = 0usize;
    if !out.is_empty() && left < 0 {
        first = (-left) as usize;
        let bl = out[0]["blocks"].as_array().cloned().unwrap_or_default();
        out[0]["blocks"] = Value::Array(bl.into_iter().skip(first).collect());
    }
    (out, i, first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merging_borrowed_items_matches_merge_turns() {
        let items = vec![
            json!({"speaker": "you", "blocks": [{"kind": "text", "text": "a"}]}),
            json!({"speaker": CLAUDE, "blocks": [{"kind": "text", "text": "b"}]}),
            json!({"speaker": TOOLS, "blocks": [{"kind": "tool", "name": "Bash"}]}),
            json!({"speaker": CLAUDE, "blocks": [{"kind": "text", "text": "c"}]}),
            json!({"speaker": "you", "via": "peer", "blocks": [{"kind": "text", "text": "d"}]}),
            json!({"speaker": "you", "blocks": [{"kind": "text", "text": "e"}]}),
            json!({"speaker": "you", "blocks": [{"kind": "text", "text": "f"}]}),
        ];
        assert_eq!(merge_turns_ref(&items), merge_turns(items.clone()));
        assert_eq!(merge_turns_ref(&items).len(), 4);
    }

    #[test]
    fn peer_wrapper_is_removed() {
        let wrapped = format!(
            "Another Claude session sent a message:\n{ORIGIN_NOTE}\n\nhello there\n\n{PEER_TAIL} ..."
        );
        assert_eq!(unwrap_peer(Some(&json!(wrapped))), "hello there");
    }

    #[test]
    fn tool_results_are_the_machine_talking() {
        let m = Message {
            kind: "user",
            message: json!({"role": "user", "content": [{"type": "tool_result", "content": "ok", "tool_use_id": "t"}]}),
            system: None,
            via: None,
        };
        let items = to_items(&[m], 0, true);
        assert_eq!(items[0]["speaker"], "tool");
    }

    #[test]
    fn a_task_notification_keeps_its_summary() {
        let m = Message {
            kind: "user",
            message: json!({"role": "user", "content": "<task-notification><summary> built ok </summary></task-notification>"}),
            system: None,
            via: None,
        };
        let items = to_items(&[m], 0, true);
        assert_eq!(items[0]["note"], NOTE_TASK);
        assert_eq!(items[0]["blocks"][0]["text"], "built ok");
    }

    #[test]
    fn long_diffs_are_cut_on_a_line() {
        let input = json!({"old_string": "a\n".repeat(2000), "new_string": "b"});
        let d = edit_diff(Some(&json!("Edit")), Some(&input));
        assert!(d.ends_with("\n\u{2026} (cut short)"));
        assert!(d.chars().count() <= DIFF_CHARS + 20);
    }

    #[test]
    fn tail_blocks_matches_python() {
        let items: Vec<Value> = (0..3).map(|i| json!({"blocks": [i, i, i]})).collect();
        let (out, start, first) = tail_blocks(&items, 4);
        assert_eq!((start, first, out.len()), (1, 2, 2));
        assert_eq!(out[0]["blocks"], json!([1]));
    }
}

/// conversation.with_origin: `content` with ORIGIN_NOTE in front, once.
pub fn with_origin(content: &str) -> String {
    if content.starts_with(ORIGIN_NOTE) {
        return content.to_string();
    }
    format!("{ORIGIN_NOTE}\n\n{content}")
}

/// test_native.py: the chain is classified as before.
#[cfg(test)]
mod core_tests {
    use super::*;

    #[test]
    fn the_chain_is_classified() {
        let chain = vec![
            json!({"type": "user", "uuid": "1", "sessionId": "s",
                   "message": {"role": "user", "content": "hello"}}),
            json!({"type": "assistant", "uuid": "2", "sessionId": "s",
                   "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}]}}),
            json!({"type": "user", "uuid": "3", "isMeta": true, "origin": {"kind": "peer"},
                   "message": {"role": "user", "content": "from the window"}}),
            json!({"type": "attachment", "uuid": "4",
                   "attachment": {"type": "queued_command", "prompt": "queued one"}}),
            json!({"type": "user", "uuid": "5", "isMeta": true,
                   "message": {"role": "user", "content": "hidden meta"}}),
            json!({"type": "user", "uuid": "6", "isCompactSummary": true,
                   "message": {"role": "user", "content": "summary"}}),
        ];
        let msgs = messages(&chain);
        // uuids 1, 2, 3, 4 and 6: the plain meta entry (5) is hidden
        assert_eq!(msgs.len(), 5);
        let texts: Vec<String> = msgs
            .iter()
            .map(|m| text_of(m.message.get("content")))
            .collect();
        assert_eq!(texts, ["hello", "hi", "from the window", "queued one", "summary"]);
        assert_eq!(msgs[2].via, Some("inbox"));
        assert_eq!(msgs[3].via, Some("inbox"));
        assert_eq!(msgs[4].system, Some(NOTE_SUMMARY));
        assert!(msgs[..4].iter().all(|m| m.system.is_none()));
        assert!(!to_items(&msgs, 0, false).is_empty());
    }
}
