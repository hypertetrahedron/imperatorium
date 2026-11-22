//! One session in numbers, read off its transcript: a port of
//! `imperatorium/sessionstat.py` (`Fold`, `summary`, the incremental
//! `Reader`). Checked against it on every transcript by
//! `rust/conformance/stats_compare.py`.
//!
//! Python's dicts keep insertion order and keep a key's place when it is
//! assigned again, and its deletions keep the others in order; `IndexMap`
//! with `shift_remove` does the same, so every list comes out in Python's
//! order. Numbers stay ints until a float joins them, as in Python.

use crate::locks::LockExt;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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

/// A teammate's message to its lead: (its name, the body).
fn teammate_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?s)<teammate-message\s+teammate_id="([^"]*)"[^>]*>(.*?)</teammate-message>"#).unwrap()
    })
}

/// What a teammate says when it has gone for good.
const TEAMMATE_ENDS: [&str; 2] = ["shutdown_approved", "teammate_terminated"];

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
    /// a teammate's name -> its agentId
    teammates: HashMap<String, String>,
    /// agentId of a teammate waiting for a message -> since when
    idle: HashMap<String, Option<f64>>,
    mcp: Vec<String>,
    hook_errors: Vec<Value>,
    /// when the Stop hooks last ran without an error
    hooks_ok_at: Option<f64>,
    seen_msgs: HashSet<String>,
    tokens: [Num; 4], // input, output, cache_read, cache_write
    prompts: HashSet<String>,
    /// when the newest prompt (a promptId not seen before) arrived
    last_prompt_at: Option<f64>,
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
        } else if kind == Some("derived-from") {
            // `claude --resume X --fork-session` writes
            // {"type":"derived-from","sessions":[{"id":X,"via":"fork"}, ...]}
            // ("compact" ones name the session itself): the newest fork is
            // the session this one branched off.
            let own = entry.get("sessionId").and_then(Value::as_str);
            let parent = entry
                .get("sessions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|s| s.get("via").and_then(Value::as_str) == Some("fork"))
                .filter_map(|s| s.get("id").and_then(Value::as_str))
                .rfind(|id| !id.is_empty() && Some(*id) != own);
            if let Some(p) = parent {
                self.out.insert("forked_from".into(), json!(p));
            }
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
                    // The Stop hooks that ran, so the window can say which
                    // to look at.
                    let hooks: Vec<Value> = entry
                        .get("hookInfos")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|h| h.get("command").and_then(Value::as_str))
                        .map(|c| json!(take(c, 200)))
                        .collect();
                    let errs = entry.get("hookErrors").and_then(Value::as_array);
                    if errs.is_none_or(|e| e.is_empty()) {
                        self.hooks_ok_at = at.or(self.hooks_ok_at);
                    }
                    for err in errs.into_iter().flatten() {
                        let text = match err {
                            Value::String(s) => s.clone(),
                            other => crate::pyish::json_dumps_sorted(other),
                        };
                        let text: String = split_ws(&text).join(" ").chars().take(300).collect();
                        let mut rec = json!({"at": at, "event": "Stop", "error": text});
                        if !hooks.is_empty() {
                            rec["hooks"] = json!(hooks);
                        }
                        self.hook_errors.push(rec);
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
            // A failed turn is written as a `<synthetic>` reply with no
            // usage ("Please run /login", API errors): nothing was sent or
            // cached, so it is not the last reply (2026-11-18: a fresh
            // session's first prompt failing showed a cache countdown).
            let synthetic = truthy(entry.get("isApiErrorMessage"))
                || msg.get("model").and_then(Value::as_str) == Some("<synthetic>");
            if !usage.is_empty() && !synthetic && !self.seen_msgs.contains(&mid) {
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
                        if name.as_str() == Some("SendMessage") {
                            let to = input.as_object().and_then(|i| i.get("to")).and_then(Value::as_str);
                            if let Some(aid) = to.and_then(|t| self.teammates.get(t)) {
                                self.idle.remove(aid);
                            }
                        }
                        // dict assignment: a repeated id keeps its place.
                        self.calls.insert(k, (id, name, input, at));
                    }
                }
            }
        } else if matches!(kind, Some("user") | Some("attachment")) {
            if kind == Some("user")
                && !side
                && truthy(entry.get("promptId"))
                && self.prompts.insert(key_of(entry.get("promptId")))
            {
                self.last_prompt_at = at.or(self.last_prompt_at);
            }
            if let Some(Value::Array(blocks)) = content {
                for b in blocks {
                    let Some(b) = b.as_object() else { continue };
                    if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                        let k = key_of(b.get("tool_use_id"));
                        self.returned(entry, b, &k, at);
                        self.started(entry, &k, at);
                        self.teammate(entry, &k, at);
                        self.calls.shift_remove(&k);
                    }
                }
            }
            let (note, task_id, status) = res();
            for text in texts(entry) {
                for m in teammate_re().captures_iter(&text) {
                    let Some(aid) = self.teammates.get(&m[1]).cloned() else { continue };
                    let said: Option<Value> = serde_json::from_str(strip(&m[2])).ok();
                    let Some(said) = said.as_ref().and_then(Value::as_object) else { continue };
                    match said.get("type").and_then(Value::as_str) {
                        Some("idle_notification") => {
                            self.idle.insert(aid, at);
                        }
                        Some(t) if TEAMMATE_ENDS.contains(&t) => {
                            self.ended.insert(aid.clone());
                            self.ended_at.insert(aid.clone(), (at, "completed".into()));
                            self.idle.remove(&aid);
                        }
                        _ => {}
                    }
                }
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

    /// Fold._teammate: a teammate spawned, or stopped by name.
    fn teammate(&mut self, entry: &Map<String, Value>, call: &str, at: Option<f64>) {
        let (name, input) = match self.calls.get(call) {
            Some((_, n, i, _)) => (n.clone(), i.clone()),
            None => (Value::Null, Value::Null),
        };
        let Some(result) = entry.get("toolUseResult").and_then(Value::as_object) else { return };
        let name = name.as_str();
        if matches!(name, Some("Agent") | Some("Task"))
            && result.get("status").and_then(Value::as_str) == Some("teammate_spawned")
            && truthy(result.get("agentId"))
        {
            let aid = py_str(&result["agentId"]);
            let who = result
                .get("name")
                .filter(|v| truthy(Some(v)))
                .cloned()
                .unwrap_or_else(|| json!(aid));
            self.teammates.insert(py_str(&who), aid.clone());
            let label_v = result
                .get("description")
                .filter(|v| truthy(Some(v)))
                .cloned()
                .unwrap_or_else(|| json!(label(Some(&input))));
            let task: Map<String, Value> = [
                ("kind", json!("agents")),
                ("label", label_v),
                ("started", at.map(|a| json!(a)).unwrap_or(Value::Null)),
                ("model", result.get("resolvedModel").cloned().unwrap_or(Value::Null)),
                ("teammate", who),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
            self.tasks.insert(aid, task);
        } else if name == Some("TaskStop") {
            let stopped = result.get("task_id").and_then(Value::as_str);
            if let Some(aid) = stopped.and_then(|t| self.teammates.get(t)).cloned() {
                self.ended.insert(aid.clone());
                self.ended_at.insert(aid.clone(), (at, "stopped".into()));
                self.idle.remove(&aid);
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
        if matches!(
            result.and_then(|r| r.get("status")).and_then(Value::as_str),
            Some("async_launched") | Some("teammate_spawned")
        ) {
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
        let mut idle: Vec<Value> = Vec::new();
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
            if let Some(since) = self.idle.get(tid) {
                // A teammate waiting for a message: alive, but nothing to wait on.
                item.insert("idle_since".into(), since.map(|a| json!(a)).unwrap_or(Value::Null));
                idle.push(Value::Object(item));
                continue;
            }
            // The fold only ever files a task under one of the four kinds;
            // anything else (Python would raise KeyError and lose the whole
            // summary) is left out rather than panicking.
            let kind = task["kind"].as_str().unwrap_or("shells");
            if let Some(list) = running.get_mut(kind) {
                list.push(Value::Object(item));
            }
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
        out.insert("idle_agents".into(), Value::Array(idle));
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
        if let (Some(ok), false) = (self.hooks_ok_at, self.hook_errors.is_empty()) {
            out.insert("hooks_ok_at".into(), json!(ok));
        }
        out.insert(
            "tokens".into(),
            json!({"input": self.tokens[0].json(), "output": self.tokens[1].json(),
                   "cache_read": self.tokens[2].json(), "cache_write": self.tokens[3].json()}),
        );
        out.insert("prompts".into(), json!(self.prompts.len()));
        if let Some(a) = self.last_prompt_at {
            out.insert("last_prompt_at".into(), json!(a));
        }
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
    crate::transcript::objects(text, crate::transcript::Split::Newline)
}

/// How long a found transcript path is trusted (while it still exists)
/// before the folders are looked through again, and how long a miss is.
const PATH_TRUSTED: Duration = Duration::from_secs(30);
const MISS_TRUSTED: Duration = Duration::from_secs(2);

/// sessionstat.transcript_path: the newest projects/*/<id>.jsonl.
///
/// Remembered, so a poll does not list every project folder: a path found
/// is used again while it exists, for up to PATH_TRUSTED, and a miss is
/// believed for MISS_TRUSTED. The look itself, and so which copy wins when
/// an id is in two folders, is Python's.
pub fn transcript_path(sid: &str) -> Option<PathBuf> {
    type Memo = HashMap<(PathBuf, String), (Instant, Option<PathBuf>)>;
    static MEMO: OnceLock<Mutex<Memo>> = OnceLock::new();
    if sid.len() != 36 || !sid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return None;
    }
    let root = crate::paths::projects_dir();
    let key = (root, sid.to_string());
    let memo = MEMO.get_or_init(Default::default);
    if let Some((at, known)) = memo.lk().get(&key) {
        match known {
            Some(p) if at.elapsed() < PATH_TRUSTED && p.exists() => return Some(p.clone()),
            None if at.elapsed() < MISS_TRUSTED => return None,
            _ => {}
        }
    }
    let found = crate::transcript::locate_newest(&key.0, sid);
    let mut m = memo.lk();
    if m.len() >= 1024 {
        m.retain(|_, (at, _)| at.elapsed() < PATH_TRUSTED);
    }
    m.insert(key, (Instant::now(), found.clone()));
    found
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
        let known = self.cache.lk().get(sid).map(|h| h.0.clone());
        let path = match known.filter(|p| p.exists()) {
            Some(p) => p,
            None => transcript_path(sid)?,
        };
        let mut cache = self.cache.lk();
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
                let chunk = crate::transcript::read_delta(&path, hit.1)?;
                let end = crate::transcript::whole_lines(&chunk);
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
    let end = crate::transcript::whole_lines(&text);
    let mut fold = Fold::new();
    fold.feed(&parse(&String::from_utf8_lossy(&text[..end])));
    Some(fold.summary(now))
}

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
    fn a_teammate_runs_until_idle_and_again_when_messaged() {
        let mut f = Fold::new();
        let mate = |at: &str, body: &str| {
            e(json!({"type": "user", "timestamp": at, "message": {"content": format!(
                "Another Claude session sent a message:\n<teammate-message teammate_id=\"waiter-1\" \
                 color=\"blue\">\n{body}\n</teammate-message>\n\nThis came from another Claude session.")}}))
        };
        f.feed(&[
            e(json!({"type": "assistant", "timestamp": "2026-11-15T01:58:38Z", "message": {"id": "m1",
                "content": [{"type": "tool_use", "id": "a1", "name": "Agent",
                             "input": {"description": "Wait 30s then OK", "name": "waiter-1"}}]}})),
            e(json!({"type": "user", "timestamp": "2026-11-15T01:58:38Z",
                "toolUseResult": {"status": "teammate_spawned", "agentId": "waiter-1-abc",
                                  "name": "waiter-1", "resolvedModel": "claude-haiku-5-5"},
                "message": {"content": [{"type": "tool_result", "tool_use_id": "a1"}]}})),
        ]);
        let s = f.summary(0.0);
        assert_eq!(s["agents"][0]["id"], "waiter-1-abc");
        assert_eq!(s["agents"][0]["teammate"], "waiter-1");
        assert_eq!(s["finished"], json!([]));
        f.feed(&[mate("2026-11-15T01:58:46Z", r#"{"type":"idle_notification","result":"OK"}"#)]);
        let s = f.summary(0.0);
        assert_eq!(s["agents"], json!([]));
        assert_eq!(s["idle_agents"][0]["id"], "waiter-1-abc");
        f.feed(&[e(json!({"type": "assistant", "timestamp": "2026-11-15T02:02:50Z", "message": {"id": "m2",
            "content": [{"type": "tool_use", "id": "s1", "name": "SendMessage",
                         "input": {"to": "waiter-1", "message": "again"}}]}}))]);
        let s = f.summary(0.0);
        assert_eq!(s["agents"].as_array().map(Vec::len), Some(1));
        assert_eq!(s["idle_agents"], json!([]));
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

/// test_sessionstat.py, ported: the same transcript entries and the same
/// expected numbers (the incremental `Reader` over a real folder is
/// tests/sessionstat_reader.rs, through the dispatcher).
#[cfg(test)]
mod py_ported {
    use super::*;

    const AT0: f64 = 1791972000.0; // 2026-10-14T10:00:00Z

    fn ts(sec: u32) -> String {
        let t = 10 * 3600 + sec;
        format!("2026-10-14T{:02}:{:02}:{:02}.000Z", t / 3600, t / 60 % 60, t % 60)
    }

    fn m(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn call_full(cid: &str, name: &str, inp: Value, sec: u32, mid: &str, usage: Value, side: bool) -> Map<String, Value> {
        m(json!({"type": "assistant", "timestamp": ts(sec), "isSidechain": side,
                 "message": {"id": mid, "model": "claude-opus-5-5", "usage": usage,
                             "content": [{"type": "tool_use", "id": cid, "name": name, "input": inp}]}}))
    }
    fn call(cid: &str, name: &str, inp: Value, sec: u32, mid: &str) -> Map<String, Value> {
        call_full(cid, name, inp, sec, mid, json!({}), false)
    }
    fn result_full(cid: &str, sec: u32, tur: Value, text: &str) -> Map<String, Value> {
        m(json!({"type": "user", "timestamp": ts(sec), "toolUseResult": tur,
                 "message": {"role": "user", "content": [
                     {"type": "tool_result", "tool_use_id": cid, "content": text}]}}))
    }
    fn result(cid: &str, sec: u32, tur: Value) -> Map<String, Value> {
        result_full(cid, sec, tur, "ok")
    }
    fn note(task_id: &str, status: &str, sec: u32, queued: bool) -> Map<String, Value> {
        let text = format!(
            "<task-notification>\n<task-id>{task_id}</task-id>\n<status>{status}</status>\n\
             <summary>done</summary>\n</task-notification>"
        );
        if queued {
            m(json!({"type": "attachment", "timestamp": ts(sec),
                     "attachment": {"type": "queued_command", "prompt": text}}))
        } else {
            m(json!({"type": "user", "timestamp": ts(sec), "message": {"role": "user", "content": text}}))
        }
    }
    fn run(entries: &[Map<String, Value>], now: f64) -> Value {
        let mut f = Fold::new();
        f.feed(entries);
        f.summary(now)
    }
    fn run_(entries: &[Map<String, Value>]) -> Value {
        run(entries, AT0 + 200.0)
    }
    fn ids(list: &Value, key: &str) -> Vec<String> {
        list.as_array().unwrap().iter().map(|x| x[key].as_str().unwrap().to_string()).collect()
    }
    fn f(v: &Value) -> f64 {
        v.as_f64().unwrap()
    }

    // class Background

    #[test]
    fn a_backgrounded_command_runs_until_its_notification() {
        let mut e = vec![
            call("c1", "Bash", json!({"command": "make", "description": "build it"}), 0, "m1"),
            result("c1", 1, json!({"backgroundTaskId": "b1"})),
        ];
        let got = run_(&e);
        let shells = got["shells"].as_array().unwrap();
        assert_eq!(shells.len(), 1);
        assert_eq!((shells[0]["id"].as_str(), shells[0]["label"].as_str()), (Some("b1"), Some("build it")));
        e.push(note("b1", "completed", 100, false));
        assert_eq!(run_(&e)["shells"], json!([]));
    }

    #[test]
    fn a_notification_that_lands_mid_turn_counts() {
        let e = vec![
            call("c1", "Bash", json!({"command": "x"}), 0, "m1"),
            result("c1", 1, json!({"backgroundTaskId": "b1"})),
            note("b1", "failed", 100, true),
        ];
        assert_eq!(run_(&e)["shells"], json!([]));
    }

    #[test]
    fn a_monitor_ends_at_its_timeout_or_on_task_stop() {
        let mut e = vec![
            call("c1", "Monitor", json!({"description": "watch log"}), 0, "m1"),
            result("c1", 1, json!({"taskId": "mon", "timeoutMs": 300000, "persistent": false})),
        ];
        let got = run_(&e);
        assert_eq!(f(&got["monitors"][0]["expires"]), AT0 + 1.0 + 300.0);
        assert_eq!(run(&e, AT0 + 400.0)["monitors"], json!([]));
        e.push(call("c2", "TaskStop", json!({"task_id": "mon"}), 50, "m2"));
        e.push(result("c2", 51, json!({"task_id": "mon", "message": "stopped"})));
        assert_eq!(run_(&e)["monitors"], json!([]));
    }

    #[test]
    fn a_persistent_monitor_has_no_end() {
        let e = vec![
            call("c1", "Monitor", json!({}), 0, "m1"),
            result("c1", 1, json!({"taskId": "m", "timeoutMs": 1000, "persistent": true})),
        ];
        assert_eq!(run(&e, AT0 + 99999.0)["monitors"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_monitor_event_is_not_its_end() {
        let e = vec![
            call("c1", "Monitor", json!({}), 0, "m1"),
            result("c1", 1, json!({"taskId": "m", "timeoutMs": 10_000_000})),
            m(json!({"type": "user", "timestamp": ts(5), "message": {"role": "user", "content":
                "<task-notification>\n<task-id>m</task-id>\n<summary>Monitor event</summary>\n\
                 <event>line</event>\n</task-notification>"}})),
        ];
        assert_eq!(run_(&e)["monitors"].as_array().unwrap().len(), 1);
    }

    /// Shapes copied from real transcripts (Claude Code 2.1.29x).
    fn wake(cid: &str, sec: u32, tur: Value, inp: Option<Value>) -> Vec<Map<String, Value>> {
        let inp = inp.unwrap_or_else(|| json!({"delaySeconds": 1200, "reason": "watching CI", "prompt": "/babysit"}));
        vec![call(cid, "ScheduleWakeup", inp, sec, cid), result(cid, sec + 1, tur)]
    }

    #[test]
    fn a_wakeup_is_pending_until_it_fires() {
        let e = wake("w1", 0, json!({"scheduledFor": (AT0 + 1200.0) * 1000.0, "clampedDelaySeconds": 1200, "wasClamped": false}), None);
        let got = run_(&e);
        let w = got["wakeups"].as_array().unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!((w[0]["id"].as_str(), w[0]["label"].as_str()), (Some("wakeup"), Some("watching CI")));
        assert_eq!(f(&w[0]["fires_at"]), AT0 + 1200.0);
        assert_eq!(run(&e, AT0 + 1201.0)["wakeups"], json!([]));
    }

    #[test]
    fn a_new_wakeup_replaces_the_last_and_stop_cancels_it() {
        let mut e = wake("w1", 0, json!({"scheduledFor": (AT0 + 1200.0) * 1000.0}), None);
        e.extend(wake("w2", 10, json!({"scheduledFor": (AT0 + 600.0) * 1000.0}), None));
        let fires: Vec<f64> = run_(&e)["wakeups"].as_array().unwrap().iter().map(|w| f(&w["fires_at"])).collect();
        assert_eq!(fires, vec![AT0 + 600.0]);
        e.extend(wake("w3", 20, json!({"scheduledFor": 0, "stopped": true, "cancelledWakeups": 1}), Some(json!({"stop": true}))));
        assert_eq!(run_(&e)["wakeups"], json!([]));
        e.extend(wake("w4", 30, json!({"scheduledFor": (AT0 + 900.0) * 1000.0}), None));
        assert_eq!(run_(&e)["wakeups"].as_array().unwrap().len(), 1);
    }

    fn cron(cid: &str, recurring: bool) -> Vec<Map<String, Value>> {
        vec![
            call(cid, "CronCreate", json!({"cron": "0 15 20 10 *", "recurring": recurring, "prompt": "write the summary"}), 0, cid),
            result(cid, 1, json!({"id": "197ff261", "humanSchedule": "0 15 20 10 *", "recurring": recurring,
                                  "durable": false, "nextFireAt": (AT0 + 500.0) * 1000.0})),
        ]
    }

    #[test]
    fn a_one_shot_cron_ends_when_it_fires() {
        let e = cron("c1", false);
        let got = run_(&e);
        let w = got["wakeups"].as_array().unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0]["label"], "write the summary");
        assert_eq!(f(&w[0]["fires_at"]), AT0 + 500.0);
        assert_eq!(run(&e, AT0 + 501.0)["wakeups"], json!([]));
    }

    #[test]
    fn a_recurring_cron_lasts_until_deleted() {
        let mut e = cron("c1", true);
        assert_eq!(run(&e, AT0 + 99999.0)["wakeups"].as_array().unwrap().len(), 1);
        e.push(call("c2", "CronDelete", json!({"id": "197ff261"}), 50, "m2"));
        e.push(result_full("c2", 51, Value::Null, "Cancelled job 197ff261."));
        assert_eq!(run_(&e)["wakeups"], json!([]));
    }

    #[test]
    fn background_and_foreground_subagents() {
        let mut e = vec![
            call("a1", "Agent", json!({"description": "research"}), 0, "m1"),
            result("a1", 1, json!({"status": "async_launched", "agentId": "ag1", "description": "research", "resolvedModel": "sonnet"})),
            call("a2", "Agent", json!({"description": "inline"}), 2, "m2"),
        ];
        let got = run_(&e);
        let mut seen: Vec<(String, bool)> = got["agents"].as_array().unwrap().iter()
            .map(|a| (a["label"].as_str().unwrap().to_string(), truthy(a.get("foreground"))))
            .collect();
        seen.sort();
        assert_eq!(seen, vec![("inline".to_string(), true), ("research".to_string(), false)]);
        e.push(note("ag1", "completed", 100, false));
        assert_eq!(run_(&e)["agents"][0]["label"], "inline");
    }

    fn mate_says(name: &str, body: &str, sec: u32) -> Map<String, Value> {
        let text = format!(
            "Another Claude session sent a message:\n<teammate-message teammate_id=\"{name}\" \
             color=\"blue\">\n{body}\n</teammate-message>\n\nThis came from another Claude \
             session - not typed by your user."
        );
        m(json!({"type": "user", "timestamp": ts(sec), "message": {"role": "user", "content": text}}))
    }

    #[test]
    fn a_teammate_runs_until_idle_and_again_when_messaged() {
        let spawned = json!({"status": "teammate_spawned", "agentId": "waiter-1-abc", "name": "waiter-1",
                             "resolvedModel": "claude-haiku-5-5", "color": "blue"});
        let mut e = vec![
            call("a1", "Agent", json!({"description": "Wait 30s then OK", "name": "waiter-1"}), 0, "m1"),
            result_full("a1", 1, spawned, "Spawned successfully."),
        ];
        let got = run_(&e);
        // Its call returned at once, but it is working: not finished, running.
        let a = &got["agents"][0];
        assert_eq!(got["agents"].as_array().unwrap().len(), 1);
        assert_eq!((a["id"].as_str(), a["teammate"].as_str(), a["label"].as_str()),
                   (Some("waiter-1-abc"), Some("waiter-1"), Some("Wait 30s then OK")));
        assert_eq!(got["idle_agents"], json!([]));
        assert!(got["finished"].as_array().unwrap().iter().all(|x| x["kind"] != "agents"));
        let idle = r#"{"type":"idle_notification","from":"waiter-1","idleReason":"available","result":"OK"}"#;
        e.push(mate_says("waiter-1", idle, 20));
        let got = run_(&e);
        assert_eq!(got["agents"], json!([]));
        assert_eq!(got["idle_agents"].as_array().unwrap().len(), 1);
        assert_eq!(got["idle_agents"][0]["id"], "waiter-1-abc");
        assert_eq!(f(&got["idle_agents"][0]["idle_since"]), AT0 + 20.0);
        // A message to it sets it working; a plain message from it does not idle it.
        e.push(call("s1", "SendMessage", json!({"to": "waiter-1", "message": "again"}), 30, "m2"));
        e.push(result("s1", 31, Value::Null));
        e.push(mate_says("waiter-1", "still on it", 40));
        let got = run_(&e);
        assert_eq!(ids(&got["agents"], "id"), vec!["waiter-1-abc"]);
        assert_eq!(got["idle_agents"], json!([]));
        // Stopped by name: gone, and listed as stopped.
        e.push(call("k1", "TaskStop", json!({"task_id": "waiter-1"}), 50, "m3"));
        e.push(result("k1", 51, json!({"task_id": "waiter-1"})));
        let got = run_(&e);
        assert_eq!((&got["agents"], &got["idle_agents"]), (&json!([]), &json!([])));
        let fin: Vec<(String, String)> = got["finished"].as_array().unwrap().iter()
            .filter(|x| x["kind"] == "agents")
            .map(|x| (x["id"].as_str().unwrap().to_string(), x["status"].as_str().unwrap().to_string()))
            .collect();
        assert_eq!(fin, vec![("waiter-1-abc".to_string(), "stopped".to_string())]);
    }

    #[test]
    fn messages_from_unknown_teammates_change_nothing() {
        let got = run_(&[mate_says("someone", r#"{"type":"idle_notification"}"#, 5)]);
        assert_eq!((&got["agents"], &got["idle_agents"]), (&json!([]), &json!([])));
    }

    // class Tokens

    fn u1() -> Value {
        json!({"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 900,
               "cache_creation_input_tokens": 90,
               "cache_creation": {"ephemeral_1h_input_tokens": 90, "ephemeral_5m_input_tokens": 0}})
    }
    fn u2() -> Value {
        json!({"input_tokens": 2, "output_tokens": 7, "cache_read_input_tokens": 990,
               "cache_creation_input_tokens": 8, "cache_creation": {}})
    }
    fn token_entries() -> Vec<Map<String, Value>> {
        vec![
            m(json!({"type": "permission-mode", "permissionMode": "auto"})),
            m(json!({"type": "user", "timestamp": ts(0), "promptId": "p1", "version": "2.1.296",
                     "gitBranch": "main", "message": {"role": "user", "content": "hi"}})),
            call_full("c1", "Read", json!({}), 1, "m1", u1(), false),
            call_full("c2", "Read", json!({}), 1, "m1", u1(), false), // same message, again
            result("c1", 2, Value::Null),
            result("c2", 2, Value::Null),
            call_full("c3", "Read", json!({}), 60, "m2", u2(), false),
            result("c3", 61, Value::Null),
            call_full("s1", "Read", json!({}), 70, "side", u1(), true),
            m(json!({"type": "system", "subtype": "turn_duration", "durationMs": 61000})),
            m(json!({"type": "system", "subtype": "compact_boundary"})),
        ]
    }

    #[test]
    fn usage_is_counted_once_per_message_across_the_chain() {
        let got = run(&token_entries(), AT0 + 100.0);
        assert_eq!(got["tokens"], json!({"input": 22, "output": 17, "cache_read": 2790, "cache_write": 188}));
    }

    #[test]
    fn cache_lifetime_and_context_come_from_the_main_chain() {
        let got = run(&token_entries(), AT0 + 100.0);
        // The 1h write from m1 still sets the lifetime; the clock runs from m2,
        // not from the subagent's sidechain message after it.
        assert_eq!(f(&got["cache"]["ttl"]), 3600.0);
        assert_eq!(f(&got["cache"]["expires_at"]), AT0 + 60.0 + 3600.0);
        assert_eq!(got["cache"]["warm"], true);
        assert_eq!(got["context_tokens"], 1000);
        assert!((f(&got["cache"]["hit"]) - 0.99).abs() < 1e-7);
        assert_eq!(run(&token_entries(), AT0 + 5000.0)["cache"]["warm"], false);
    }

    #[test]
    fn the_rest_of_the_summary() {
        let got = run(&token_entries(), AT0 + 100.0);
        assert_eq!((&got["prompts"], &got["turns"], &got["last_turn_ms"], &got["compactions"]),
                   (&json!(1), &json!(1), &json!(61000), &json!(1)));
        assert_eq!((&got["permission_mode"], &got["version"], &got["branch"], &got["model_id"]),
                   (&json!("auto"), &json!("2.1.296"), &json!("main"), &json!("claude-opus-5-5")));
        assert_eq!(f(&got["started"]), AT0);
        assert_eq!(f(&got["last_prompt_at"]), AT0);
    }

    /// 2026-11-21, the shape Claude Code 2.1.330 writes: a fork of a
    /// compacted session names the parent twice - via "compact" (carried
    /// over) and via "fork"; a session's own compaction names itself.
    #[test]
    fn a_fork_names_the_session_it_branched_off() {
        let fork = m(json!({"type": "derived-from", "v": 1, "sessionId": "child",
            "sessions": [{"type": "claude_code_session", "id": "parent", "via": "compact"},
                         {"type": "claude_code_session", "id": "parent", "via": "fork"}]}));
        assert_eq!(run(&[fork], AT0)["forked_from"], "parent");
        let own = m(json!({"type": "derived-from", "v": 1, "sessionId": "solo",
            "sessions": [{"type": "claude_code_session", "id": "solo", "via": "compact"}]}));
        assert!(run(&[own], AT0).get("forked_from").is_none());
        let twice = m(json!({"type": "derived-from", "v": 1, "sessionId": "c2",
            "sessions": [{"id": "grand", "via": "fork"}, {"id": "parent", "via": "fork"}]}));
        assert_eq!(run(&[twice], AT0)["forked_from"], "parent");
    }

    /// 2026-11-18, from a real transcript: a /clear, then a first prompt that
    /// failed with "Please run /login". The failed turn is written as a
    /// `<synthetic>` reply with zero usage - no reply, so no cache clock.
    #[test]
    fn a_synthetic_error_reply_is_not_a_reply() {
        let synthetic = m(json!({"type": "assistant", "timestamp": ts(5), "isApiErrorMessage": true,
            "error": "authentication_failed",
            "message": {"id": "e1", "model": "<synthetic>", "role": "assistant",
                "usage": {"input_tokens": 0, "output_tokens": 0, "cache_creation_input_tokens": 0,
                          "cache_read_input_tokens": 0,
                          "cache_creation": {"ephemeral_1h_input_tokens": 0, "ephemeral_5m_input_tokens": 0}},
                "content": [{"type": "text", "text": "Login reached its time limit · Please run /login"}]}}));
        let prompt = m(json!({"type": "user", "timestamp": ts(0), "promptId": "p1",
                              "message": {"role": "user", "content": "hi"}}));
        let got = run(&[prompt.clone(), synthetic.clone()], AT0 + 10.0);
        assert!(got.get("cache").is_none(), "{got}");
        assert!(got.get("last_reply_at").is_none(), "{got}");
        assert!(got.get("model_id").is_none(), "{got}");
        // After a real reply, a later failure leaves that reply's clock alone.
        let mut entries = token_entries();
        let mut late = synthetic.clone();
        late.insert("timestamp".into(), json!(ts(500)));
        entries.push(late);
        let got = run(&entries, AT0 + 600.0);
        assert_eq!(f(&got["last_reply_at"]), AT0 + 60.0);
        assert_eq!(got["model_id"], "claude-opus-5-5");
    }

    #[test]
    fn a_hook_error_names_the_stop_hooks_that_ran() {
        let summary = |infos: Value| m(json!({"type": "system", "subtype": "stop_hook_summary",
            "timestamp": ts(3), "hookInfos": infos, "hookErrors": ["exit code 1: boom"]}));
        let got = run(&[summary(json!([{"command": "imperatorium"}, {"command": "npx lint"}]))], AT0);
        assert_eq!(got["hook_errors"][0]["hooks"], json!(["imperatorium", "npx lint"]));
        assert_eq!(got["hook_errors"][0]["error"], "exit code 1: boom");
        let got = run(&[summary(json!(null))], AT0);
        assert!(got["hook_errors"][0].get("hooks").is_none());
        assert!(got.get("hooks_ok_at").is_none());
        // A later Stop whose hooks ran cleanly: the errors are earlier ones.
        let clean = m(json!({"type": "system", "subtype": "stop_hook_summary", "timestamp": ts(9),
                             "hookInfos": [{"command": "imperatorium"}], "hookErrors": []}));
        let got = run(&[summary(json!(null)), clean.clone()], AT0);
        assert_eq!(f(&got["hooks_ok_at"]), AT0 + 9.0);
        // nothing ever failed: nothing to say
        assert!(run(&[clean], AT0).get("hooks_ok_at").is_none());
    }

    #[test]
    fn the_newest_prompt_is_when_its_id_first_appears() {
        let user = |sec: u32, pid: &str| m(json!({"type": "user", "timestamp": ts(sec), "promptId": pid,
                                                   "message": {"role": "user", "content": "x"}}));
        let mut side = user(40, "p3");
        side.insert("isSidechain".into(), json!(true));
        // p1's tool results carry p1 again: not a new prompt.
        let got = run(&[user(0, "p1"), user(10, "p2"), user(20, "p1"), user(30, "p2"), side], AT0);
        assert_eq!(f(&got["last_prompt_at"]), AT0 + 10.0);
    }

    // class Folding

    #[test]
    fn feeding_in_pieces_gives_the_same_answer() {
        let mut e = token_entries();
        e.extend([
            call("c1", "Bash", json!({"command": "make"}), 80, "m3"),
            result("c1", 81, json!({"backgroundTaskId": "b1"})),
            call("c2", "Agent", json!({"description": "inline"}), 82, "m4"),
            call("c3", "Monitor", json!({}), 83, "m5"),
            result("c3", 84, json!({"taskId": "mon", "timeoutMs": 1_000_000})),
            note("b1", "completed", 90, false),
        ]);
        let whole = run(&e, AT0 + 100.0);
        for cut in 0..=e.len() {
            let mut fold = Fold::new();
            fold.feed(&e[..cut]);
            fold.feed(&e[cut..]);
            assert_eq!(fold.summary(AT0 + 100.0), whole, "cut at {cut}");
        }
    }

    #[test]
    fn a_line_separator_inside_a_string_does_not_split_the_record() {
        // JSON leaves U+2028/U+2029 and form feed raw; only "\n" ends a line.
        let lines = [
            Value::Object(call("c1", "Bash", json!({"command": "grep"}), 0, "m1")),
            Value::Object(result_full("c1", 1, Value::Null, "a\u{2028}b\u{2029}c\x0cd\u{85}e")),
        ];
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let entries = parse(&text);
        assert_eq!(entries.len(), 2);
        assert_eq!(run(&entries, AT0 + 5.0)["shells"], json!([]));
    }

    #[test]
    fn answered_calls_are_not_kept() {
        let mut fold = Fold::new();
        fold.feed(&[call("c1", "Read", json!({}), 0, "m1"), result("c1", 1, Value::Null)]);
        assert!(fold.calls.is_empty());
    }

    // class ReaderTests (what needs no folder)

    #[test]
    fn unknown_or_malformed_id() {
        let r = Reader::new();
        assert!(r.read("../../etc/passwd", AT0).is_none());
        assert!(transcript_path("../../etc/passwd").is_none());
    }

    // class StatuslineExtras

    #[test]
    fn statusline_durations_and_lines_are_kept_when_sent() {
        let payload = m(json!({"cost": {"total_cost_usd": 1.5, "total_duration_ms": 9000,
                                        "total_api_duration_ms": 4000,
                                        "total_lines_added": 12, "total_lines_removed": 3}}));
        let got = crate::usage::reading(&payload);
        // Numbers compare as Python's == does: 9000.0 is 9000.
        let nums: Vec<f64> = ["duration_ms", "api_duration_ms", "lines_added", "lines_removed"]
            .iter().map(|k| got[*k].as_f64().unwrap()).collect();
        assert_eq!(nums, vec![9000.0, 4000.0, 12.0, 3.0]);
        assert!(crate::usage::reading(&Map::new())["lines_added"].is_null());
    }
}
