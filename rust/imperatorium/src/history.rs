//! History: every session on the machine, and search across them. A port of
//! `imperatorium/history.py`'s `Index` (list, text cache, search) and its
//! pure pieces (`info_dict`, `plain_text`, `matches`, `snippet`).

use crate::locks::LockExt;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
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
type Listed = (Instant, Arc<Vec<Map<String, Value>>>);

/// The plain-text cache's budget: total bytes of text kept.
const TEXT_BUDGET: usize = 64 * 1024 * 1024;

/// Each session's plain text, least recently used first, by its size then.
#[derive(Default)]
struct Texts {
    by_sid: IndexMap<String, (Value, Arc<str>)>,
    bytes: usize,
}

pub struct Index {
    listed: Mutex<Option<Listed>>,
    text: Mutex<Texts>,
}

impl Index {
    pub fn new() -> Self {
        Index {
            listed: Mutex::new(None),
            text: Mutex::new(Texts::default()),
        }
    }

    /// Index.list: newest first, at most MAX_SESSIONS, cached 30 s.
    pub fn list(&self) -> Arc<Vec<Map<String, Value>>> {
        if let Some((at, rows)) = self.listed.lk().as_ref() {
            if at.elapsed().as_secs_f64() < 30.0 {
                return Arc::clone(rows);
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
        let rows = Arc::new(rows);
        *self.listed.lk() = Some((Instant::now(), Arc::clone(&rows)));
        rows
    }

    /// Index.text: a session's plain text, read again when its size moves.
    /// `read` gives the text, or None when the session cannot be read: that
    /// is "" for this search, as in Python, but not remembered, so the next
    /// search tries again. The cache is bounded by TEXT_BUDGET bytes, the
    /// least recently used text going first.
    fn text(&self, record: &Map<String, Value>, read: &dyn Fn(&str) -> Option<String>) -> Arc<str> {
        let sid = record.get("session_id").map(py_str).unwrap_or_default();
        let size = record.get("size").cloned().unwrap_or(Value::Null);
        {
            let mut t = self.text.lk();
            if let Some(i) = t.by_sid.get_index_of(&sid) {
                if t.by_sid[i].0 == size {
                    let last = t.by_sid.len() - 1;
                    t.by_sid.move_index(i, last);
                    return Arc::clone(&t.by_sid[last].1);
                }
            }
        }
        let Some(text) = read(&sid) else {
            return Arc::from("");
        };
        let text: Arc<str> = Arc::from(text);
        let mut t = self.text.lk();
        if let Some((_, old)) = t.by_sid.shift_remove(&sid) {
            t.bytes -= old.len();
        }
        if text.len() <= TEXT_BUDGET {
            t.bytes += text.len();
            t.by_sid.insert(sid, (size, Arc::clone(&text)));
            while t.bytes > TEXT_BUDGET {
                match t.by_sid.shift_remove_index(0) {
                    Some((_, (_, old))) => t.bytes -= old.len(),
                    None => break,
                }
            }
        }
        text
    }

    /// Index.search: newest first; full text read until `budget` seconds
    /// are spent, so a first search answers rather than hanging.
    pub fn search(&self, query: &str, limit: usize, read: &dyn Fn(&str) -> Option<String>) -> Value {
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
        for r in rows.iter() {
            let mut text: Arc<str> = Arc::from("");
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
                    text.to_string()
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

/// Ported from the Python suite (test_features.py `Matches`, `IndexTests`).
#[cfg(test)]
mod features_tests {
    use super::*;

    fn rec(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn row(sid: &str, title: &str, modified: f64) -> Map<String, Value> {
        rec(json!({"session_id": sid, "title": title, "first_prompt": "", "cwd": "",
                   "modified": modified, "size": 1}))
    }

    /// An index whose listing is already taken (so the transcripts on this
    /// machine are never read).
    fn index_of(rows: Vec<Map<String, Value>>) -> Index {
        let idx = Index::new();
        *idx.listed.lk() = Some((Instant::now(), Arc::new(rows)));
        idx
    }

    #[test]
    fn features_a_body_match_needs_the_text_and_an_empty_query_matches_nothing() {
        let r = rec(json!({"title": "session 4", "first_prompt": "", "cwd": ""}));
        assert_eq!(matches("leaky", &r, ""), None);
        assert_eq!(matches("leaky", &r, "a leaky pipe"), Some("text"));
        assert_eq!(matches("", &rec(json!({"title": "x"})), ""), None);
    }

    #[test]
    fn features_the_text_is_cached_by_size_and_rebuilt_when_it_changes() {
        let idx = Index::new();
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        let read = |sid: &str| {
            calls.borrow_mut().push(sid.to_string());
            Some("hello world".to_string())
        };
        let r = rec(json!({"session_id": "s1", "size": 10}));
        assert!(idx.text(&r, &read).contains("hello"));
        idx.text(&r, &read);
        assert_eq!(*calls.borrow(), vec!["s1"]); // same size, no re-read
        idx.text(&rec(json!({"session_id": "s1", "size": 20})), &read);
        assert_eq!(*calls.borrow(), vec!["s1", "s1"]); // size changed, re-read
    }

    #[test]
    fn features_a_cached_listing_is_not_taken_again() {
        let idx = index_of(vec![row("a", "x", 1.0), row("b", "y", 5.0)]);
        let first = idx.list();
        let second = idx.list();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn features_search_title_match_reads_no_transcript() {
        let idx = index_of(vec![row("s1", "Raspberry Pi tunnel", 1.0)]);
        let reads = std::cell::RefCell::new(0);
        let read = |_: &str| {
            *reads.borrow_mut() += 1;
            Some(String::new())
        };
        let got = idx.search("tunnel", 50, &read);
        assert_eq!(got["results"].as_array().unwrap().len(), 1);
        assert_eq!(got["results"][0]["where"], "title");
        assert_eq!(*reads.borrow(), 0);
    }

    #[test]
    fn features_search_falls_back_to_full_text() {
        let idx = index_of(vec![row("s1", "session 9", 1.0)]);
        let read = |_: &str| Some("a leaky pipe here".to_string());
        let got = idx.search("leaky", 50, &read);
        assert_eq!(got["results"].as_array().unwrap().len(), 1);
        assert_eq!(got["results"][0]["where"], "text");
        assert!(got["results"][0]["snippet"].as_str().unwrap().contains("leaky"));
        assert_eq!(got["complete"], true);
    }

    #[test]
    fn features_an_empty_query_returns_the_plain_list() {
        let idx = index_of(vec![row("s1", "session 9", 1.0)]);
        let read = |_: &str| Some(String::new());
        let got = idx.search("", 50, &read);
        assert_eq!(got["complete"], true);
        assert_eq!(got["results"][0]["where"], Value::Null);
    }
}
