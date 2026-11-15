//! The one-line "what is it doing" for a tile: `activity.doing` and
//! `first_line` from `imperatorium/activity.py`.

use serde_json::{json, Value};

use crate::pyish::{py_str, strip, take};

const ACTIVITY_CHARS: usize = 140;

/// Python's `str.splitlines()`: every line boundary it knows, `\r\n` as one.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let boundary = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !boundary {
            continue;
        }
        out.push(&text[start..i]);
        let mut next = i + c.len_utf8();
        if c == '\r' {
            if let Some(&(j, '\n')) = chars.peek() {
                chars.next();
                next = j + 1;
            }
        }
        start = next;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// The first non-blank line, cut to `limit` with an ellipsis.
pub fn first_line(text: Option<&Value>) -> String {
    let empty = json!("");
    let s = py_str(if crate::pyish::truthy(text) {
        text.unwrap()
    } else {
        &empty
    });
    for line in splitlines(&s) {
        let line = strip(line);
        if !line.is_empty() {
            let more = line.chars().count() > ACTIVITY_CHARS;
            return format!(
                "{}{}",
                take(line, ACTIVITY_CHARS),
                if more { "\u{2026}" } else { "" }
            );
        }
    }
    String::new()
}

/// activity.doing: the newest tool call, else the newest thing said.
pub fn doing(items: &[Value]) -> String {
    for item in items.iter().rev() {
        let speaker = item.get("speaker").and_then(Value::as_str);
        let blocks = item
            .get("blocks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for b in blocks.iter().rev() {
            match b.get("kind").and_then(Value::as_str) {
                Some("tool") => {
                    let name = b.get("name").map(py_str).unwrap_or_else(|| "None".into());
                    return format!("{} {}", name, first_line(b.get("summary")));
                }
                Some("text") if speaker == Some("claude") => return first_line(b.get("text")),
                Some("text") if speaker == Some("you") => {
                    return format!("asked: {}", first_line(b.get("text")))
                }
                _ => {}
            }
        }
    }
    String::new()
}

/// activity.timeline: [{tool, summary, ok, error}] in order; a call whose
/// result is not in view has ok=None.
pub fn timeline(items: &[Value]) -> Vec<Value> {
    let mut calls: Vec<Value> = Vec::new();
    let mut by_id: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for item in items {
        for b in item
            .get("blocks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match b.get("kind").and_then(Value::as_str) {
                Some("tool") => {
                    let summary = b
                        .get("summary")
                        .filter(|v| crate::pyish::truthy(Some(v)))
                        .cloned()
                        .unwrap_or(json!(""));
                    calls.push(
                        json!({"tool": b.get("name").cloned().unwrap_or(Value::Null),
                                      "summary": summary, "ok": null, "error": null}),
                    );
                    if let Some(id) = b.get("id").filter(|v| crate::pyish::truthy(Some(v))) {
                        by_id.insert(py_str(id), calls.len() - 1);
                    }
                }
                Some("result") => {
                    let key = b.get("id").map(py_str).unwrap_or_default();
                    let Some(&i) = b
                        .get("id")
                        .filter(|v| !v.is_null())
                        .and_then(|_| by_id.get(&key))
                    else {
                        continue;
                    };
                    let err = crate::pyish::truthy(b.get("is_error"));
                    calls[i]["ok"] = json!(!err);
                    if err {
                        calls[i]["error"] = json!(first_line_n(b.get("text"), 200));
                    }
                }
                _ => {}
            }
        }
    }
    calls
}

/// first_line with its own limit.
pub fn first_line_n(text: Option<&Value>, limit: usize) -> String {
    let empty = json!("");
    let s = py_str(if crate::pyish::truthy(text) {
        text.unwrap()
    } else {
        &empty
    });
    for line in splitlines(&s) {
        let line = strip(line);
        if !line.is_empty() {
            let more = line.chars().count() > limit;
            return format!(
                "{}{}",
                take(line, limit),
                if more { "\u{2026}" } else { "" }
            );
        }
    }
    String::new()
}

fn categories() -> &'static [(&'static str, regex::Regex)] {
    static C: std::sync::OnceLock<Vec<(&'static str, regex::Regex)>> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        [
            (
                "rejected by user",
                r"user (doesn't want|rejected|denied)|permission (was )?denied|was blocked",
            ),
            (
                "file not found",
                r"(does not exist|no such file|not found|cannot find the (file|path))",
            ),
            (
                "edit did not match",
                r"string to replace|old_string|not unique|found \d+ matches",
            ),
            (
                "file not read first",
                r"(has not been read|read it first|must read)",
            ),
            ("file changed on disk", r"modified since|changed on disk"),
            ("timed out", r"timed? ?out|timeout"),
            (
                "command failed",
                r"exit code [1-9]|exited with|returned non-zero|error:",
            ),
            ("network", r"ECONN|network|fetch failed|429|503|getaddrinfo"),
        ]
        .into_iter()
        .map(|(n, p)| (n, regex::Regex::new(&format!("(?i){p}")).unwrap()))
        .collect()
    })
}

pub fn categorise(text: &str) -> &'static str {
    categories()
        .iter()
        .find(|(_, re)| re.is_match(text))
        .map(|(n, _)| *n)
        .unwrap_or("other")
}

/// A dict key as json.dumps writes it: None becomes "null".
fn key_of(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        other => py_str(other),
    }
}

/// activity.tally: counts for one timeline.
pub fn tally(calls: &[Value]) -> Value {
    let mut by_tool: indexmap::IndexMap<String, [i64; 2]> = indexmap::IndexMap::new();
    let mut by_cat: indexmap::IndexMap<&'static str, i64> = indexmap::IndexMap::new();
    let mut examples: indexmap::IndexMap<&'static str, String> = indexmap::IndexMap::new();
    let mut errors = 0;
    for c in calls {
        let tool = c.get("tool").cloned().unwrap_or(Value::Null);
        let slot = by_tool.entry(key_of(&tool)).or_insert([0, 0]);
        slot[0] += 1;
        if c.get("ok") == Some(&json!(false)) {
            errors += 1;
            slot[1] += 1;
            let err = c
                .get("error")
                .filter(|v| crate::pyish::truthy(Some(v)))
                .map(py_str)
                .unwrap_or_default();
            let cat = categorise(&err);
            *by_cat.entry(cat).or_insert(0) += 1;
            let tool_text = if tool.is_null() {
                "None".to_string()
            } else {
                py_str(&tool)
            };
            examples
                .entry(cat)
                .or_insert_with(|| format!("{tool_text}: {err}"));
        }
    }
    json!({
        "calls": calls.len(), "errors": errors,
        "by_tool": by_tool.into_iter().map(|(k, v)| (k, json!(v))).collect::<serde_json::Map<String, Value>>(),
        "by_category": by_cat.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<String, Value>>(),
        "examples": examples.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<String, Value>>(),
    })
}

/// activity.merge_tallies.
pub fn merge_tallies(tallies: &[Value]) -> serde_json::Map<String, Value> {
    let (mut calls, mut errors) = (0i64, 0i64);
    let mut by_tool: indexmap::IndexMap<String, [i64; 2]> = indexmap::IndexMap::new();
    let mut by_cat: indexmap::IndexMap<String, i64> = indexmap::IndexMap::new();
    let mut examples: indexmap::IndexMap<String, Value> = indexmap::IndexMap::new();
    for t in tallies {
        calls += t["calls"].as_i64().unwrap_or(0);
        errors += t["errors"].as_i64().unwrap_or(0);
        for (tool, ne) in t["by_tool"].as_object().into_iter().flatten() {
            let cur = by_tool.entry(tool.clone()).or_insert([0, 0]);
            cur[0] += ne[0].as_i64().unwrap_or(0);
            cur[1] += ne[1].as_i64().unwrap_or(0);
        }
        for (cat, n) in t["by_category"].as_object().into_iter().flatten() {
            *by_cat.entry(cat.clone()).or_insert(0) += n.as_i64().unwrap_or(0);
        }
        for (cat, ex) in t["examples"].as_object().into_iter().flatten() {
            examples.entry(cat.clone()).or_insert_with(|| ex.clone());
        }
    }
    let mut out = serde_json::Map::new();
    out.insert("calls".into(), json!(calls));
    out.insert("errors".into(), json!(errors));
    out.insert(
        "by_tool".into(),
        Value::Object(by_tool.into_iter().map(|(k, v)| (k, json!(v))).collect()),
    );
    out.insert(
        "by_category".into(),
        Value::Object(by_cat.into_iter().map(|(k, v)| (k, json!(v))).collect()),
    );
    out.insert(
        "examples".into(),
        Value::Object(examples.into_iter().collect()),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_as_python_does() {
        assert_eq!(splitlines("a\r\nb\rc\u{2028}d\n"), vec!["a", "b", "c", "d"]);
        assert_eq!(splitlines(""), Vec::<&str>::new());
        assert_eq!(first_line(Some(&json!("\n  \n  hello  \nworld"))), "hello");
    }

    #[test]
    fn the_newest_thing_wins() {
        let items = vec![
            json!({"speaker": "you", "blocks": [{"kind": "text", "text": "fix it"}]}),
            json!({"speaker": "claude", "blocks": [{"kind": "text", "text": "on it"},
                   {"kind": "tool", "name": "Bash", "summary": "make\nmore"}]}),
        ];
        assert_eq!(doing(&items), "Bash make");
        assert_eq!(doing(&items[..1]), "asked: fix it");
    }
}

/// Ported from the Python suite (test_features.py): timelines, the doing
/// line, error categories and tallies.
#[cfg(test)]
mod features_tests {
    use super::*;

    fn tool_item(name: &str, summary: &str, tid: &str) -> Value {
        json!({"speaker": "claude", "blocks": [
            {"kind": "tool", "name": name, "summary": summary, "id": tid}]})
    }

    fn result_item(tid: &str, ok: bool, text: &str) -> Value {
        json!({"speaker": "tool", "blocks": [
            {"kind": "result", "id": tid, "is_error": !ok, "text": text}]})
    }

    fn text_item(text: &str, speaker: &str) -> Value {
        json!({"speaker": speaker, "blocks": [{"kind": "text", "text": text}]})
    }

    #[test]
    fn features_timeline_pairs_calls_with_results_and_leaves_running_ones_open() {
        let calls = timeline(&[
            tool_item("Bash", "npm test", "t1"),
            result_item("t1", false, "exit code 1: failed\nmore"),
        ]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["tool"], "Bash");
        assert_eq!(calls[0]["ok"], false);
        assert!(calls[0]["error"].as_str().unwrap().contains("exit code 1"));
        // A call still running has ok None.
        let running = timeline(&[tool_item("Read", "f.py", "t2")]);
        assert_eq!(running[0]["ok"], Value::Null);
    }

    #[test]
    fn features_doing_after_a_finished_turn_and_when_empty() {
        let items = vec![
            tool_item("Bash", "npm test", "t1"),
            result_item("t1", true, ""),
            text_item("all done", "claude"),
        ];
        assert_eq!(doing(&items), "all done");
        assert_eq!(doing(&[]), "");
        // A running tool call wins over earlier text.
        let running = vec![text_item("earlier", "claude"), tool_item("Bash", "npm test", "t1")];
        assert_eq!(doing(&running), "Bash npm test");
        assert_eq!(doing(&[text_item("do the thing", "you")]), "asked: do the thing");
    }

    #[test]
    fn features_categorise_known_shapes() {
        for (text, want) in [
            ("the user rejected this", "rejected by user"),
            ("No such file or directory", "file not found"),
            ("found 3 matches for old_string", "edit did not match"),
            ("the file has not been read first", "file not read first"),
            ("file changed on disk since last read", "file changed on disk"),
            ("the command timed out", "timed out"),
            ("exit code 1", "command failed"),
            ("ECONNREFUSED talking to host", "network"),
            ("something entirely unrecognised", "other"),
        ] {
            assert_eq!(categorise(text), want, "{text}");
        }
    }

    #[test]
    fn features_tally_counts_calls_and_errors_and_merges() {
        let calls = timeline(&[
            tool_item("Bash", "a", "t1"),
            result_item("t1", false, "exit code 1"),
            tool_item("Read", "b", "t2"),
            result_item("t2", true, ""),
        ]);
        let t = tally(&calls);
        assert_eq!(t["calls"], 2);
        assert_eq!(t["errors"], 1);
        assert_eq!(t["by_tool"]["Bash"], json!([1, 1]));
        assert_eq!(t["by_tool"]["Read"], json!([1, 0]));
        assert_eq!(t["by_category"], json!({"command failed": 1}));

        let a = tally(&timeline(&[
            tool_item("Bash", "a", "t1"),
            result_item("t1", false, "x"),
        ]));
        let b = tally(&timeline(&[
            tool_item("Bash", "b", "t2"),
            result_item("t2", true, ""),
        ]));
        let merged = merge_tallies(&[a, b]);
        assert_eq!(merged["calls"], 2);
        assert_eq!(merged["by_tool"]["Bash"], json!([2, 1]));
    }
}
