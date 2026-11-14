//! What the commands a session is waiting on are printing: a port of
//! `imperatorium/taskout.py`. Reads the output files Claude Code writes
//! under its temp folder, and subagents' transcripts, and nothing else;
//! nothing here writes, locks, truncates or deletes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use crate::pyish::{py_str, strip, take, truthy};

const TAIL_BYTES: u64 = 16 * 1024;
const AGENT_TAIL_BYTES: u64 = 256 * 1024;
const SLACK: f64 = 3.0;

/// tempfile.gettempdir(): TMPDIR, TEMP, TMP, then the platform's.
fn gettempdir() -> String {
    for var in ["TMPDIR", "TEMP", "TMP"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() && Path::new(&v).is_dir() {
                return crate::paths::abspath(&v).unwrap_or(v);
            }
        }
    }
    std::env::temp_dir()
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string()
}

/// Where Claude Code keeps its temp folder.
pub fn temp_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for base in [
        std::env::var("CLAUDE_CODE_TMPDIR")
            .ok()
            .filter(|v| !v.is_empty()),
        Some(gettempdir()),
    ]
    .into_iter()
    .flatten()
    {
        let p = Path::new(&base).join("claude");
        if !roots.contains(&p) {
            roots.push(p);
        }
    }
    roots
}

fn valid_id(s: &str) -> bool {
    s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn mtime(p: &Path) -> Option<f64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    let d = m.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_secs() as f64 + d.subsec_nanos() as f64 * 1e-9)
}

/// The session's tasks folder: <temp>/claude/*/<id>/tasks, the newest.
pub fn tasks_dir(sid: &str) -> Option<PathBuf> {
    if !valid_id(sid) {
        return None;
    }
    for root in temp_roots() {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        let hits: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path().join(sid).join("tasks"))
            .filter(|p| p.is_dir())
            .collect();
        if let Some(best) = hits.into_iter().max_by(|a, b| {
            mtime(a)
                .partial_cmp(&mtime(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            return Some(best);
        }
    }
    None
}

fn ansi() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-_]")
            .unwrap()
    })
}

/// Terminal output as plain lines: colour codes gone, a redrawn line kept
/// as its last drawing.
pub fn clean(text: &str) -> String {
    let text = ansi()
        .replace_all(&text.replace("\r\n", "\n"), "")
        .into_owned();
    text.split('\n')
        .map(|l| l.rsplit('\r').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// (bytes from the end, cut at a line boundary when it had to cut, size, mtime)
fn read_end(path: &Path, limit: u64, empty_if_no_newline: bool) -> Option<(Vec<u8>, u64, f64)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let meta = f.metadata().ok()?;
    let size = meta.len();
    let start = size.saturating_sub(limit);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut data = vec![0u8; (size - start) as usize];
    let mut got = 0;
    while got < data.len() {
        match f.read(&mut data[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(_) => return None,
        }
    }
    data.truncate(got);
    if start > 0 {
        data = match data.iter().position(|b| *b == b'\n') {
            Some(cut) => data[cut + 1..].to_vec(),
            None if empty_if_no_newline => vec![],
            None => data,
        };
    }
    let m = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((
        data,
        size,
        m.as_secs() as f64 + m.subsec_nanos() as f64 * 1e-9,
    ))
}

/// taskout.tail: (text, size, mtime), ("", 0, None) if gone.
pub fn tail(path: &Path, limit: u64) -> (String, u64, Value) {
    match read_end(path, limit, false) {
        Some((data, size, m)) => (clean(&String::from_utf8_lossy(&data)), size, json!(m)),
        None => (String::new(), 0, Value::Null),
    }
}

pub fn subagents_dir(transcript: Option<&Path>) -> Option<PathBuf> {
    let t = transcript?.to_string_lossy().into_owned();
    let stem = t.strip_suffix(".jsonl")?;
    let p = Path::new(stem).join("subagents");
    p.is_dir().then_some(p)
}

/// {id: transcript} for each subagent, under its agent id and the id of the
/// call that started it.
fn agent_files(folder: Option<&Path>) -> std::collections::HashMap<String, PathBuf> {
    let mut out = std::collections::HashMap::new();
    let Some(folder) = folder else { return out };
    let Ok(rd) = std::fs::read_dir(folder) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name
            .strip_prefix("agent-")
            .and_then(|n| n.strip_suffix(".jsonl"))
        else {
            continue;
        };
        let path = e.path();
        out.insert(id.to_string(), path.clone());
        let meta = folder.join(format!("agent-{id}.meta.json"));
        let call = std::fs::read_to_string(meta)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("toolUseId").cloned());
        if let Some(c) = call.filter(|c| truthy(Some(c))) {
            out.insert(py_str(&c), path);
        }
    }
    out
}

fn first_line(text: &str, limit: usize) -> String {
    let line = strip(text).split('\n').next().unwrap_or("").to_string();
    if line.chars().count() <= limit {
        line
    } else {
        format!("{}\u{2026}", take(&line, limit - 1))
    }
}

/// activity.summarise_input, the hooks' version (no overall length cap).
fn summarise_input(input: Option<&Value>) -> String {
    let Some(Value::Object(ti)) = input else {
        let empty = json!("");
        return take(
            &py_str(if truthy(input) {
                input.unwrap()
            } else {
                &empty
            }),
            200,
        );
    };
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
            return take(&py_str(v), 200);
        }
    }
    let mut keys: Vec<&String> = ti.keys().collect();
    keys.sort();
    keys.iter()
        .take(6)
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// taskout.story: a subagent's transcript as plain lines.
fn story(entries: &[Value]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for entry in entries {
        let kind = entry.get("type").and_then(Value::as_str);
        if !matches!(kind, Some("assistant") | Some("user"))
            || entry.get("parentUuid").is_none_or(Value::is_null)
        {
            continue;
        }
        let Some(Value::Array(content)) = entry.get("message").and_then(|m| m.get("content"))
        else {
            continue;
        };
        for block in content.iter().filter_map(Value::as_object) {
            match block.get("type").and_then(Value::as_str) {
                Some("text") if kind == Some("assistant") => {
                    let t = block.get("text").and_then(Value::as_str).unwrap_or("");
                    if !strip(t).is_empty() {
                        lines.push(strip(t).to_string());
                    }
                }
                Some("tool_use") => {
                    let name = block
                        .get("name")
                        .filter(|v| truthy(Some(v)))
                        .map(py_str)
                        .unwrap_or_else(|| "?".into());
                    lines.push(format!(
                        "> {} {}",
                        name,
                        first_line(&summarise_input(block.get("input")), 200)
                    ));
                }
                Some("tool_result") if truthy(block.get("is_error")) => {
                    let body = match block.get("content") {
                        Some(Value::Array(parts)) => parts
                            .iter()
                            .filter_map(Value::as_object)
                            .map(|b| {
                                b.get("text")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string()
                            })
                            .collect::<Vec<_>>()
                            .join(" "),
                        Some(v) if truthy(Some(v)) => py_str(v),
                        _ => String::new(),
                    };
                    lines.push(format!("  x {}", first_line(&body, 200)));
                }
                _ => {}
            }
        }
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn agent_tail(path: &Path) -> (String, u64, Value) {
    match read_end(path, AGENT_TAIL_BYTES, true) {
        Some((data, size, m)) => {
            let entries: Vec<Value> = String::from_utf8_lossy(&data)
                .split('\n')
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter(Value::is_object)
                .collect();
            (story(&entries), size, json!(m))
        }
        None => (String::new(), 0, Value::Null),
    }
}

/// st_birthtime (Python 3.12 on Windows), else st_ctime.
fn created(p: &Path) -> Option<f64> {
    let meta = std::fs::metadata(p).ok()?;
    let t = meta.created().or_else(|_| meta.modified()).ok()?;
    let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_secs() as f64 + d.subsec_nanos() as f64 * 1e-9)
}

/// taskout.awaited: each running shell, monitor and subagent, oldest first.
pub fn awaited(stat: &Value, folder: Option<&Path>, agents: Option<&Path>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut files: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    if let Some(f) = folder {
        if let Ok(rd) = std::fs::read_dir(f) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(id) = name.strip_suffix(".output") {
                    files.insert(id.to_string(), e.path());
                }
            }
        }
    }
    let known: HashSet<String> = stat
        .get("known_tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(py_str)
        .collect();
    let add =
        |out: &mut Vec<Value>, id: &Value, item: &Value, kind: &str, path: Option<&PathBuf>| {
            let (text, size, m) = match path {
                None => (String::new(), 0, Value::Null),
                Some(p) if kind == "agent" => agent_tail(p),
                Some(p) => tail(p, TAIL_BYTES),
            };
            let label = item
                .get("label")
                .filter(|v| truthy(Some(v)))
                .cloned()
                .unwrap_or(json!(""));
            out.push(json!({"id": id, "kind": kind, "label": label,
                        "foreground": truthy(item.get("foreground")),
                        "started": item.get("started").cloned().unwrap_or(Value::Null),
                        "size": size, "mtime": m, "text": text}));
        };
    let mut foreground: Vec<Value> = Vec::new();
    for (kind, name) in [("shells", "shell"), ("monitors", "monitor")] {
        for item in stat
            .get(kind)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if truthy(item.get("foreground")) {
                foreground.push(item.clone());
            } else if let Some(id) = item.get("id").filter(|v| truthy(Some(v))) {
                add(&mut out, id, item, name, files.get(&py_str(id)));
            }
        }
    }
    let transcripts = agent_files(agents);
    for item in stat
        .get("agents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = item.get("id").filter(|v| truthy(Some(v))) {
            add(&mut out, id, item, "agent", transcripts.get(&py_str(id)));
        }
    }
    // A scheduled wakeup (ScheduleWakeup, CronCreate) prints nothing; what it
    // will do when it fires is its label.
    for item in stat
        .get("wakeups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = item.get("id").filter(|v| truthy(Some(v))) {
            let label = item.get("label").filter(|v| truthy(Some(v))).cloned().unwrap_or(json!(""));
            out.push(json!({"id": id, "kind": "wakeup", "label": label.clone(),
                            "foreground": false,
                            "started": item.get("started").cloned().unwrap_or(Value::Null),
                            "size": 0, "mtime": null, "text": label,
                            "fires_at": item.get("fires_at").cloned().unwrap_or(Value::Null),
                            "recurring": truthy(item.get("recurring"))}));
        }
    }
    if !foreground.is_empty() {
        let started = |i: &Value| i.get("started").and_then(Value::as_f64).unwrap_or(0.0);
        foreground.sort_by(|a, b| {
            started(a)
                .partial_cmp(&started(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let earliest = foreground.iter().map(started).fold(f64::INFINITY, f64::min) - SLACK;
        let mut loose: Vec<(f64, String, PathBuf)> = files
            .iter()
            .filter(|(tid, _)| !known.contains(*tid) && !tid.starts_with('a'))
            .map(|(tid, p)| (created(p).unwrap_or(0.0), tid.clone(), p.clone()))
            .collect();
        loose.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.cmp(&b.1))
        });
        let mut loose: Vec<_> = loose.into_iter().filter(|x| x.0 >= earliest).collect();
        if loose.len() > foreground.len() {
            loose.drain(..loose.len() - foreground.len());
        }
        for (i, item) in foreground.iter().enumerate() {
            match loose.get(i) {
                Some((_, tid, p)) => add(&mut out, &json!(tid), item, "shell", Some(p)),
                None => add(
                    &mut out,
                    item.get("id").unwrap_or(&Value::Null),
                    item,
                    "shell",
                    None,
                ),
            }
        }
    }
    let started = |t: &Value| t.get("started").and_then(Value::as_f64).unwrap_or(0.0);
    out.sort_by(|a, b| {
        started(a)
            .partial_cmp(&started(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_output_is_cleaned() {
        assert_eq!(clean("\x1b[31mred\x1b[0m\r\nnext"), "red\nnext");
        assert_eq!(clean("10%\r50%\r100%"), "100%");
    }

    #[test]
    fn a_subagent_is_told_as_lines() {
        let entries = vec![
            json!({"type": "assistant", "parentUuid": "p", "message": {"content": [
                {"type": "text", "text": " looking "}, {"type": "tool_use", "name": "Grep", "input": {"pattern": "x"}}]}}),
            json!({"type": "user", "parentUuid": "p", "message": {"content": [
                {"type": "tool_result", "is_error": true, "content": [{"type": "text", "text": "no match\nmore"}]}]}}),
            json!({"type": "user", "parentUuid": null, "message": {"content": [{"type": "text", "text": "the task"}]}}),
        ];
        assert_eq!(story(&entries), "looking\n> Grep x\n  x no match\n");
    }
}
