//! One session in numbers, read off its transcript: a port of
//! `imperatorium/sessionstat.py` (`Fold`, `summary`, the incremental
//! `Reader`). Checked against it on every transcript by
//! `rust/conformance/stats_compare.py`.
//!
//! Python's dicts keep insertion order and keep a key's place when it is
//! assigned again, and its deletions keep the others in order; `IndexMap`
//! with `shift_remove` does the same, so every list comes out in Python's
//! order. Numbers stay ints until a float joins them, as in Python.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde_json::{json, Map, Value};

use crate::pyish::{py_str, split_ws, strip, take, truthy};

const TERMINAL: &[&str] = &["completed", "failed", "killed", "stopped"];
const CACHE_5M: f64 = 300.0;
const CACHE_1H: f64 = 3600.0;
const LABEL_CHARS: usize = 80;
const WAKEUP: &str = "wakeup";
const CRON: &str = "cron:";
/// sessionstat.FINISHED_KEEP / FINISHED_MAX / HOOK_ERRORS_KEEP.
const FINISHED_KEEP: f64 = 1800.0;
const FINISHED_MAX: usize = 20;
const HOOK_ERRORS_KEEP: usize = 10;

/// A Python int or float, summed as Python sums them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Num {
    Int(i64),
    Float(f64),
}

impl Default for Num {
    fn default() -> Self {
        Num::Int(0)
    }
}

impl Num {
    fn add(self, other: Num) -> Num {
        match (self, other) {
            (Num::Int(a), Num::Int(b)) => Num::Int(a.saturating_add(b)),
            (a, b) => Num::Float(a.f() + b.f()),
        }
    }
    fn f(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(f) => f,
        }
    }
    fn json(self) -> Value {
        match self {
            Num::Int(i) => json!(i),
            Num::Float(f) => json!(f),
        }
    }
    fn truthy(self) -> bool {
        self.f() != 0.0
    }
}

/// sessionstat._num: an int or float (not a bool), else 0.
fn num(v: Option<&Value>) -> Num {
    match v {
        Some(Value::Number(n)) => match n.as_i64() {
            Some(i) => Num::Int(i),
            None => Num::Float(n.as_f64().unwrap_or(0.0)),
        },
        _ => Num::Int(0),
    }
}

/// `datetime.fromisoformat(stamp.replace("Z", "+00:00")).timestamp()`.
pub fn when(stamp: Option<&Value>) -> Option<f64> {
    let s = stamp?.as_str()?;
    crate::sessions::iso_seconds(&s.replace('Z', "+00:00"))
}

/// A hashable stand-in for a JSON value used as a dict key (ids are nearly
/// always strings; None and numbers stay distinct from them).
fn key_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => format!("s{s}"),
        Some(other) => format!("v{other}"),
        None => "vnull".into(),
    }
}

fn label(input: Option<&Value>) -> String {
    let Some(Value::Object(ti)) = input else {
        return String::new();
    };
    for key in ["description", "command", "prompt"] {
        if let Some(v) = ti.get(key).filter(|v| truthy(Some(v))) {
            return take(&split_ws(&py_str(v)).join(" "), LABEL_CHARS);
        }
    }
    String::new()
}

fn res() -> &'static (Regex, Regex, Regex) {
    static R: OnceLock<(Regex, Regex, Regex)> = OnceLock::new();
    R.get_or_init(|| {
        (
            Regex::new(r"(?s)<task-notification>(.*?)</task-notification>").unwrap(),
            Regex::new(r"<task-id>(.*?)</task-id>").unwrap(),
            Regex::new(r"<status>(.*?)</status>").unwrap(),
        )
    })
}

#[derive(Default, Clone)]
pub struct Fold {
    /// unanswered tool_use id -> (id, name, input, when it was made)
    calls: IndexMap<String, (Value, Value, Value, Option<f64>)>,
    /// task id -> task (key order as Python builds it)
    tasks: IndexMap<String, Map<String, Value>>,
    ended: IndexSet<String>,
    /// task id -> (when it ended, how)
    ended_at: HashMap<String, (Option<f64>, String)>,
    /// a foreground subagent that returned -> its record
    done_calls: IndexMap<String, Map<String, Value>>,
    mcp: Vec<String>,
    hook_errors: Vec<Value>,
    seen_msgs: HashSet<String>,
    tokens: [Num; 4], // input, output, cache_read, cache_write
    prompts: HashSet<String>,
    last: Option<(Option<f64>, Map<String, Value>, Value)>,
    ttl: f64,
    out: Map<String, Value>,
}

impl Fold {
    pub fn new() -> Self {
        let mut out = Map::new();
        out.insert("compactions".into(), json!(0));
        out.insert("turns".into(), json!(0));
        Fold {
            tokens: [Num::Int(0); 4],
            ttl: CACHE_5M,
            out,
            ..Default::default()
        }
    }

    pub fn feed(&mut self, entries: &[Map<String, Value>]) {
        for e in entries {
            self.one(e);
        }
    }

    fn bump(&mut self, k: &str) {
        let n = self.out.get(k).and_then(Value::as_i64).unwrap_or(0);
        self.out.insert(k.into(), json!(n + 1));
    }

    fn one(&mut self, entry: &Map<String, Value>) {
        let kind = entry.get("type").and_then(Value::as_str);
        let side = truthy(entry.get("isSidechain"));
        let at = when(entry.get("timestamp"));
        if let Some(a) = at.filter(|a| *a != 0.0) {
            if !self.out.contains_key("started") {
                self.out.insert("started".into(), json!(a));
            }
        }
        for (field, name) in [("version", "version"), ("gitBranch", "branch")] {
            if let Some(v) = entry.get(field).filter(|v| truthy(Some(v))) {
                self.out.insert(name.into(), v.clone());
            }
        }
        if kind == Some("permission-mode") && truthy(entry.get("permissionMode")) {
            self.out
                .insert("permission_mode".into(), entry["permissionMode"].clone());
        } else if kind == Some("system") {
            match entry.get("subtype").and_then(Value::as_str) {
                Some("compact_boundary") => self.bump("compactions"),
                Some("turn_duration") if !side => {
                    self.bump("turns");
                    if let Some(d) = entry
                        .get("durationMs")
                        .filter(|d| d.is_number() || d.is_boolean())
                    {
                        self.out.insert("last_turn_ms".into(), d.clone());
                    }
                }
                Some("stop_hook_summary") => {
                    for err in entry.get("hookErrors").and_then(Value::as_array).into_iter().flatten() {
                        let text = match err {
                            Value::String(s) => s.clone(),
                            other => crate::pyish::json_dumps_sorted(other),
                        };
                        let text: String = split_ws(&text).join(" ").chars().take(300).collect();
                        self.hook_errors.push(json!({"at": at, "event": "Stop", "error": text}));
                    }
                    if self.hook_errors.len() > HOOK_ERRORS_KEEP {
                        let cut = self.hook_errors.len() - HOOK_ERRORS_KEEP;
                        self.hook_errors.drain(..cut);
                    }
                }
                _ => {}
            }
        }
        if kind == Some("attachment") {
            if let Some(att) = entry.get("attachment").and_then(Value::as_object) {
                if att.get("type").and_then(Value::as_str) == Some("mcp_instructions_delta") {
                    for n in att.get("addedNames").and_then(Value::as_array).into_iter().flatten() {
                        if let Some(n) = n.as_str() {
                            if !self.mcp.iter().any(|m| m == n) {
                                self.mcp.push(n.to_string());
                            }
                        }
                    }
                    for n in att.get("removedNames").and_then(Value::as_array).into_iter().flatten() {
                        if let Some(n) = n.as_str() {
                            self.mcp.retain(|m| m != n);
                        }
                    }
                }
            }
        }
        let empty = Map::new();
        let msg = entry
            .get("message")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let content = msg.get("content");
        if kind == Some("assistant") {
            let mid = key_of(msg.get("id"));
            let usage = msg
                .get("usage")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if !usage.is_empty() && !self.seen_msgs.contains(&mid) {
                self.seen_msgs.insert(mid);
                for (i, k) in [
                    "input_tokens",
                    "output_tokens",
                    "cache_read_input_tokens",
                    "cache_creation_input_tokens",
                ]
                .iter()
                .enumerate()
                {
                    self.tokens[i] = self.tokens[i].add(num(usage.get(*k)));
                }
                if !side {
                    let made = usage
                        .get("cache_creation")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    if num(made.get("ephemeral_1h_input_tokens")).truthy() {
                        self.ttl = CACHE_1H;
                    } else if num(made.get("ephemeral_5m_input_tokens")).truthy() {
                        self.ttl = CACHE_5M;
                    }
                    self.last = Some((at, usage, msg.get("model").cloned().unwrap_or(Value::Null)));
                }
            }
            if let Some(Value::Array(blocks)) = content {
                for b in blocks {
                    let Some(b) = b.as_object() else { continue };
                    if b.get("type").and_then(Value::as_str) == Some("tool_use") {
                        let id = b.get("id").cloned().unwrap_or(Value::Null);
                        let k = key_of(Some(&id));
                        let name = b.get("name").cloned().unwrap_or(Value::Null);
                        let input = b.get("input").cloned().unwrap_or(Value::Null);
                        // dict assignment: a repeated id keeps its place.
                        self.calls.insert(k, (id, name, input, at));
                    }
                }
            }
        } else if matches!(kind, Some("user") | Some("attachment")) {
            if kind == Some("user") && !side && truthy(entry.get("promptId")) {
                self.prompts.insert(key_of(entry.get("promptId")));
            }
            if let Some(Value::Array(blocks)) = content {
                for b in blocks {
                    let Some(b) = b.as_object() else { continue };
                    if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                        let k = key_of(b.get("tool_use_id"));
                        self.returned(entry, b, &k, at);
                        self.started(entry, &k, at);
                        self.calls.shift_remove(&k);
                    }
                }
            }
            let (note, task_id, status) = res();
            for text in texts(entry) {
                for m in note.captures_iter(&text) {
                    let body = &m[1];
                    if let Some(s) = status.captures(body) {
                        let how = strip(&s[1]).to_string();
                        if TERMINAL.contains(&how.as_str()) {
                            for t in task_id.captures_iter(body) {
                                self.ended.insert(strip(&t[1]).to_string());
                                self.ended_at.insert(strip(&t[1]).to_string(), (at, how.clone()));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Fold._returned: a subagent run in the foreground came back.
    fn returned(&mut self, entry: &Map<String, Value>, block: &Map<String, Value>, call: &str, at: Option<f64>) {
        let Some((id, name, input, made)) = self.calls.get(call).cloned() else { return };
        if !matches!(name.as_str(), Some("Agent") | Some("Task")) {
            return;
        }
        let result = entry.get("toolUseResult").and_then(Value::as_object);
        if result.and_then(|r| r.get("status")).and_then(Value::as_str) == Some("async_launched") {
            return;
        }
        let agent = result
            .and_then(|r| r.get("agentId"))
            .filter(|v| truthy(Some(v)))
            .map(py_str)
            .unwrap_or_else(|| py_str(&id));
        let rec: Map<String, Value> = [
            ("kind", json!("agents")),
            ("label", json!(label(Some(&input)))),
            ("started", made.map(|a| json!(a)).unwrap_or(Value::Null)),
            ("ended", at.map(|a| json!(a)).unwrap_or(Value::Null)),
            ("status", json!(if truthy(block.get("is_error")) { "failed" } else { "completed" })),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        self.done_calls.insert(agent, rec);
    }

    /// Fold.finished: what ended in the last FINISHED_KEEP seconds, newest first.
    fn finished(&self, now: f64) -> Vec<Value> {
        let mut out: Vec<Map<String, Value>> = Vec::new();
        for (tid, task) in &self.tasks {
            let expires = task.get("expires").and_then(Value::as_f64).filter(|e| *e != 0.0);
            let (when, how) = if self.ended.contains(tid) {
                match self.ended_at.get(tid) {
                    Some((w, h)) => (w.map(|w| json!(w)).unwrap_or(Value::Null), h.clone()),
                    None => (Value::Null, "completed".to_string()),
                }
            } else if expires.is_some_and(|e| e <= now) {
                let how = if task.get("kind").and_then(Value::as_str) == Some("wakeups") { "fired" } else { "timed out" };
                (task["expires"].clone(), how.to_string())
            } else {
                continue;
            };
            let mut item = task.clone();
            item.insert("id".into(), json!(tid));
            item.insert("ended".into(), when);
            item.insert("status".into(), json!(how));
            out.push(item);
        }
        for (aid, rec) in &self.done_calls {
            let mut item = rec.clone();
            item.insert("id".into(), json!(aid));
            out.push(item);
        }
        let ended = |m: &Map<String, Value>| m.get("ended").and_then(Value::as_f64).filter(|e| *e != 0.0);
        let mut out: Vec<Map<String, Value>> =
            out.into_iter().filter(|m| ended(m).is_some_and(|e| now - e <= FINISHED_KEEP)).collect();
        out.sort_by(|a, b| ended(b).partial_cmp(&ended(a)).unwrap_or(std::cmp::Ordering::Equal));
        out.into_iter().take(FINISHED_MAX).map(Value::Object).collect()
    }

    /// sessionstat._started: what one tool result put in the background.
    fn started(&mut self, entry: &Map<String, Value>, call: &str, at: Option<f64>) {
        let (name, input) = match self.calls.get(call) {
            Some((_, n, i, _)) => (n.clone(), i.clone()),
            None => (Value::Null, Value::Null),
        };
        let name = name.as_str().map(str::to_string);
        if name.as_deref() == Some("CronDelete") {
            if let Some(id) = input
                .as_object()
                .and_then(|i| i.get("id"))
                .filter(|v| truthy(Some(v)))
            {
                self.ended.insert(format!("{CRON}{}", py_str(id)));
                self.ended_at.insert(format!("{CRON}{}", py_str(id)), (at, "cancelled".into()));
            }
        }
        let Some(result) = entry.get("toolUseResult").and_then(Value::as_object) else {
            return;
        };
        let lbl = label(Some(&input));
        let at_v = at.map(|a| json!(a)).unwrap_or(Value::Null);
        let t = |pairs: Vec<(&str, Value)>| -> Map<String, Value> {
            pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
        };
        if let Some(id) = result.get("backgroundTaskId").filter(|v| truthy(Some(v))) {
            self.tasks.insert(
                py_str(id),
                t(vec![
                    ("kind", json!("shells")),
                    ("label", json!(lbl)),
                    ("started", at_v),
                ]),
            );
        } else if let (Some(id), Some("Monitor")) = (
            result.get("taskId").filter(|v| truthy(Some(v))),
            name.as_deref(),
        ) {
            let timeout = result
                .get("timeoutMs")
                .filter(|v| v.is_number() || v.is_boolean());
            let expires = match (at.filter(|a| *a != 0.0), timeout) {
                (Some(a), Some(to)) if !truthy(result.get("persistent")) => {
                    let ms =
                        to.as_f64()
                            .unwrap_or(if to.as_bool() == Some(true) { 1.0 } else { 0.0 });
                    json!(a + ms / 1000.0)
                }
                _ => Value::Null,
            };
            self.tasks.insert(
                py_str(id),
                t(vec![
                    ("kind", json!("monitors")),
                    ("label", json!(lbl)),
                    ("started", at_v),
                    ("expires", expires),
                ]),
            );
        } else if result.get("status").and_then(Value::as_str) == Some("async_launched")
            && truthy(result.get("agentId"))
        {
            let desc = result
                .get("description")
                .filter(|v| truthy(Some(v)))
                .cloned()
                .unwrap_or(json!(lbl));
            self.tasks.insert(
                py_str(&result["agentId"]),
                t(vec![
                    ("kind", json!("agents")),
                    ("label", desc),
                    ("started", at_v),
                    (
                        "model",
                        result.get("resolvedModel").cloned().unwrap_or(Value::Null),
                    ),
                ]),
            );
        } else if name.as_deref() == Some("ScheduleWakeup") {
            let w = num(result.get("scheduledFor")).f() / 1000.0;
            if truthy(result.get("stopped")) || w == 0.0 {
                self.ended.insert(WAKEUP.into());
                self.ended_at.insert(WAKEUP.into(), (at, "cancelled".into()));
            } else {
                self.ended.shift_remove(WAKEUP);
                self.ended_at.remove(WAKEUP);
                let reason = input
                    .as_object()
                    .and_then(|i| i.get("reason"))
                    .filter(|v| truthy(Some(v)));
                let text = match reason {
                    Some(r) => py_str(r),
                    None => lbl.clone(),
                };
                self.tasks.insert(
                    WAKEUP.into(),
                    t(vec![
                        ("kind", json!("wakeups")),
                        (
                            "label",
                            json!(take(&split_ws(&text).join(" "), LABEL_CHARS)),
                        ),
                        ("started", at_v),
                        ("fires_at", json!(w)),
                        ("expires", json!(w)),
                    ]),
                );
            }
        } else if name.as_deref() == Some("CronCreate")
            && truthy(result.get("id"))
            && (truthy(result.get("recurring")) || num(result.get("nextFireAt")).truthy())
        {
            let w = num(result.get("nextFireAt")).f() / 1000.0;
            let fires = if w != 0.0 { json!(w) } else { Value::Null };
            let recurring = truthy(result.get("recurring"));
            self.tasks.insert(
                format!("{CRON}{}", py_str(&result["id"])),
                t(vec![
                    ("kind", json!("wakeups")),
                    ("label", json!(lbl)),
                    ("started", at_v),
                    ("fires_at", fires.clone()),
                    ("recurring", json!(recurring)),
                    ("expires", if recurring { Value::Null } else { fires }),
                ]),
            );
        }
        if name.as_deref() == Some("TaskStop") {
            if let Some(id) = result.get("task_id").filter(|v| truthy(Some(v))) {
                self.ended.insert(py_str(id));
                self.ended_at.insert(py_str(id), (at, "stopped".into()));
            }
        }
    }

    pub fn summary(&self, now: f64) -> Value {
        let mut out = self.out.clone();
        let mut running: IndexMap<&str, Vec<Value>> = ["agents", "shells", "monitors", "wakeups"]
            .into_iter()
            .map(|k| (k, Vec::new()))
            .collect();
        for (tid, task) in &self.tasks {
            let expires = task
                .get("expires")
                .and_then(Value::as_f64)
                .filter(|e| *e != 0.0);
            if self.ended.contains(tid) || expires.is_some_and(|e| e <= now) {
                continue;
            }
            let mut item = task.clone();
            item.insert("id".into(), json!(tid));
            let kind = task["kind"].as_str().unwrap_or("shells");
            running.get_mut(kind).unwrap().push(Value::Object(item));
        }
        for (id, name, input, at) in self.calls.values() {
            let group = match name.as_str() {
                Some("Agent") | Some("Task") => "agents",
                Some("Bash") | Some("PowerShell") => "shells",
                _ => continue,
            };
            running[group].push(json!({
                "id": id, "label": label(Some(input)), "foreground": true,
                "started": at.map(|a| json!(a)).unwrap_or(Value::Null), "kind": group,
            }));
        }
        for (k, v) in running {
            out.insert(k.into(), Value::Array(v));
        }
        let mut known: Vec<String> = self
            .tasks
            .keys()
            .cloned()
            .chain(self.ended.iter().cloned())
            .collect();
        known.sort();
        known.dedup();
        out.insert("known_tasks".into(), json!(known));
        out.insert("finished".into(), Value::Array(self.finished(now)));
        out.insert("mcp_servers".into(), json!(self.mcp));
        out.insert("hook_errors".into(), Value::Array(self.hook_errors.clone()));
        out.insert(
            "tokens".into(),
            json!({"input": self.tokens[0].json(), "output": self.tokens[1].json(),
                   "cache_read": self.tokens[2].json(), "cache_write": self.tokens[3].json()}),
        );
        out.insert("prompts".into(), json!(self.prompts.len()));
        if let Some((at, usage, model)) = &self.last {
            let read = num(usage.get("cache_read_input_tokens"));
            let context = num(usage.get("input_tokens"))
                .add(read)
                .add(num(usage.get("cache_creation_input_tokens")));
            let at_ok = at.filter(|a| *a != 0.0);
            out.insert("model_id".into(), model.clone());
            out.insert(
                "last_reply_at".into(),
                at.map(|a| json!(a)).unwrap_or(Value::Null),
            );
            out.insert("context_tokens".into(), context.json());
            out.insert(
                "cache".into(),
                json!({
                    "ttl": self.ttl,
                    "expires_at": at_ok.map(|a| json!(a + self.ttl)).unwrap_or(Value::Null),
                    "warm": at_ok.is_some_and(|a| now < a + self.ttl),
                    "hit": if context.truthy() { json!(read.f() / context.f()) } else { Value::Null },
                }),
            );
        }
        Value::Object(out)
    }
}

/// sessionstat._texts: the plain text a user-side entry carries.
fn texts(entry: &Map<String, Value>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(msg) = entry.get("message").and_then(Value::as_object) {
        match msg.get("content") {
            Some(Value::String(s)) => out.push(s.clone()),
            Some(Value::Array(blocks)) => {
                for b in blocks.iter().filter_map(Value::as_object) {
                    if b.get("type").and_then(Value::as_str) == Some("text") {
                        let empty = json!("");
                        out.push(py_str(
                            crate::pyish::or(b.get("text"), Some(&empty)).unwrap(),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(p) = entry
        .get("attachment")
        .and_then(|a| a.get("prompt"))
        .and_then(Value::as_str)
    {
        out.push(p.to_string());
    }
    out
}

/// sessionstat.parse: every JSON object, one per "\n"-ended line.
pub fn parse(text: &str) -> Vec<Map<String, Value>> {
    text.split('\n')
        .map(strip)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| match v {
            Value::Object(o) => Some(o),
            _ => None,
        })
        .collect()
}

/// sessionstat.transcript_path: the newest projects/*/<id>.jsonl.
pub fn transcript_path(sid: &str) -> Option<PathBuf> {
    if sid.len() != 36 || !sid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return None;
    }
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for dir in std::fs::read_dir(crate::sessions::projects_dir())
        .ok()?
        .flatten()
    {
        let p = dir.path().join(format!("{sid}.jsonl"));
        let Ok(m) = std::fs::metadata(&p).and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|b| m > b.1) {
            best = Some((p, m));
        }
    }
    best.map(|b| b.0)
}

/// sessionstat.Reader: a Fold per session, fed only what was appended.
pub struct Reader {
    cache: Mutex<IndexMap<String, (PathBuf, u64, Fold)>>,
}

impl Reader {
    const KEEP: usize = 64;

    pub fn new() -> Self {
        Reader {
            cache: Mutex::new(IndexMap::new()),
        }
    }

    pub fn read(&self, sid: &str, now: f64) -> Option<Value> {
        let known = self.cache.lock().unwrap().get(sid).map(|h| h.0.clone());
        let path = match known.filter(|p| p.exists()) {
            Some(p) => p,
            None => transcript_path(sid)?,
        };
        let mut cache = self.cache.lock().unwrap();
        let mut hit = match cache.shift_remove(sid) {
            Some(h) if h.0 == path => h,
            _ => (path.clone(), 0, Fold::new()),
        };
        let result = (|| -> Option<Value> {
            let size = std::fs::metadata(&path).ok()?.len();
            if size < hit.1 {
                hit.1 = 0;
                hit.2 = Fold::new();
            }
            if size > hit.1 {
                let mut f = std::fs::File::open(&path).ok()?;
                f.seek(SeekFrom::Start(hit.1)).ok()?;
                let mut chunk = vec![0u8; (size - hit.1) as usize];
                let mut got = 0;
                while got < chunk.len() {
                    match f.read(&mut chunk[got..]) {
                        Ok(0) => break,
                        Ok(n) => got += n,
                        Err(_) => return None,
                    }
                }
                chunk.truncate(got);
                let end = chunk
                    .iter()
                    .rposition(|b| *b == b'\n')
                    .map(|i| i + 1)
                    .unwrap_or(0);
                hit.2.feed(&parse(&String::from_utf8_lossy(&chunk[..end])));
                hit.1 += end as u64;
            }
            Some(hit.2.summary(now))
        })();
        cache.insert(sid.to_string(), hit);
        while cache.len() > Self::KEEP {
            cache.shift_remove_index(0);
        }
        result
    }
}

/// Every session's summary straight from its file, for the conformance test.
pub fn summarise_file(sid: &str, now: f64) -> Option<Value> {
    let text = std::fs::read(transcript_path(sid)?).ok()?;
    let end = text
        .iter()
        .rposition(|b| *b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let mut fold = Fold::new();
    fold.feed(&parse(&String::from_utf8_lossy(&text[..end])));
    Some(fold.summary(now))
}

#[allow(dead_code)]
fn _unused(_: HashMap<(), ()>) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn a_background_command_runs_until_its_notification() {
        let mut f = Fold::new();
        f.feed(&[
            e(json!({"type": "assistant", "timestamp": "2026-11-12T10:00:00Z", "message": {"id": "m1",
                "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "make  test"}}]}})),
            e(json!({"type": "user", "toolUseResult": {"backgroundTaskId": "b1"},
                "message": {"content": [{"type": "tool_result", "tool_use_id": "t1"}]}})),
        ]);
        let s = f.summary(0.0);
        assert_eq!(s["shells"][0]["id"], "b1");
        assert_eq!(s["shells"][0]["label"], "make test");
        f.feed(&[e(json!({"type": "user", "message": {"content":
            "<task-notification><task-id>b1</task-id><status>completed</status></task-notification>"}}))]);
        assert_eq!(f.summary(0.0)["shells"], json!([]));
    }

    #[test]
    fn usage_is_counted_once_per_message() {
        let mut f = Fold::new();
        let u = json!({"type": "assistant", "timestamp": "2026-11-12T10:00:00Z", "message": {"id": "m",
            "model": "claude-opus-5-5", "usage": {"input_tokens": 3, "cache_read_input_tokens": 7,
            "cache_creation": {"ephemeral_1h_input_tokens": 1}}}});
        f.feed(&[e(u.clone()), e(u)]);
        let s = f.summary(0.0);
        assert_eq!(s["tokens"]["input"], 3);
        assert_eq!(s["context_tokens"], 10);
        assert_eq!(s["cache"]["ttl"], 3600.0);
        assert_eq!(s["cache"]["hit"], 0.7);
    }
}
