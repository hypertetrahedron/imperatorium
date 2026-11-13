//! Which sessions exist: a port of `imperatorium/registry.py` (`agents`,
//! `session_files`, `merge`, `attach_credentials`, `needs_attention`,
//! `moved_on`, `still_running`, `finished`, `place`, `label`,
//! `short_label`). Read-only: it runs `claude agents`, reads the session
//! files and the dispatcher's state.json, and writes nothing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::paths;
use crate::pyish::{py_str, truthy};

pub type Row = Map<String, Value>;

const ATTENTION_STATUSES: &[&str] = &["waiting"];
const ATTENTION_STATES: &[&str] = &["blocked", "failed"];
pub const ACTIVE_STATUSES: &[&str] = &["busy", "shell"];
const SETTLE_SECONDS: f64 = 3.0;

fn claude_home() -> PathBuf {
    match std::env::var("CLAUDE_CONFIG_DIR") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => {
            #[cfg(windows)]
            let home = std::env::var("USERPROFILE").unwrap_or_default();
            #[cfg(not(windows))]
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".claude")
        }
    }
}

/// `claude agents --json --all`, or [] when it fails or takes over `timeout`.
pub fn agents(timeout: Duration) -> Vec<Value> {
    let mut cmd = Command::new("claude");
    cmd.args(["agents", "--json", "--all"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let Ok(mut child) = cmd.spawn() else {
        return vec![];
    };
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut buf);
        buf
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().unwrap_or_default();
                if !status.success() {
                    return vec![];
                }
                return match serde_json::from_slice::<Value>(&out) {
                    Ok(Value::Array(rows)) => rows,
                    _ => vec![],
                };
            }
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return vec![];
            }
        }
    }
}

/// ~/.claude/sessions/*.json, keyed by sessionId.
pub fn session_files() -> HashMap<String, Row> {
    let mut found = HashMap::new();
    let Ok(rd) = std::fs::read_dir(claude_home().join("sessions")) else {
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
