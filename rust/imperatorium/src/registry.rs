//! Which sessions exist and what each is called: the parts of
//! `imperatorium/registry.py` and the dispatcher's row lookup that phase 3's
//! first endpoints need. Read-only: it runs `claude agents`, reads the
//! session files and the dispatcher's state.json, and writes nothing.
//!
//! Grows into the whole snapshot (procs, work, overlay, decorate) as the
//! session endpoints are ported.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::paths;
use crate::pyish::truthy;

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
pub fn session_files() -> HashMap<String, Map<String, Value>> {
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
pub fn state() -> Map<String, Value> {
    let dir = paths::state_dir();
    std::fs::read_to_string(dir.join("state.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// A finished background job (registry.finished).
pub fn finished(row: &Map<String, Value>) -> bool {
    row.get("kind").and_then(Value::as_str) == Some("background")
        && row.get("state").and_then(Value::as_str) == Some("done")
        && !truthy(row.get("pid"))
}

fn finished_at(row: &Map<String, Value>) -> f64 {
    let num = |k: &str| {
        row.get(k)
            .filter(|v| !v.is_boolean())
            .and_then(Value::as_f64)
            .filter(|v| *v > 0.0)
    };
    num("last_event_at")
        .or_else(|| num("startedAt").map(|s| s / 1000.0))
        .unwrap_or(0.0)
}

/// The rows the dispatcher's snapshot would list, as far as the ported
/// endpoints need them: agents and session files merged, hook data folded
/// in for name/cwd and the finished-job age, dismissed and hidden rows
/// left out.
pub fn rows(config: &Value) -> Vec<Map<String, Value>> {
    let mut rows: Vec<Map<String, Value>> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for a in agents(Duration::from_secs(20)) {
        let Some(o) = a.as_object() else { continue };
        let Some(sid) = o
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        index.insert(sid.to_string(), rows.len());
        rows.push(o.clone());
    }
    for (sid, rec) in session_files() {
        let at = *index.entry(sid.clone()).or_insert_with(|| {
            let mut r = Map::new();
            r.insert("sessionId".into(), Value::String(sid.clone()));
            r.insert(
                "kind".into(),
                rec.get("kind").cloned().unwrap_or(Value::Null),
            );
            rows.push(r);
            rows.len() - 1
        });
        let row = &mut rows[at];
        for (field, src) in [
            ("cwd", "cwd"),
            ("name", "name"),
            ("pid", "pid"),
            ("status", "status"),
        ] {
            if !row.contains_key(field) {
                row.insert(field.into(), rec.get(src).cloned().unwrap_or(Value::Null));
            }
        }
    }
    let st = state();
    let known = st
        .get("sessions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let dismissed = st
        .get("dismissed")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let hours = config
        .get("list")
        .and_then(|l| l.get("hide_finished_hours"))
        .filter(|v| !v.is_boolean())
        .and_then(Value::as_f64)
        .filter(|h| *h >= 0.0)
        .unwrap_or(0.0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut out = Vec::new();
    for mut row in rows {
        let sid = row
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if let Some(Value::Object(extra)) = known.get(&sid) {
            for field in ["name", "cwd", "last_event_at"] {
                if truthy(extra.get(field)) && !truthy(row.get(field)) {
                    row.insert(field.into(), extra[field].clone());
                }
            }
        }
        if dismissed.contains_key(&sid) && !truthy(row.get("pid")) {
            continue;
        }
        if hours > 0.0 && finished(&row) && finished_at(&row) < now - hours * 3600.0 {
            continue;
        }
        row.insert("key".into(), Value::String(sid));
        out.push(row);
    }
    out
}

/// The row for a session id (or its short `id`), with a 15 s cache like the
/// dispatcher's LOOKUP_AGE, looked for again in a fresh list when missing.
/// One snapshot of the rows and when it was taken.
type Snapshot = (Instant, Vec<Map<String, Value>>);

pub struct Lookup {
    cache: Mutex<Option<Snapshot>>,
}

impl Lookup {
    pub fn new() -> Self {
        Lookup {
            cache: Mutex::new(None),
        }
    }

    pub fn find(&self, session_id: &str, config: &Value) -> Option<Map<String, Value>> {
        let hit = |rows: &[Map<String, Value>]| {
            rows.iter()
                .find(|r| {
                    r.get("sessionId").and_then(Value::as_str) == Some(session_id)
                        || r.get("id").and_then(Value::as_str) == Some(session_id)
                })
                .cloned()
        };
        {
            let cache = self.cache.lock().unwrap();
            if let Some((at, rows)) = cache.as_ref() {
                if at.elapsed() < Duration::from_secs(15) {
                    if let Some(r) = hit(rows) {
                        return Some(r);
                    }
                }
                // Not in it: _row looks again in a snapshot at most 2 s old,
                // and this one is that. A session only History knows is not
                // in any snapshot, and must not cost `claude agents` (about
                // a second) on every poll.
                if at.elapsed() < Duration::from_secs(2) {
                    return None;
                }
            }
        }
        let fresh = rows(config);
        let found = hit(&fresh);
        *self.cache.lock().unwrap() = Some((Instant::now(), fresh));
        found
    }
}
