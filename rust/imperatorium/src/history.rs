//! History: every session on the machine, and search across them. A port of
//! `imperatorium/history.py`'s `Index` (list, text cache, search) and its
//! pure pieces (`info_dict`, `plain_text`, `matches`, `snippet`).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Map, Value};

use crate::pyish::{py_str, split_ws, strip, take, truthy};

const MAX_SESSIONS: usize = 400;
const SNIPPET: usize = 90;

/// history.info_dict, from a `sessions list` row (the SDK's fields).
pub fn info_dict(info: &Map<String, Value>) -> Map<String, Value> {
    let get = |k: &str| info.get(k).cloned().unwrap_or(Value::Null);
    let title = [info.get("custom_title"), info.get("summary")]
        .into_iter()
        .find(|v| truthy(*v))
        .flatten()
        .cloned()
        .unwrap_or(json!(""));
    let prompt = info
        .get("first_prompt")
        .filter(|v| truthy(Some(v)))
        .map(py_str)
        .unwrap_or_default();
    let secs = |k: &str| {
        let v = info
            .get(k)
            .filter(|v| truthy(Some(v)))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        json!(v / 1000.0)
    };
    let mut out = Map::new();
    out.insert("session_id".into(), get("session_id"));
    out.insert("title".into(), title);
    out.insert("first_prompt".into(), json!(take(&prompt, 300)));
    out.insert("cwd".into(), get("cwd"));
    out.insert("branch".into(), get("git_branch"));
    out.insert("modified".into(), secs("last_modified"));
    out.insert("created".into(), secs("created_at"));
    out.insert(
        "size".into(),
        info.get("file_size")
            .filter(|v| truthy(Some(v)))
            .cloned()
            .unwrap_or(json!(0)),
    );
    out
}

/// Everything a person or Claude wrote in a session, one string.
pub fn plain_text(items: &[Value]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for item in items {
        for b in item
            .get("blocks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if matches!(
                b.get("kind").and_then(Value::as_str),
                Some("text") | Some("tool")
            ) {
                match b.get("text").filter(|v| truthy(Some(v))) {
                    Some(t) => parts.push(py_str(t)),
                    None => parts.push(format!(
                        "{} {}",
                        b.get("name").map(py_str).unwrap_or_else(|| "None".into()),
                        b.get("summary")
                            .map(py_str)
                            .unwrap_or_else(|| "None".into())
                    )),
                }
            }
        }
    }
    parts.join("\n")
}

/// The match in context, on one line. As Python does it: the position is
/// found in the lowercased text and used on the original.
pub fn snippet(text: &str, query: &str) -> String {
    let lower: Vec<char> = text.to_lowercase().chars().collect();
    let needle: Vec<char> = query.to_lowercase().chars().collect();
    let at = if needle.is_empty() {
        Some(0)
    } else {
        lower
            .windows(needle.len())
            .position(|w| w == needle.as_slice())
    };
    let Some(at) = at else { return String::new() };
    let chars: Vec<char> = text.chars().collect();
    let qlen = query.chars().count();
    let start = at.saturating_sub(SNIPPET / 2);
    let end = (at + qlen + SNIPPET / 2).min(chars.len());
    let piece: String = chars[start.min(chars.len())..end.max(start.min(chars.len()))]
        .iter()
        .collect();
    let body = split_ws(&piece).join(" ");
    format!(
        "{}{}{}",
        if start > 0 { "\u{2026}" } else { "" },
        body,
        if end < chars.len() { "\u{2026}" } else { "" }
    )
}

/// Where `query` appears: "title", "prompt", "text", or None. Every word
/// must appear somewhere, in any order.
pub fn matches(query: &str, record: &Map<String, Value>, text: &str) -> Option<&'static str> {
    let q = query.to_lowercase();
    let words = split_ws(&q);
    if words.is_empty() {
        return None;
    }
    let field = |k: &str| {
        record
            .get(k)
            .filter(|v| truthy(Some(v)))
            .map(py_str)
            .unwrap_or_default()
            .to_lowercase()
    };
    let (title, prompt, place) = (field("title"), field("first_prompt"), field("cwd"));
    let hay = [
        title.as_str(),
        prompt.as_str(),
        place.as_str(),
        &text.to_lowercase(),
    ]
    .join("\n");
    if !words.iter().all(|w| hay.contains(w)) {
        return None;
    }
    if words.iter().all(|w| title.contains(w) || place.contains(w)) {
        return Some("title");
    }
    if words
        .iter()
        .all(|w| title.contains(w) || prompt.contains(w) || place.contains(w))
    {
        return Some("prompt");
    }
    Some("text")
}

/// One listing of the sessions and when it was taken.
type Listed = (Instant, Vec<Map<String, Value>>);

pub struct Index {
    listed: Mutex<Option<Listed>>,
    text: Mutex<HashMap<String, (Value, String)>>,
}

impl Index {
    pub fn new() -> Self {
        Index {
            listed: Mutex::new(None),
            text: Mutex::new(HashMap::new()),
        }
    }

    /// Index.list: newest first, at most MAX_SESSIONS, cached 30 s.
    pub fn list(&self) -> Vec<Map<String, Value>> {
        if let Some((at, rows)) = self.listed.lock().unwrap().as_ref() {
            if at.elapsed().as_secs_f64() < 30.0 {
                return rows.clone();
            }
        }
        let mut rows: Vec<Map<String, Value>> = match crate::sessions::list_json() {
            Value::Array(a) => a
                .iter()
                .filter_map(Value::as_object)
                .map(info_dict)
                .collect(),
            _ => vec![],
        };
        rows.sort_by(|a, b| {
            let m =
                |r: &Map<String, Value>| r.get("modified").and_then(Value::as_f64).unwrap_or(0.0);
            m(b).partial_cmp(&m(a)).unwrap_or(std::cmp::Ordering::Equal)
        });
        rows.truncate(MAX_SESSIONS);
        *self.listed.lock().unwrap() = Some((Instant::now(), rows.clone()));
        rows
    }

    /// Index.text: a session's plain text, read again when its size moves.
    fn text(&self, record: &Map<String, Value>, read: &dyn Fn(&str) -> Vec<Value>) -> String {
        let sid = record.get("session_id").map(py_str).unwrap_or_default();
        let size = record.get("size").cloned().unwrap_or(Value::Null);
        if let Some((s, t)) = self.text.lock().unwrap().get(&sid) {
            if *s == size {
                return t.clone();
            }
        }
        let text = plain_text(&read(&sid));
        self.text.lock().unwrap().insert(sid, (size, text.clone()));
        text
    }

    /// Index.search: newest first; full text read until `budget` seconds
    /// are spent, so a first search answers rather than hanging.
    pub fn search(&self, query: &str, limit: usize, read: &dyn Fn(&str) -> Vec<Value>) -> Value {
        let query = strip(query);
        let rows = self.list();
        if query.is_empty() {
            let results: Vec<Value> = rows
                .iter()
                .take(limit)
                .map(|r| {
                    let mut r = r.clone();
                    r.insert("where".into(), Value::Null);
                    r.insert("snippet".into(), json!(""));
                    Value::Object(r)
                })
                .collect();
            return json!({"results": results, "complete": true});
        }
        let deadline = Instant::now() + std::time::Duration::from_secs(8);
        let first_word = split_ws(query)
            .first()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let (mut out, mut complete) = (Vec::new(), true);
        for r in &rows {
            let mut text = String::new();
            let mut found = matches(query, r, "");
            if found.is_none() {
                if Instant::now() > deadline {
                    complete = false;
                    continue;
                }
                text = self.text(r, read);
                found = matches(query, r, &text);
            }
            if let Some(w) = found {
                let source = if !text.is_empty() {
                    text.clone()
                } else {
                    [r.get("first_prompt"), r.get("title")]
                        .into_iter()
                        .find(|v| truthy(*v))
                        .flatten()
                        .map(py_str)
                        .unwrap_or_default()
                };
                let mut hit = r.clone();
                hit.insert("where".into(), json!(w));
                hit.insert("snippet".into(), json!(snippet(&source, &first_word)));
                out.push(Value::Object(hit));
                if out.len() >= limit {
                    break;
                }
            }
        }
        json!({"results": out, "complete": complete})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn every_word_somewhere() {
        let r = rec(
            json!({"title": "Raspberry Pi tunnel fix", "first_prompt": "set up ssh", "cwd": "C:/w/x"}),
        );
        assert_eq!(matches("tunnel pi", &r, ""), Some("title"));
        assert_eq!(matches("tunnel ssh", &r, ""), Some("prompt"));
        assert_eq!(
            matches("tunnel nginx", &r, "the nginx config"),
            Some("text")
        );
        assert_eq!(matches("absent", &r, ""), None);
    }

    #[test]
    fn snippets_match_python() {
        // python -c "from imperatorium import history as h; print(repr(h.snippet('x'*100 + 'Needle here' + 'y'*100, 'needle')))"
        let text = format!("{}Needle here{}", "x".repeat(100), "y".repeat(100));
        let want = format!(
            "\u{2026}{}Needle here{}\u{2026}",
            "x".repeat(45),
            "y".repeat(40)
        );
        assert_eq!(snippet(&text, "needle"), want);
        assert_eq!(snippet("short  text\nhere", "text"), "short text here");
        assert_eq!(snippet("nothing", "absent"), "");
    }
}
