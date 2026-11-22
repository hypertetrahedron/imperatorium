//! Which sessions exist: a port of `imperatorium/registry.py` (`agents`,
//! `session_files`, `merge`, `attach_credentials`, `needs_attention`,
//! `moved_on`, `still_running`, `finished`, `place`, `label`,
//! `short_label`). Read-only: it runs `claude agents`, reads the session
//! files and the dispatcher's state.json, and writes nothing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::paths;
use crate::pyish::{py_str, truthy};

pub type Row = Map<String, Value>;

const ATTENTION_STATUSES: &[&str] = &["waiting"];
const ATTENTION_STATES: &[&str] = &["blocked", "failed"];
pub const ACTIVE_STATUSES: &[&str] = &["busy", "shell"];
const SETTLE_SECONDS: f64 = 3.0;

/// `claude agents --json --all`, or [] when it fails or takes over `timeout`.
pub fn agents(timeout: Duration) -> Vec<Value> {
    let mut cmd = Command::new("claude");
    cmd.args(["agents", "--json", "--all"]);
    let Ok(o) = crate::proc::run_timeout(&mut cmd, timeout) else {
        return vec![];
    };
    if !o.status.success() {
        return vec![];
    }
    match serde_json::from_slice::<Value>(&o.stdout) {
        Ok(Value::Array(rows)) => rows,
        _ => vec![],
    }
}

/// ~/.claude/sessions/*.json, keyed by sessionId.
pub fn session_files() -> HashMap<String, Row> {
    let mut found = HashMap::new();
    let Ok(rd) = std::fs::read_dir(crate::paths::claude_home().join("sessions")) else {
        return found;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(Value::Object(rec)) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(sid) = rec
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            found.insert(sid.to_string(), rec);
        }
    }
    found
}

/// The dispatcher's state.json, read only. {} when absent or unreadable.
pub fn state() -> Row {
    std::fs::read_to_string(paths::state_dir().join("state.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// store.credentials(): {sid: (token, the socket it was recorded against)}.
pub fn credentials(state: &Row) -> HashMap<String, (Value, Value)> {
    let empty = Map::new();
    let tokens = state
        .get("tokens")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let sessions = state
        .get("sessions")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    tokens
        .iter()
        .map(|(sid, token)| {
            let socket = sessions
                .get(sid)
                .and_then(|r| r.get("socket"))
                .cloned()
                .unwrap_or(Value::Null);
            (sid.clone(), (token.clone(), socket))
        })
        .collect()
}

fn s<'a>(r: &'a Row, k: &str) -> Option<&'a str> {
    r.get(k).and_then(Value::as_str)
}

/// registry.TASK_KINDS: what an idle session can still be waiting on.
pub const TASK_KINDS: [&str; 4] = ["shells", "monitors", "agents", "wakeups"];
pub const COMPLETE: &str = "complete";
/// Work only the Stop hook reports: MCP tasks, workflows and kinds to come.
pub const OTHER_TASKS: &str = "other";

/// registry.pending_tasks: {kind: count} of what an idle session has going.
pub fn pending_tasks(row: &Row) -> Map<String, Value> {
    let count = |v: Option<&Value>| match v {
        Some(Value::Array(a)) => a.len() as u64,
        Some(v) => v.as_u64().unwrap_or(0),
        None => 0,
    };
    let work = row.get("work").and_then(Value::as_object);
    // What the last Stop hook listed, by kind: Claude Code's own account,
    // which also sees what the transcript and process list cannot (MCP
    // tasks, workflows). Neither source alone is the whole of it.
    let mut listed: Map<String, Value> = Map::new();
    let mut add = |k: &str| {
        let n = listed.get(k).and_then(Value::as_u64).unwrap_or(0);
        listed.insert(k.into(), json!(n + 1));
    };
    if let Some(tasks) = row.get("stop_background_tasks").and_then(Value::as_array) {
        for t in tasks {
            add(match t.get("type").and_then(Value::as_str) {
                Some("shell") => "shells",
                Some("monitor") => "monitors",
                Some("subagent") => "agents",
                _ => OTHER_TASKS,
            });
        }
    }
    for _ in row.get("stop_crons").and_then(Value::as_array).into_iter().flatten() {
        add("wakeups");
    }
    let mut out = Map::new();
    for k in TASK_KINDS.iter().copied().chain([OTHER_TASKS]) {
        let mut v = count(work.and_then(|w| w.get(k))).max(count(listed.get(k)));
        if k == "shells" {
            v = v.max(count(row.get("background_tasks")));
        }
        if v > 0 {
            out.insert(k.into(), json!(v));
        }
    }
    out
}

/// registry.classify: an idle session is waiting on tasks, complete, or
/// ready (left as it is). See the Python for the reasons.
pub fn classify(row: &mut Row) {
    let s = |row: &Row, k: &str| row.get(k).and_then(Value::as_str).map(str::to_owned);
    if s(row, "status").as_deref() != Some("idle")
        || matches!(s(row, "state").as_deref(), Some("done" | "failed" | "stopped"))
    {
        return;
    }
    let tasks = pending_tasks(row);
    let from_idle = truthy(row.get("attention"))
        && s(row, "attention_kind").as_deref() != Some("failure")
        && s(row, "last_notification").as_deref() == Some("idle_prompt")
        && !truthy(row.get("waitingFor"));
    if !tasks.is_empty() {
        let total: u64 = tasks.values().filter_map(Value::as_u64).sum();
        row.insert("tasks".into(), Value::Object(tasks));
        row.insert("waiting_tasks".into(), json!(total));
        if from_idle {
            row.insert("attention".into(), json!(false));
        }
        return;
    }
    if from_idle {
        row.insert("attention_kind".into(), json!(COMPLETE));
        row.insert("attention_reason".into(), json!(COMPLETE));
    } else if !truthy(row.get("attention")) && s(row, "last_event").as_deref() == Some("Stop") {
        let since = ["status_changed_at", "last_event_at"]
            .iter()
            .find_map(|k| row.get(*k).filter(|v| truthy(Some(v))).cloned())
            .unwrap_or(Value::Null);
        row.insert("attention".into(), json!(true));
        row.insert("attention_kind".into(), json!(COMPLETE));
        row.insert("attention_reason".into(), json!(COMPLETE));
        row.insert("attention_since".into(), since);
    }
}

/// registry.attention_start: when the current wait on a human began.
pub fn attention_start(row: &Row) -> f64 {
    for k in ["attention_since", "status_changed_at"] {
        if let Some(v) = row.get(k).filter(|v| truthy(Some(v))).and_then(Value::as_f64) {
            return v;
        }
    }
    0.0
}

/// registry.in_progress: work still going that nobody can review yet -
/// working, waiting on its own tasks, or blocked on a permission or question.
pub fn in_progress(row: &Row) -> bool {
    s(row, "status").is_some_and(|x| ACTIVE_STATUSES.contains(&x) || ATTENTION_STATUSES.contains(&x))
        || s(row, "state") == Some("blocked")
        || truthy(row.get("waitingFor"))
        || truthy(row.get("waiting_tasks"))
        || !pending_tasks(row).is_empty()
}

/// registry.apply_reviewed: a row whose wait you have already looked at is
/// `reviewed` rather than in attention; attention that starts later shows.
/// Never while work is in progress.
pub fn apply_reviewed(rows: &mut [Row], reviewed: &Map<String, Value>) {
    for row in rows.iter_mut() {
        let Some(at) = row
            .get("sessionId")
            .and_then(Value::as_str)
            .and_then(|sid| reviewed.get(sid))
            .and_then(Value::as_f64)
        else {
            continue;
        };
        if truthy(row.get("attention")) && at > 0.0 && at >= attention_start(row)
            && !in_progress(row)
        {
            row.insert("attention".into(), json!(false));
            row.insert("reviewed".into(), json!(true));
            row.insert("reviewed_at".into(), json!(at));
        }
    }
}

#[cfg(test)]
mod reviewed_tests {
    use super::*;

    #[test]
    fn idle_is_tasks_complete_or_ready() {
        let row = |v: Value| v.as_object().unwrap().clone();
        let mut r = row(json!({"status": "idle", "status_changed_at": 50.0, "background_tasks": 2,
                               "work": {"monitors": 1, "wakeups": 1, "shells": 0}}));
        classify(&mut r);
        assert_eq!(r["tasks"], json!({"shells": 2, "monitors": 1, "wakeups": 1}));
        assert_eq!(r["waiting_tasks"], json!(4));
        let mut r = row(json!({"status": "idle", "status_changed_at": 50.0, "last_event": "Stop", "work": {}}));
        classify(&mut r);
        assert_eq!((r["attention"].clone(), r["attention_kind"].clone(), r["attention_since"].clone()),
                   (json!(true), json!("complete"), json!(50.0)));
        let mut r = row(json!({"status": "idle", "attention": true, "attention_kind": "notification",
                               "last_notification": "permission_prompt", "work": {}}));
        classify(&mut r);
        assert_eq!(r["attention_kind"], json!("notification"));
        let mut r = row(json!({"status": "idle", "last_event": "SessionStart", "work": {}}));
        classify(&mut r);
        assert!(!r.contains_key("attention"));
    }

    #[test]
    fn what_the_stop_hook_listed_counts_as_tasks() {
        let row = |v: Value| v.as_object().unwrap().clone();
        let mut r = row(json!({"status": "idle", "last_event": "Stop", "work": {"shells": 1},
            "stop_background_tasks": [{"type": "shell"}, {"type": "shell"}, {"type": "monitor"},
                                      {"type": "subagent"}, {"type": "MCP task"}, {"type": "workflow"}],
            "stop_crons": [{"id": "c1"}]}));
        classify(&mut r);
        // The larger of the two accounts per kind, not their sum.
        assert_eq!(r["tasks"], json!({"shells": 2, "monitors": 1, "agents": 1, "wakeups": 1, "other": 2}));
        assert_eq!(r["waiting_tasks"], json!(7));
        assert!(!r.contains_key("attention"));
        let mut r = row(json!({"status": "idle", "last_event": "Stop", "work": {},
                               "stop_background_tasks": [], "stop_crons": []}));
        classify(&mut r);
        assert_eq!(r["attention_kind"], json!("complete"));
    }

    #[test]
    fn reviewed_after_the_wait_began_clears_it() {
        let mut rows: Vec<Row> = [
            json!({"sessionId": "a", "attention": true, "attention_since": 100.0}),
            json!({"sessionId": "b", "attention": true, "status_changed_at": 50.0}),
            json!({"sessionId": "c", "attention": true, "attention_since": 300.0}),
        ]
        .into_iter()
        .map(|v| v.as_object().unwrap().clone())
        .collect();
        let seen = json!({"a": 150.0, "b": 60.0, "c": 200.0});
        apply_reviewed(&mut rows, seen.as_object().unwrap());
        assert_eq!(rows[0]["attention"], json!(false));
        assert_eq!(rows[1]["reviewed"], json!(true));
        assert_eq!(rows[2]["attention"], json!(true));
        assert!(!rows[2].contains_key("reviewed"));
    }

    #[test]
    fn work_in_progress_is_never_reviewed() {
        let seen = json!({"a": 150.0});
        for busy in [
            json!({"status": "busy"}),
            json!({"status": "shell"}),
            json!({"status": "waiting"}),
            json!({"state": "blocked"}),
            json!({"waitingFor": "permission"}),
            json!({"status": "idle", "waiting_tasks": 1}),
            json!({"status": "idle", "work": {"agents": 1}}),
        ] {
            let mut row = busy.as_object().unwrap().clone();
            row.insert("sessionId".into(), json!("a"));
            row.insert("attention".into(), json!(true));
            row.insert("attention_since".into(), json!(100.0));
            assert!(in_progress(&row), "{busy}");
            let mut rows = vec![row];
            apply_reviewed(&mut rows, seen.as_object().unwrap());
            assert_eq!(rows[0]["attention"], json!(true), "{busy}");
            assert!(!rows[0].contains_key("reviewed"), "{busy}");
        }
        let idle = json!({"status": "idle", "work": {}}).as_object().unwrap().clone();
        assert!(!in_progress(&idle));
    }
}

pub fn needs_attention(row: &Row) -> bool {
    s(row, "status").is_some_and(|x| ATTENTION_STATUSES.contains(&x))
        || s(row, "state").is_some_and(|x| ATTENTION_STATES.contains(&x))
        || truthy(row.get("waitingFor"))
}

pub fn finished(row: &Row) -> bool {
    s(row, "kind") == Some("background")
        && s(row, "state") == Some("done")
        && !truthy(row.get("pid"))
}

fn active(row: &Row) -> bool {
    s(row, "status").is_some_and(|x| ACTIVE_STATUSES.contains(&x))
}

/// Has this session resumed work since `since` raised attention?
pub fn moved_on(row: &Row, since: Option<f64>) -> bool {
    let Some(since) = since.filter(|s| *s != 0.0) else {
        return false;
    };
    if !active(row) {
        return false;
    }
    if let Some(at_change) = row.get("status_at_change").filter(|v| !v.is_null()) {
        if !at_change
            .as_str()
            .is_some_and(|x| ACTIVE_STATUSES.contains(&x))
        {
            return false;
        }
    }
    let changed = row
        .get("status_changed_at")
        .and_then(Value::as_f64)
        .filter(|c| *c != 0.0);
    changed.is_some_and(|c| c > since + SETTLE_SECONDS)
}

/// Is an `idle_prompt` contradicted by the session still having work on?
pub fn still_running(row: &Row, notification: Option<&Value>) -> bool {
    if notification.and_then(Value::as_str) != Some("idle_prompt") {
        return false;
    }
    active(row) || truthy(row.get("background_tasks"))
}

fn base(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_string()
}

/// (project, worktree) for a working directory.
pub fn place(cwd: Option<&Value>) -> (Value, Value) {
    static RES: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (wt, gitdir) = RES.get_or_init(|| {
        (
            Regex::new(r"^(?P<root>.*?)[\\/]\.claude[\\/]worktrees[\\/](?P<name>[^\\/]+)[\\/]*$").unwrap(),
            Regex::new(r"(?m)^gitdir:[\s\x1c-\x1f]*(?P<main>.*?)[\\/]\.git[\\/]worktrees[\\/][^\\/]+[\s\x1c-\x1f]*$").unwrap(),
        )
    });
    if !truthy(cwd) {
        return (Value::Null, Value::Null);
    }
    let cwd = py_str(cwd.unwrap());
    if let Some(m) = wt.captures(&cwd) {
        return (json!(base(&m["root"])), json!(&m["name"]));
    }
    let head: Option<String> = std::fs::read(PathBuf::from(&cwd).join(".git"))
        .ok()
        .map(|b| {
            let text = String::from_utf8_lossy(&b).into_owned();
            text.chars().take(4096).collect()
        });
    if let Some(m) = head.as_deref().and_then(|h| gitdir.captures(h)) {
        return (json!(base(&m["main"])), json!(base(&cwd)));
    }
    (json!(base(&cwd)), Value::Null)
}

/// Is `name` the one Claude Code made up - "<folder>-<short id>"?
fn default_name(row: &Row) -> bool {
    let name = s(row, "name").unwrap_or("");
    let folder = row
        .get("cwd")
        .map(py_str)
        .map(|c| base(&c))
        .unwrap_or_default();
    if folder.is_empty() {
        return false;
    }
    let Some(rest) = name.strip_prefix(&folder).and_then(|r| r.strip_prefix('-')) else {
        return false;
    };
    (1..=8).contains(&rest.len())
        && rest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn label(row: &Row) -> String {
    let name = s(row, "name").unwrap_or("").to_string();
    if truthy(row.get("worktree")) && (name.is_empty() || default_name(row)) {
        return format!("{}-{}", py_str(&row["project"]), py_str(&row["worktree"]));
    }
    name
}

pub fn short_label(row: &Row) -> String {
    let name = s(row, "name").unwrap_or("").to_string();
    if !name.is_empty() && !default_name(row) {
        return name;
    }
    if truthy(row.get("worktree")) {
        return py_str(&row["worktree"]);
    }
    let folder = row
        .get("cwd")
        .map(py_str)
        .map(|c| base(&c))
        .unwrap_or_default();
    let cut = folder.chars().count() + 1;
    let rest: String = name.chars().skip(cut).collect();
    if rest.is_empty() {
        name
    } else {
        rest
    }
}

/// registry.merge: one row per session, then credentials, attention,
/// place and labels; sorted by (name, sessionId).
pub fn merge(
    agent_rows: &[Value],
    files: &HashMap<String, Row>,
    creds: &HashMap<String, (Value, Value)>,
) -> Vec<Row> {
    let mut rows: indexmap::IndexMap<String, Row> = indexmap::IndexMap::new();
    for a in agent_rows {
        let Some(o) = a.as_object() else { continue };
        let Some(sid) = o.get("sessionId").filter(|v| truthy(Some(v))) else {
            continue;
        };
        rows.insert(py_str(sid), o.clone());
    }
    let mut file_ids: Vec<&String> = files.keys().collect();
    file_ids.sort(); // dict order of a directory listing; only `setdefault` depends on it
    for sid in file_ids {
        let rec = &files[sid];
        let row = rows.entry(sid.clone()).or_insert_with(|| {
            let mut r = Map::new();
            r.insert("sessionId".into(), json!(sid));
            r.insert(
                "kind".into(),
                rec.get("kind").cloned().unwrap_or(Value::Null),
            );
            r
        });
        for k in ["cwd", "name", "pid", "status"] {
            if !row.contains_key(k) {
                row.insert(k.into(), rec.get(k).cloned().unwrap_or(Value::Null));
            }
        }
        row.insert(
            "socket".into(),
            rec.get("messagingSocketPath")
                .cloned()
                .unwrap_or(Value::Null),
        );
        row.insert(
            "bridgeSessionId".into(),
            rec.get("bridgeSessionId").cloned().unwrap_or(Value::Null),
        );
        row.insert(
            "entrypoint".into(),
            rec.get("entrypoint").cloned().unwrap_or(Value::Null),
        );
        row.insert(
            "desktop_app".into(),
            json!(s(rec, "entrypoint") == Some("claude-desktop")),
        );
        if s(rec, "entrypoint") == Some("claude-vscode") {
            row.insert("ide".into(), json!("vscode"));
        }
        if let Some(stamp) = rec.get("statusUpdatedAt").filter(|v| v.is_number()) {
            row.insert(
                "status_changed_at".into(),
                json!(stamp.as_f64().unwrap_or(0.0) / 1000.0),
            );
            row.insert(
                "status_at_change".into(),
                rec.get("status").cloned().unwrap_or(Value::Null),
            );
        }
    }
    for (sid, (token, recorded)) in creds {
        let Some(row) = rows.get_mut(sid) else {
            continue;
        };
        if !truthy(Some(token)) {
            continue;
        }
        let live = row.get("socket").cloned().unwrap_or(Value::Null);
        if truthy(Some(recorded)) && truthy(Some(&live)) && *recorded == live {
            row.insert("token".into(), token.clone());
        } else {
            row.insert("stale_credential".into(), json!(true));
        }
    }
    let mut out: Vec<Row> = rows.into_values().collect();
    for row in out.iter_mut() {
        let d = truthy(row.get("socket")) && truthy(row.get("token"));
        row.insert("dispatchable".into(), json!(d));
        row.insert("attention".into(), json!(needs_attention(row)));
        let (project, worktree) = place(row.get("cwd"));
        row.insert("project".into(), project);
        row.insert("worktree".into(), worktree);
        let l = label(row);
        row.insert("display_name".into(), json!(l));
        let sl = short_label(row);
        row.insert("short_name".into(), json!(sl));
    }
    out.sort_by(|a, b| {
        let key = |r: &Row| {
            (
                s(r, "name").unwrap_or("").to_string(),
                r.get("sessionId").map(py_str).unwrap_or_default(),
            )
        };
        key(a).cmp(&key(b))
    });
    out
}

/// registry.failure_caption: the line a tile shows for a failed turn, or
/// None for the failures Claude Code itself shrugs at.
pub fn failure_caption(error: Option<&str>, detail: &str) -> Option<&'static str> {
    if error == Some("max_output_tokens") {
        return None;
    }
    if error == Some("invalid_request") {
        static OVERSIZE: OnceLock<Regex> = OnceLock::new();
        let re = OVERSIZE.get_or_init(|| {
            Regex::new(r"(?i)\b(too long|too large|exceeds|token limit)\b").unwrap()
        });
        if re.is_match(detail) {
            return Some("request too large - /compact or trim");
        }
    }
    Some(match error.unwrap_or("") {
        "overloaded" => "API overloaded - wait and retry",
        "server_error" => "API unavailable - retry",
        "rate_limit" => "rate limited - wait and retry",
        "billing_error" => "usage limit reached - check plan",
        "authentication_failed" => "login required - run /login",
        "oauth_org_not_allowed" => "org disabled OAuth - use API key or ask admin",
        "account_on_hold" => "account on hold - see detail",
        "verification_required" => "verification required - see detail",
        "cloud_credential_error" => "cloud credentials unavailable - check or refresh",
        "invalid_request" => "invalid API request - see detail",
        "model_not_found" => "model unavailable - see detail",
        _ => "API error",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(v: Value) -> Row {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn worktree_names() {
        let mut r = row(json!({"name": "x-1a2b", "cwd": "C:/w/proj/.claude/worktrees/x"}));
        let (p, w) = place(r.get("cwd"));
        assert_eq!((p.clone(), w.clone()), (json!("proj"), json!("x")));
        r.insert("project".into(), p);
        r.insert("worktree".into(), w);
        assert_eq!(label(&r), "proj-x");
        assert_eq!(short_label(&r), "x");
        let plain = row(
            json!({"name": "proj-5b", "cwd": "C:/w/proj", "project": "proj", "worktree": null}),
        );
        assert_eq!(label(&plain), "proj-5b");
        assert_eq!(short_label(&plain), "5b");
    }

    #[test]
    fn credentials_pair_only_with_the_same_socket() {
        let agents = vec![
            json!({"sessionId": "a", "name": "a"}),
            json!({"sessionId": "b", "name": "b"}),
        ];
        let mut files = HashMap::new();
        files.insert(
            "a".to_string(),
            row(json!({"sessionId": "a", "messagingSocketPath": "p1"})),
        );
        files.insert(
            "b".to_string(),
            row(json!({"sessionId": "b", "messagingSocketPath": "p2"})),
        );
        let mut creds = HashMap::new();
        creds.insert("a".to_string(), (json!("t"), json!("p1")));
        creds.insert("b".to_string(), (json!("t"), json!("old")));
        let out = merge(&agents, &files, &creds);
        assert_eq!(out[0]["dispatchable"], true);
        assert_eq!(out[1]["stale_credential"], true);
        assert_eq!(out[1]["dispatchable"], false);
    }

    #[test]
    fn moved_on_needs_running_again_after_settling() {
        let r =
            row(json!({"status": "busy", "status_at_change": "busy", "status_changed_at": 110.0}));
        assert!(moved_on(&r, Some(100.0)));
        assert!(!moved_on(&r, Some(108.0)));
        let r = row(json!({"status": "idle", "status_changed_at": 110.0}));
        assert!(!moved_on(&r, Some(100.0)));
    }
}

/// test_core.py: IdleStates, StaleAttention, FailureCaptions,
/// AttentionBookkeeping (the moved_on half) and Worktrees.
#[cfg(test)]
mod core_tests {
    use super::*;

    fn row(v: Value) -> Row {
        v.as_object().unwrap().clone()
    }

    fn idle(extra: Value) -> Row {
        let mut r = row(json!({"sessionId": "a", "status": "idle", "status_changed_at": 50.0,
                               "work": {"alive": true, "agents": 0, "shells": 0, "monitors": 0, "wakeups": 0}}));
        for (k, v) in extra.as_object().unwrap() {
            r.insert(k.clone(), v.clone());
        }
        r
    }

    #[test]
    fn an_idle_prompt_while_tasks_run_is_not_shown() {
        let mut r = idle(json!({"work": {"monitors": 1}, "attention": true,
                                "attention_kind": "notification", "last_notification": "idle_prompt"}));
        classify(&mut r);
        assert_eq!(r["attention"], json!(false));
    }

    #[test]
    fn idle_prompt_is_complete() {
        let mut r = idle(json!({"attention": true, "attention_kind": "notification",
                                "attention_reason": "waiting for you",
                                "last_notification": "idle_prompt", "attention_since": 9.0}));
        classify(&mut r);
        assert_eq!((r["attention_kind"].clone(), r["attention_since"].clone()), (json!("complete"), json!(9.0)));
    }

    #[test]
    fn blocked_and_failed_keep_their_attention() {
        let mut r = idle(json!({"attention": true, "attention_kind": "notification",
                                "last_notification": "permission_prompt"}));
        classify(&mut r);
        assert_eq!(r["attention_kind"], json!("notification"));
        let mut r = idle(json!({"attention": true, "attention_kind": "failure",
                                "last_notification": "idle_prompt"}));
        classify(&mut r);
        assert_eq!(r["attention_kind"], json!("failure"));
    }

    #[test]
    fn working_and_finished_jobs_are_left_alone() {
        let mut r = idle(json!({"status": "busy", "last_event": "Stop"}));
        classify(&mut r);
        assert!(!r.contains_key("attention"));
        let mut r = idle(json!({"state": "done", "last_event": "Stop"}));
        classify(&mut r);
        assert!(!r.contains_key("attention"));
    }

    fn stale(status: &str, changed: Option<f64>) -> Row {
        let mut r = row(json!({"sessionId": "sid", "status": status}));
        if let Some(c) = changed {
            r.insert("status_changed_at".into(), json!(c));
        }
        r
    }

    #[test]
    fn moved_on_edges() {
        const PROMPTED: f64 = 1000.0;
        // working again means the prompt was answered
        assert!(moved_on(&stale("busy", Some(PROMPTED + 10.0)), Some(PROMPTED)));
        // a status older than the prompt proves nothing
        assert!(!moved_on(&stale("busy", Some(PROMPTED - 10.0)), Some(PROMPTED)));
        // the same instant is the prompt appearing, not an answer
        assert!(!moved_on(&stale("busy", Some(PROMPTED + 0.2)), Some(PROMPTED)));
        // no status clock leaves the hook in charge
        assert!(!moved_on(&stale("busy", None), Some(PROMPTED)));
        // nothing was waiting in the first place
        assert!(!moved_on(&stale("busy", Some(PROMPTED + 10.0)), None));
        // still idle is still waiting
        assert!(!moved_on(&stale("idle", Some(PROMPTED + 10.0)), Some(PROMPTED)));
    }

    #[test]
    fn moved_on_sources() {
        let at = |s: &str| {
            row(json!({"sessionId": "sid", "status": "busy", "status_changed_at": 2000.0,
                       "status_at_change": s}))
        };
        // `moved_on` is only consulted for notification attention: work
        // resuming proves a permission prompt was answered.
        assert!(moved_on(&at("busy"), Some(1000.0)));
        // `shell` in the session file, `busy` from `agents --json`: both mean executing.
        assert!(moved_on(&at("shell"), Some(1000.0)));
        // sources disagreeing about anything else clears nothing
        assert!(!moved_on(&at("idle"), Some(1000.0)));
    }

    fn merged(agents: Value, files: Value) -> Vec<Row> {
        let agents: Vec<Value> = agents.as_array().unwrap().clone();
        let files: HashMap<String, Row> = files
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_object().unwrap().clone()))
            .collect();
        merge(&agents, &files, &HashMap::new())
    }

    #[test]
    fn the_status_clock_is_carried_off_the_session_file() {
        let rows = merged(
            json!([{"sessionId": "sid", "status": "busy"}]),
            json!({"sid": {"sessionId": "sid", "statusUpdatedAt": 1790036818458u64}}),
        );
        let got = rows[0]["status_changed_at"].as_f64().unwrap();
        assert!((got - 1790036818.458).abs() < 1e-6, "{got}");
    }

    #[test]
    fn a_missing_or_junk_stamp_is_simply_absent() {
        for stamp in [Value::Null, json!("soon"), json!(true)] {
            let rows = merged(
                json!([{"sessionId": "sid", "status": "busy"}]),
                json!({"sid": {"sessionId": "sid", "statusUpdatedAt": stamp}}),
            );
            assert!(!rows[0].contains_key("status_changed_at"), "{stamp}");
        }
    }

    #[test]
    fn a_desktop_app_session_is_flagged() {
        let rows = merged(
            json!([{"sessionId": "app"}, {"sessionId": "cli"}, {"sessionId": "bare"}]),
            json!({"app": {"sessionId": "app", "entrypoint": "claude-desktop"},
                   "cli": {"sessionId": "cli", "entrypoint": "cli"}}),
        );
        let flag = |sid: &str| {
            let r = rows.iter().find(|r| r["sessionId"] == sid).unwrap();
            truthy(r.get("desktop_app"))
        };
        assert_eq!((flag("app"), flag("cli"), flag("bare")), (true, false, false));
    }

    #[test]
    fn a_vs_code_session_is_flagged() {
        let rows = merged(
            json!([{"sessionId": "ide"}, {"sessionId": "cli"}]),
            json!({"ide": {"sessionId": "ide", "entrypoint": "claude-vscode"},
                   "cli": {"sessionId": "cli", "entrypoint": "cli"}}),
        );
        let ide = |sid: &str| rows.iter().find(|r| r["sessionId"] == sid).unwrap().get("ide").cloned();
        assert_eq!(ide("ide"), Some(json!("vscode")));
        assert!(ide("cli").is_none_or(|v| v.is_null()));
    }

    #[test]
    fn failure_captions() {
        assert_eq!(failure_caption(Some("overloaded"), ""), Some("API overloaded - wait and retry"));
        assert_eq!(failure_caption(Some("server_error"), ""), Some("API unavailable - retry"));
        // Claude Code maps this one to no blocked state, so nor do we.
        assert_eq!(failure_caption(Some("max_output_tokens"), ""), None);
        assert_eq!(
            failure_caption(Some("invalid_request"), "prompt is too long"),
            Some("request too large - /compact or trim")
        );
        assert_eq!(
            failure_caption(Some("invalid_request"), "bad field"),
            Some("invalid API request - see detail")
        );
        for error in [Some("a_new_error_code"), None, Some("unknown")] {
            assert_eq!(failure_caption(error, ""), Some("API error"), "{error:?}");
        }
    }

    #[test]
    fn every_documented_error_has_a_caption() {
        // The enum as the binary lists it; a new one must not crash a tile.
        for error in [
            "authentication_failed", "oauth_org_not_allowed", "account_on_hold",
            "verification_required", "billing_error", "rate_limit", "overloaded",
            "invalid_request", "model_not_found", "server_error", "unknown",
            "max_output_tokens", "cloud_credential_error",
        ] {
            let caption = failure_caption(Some(error), "");
            assert!(caption.is_none_or(|c| !c.trim().is_empty()), "{error}");
        }
    }

    fn placed(cwd: &str) -> (Value, Value) {
        place(Some(&json!(cwd)))
    }

    #[test]
    fn worktree_paths_on_either_platform() {
        for cwd in [
            r"C:\src\hai-proof\.claude\worktrees\fizzy-wondering-lark",
            "/home/me/hai-proof/.claude/worktrees/fizzy-wondering-lark/",
        ] {
            assert_eq!(placed(cwd), (json!("hai-proof"), json!("fizzy-wondering-lark")), "{cwd}");
        }
    }

    #[test]
    fn a_plain_folder_is_its_own_project() {
        assert_eq!(placed(r"C:\src\hai-proof"), (json!("hai-proof"), Value::Null));
        assert_eq!(place(None), (Value::Null, Value::Null));
    }

    #[test]
    fn a_linked_worktree_is_read_from_its_git_file() {
        let d = std::env::temp_dir().join(format!("imp-place-core-{}", std::process::id()));
        let wt = d.join("side-branch");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: C:/src/hai-proof/.git/worktrees/side-branch\n").unwrap();
        let got = placed(&wt.to_string_lossy());
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(got, (json!("hai-proof"), json!("side-branch")));
    }

    fn labelled(cwd: &str, name: &str) -> Row {
        let (project, worktree) = placed(cwd);
        row(json!({"cwd": cwd, "name": name, "project": project, "worktree": worktree}))
    }

    #[test]
    fn chosen_names_and_plain_sessions_keep_theirs() {
        let wt = r"C:\src\hai-proof\.claude\worktrees\fizzy-wondering-lark";
        assert_eq!(label(&labelled(wt, "Proof checker")), "Proof checker");
        assert_eq!(label(&labelled(r"C:\src\hai-proof", "hai-proof-5b")), "hai-proof-5b");
        // the default name: the worktree says it all
        assert_eq!(label(&labelled(wt, "fizzy-wondering-lark-c9")), "hai-proof-fizzy-wondering-lark");
        assert_eq!(short_label(&labelled(wt, "fizzy-wondering-lark-c9")), "fizzy-wondering-lark");
        assert_eq!(short_label(&labelled(r"C:\src\hai-proof", "hai-proof-5b")), "5b");
        assert_eq!(short_label(&labelled(wt, "Proof checker")), "Proof checker");
    }

    #[test]
    fn merge_fills_the_fields() {
        let rows = merged(
            json!([{"sessionId": "a", "name": "lark-c9", "cwd": "/r/hai-proof/.claude/worktrees/lark"}]),
            json!({}),
        );
        assert_eq!(
            (rows[0]["project"].clone(), rows[0]["worktree"].clone(), rows[0]["display_name"].clone()),
            (json!("hai-proof"), json!("lark"), json!("hai-proof-lark"))
        );
    }
}

/// test_finished.py::Finished and test_dispatcher.py::IdleWhileRunning's
/// background-shell case, ported.
#[cfg(test)]
mod finished_tests {
    use super::*;

    fn job(extra: Value) -> Row {
        let mut r = json!({"sessionId": "04f327fc-a375-48b3-b69b-3142826d07c4", "id": "04f327fc",
                           "kind": "background", "state": "done", "name": "reply-with-the-single",
                           "cwd": "C:/w/imperatorium"})
        .as_object()
        .unwrap()
        .clone();
        r.extend(extra.as_object().unwrap().clone());
        r
    }

    #[test]
    fn only_a_completed_background_job_without_a_process() {
        assert!(finished(&job(json!({}))));
        assert!(!finished(&job(json!({"state": "working"}))));
        assert!(!finished(&job(json!({"state": "blocked"})))); // waits on you
        assert!(!finished(&job(json!({"state": "failed"})))); // needs you too
        assert!(!finished(&job(json!({"pid": 4242})))); // resumed
        assert!(!finished(&job(json!({"kind": "interactive"}))));
    }

    #[test]
    fn an_idle_prompt_is_contradicted_by_a_background_shell() {
        let idle = json!({"status": "idle"}).as_object().unwrap().clone();
        let with_shell = json!({"status": "idle", "background_tasks": 1}).as_object().unwrap().clone();
        let busy = json!({"status": "busy"}).as_object().unwrap().clone();
        let prompt = json!("idle_prompt");
        assert!(!still_running(&idle, Some(&prompt)));
        assert!(still_running(&with_shell, Some(&prompt)));
        assert!(still_running(&busy, Some(&prompt)));
        // Only an idle_prompt is contradicted.
        assert!(!still_running(&busy, Some(&json!("permission_prompt"))));
    }
}
