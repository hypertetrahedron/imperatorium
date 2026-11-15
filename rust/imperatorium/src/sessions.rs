//! Reading Claude Code's session transcripts: a port of the parts of the
//! Agent SDK (`claude_agent_sdk._internal.sessions`, 0.2.155) Imperatorium
//! used, so the 226 MB SDK is no longer needed to show a conversation.
//!
//!     imperatorium sessions chain <id>   # the conversation chain, compactions put back
//!     imperatorium sessions info <id>    # get_session_info, or null
//!     imperatorium sessions list         # list_sessions() across every project
//!
//! `chain` is `_build_conversation_chain` followed by Imperatorium's own
//! `conversation.with_compacted`; Python then classifies each entry exactly as
//! before. Each entry is cut down to the keys that classification and
//! `to_items` read. `rust/conformance/sessions_compare.py` checks all three
//! against the SDK on every transcript on the machine.
//!
//! Only the no-directory forms are ported: that is all Imperatorium calls.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::locks::LockExt;
use crate::pyish::{py_regex, strip, truthy};

const LITE_READ_BUF_SIZE: u64 = 65536;
const ENTRY_TYPES: &[&str] = &["user", "assistant", "progress", "system", "attachment"];
/// What `chain` keeps of each entry: what conversation.py's classification
/// (`_is_visible_message`, `_is_peer_prompt`, `_is_queued_prompt`,
/// `_system_origin`, `_Sent`, `_Queued`) and `to_items` read.
const KEEP: &[&str] = &[
    "type",
    "uuid",
    "sessionId",
    "message",
    "isMeta",
    "isSidechain",
    "teamName",
    "isCompactSummary",
    "origin",
    "attachment",
    "subtype",
];

pub fn valid_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    b.iter().enumerate().all(|(i, c)| match i {
        8 | 13 | 18 | 23 => *c == b'-',
        _ => c.is_ascii_hexdigit(),
    })
}

fn dirents(dir: &Path) -> Vec<PathBuf> {
    match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(_) => vec![],
    }
}

// -- chain --------------------------------------------------------------------

/// `_read_session_file(id, None)`: the first non-empty file of that name in
/// any project folder. Err for a file that is not UTF-8 (Python raises).
fn read_session_file(session_id: &str) -> Result<Option<String>, String> {
    let name = format!("{session_id}.jsonl");
    for dir in dirents(&crate::paths::projects_dir()) {
        let Ok(bytes) = fs::read(dir.join(&name)) else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }
        return String::from_utf8(bytes)
            .map(Some)
            .map_err(|e| format!("{}: not UTF-8: {e}", dir.join(&name).display()));
    }
    Ok(None)
}

/// `_parse_transcript_entries`: transcript-type records with a string uuid.
pub fn parse_entries(content: &str) -> Vec<Map<String, Value>> {
    let mut out = Vec::new();
    // Python's read_text turns "\r\n" and a lone "\r" into "\n". Neither can
    // occur raw inside a JSON string, so this only ever splits records.
    let content = content.replace("\r\n", "\n").replace('\r', "\n");
    for line in content.split('\n') {
        let line = strip(line);
        if line.is_empty() {
            continue;
        }
        let Ok(Value::Object(entry)) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let typed = entry
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| ENTRY_TYPES.contains(&t));
        if typed && entry.get("uuid").is_some_and(Value::is_string) {
            out.push(entry);
        }
    }
    out
}

fn uuid_of(e: &Map<String, Value>) -> &str {
    e.get("uuid").and_then(Value::as_str).unwrap_or("")
}

/// `entry.get("parentUuid")` when truthy and a string (a non-string parent is
/// never found in the index, in Python either).
fn parent_of(e: &Map<String, Value>) -> Option<&str> {
    e.get("parentUuid")
        .filter(|p| truthy(Some(p)))
        .and_then(Value::as_str)
}

/// `_build_conversation_chain`: from the most recent main-chain leaf back to
/// the root, in chronological order. Indices into `entries`.
pub fn build_chain(entries: &[Map<String, Value>]) -> Vec<usize> {
    if entries.is_empty() {
        return vec![];
    }
    let mut by_uuid: HashMap<&str, usize> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        by_uuid.insert(uuid_of(e), i); // the last one wins, as in a dict
    }
    let parents: HashSet<&str> = entries.iter().filter_map(parent_of).collect();
    let mut leaves: Vec<usize> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        if parents.contains(uuid_of(e)) {
            continue;
        }
        let mut cur = Some(i);
        let mut seen: HashSet<&str> = HashSet::new();
        while let Some(c) = cur {
            let uid = uuid_of(&entries[c]);
            if !seen.insert(uid) {
                break;
            }
            let t = entries[c].get("type").and_then(Value::as_str);
            if matches!(t, Some("user") | Some("assistant")) {
                leaves.push(c);
                break;
            }
            cur = parent_of(&entries[c]).and_then(|p| by_uuid.get(p).copied());
        }
    }
    if leaves.is_empty() {
        return vec![];
    }
    let main: Vec<usize> = leaves
        .iter()
        .copied()
        .filter(|&l| {
            let e = &entries[l];
            !truthy(e.get("isSidechain")) && !truthy(e.get("teamName")) && !truthy(e.get("isMeta"))
        })
        .collect();
    // _pick_best: the highest file position, by the uuid's (last) index; the
    // first candidate wins a tie.
    let pick = |cands: &[usize]| -> usize {
        let mut best = cands[0];
        let mut best_idx = by_uuid[uuid_of(&entries[best])];
        for &c in &cands[1..] {
            let idx = by_uuid[uuid_of(&entries[c])];
            if idx > best_idx {
                best = c;
                best_idx = idx;
            }
        }
        best
    };
    let leaf = if main.is_empty() {
        pick(&leaves)
    } else {
        pick(&main)
    };
    let mut chain = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut cur = Some(leaf);
    while let Some(c) = cur {
        if !seen.insert(uuid_of(&entries[c])) {
            break;
        }
        chain.push(c);
        cur = parent_of(&entries[c]).and_then(|p| by_uuid.get(p).copied());
    }
    chain.reverse();
    chain
}

/// conversation.with_compacted: everything before each compaction put back
/// in front, by following `logicalParentUuid`.
pub fn with_compacted(entries: &[Map<String, Value>], mut chain: Vec<usize>) -> Vec<usize> {
    let mut by_uuid: HashMap<&str, usize> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        if let Some(u) = e
            .get("uuid")
            .filter(|u| truthy(Some(u)))
            .and_then(Value::as_str)
        {
            by_uuid.insert(u, i);
        }
    }
    let mut seen: HashSet<&str> = chain.iter().map(|&i| uuid_of(&entries[i])).collect();
    while let Some(&first) = chain.first() {
        if entries[first].get("subtype").and_then(Value::as_str) != Some("compact_boundary") {
            break;
        }
        let link = entries[first]
            .get("logicalParentUuid")
            .filter(|v| truthy(Some(v)));
        let mut cur = link
            .and_then(Value::as_str)
            .and_then(|l| by_uuid.get(l).copied());
        let mut earlier = Vec::new();
        while let Some(c) = cur {
            let uid = uuid_of(&entries[c]);
            if seen.contains(uid) {
                break;
            }
            seen.insert(uid);
            earlier.push(c);
            cur = parent_of(&entries[c]).and_then(|p| by_uuid.get(p).copied());
        }
        if earlier.is_empty() {
            break;
        }
        earlier.reverse();
        earlier.extend(chain);
        chain = earlier;
    }
    chain
}

/// The conversation chain of a transcript's entries, each cut to KEEP: what
/// `chain_json` serves for a session, and taskout for a subagent.
pub fn chain_values(entries: &[Map<String, Value>]) -> Vec<Value> {
    let chain = with_compacted(entries, build_chain(entries));
    chain
        .into_iter()
        .map(|i| {
            let e = &entries[i];
            let kept: Map<String, Value> = KEEP
                .iter()
                .filter_map(|k| e.get(*k).map(|v| ((*k).to_string(), v.clone())))
                .collect();
            Value::Object(kept)
        })
        .collect()
}

pub fn chain_json(session_id: &str) -> Result<Value, String> {
    if !valid_uuid(session_id) {
        return Ok(json!([]));
    }
    let Some(content) = read_session_file(session_id)? else {
        return Ok(json!([]));
    };
    let entries = parse_entries(&content);
    let chain = with_compacted(&entries, build_chain(&entries));
    let out: Vec<Value> = chain
        .into_iter()
        .map(|i| {
            let e = &entries[i];
            let kept: Map<String, Value> = KEEP
                .iter()
                .filter_map(|k| e.get(*k).map(|v| ((*k).to_string(), v.clone())))
                .collect();
            Value::Object(kept)
        })
        .collect();
    Ok(Value::Array(out))
}

// -- info and list ------------------------------------------------------------

struct Lite {
    mtime: i64,
    size: u64,
    head: String,
    tail: String,
}

/// `_read_session_lite`: stat, the first and last 64 KiB, decoded leniently.
fn read_lite(path: &Path) -> Option<Lite> {
    let mut f = fs::File::open(path).ok()?;
    let meta = f.metadata().ok()?;
    let size = meta.len();
    let mtime = meta.modified().ok().map(mtime_ms).unwrap_or(0);
    let mut head = vec![0u8; LITE_READ_BUF_SIZE as usize];
    let n = read_full(&mut f, &mut head)?;
    if n == 0 {
        return None;
    }
    head.truncate(n);
    let head = String::from_utf8_lossy(&head).into_owned();
    let offset = size.saturating_sub(LITE_READ_BUF_SIZE);
    let tail = if offset == 0 {
        head.clone()
    } else {
        f.seek(SeekFrom::Start(offset)).ok()?;
        let mut buf = vec![0u8; LITE_READ_BUF_SIZE as usize];
        let n = read_full(&mut f, &mut buf)?;
        buf.truncate(n);
        String::from_utf8_lossy(&buf).into_owned()
    };
    Some(Lite {
        mtime,
        size,
        head,
        tail,
    })
}

fn read_full(f: &mut fs::File, buf: &mut [u8]) -> Option<usize> {
    let mut got = 0;
    while got < buf.len() {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(_) => return None,
        }
    }
    Some(got)
}

/// `int(st_mtime * 1000)`, with st_mtime built as Python builds it.
fn mtime_ms(t: std::time::SystemTime) -> i64 {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => ((d.as_secs() as f64 + d.subsec_nanos() as f64 * 1e-9) * 1000.0) as i64,
        Err(e) => {
            let d = e.duration();
            ((-(d.as_secs() as f64) - d.subsec_nanos() as f64 * 1e-9) * 1000.0) as i64
        }
    }
}

/// `_unescape_json_string`.
fn unescape(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_string();
    }
    match serde_json::from_str::<Value>(&format!("\"{raw}\"")) {
        Ok(Value::String(s)) => s,
        _ => raw.to_string(),
    }
}

/// The value after one occurrence of `pattern` at `idx`, and where the scan
/// stopped (the closing quote, or the end).
fn value_at(text: &str, value_start: usize) -> (Option<String>, usize) {
    let b = text.as_bytes();
    let mut i = value_start;
    while i < b.len() {
        if b[i] == b'\\' {
            // Python skips one character; skipping one byte and the next
            // character's lead byte finds the same closing quote.
            i += 2;
            continue;
        }
        if b[i] == b'"' {
            return (Some(unescape(&text[value_start..i])), i);
        }
        i += 1;
    }
    (None, i)
}

/// `_extract_json_string_field`: the first `"key":"value"`.
fn first_field(text: &str, key: &str) -> Option<String> {
    for pattern in [format!("\"{key}\":\""), format!("\"{key}\": \"")] {
        if let Some(idx) = text.find(&pattern) {
            if let (Some(v), _) = value_at(text, idx + pattern.len()) {
                return Some(v);
            }
        }
    }
    None
}

/// `_extract_last_json_string_field`: the last one, compact form first.
fn last_field(text: &str, key: &str) -> Option<String> {
    let mut last = None;
    for pattern in [format!("\"{key}\":\""), format!("\"{key}\": \"")] {
        let mut from = 0;
        while from <= text.len() {
            let Some(rel) = text.get(from..).and_then(|t| t.find(&pattern)) else {
                break;
            };
            let (v, stop) = value_at(text, from + rel + pattern.len());
            if v.is_some() {
                last = v;
            }
            from = stop + 1;
            while from < text.len() && !text.is_char_boundary(from) {
                from += 1;
            }
        }
    }
    last
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty())
}

fn first_prompt_from_head(head: &str) -> String {
    static RES: std::sync::OnceLock<(Regex, Regex)> = std::sync::OnceLock::new();
    let (skip, command) = RES.get_or_init(|| {
        let skip = py_regex(concat!(
            r"^(?:<local-command-stdout>|<session-start-hook>|<tick>|<goal>|",
            r"\[Request interrupted by user[^\]]*\]|",
            r"\s*<ide_opened_file>[\s\S]*</ide_opened_file>\s*$|",
            r"\s*<ide_selection>[\s\S]*</ide_selection>\s*$)"
        ));
        (
            Regex::new(&skip).unwrap(),
            Regex::new(r"<command-name>(.*?)</command-name>").unwrap(),
        )
    });
    let mut fallback = String::new();
    for line in head.split('\n') {
        if !line.contains("\"type\":\"user\"") && !line.contains("\"type\": \"user\"") {
            continue;
        }
        if line.contains("\"tool_result\"")
            || line.contains("\"isMeta\":true")
            || line.contains("\"isMeta\": true")
            || line.contains("\"isCompactSummary\":true")
            || line.contains("\"isCompactSummary\": true")
        {
            continue;
        }
        let Ok(Value::Object(entry)) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if entry.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Some(Value::Object(message)) = entry.get("message") else {
            continue;
        };
        let mut texts: Vec<&str> = Vec::new();
        match message.get("content") {
            Some(Value::String(s)) => texts.push(s),
            Some(Value::Array(blocks)) => {
                for b in blocks {
                    if b.get("type").and_then(Value::as_str) == Some("text") {
                        if let Some(t) = b.get("text").and_then(Value::as_str) {
                            texts.push(t);
                        }
                    }
                }
            }
            _ => {}
        }
        for raw in texts {
            let replaced = raw.replace('\n', " ");
            let result = strip(&replaced);
            if result.is_empty() {
                continue;
            }
            if let Some(m) = command.captures(result) {
                if fallback.is_empty() {
                    fallback = m[1].to_string();
                }
                continue;
            }
            if skip.is_match(result) {
                continue;
            }
            if result.chars().count() > 200 {
                let cut: String = result.chars().take(200).collect();
                return format!("{}\u{2026}", cut.trim_end_matches(crate::pyish::is_space));
            }
            return result.to_string();
        }
    }
    fallback
}

/// `datetime.fromisoformat(ts).timestamp() * 1000`, truncated, for the
/// forms Claude Code writes (`2026-11-12T19:16:41.123Z`, or an offset).
fn iso_ms(ts: &str) -> Option<i64> {
    iso_seconds(ts).map(|secs| (secs * 1000.0) as i64)
}

/// `datetime.fromisoformat(ts).timestamp()` for an aware timestamp (a
/// trailing Z read as +00:00): seconds, as Python computes them.
pub fn iso_seconds(ts: &str) -> Option<f64> {
    iso_seconds_as(ts, false)
}

/// As `iso_seconds`, but with `naive_utc` a timestamp without an offset is
/// read as UTC (costs._when: "naive means UTC here").
pub fn iso_seconds_as(ts: &str, naive_utc: bool) -> Option<f64> {
    let has_offset = {
        let t = ts.strip_suffix('Z').unwrap_or(ts);
        ts.ends_with('Z')
            || t.split_once(['T', ' '])
                .is_some_and(|(_, rest)| rest.contains(['+', '-']))
    };
    if !has_offset && naive_utc {
        return iso_seconds_as(&format!("{ts}+00:00"), false);
    }
    iso_parse(ts)
}

fn iso_parse(ts: &str) -> Option<f64> {
    let ts = ts
        .strip_suffix('Z')
        .map(|t| format!("{t}+00:00"))
        .unwrap_or_else(|| ts.to_string());
    let (date, rest) = ts.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let (y, mo, da): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    if d.next().is_some() || date.len() != 10 {
        return None;
    }
    // A naive timestamp would be local time in Python; Claude Code never
    // writes one, so it gives no created_at here.
    let at = rest.rfind(['+', '-'])?;
    let (time, off) = rest.split_at(at);
    let sign = if off.starts_with('-') { -1 } else { 1 };
    let mut parts = off[1..].split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next().unwrap_or("0").parse().ok()?;
    let offset_min = sign * (h * 60 + m);
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut t = hms.split(':');
    let (h, mi): (i64, i64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?);
    let s: i64 = t.next().unwrap_or("0").parse().ok()?;
    if frac.len() > 6 || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let us: i64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<6}").parse().ok()?
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let (yy, mm) = if mo <= 2 {
        (y - 1, mo + 9)
    } else {
        (y, mo - 3)
    };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let total_us = ((days * 86_400 + h * 3600 + mi * 60 + s - offset_min * 60) * 1_000_000) + us;
    // timedelta.total_seconds() is int / 10**6 (correctly rounded), then * 1000.
    Some(total_us as f64 / 1e6)
}

/// `_parse_session_info_from_lite`, as JSON (SDKSessionInfo's fields).
fn info_from_lite(session_id: &str, lite: &Lite) -> Option<Value> {
    let (head, tail) = (&lite.head, &lite.tail);
    let first_line = head.split('\n').next().unwrap_or("");
    if first_line.contains("\"isSidechain\":true") || first_line.contains("\"isSidechain\": true") {
        return None;
    }
    let custom_title = non_empty(last_field(tail, "customTitle"))
        .or_else(|| non_empty(last_field(head, "customTitle")))
        .or_else(|| non_empty(last_field(tail, "aiTitle")))
        .or_else(|| non_empty(last_field(head, "aiTitle")));
    let first_prompt = non_empty(Some(first_prompt_from_head(head)));
    let summary = custom_title
        .clone()
        .or_else(|| non_empty(last_field(tail, "lastPrompt")))
        .or_else(|| non_empty(last_field(tail, "summary")))
        .or_else(|| first_prompt.clone())?;
    let git_branch = non_empty(last_field(tail, "gitBranch"))
        .or_else(|| non_empty(first_field(head, "gitBranch")));
    let cwd = non_empty(first_field(head, "cwd"));
    let tag_line = tail
        .split('\n')
        .rev()
        .find(|l| l.starts_with("{\"type\":\"tag\""));
    let tag = tag_line.and_then(|l| non_empty(last_field(l, "tag")));
    let created_at = first_field(head, "timestamp").and_then(|t| iso_ms(&t));
    Some(json!({
        "session_id": session_id, "summary": summary, "last_modified": lite.mtime,
        "file_size": lite.size, "custom_title": custom_title, "first_prompt": first_prompt,
        "git_branch": git_branch, "cwd": cwd, "tag": tag, "created_at": created_at,
    }))
}

/// `get_session_info(id)`: the first project folder holding the file.
pub fn info_json(session_id: &str) -> Value {
    if !valid_uuid(session_id) {
        return Value::Null;
    }
    let name = format!("{session_id}.jsonl");
    for dir in dirents(&crate::paths::projects_dir()).into_iter().filter(|d| d.is_dir()) {
        if let Some(lite) = read_lite(&dir.join(&name)) {
            return info_from_lite(session_id, &lite).unwrap_or(Value::Null);
        }
    }
    Value::Null
}

/// `{id: title}` for many sessions in one call: what conversation.title()
/// returns for each (custom title, else summary, else ""). The dispatcher
/// asks for every tile's title on each snapshot, and one process per tile
/// would cost more than the read.
///
/// The project folders are listed once per call (info_json lists them per
/// id), and a title is kept per file and read again only when the file's
/// size or mtime moves. Which file answers is info_json's: the first folder
/// holding a readable, non-empty one.
pub fn titles_json(ids: &[String]) -> Value {
    type Seen = HashMap<PathBuf, (u64, Option<std::time::SystemTime>, String)>;
    static SEEN: std::sync::OnceLock<std::sync::Mutex<Seen>> = std::sync::OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    let dirs: Vec<PathBuf> = dirents(&crate::paths::projects_dir())
        .into_iter()
        .filter(|d| d.is_dir())
        .collect();
    let mut out = Map::new();
    for id in ids {
        let mut title = String::new();
        if valid_uuid(id) {
            let name = format!("{id}.jsonl");
            for dir in &dirs {
                let path = dir.join(&name);
                let Ok(meta) = fs::metadata(&path) else {
                    continue;
                };
                let stamp = (meta.len(), meta.modified().ok());
                let known = seen
                    .lk()
                    .get(&path)
                    .filter(|(size, mtime, _)| (*size, *mtime) == stamp)
                    .map(|(_, _, t)| t.clone());
                if let Some(t) = known {
                    title = t;
                    break;
                }
                // read_lite is None for a file that is empty or unreadable:
                // info_json goes on to the next folder then.
                let Some(lite) = read_lite(&path) else {
                    continue;
                };
                let info = info_from_lite(id, &lite).unwrap_or(Value::Null);
                title = title_of(&info);
                let mut s = seen.lk();
                if s.len() >= 4096 {
                    s.clear();
                }
                s.insert(path, (stamp.0, stamp.1, title.clone()));
                break;
            }
        }
        out.insert(id.clone(), Value::String(title));
    }
    Value::Object(out)
}

/// conversation.title() of a session's info: custom title, else summary.
fn title_of(info: &Value) -> String {
    info.get("custom_title")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .or_else(|| info.get("summary").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

/// `list_sessions()`: every project, newest first, one row per session id.
pub fn list_json() -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for dir in dirents(&crate::paths::projects_dir()).into_iter().filter(|d| d.is_dir()) {
        for file in dirents(&dir) {
            let Some(name) = file.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(id) = name.strip_suffix(".jsonl") else {
                continue;
            };
            if !valid_uuid(id) {
                continue;
            }
            let Some(lite) = read_lite(&file) else {
                continue;
            };
            let Some(info) = info_from_lite(id, &lite) else {
                continue;
            };
            // _deduplicate_by_session_id: the newest wins, in the first one's place.
            match index.get(id) {
                Some(&at) => {
                    if info["last_modified"].as_i64() > rows[at]["last_modified"].as_i64() {
                        rows[at] = info;
                    }
                }
                None => {
                    index.insert(id.to_string(), rows.len());
                    rows.push(info);
                }
            }
        }
    }
    rows.sort_by(|a, b| {
        b["last_modified"]
            .as_i64()
            .cmp(&a["last_modified"].as_i64())
    });
    Value::Array(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn uuids() {
        assert!(valid_uuid("0d4f1c2a-1b2c-4d5e-8f90-a1b2c3d4e5f6"));
        assert!(valid_uuid("0D4F1C2A-1B2C-4D5E-8F90-A1B2C3D4E5F6"));
        assert!(!valid_uuid("0d4f1c2a-1b2c-4d5e-8f90-a1b2c3d4e5f"));
        assert!(!valid_uuid("../../etc/passwd-0000-0000-0000-000000"));
    }

    #[test]
    fn chain_follows_the_newest_main_leaf() {
        let entries = vec![
            e(json!({"type": "user", "uuid": "a"})),
            e(json!({"type": "assistant", "uuid": "b", "parentUuid": "a"})),
            e(json!({"type": "user", "uuid": "c", "parentUuid": "a", "isSidechain": true})),
            e(json!({"type": "progress", "uuid": "d", "parentUuid": "b"})),
        ];
        let chain: Vec<&str> = build_chain(&entries)
            .iter()
            .map(|&i| uuid_of(&entries[i]))
            .collect();
        assert_eq!(chain, vec!["a", "b"]);
    }

    #[test]
    fn compactions_are_put_back() {
        let entries = vec![
            e(json!({"type": "user", "uuid": "a"})),
            e(json!({"type": "assistant", "uuid": "b", "parentUuid": "a"})),
            e(
                json!({"type": "system", "uuid": "c", "subtype": "compact_boundary", "logicalParentUuid": "b"}),
            ),
            e(json!({"type": "user", "uuid": "d", "parentUuid": "c"})),
        ];
        let chain = with_compacted(&entries, build_chain(&entries));
        let ids: Vec<&str> = chain.iter().map(|&i| uuid_of(&entries[i])).collect();
        assert_eq!(ids, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn fields_without_parsing() {
        let t = r#"{"cwd":"C:\\w","aiTitle":"one"} {"aiTitle": "two \"q\""}"#;
        assert_eq!(first_field(t, "cwd").unwrap(), "C:\\w");
        // Both forms are scanned, compact first, so a spaced match wins.
        assert_eq!(last_field(t, "aiTitle").unwrap(), r#"two "q""#);
        assert_eq!(
            last_field(r#"{"aiTitle":"one"}{"aiTitle":"two"}"#, "aiTitle").unwrap(),
            "two"
        );
        assert_eq!(
            last_field(r#"{"aiTitle": "two \"q\""}"#, "aiTitle").unwrap(),
            "two \"q\""
        );
        assert!(last_field("{\"aiTitle\":\"unterminated", "aiTitle").is_none());
    }

    #[test]
    fn iso_timestamps() {
        assert_eq!(iso_ms("1970-01-01T00:00:01.5Z"), Some(1500));
        assert_eq!(iso_ms("2026-11-12T19:16:41.123Z"), Some(1794511001123));
        assert_eq!(iso_ms("2026-11-12T20:16:41.123+01:00"), Some(1794511001123));
    }
}
