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
    #[cfg(test)]
    pub fn search(&self, query: &str, limit: usize, read: &dyn Fn(&str) -> Option<String>) -> Value {
        self.search_with(query, limit, &Filter::default(), read)
    }

    /// [`Index::search`] over the sessions `filter` keeps, in its order,
    /// with `folders`: every project folder in the listing (a worktree
    /// counted under its main checkout), most recently active first, for
    /// the History filter (2026-11-23).
    pub fn search_with(
        &self,
        query: &str,
        limit: usize,
        filter: &Filter,
        read: &dyn Fn(&str) -> Option<String>,
    ) -> Value {
        let all = self.list();
        let folders = folders_of(&all);
        let mut got = self.search_rows(query, limit, &filter.apply(&all), read);
        got["folders"] = folders;
        got
    }

    fn search_rows(
        &self,
        query: &str,
        limit: usize,
        rows: &[Map<String, Value>],
        read: &dyn Fn(&str) -> Option<String>,
    ) -> Value {
        let query = strip(query);
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

/// What History is narrowed to and in which order (2026-11-23).
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Filter {
    /// A project folder: its sessions and those of its worktrees under
    /// `.claude/worktrees`.
    pub folder: Option<String>,
    /// Active (modified) at or after this, in seconds.
    pub since: Option<f64>,
    pub sort: Sort,
}

#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub enum Sort {
    /// Most recently active first (the listing's own order).
    #[default]
    Modified,
    /// Newest first by when it was started.
    Created,
    /// Oldest started first.
    Oldest,
    /// Largest transcript first.
    Size,
    /// By title, A to Z.
    Title,
}

impl Sort {
    pub fn parse(s: &str) -> Sort {
        match s {
            "created" => Sort::Created,
            "oldest" => Sort::Oldest,
            "size" => Sort::Size,
            "title" => Sort::Title,
            _ => Sort::Modified,
        }
    }
}

/// A folder compared whatever its slashes, case or trailing slash.
pub fn norm_dir(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// The checkout a folder belongs to: a worktree under `.claude/worktrees`
/// is its main folder's (as the window's managedWorktree), else itself.
pub fn main_folder(cwd: &str) -> &str {
    let t = cwd.trim_end_matches(['\\', '/']);
    let flat = t.replace('\\', "/");
    match flat.rfind("/.claude/worktrees/") {
        Some(i) if !flat[i + "/.claude/worktrees/".len()..].contains('/') => &t[..i],
        _ => t,
    }
}

fn num(r: &Map<String, Value>, k: &str) -> f64 {
    r.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

fn title_of(r: &Map<String, Value>) -> String {
    [r.get("title"), r.get("first_prompt")]
        .into_iter()
        .flatten()
        .find(|v| truthy(Some(v)))
        .map(py_str)
        .unwrap_or_default()
        .to_lowercase()
}

impl Filter {
    /// The rows it keeps, in its order (ties keep the listing's order).
    pub fn apply(&self, rows: &[Map<String, Value>]) -> Vec<Map<String, Value>> {
        let want = self.folder.as_deref().map(norm_dir).filter(|f| !f.is_empty());
        let mut out: Vec<Map<String, Value>> = rows
            .iter()
            .filter(|r| {
                let cwd = r.get("cwd").map(py_str).unwrap_or_default();
                want.as_ref().is_none_or(|f| norm_dir(main_folder(&cwd)) == *f)
                    && self.since.is_none_or(|t| num(r, "modified") >= t)
            })
            .cloned()
            .collect();
        let desc = |k: &'static str| move |a: &Map<String, Value>, b: &Map<String, Value>| {
            num(b, k).partial_cmp(&num(a, k)).unwrap_or(std::cmp::Ordering::Equal)
        };
        match self.sort {
            Sort::Modified => {}
            Sort::Created => out.sort_by(desc("created")),
            Sort::Size => out.sort_by(desc("size")),
            Sort::Oldest => out.sort_by(|a, b| {
                num(a, "created").partial_cmp(&num(b, "created")).unwrap_or(std::cmp::Ordering::Equal)
            }),
            Sort::Title => out.sort_by_key(title_of),
        }
        out
    }
}

/// [{folder, count, modified}] - each main folder in `rows` once.
fn folders_of(rows: &[Map<String, Value>]) -> Value {
    let mut by: IndexMap<String, (String, usize, f64)> = IndexMap::new();
    for r in rows {
        let cwd = r.get("cwd").map(py_str).unwrap_or_default();
        if cwd.is_empty() {
            continue;
        }
        let main = main_folder(&cwd).to_string();
        let e = by.entry(norm_dir(&main)).or_insert((main, 0, 0.0));
        e.1 += 1;
        e.2 = e.2.max(num(r, "modified"));
    }
    let mut list: Vec<(String, usize, f64)> = by.into_values().collect();
    list.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    Value::Array(
        list.into_iter()
            .map(|(f, n, m)| json!({"folder": f, "count": n, "modified": m}))
            .collect(),
    )
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

    fn at(sid: &str, cwd: &str, modified: f64, created: f64, size: u64, title: &str) -> Map<String, Value> {
        rec(json!({"session_id": sid, "title": title, "first_prompt": "", "cwd": cwd,
                   "modified": modified, "created": created, "size": size}))
    }

    fn ids(v: &Value) -> Vec<String> {
        v["results"].as_array().unwrap().iter().map(|r| r["session_id"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn a_worktree_belongs_to_its_main_folder() {
        assert_eq!(main_folder(r"C:\w\app\.claude\worktrees\fix-1"), r"C:\w\app");
        assert_eq!(main_folder("/w/app/.claude/worktrees/fix-1/"), "/w/app");
        assert_eq!(main_folder(r"C:\w\app"), r"C:\w\app");
        // Deeper than a worktree's own folder is not the worktree.
        assert_eq!(main_folder("/w/app/.claude/worktrees/fix-1/sub"), "/w/app/.claude/worktrees/fix-1/sub");
        assert_eq!(norm_dir(r"C:\W\App\"), "c:/w/app");
    }

    #[test]
    fn history_narrows_to_a_folder_and_its_worktrees_before_the_limit() {
        let mut rows = vec![];
        // 50 newer sessions elsewhere would fill a limit of 40 on their own.
        for i in 0..50 {
            rows.push(at(&format!("o{i}"), r"C:\w\other", 100.0 + i as f64, 1.0, 1, "x"));
        }
        rows.push(at("a1", r"C:\w\App", 50.0, 1.0, 1, "x"));
        rows.push(at("a2", r"c:/w/app/.claude/worktrees/fix", 40.0, 1.0, 1, "x"));
        rows.push(at("b1", r"C:\w\apple", 45.0, 1.0, 1, "x"));
        rows.sort_by(|a, b| num(b, "modified").partial_cmp(&num(a, "modified")).unwrap());
        let idx = index_of(rows);
        let f = Filter {folder: Some(r"C:\w\app\".into()), ..Default::default()};
        let got = idx.search_with("", 40, &f, &|_| None);
        assert_eq!(ids(&got), vec!["a1", "a2"]);
        let folders = got["folders"].as_array().unwrap();
        assert_eq!(folders.len(), 3);
        assert_eq!(folders[0]["folder"], r"C:\w\other");
        assert_eq!(folders[0]["count"], 50);
        let app = folders.iter().find(|f| f["folder"] == r"C:\w\App").unwrap();
        assert_eq!(app["count"], 2); // the worktree counts under its checkout
    }

    #[test]
    fn history_sorts_and_keeps_recent_ones() {
        let idx = index_of(vec![
            at("new", "/w", 30.0, 10.0, 5, "beta"),
            at("mid", "/w", 20.0, 30.0, 50, "Alpha"),
            at("old", "/w", 10.0, 20.0, 1, "gamma"),
        ]);
        let by = |sort, since| ids(&idx.search_with("", 40, &Filter {folder: None, since, sort}, &|_| None));
        assert_eq!(by(Sort::Modified, None), vec!["new", "mid", "old"]);
        assert_eq!(by(Sort::Created, None), vec!["mid", "old", "new"]);
        assert_eq!(by(Sort::Oldest, None), vec!["new", "old", "mid"]);
        assert_eq!(by(Sort::Size, None), vec!["mid", "new", "old"]);
        assert_eq!(by(Sort::Title, None), vec!["mid", "new", "old"]);
        assert_eq!(by(Sort::Modified, Some(15.0)), vec!["new", "mid"]);
        assert_eq!(Sort::parse("created"), Sort::Created);
        assert_eq!(Sort::parse("anything"), Sort::Modified);
    }

    #[test]
    fn a_search_keeps_the_filter_and_its_order() {
        let idx = index_of(vec![
            at("x1", "/w/a", 30.0, 1.0, 1, "tunnel one"),
            at("x2", "/w/b", 20.0, 3.0, 1, "tunnel two"),
            at("x3", "/w/a", 10.0, 2.0, 1, "tunnel three"),
        ]);
        let f = Filter {folder: Some("/w/a".into()), since: None, sort: Sort::Created};
        assert_eq!(ids(&idx.search_with("tunnel", 40, &f, &|_| None)), vec!["x3", "x1"]);
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
