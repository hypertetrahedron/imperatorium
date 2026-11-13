//! `imperatorium serve`: the dispatcher's HTTP API, ported endpoint by
//! endpoint (native port phase 3). Until the cutover it runs beside the
//! Python dispatcher as a shadow (port 8811 by default), and
//! `rust/conformance/shadow.py` sends both the same requests and compares
//! the answers. It reads config.json, state.json and the transcripts, and
//! writes nothing.
//!
//! Ported so far: GET /api/health, GET /api/conversation, GET /api/sessions.
//! Anything else is a 404, as the Python dispatcher answers an unknown path.
//!
//! The session rows are the Python dispatcher's `sessions()` less what only
//! it can know, because it is fed by POSTs this shadow never receives: a
//! tool hook's live activity, the statusline's usage, the mod's beats, and
//! the PR badges it polls `gh` for. Those come with step 2.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};

use crate::pyish::{json_dumps_sorted, truthy};
use crate::registry::Row;
use crate::{
    activity, changes, conversation, costs, history, paths, procs, projects, registry, sessions,
    sessionstat, settings, ui,
};

pub struct Shared {
    /// Control._errors (the whole answer, ERRORS_EVERY) and _tallies
    /// (each session's tally, until its transcript's size moves).
    errors_cache: Mutex<Option<(f64, f64, Value)>>,
    tallies: Mutex<HashMap<String, (Value, Value)>>,
    /// `--home`: where config.json is, read again for the Settings form.
    home: Option<String>,
    /// Handed to the pages when they load (ui._with_meta); never written down.
    window_token: String,
    history: history::Index,
    config: Value,
    boot: String,
    /// The dispatcher's snapshot cache: (when, rows), the rows still holding
    /// their tokens - only `public()` copies leave the process.
    snap: Mutex<Option<(f64, Vec<Row>)>>,
    stats: sessionstat::Reader,
    /// _doing_line's cache: sid -> (when, line).
    doing: Mutex<HashMap<String, (f64, String)>>,
    convs: Mutex<HashMap<String, Arc<Cached>>>,
    /// convcache._paths: each session's transcript, found once.
    paths: Mutex<HashMap<String, PathBuf>>,
}

struct Cached {
    stamp: (PathBuf, u64, u128),
    raw: Vec<Value>,
    merged: Vec<Value>,
    /// `merged` serialised once, for the full-conversation answer.
    merged_json: String,
    sig: String,
}

/// convcache.signature: sha1 of json.dumps(items, sort_keys=True), 16 hex.
pub fn signature(items: &[Value]) -> String {
    let text = json_dumps_sorted(&Value::Array(items.to_vec()));
    let digest = Sha1::digest(text.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn load_config(home: Option<&str>) -> Value {
    std::fs::read_to_string(paths::config_home(home).join("config.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// settings._get: a dotted key, found or not.
fn get<'a>(config: &'a Value, key: &str) -> Option<&'a Value> {
    let mut node = config;
    for part in key.split('.') {
        node = node.as_object()?.get(part)?;
    }
    Some(node)
}

const HIGHLIGHTS: &[&str] = &[
    "code", "diffs", "prose", "tools", "signals", "mute", "guides", "shape",
];
const COLORS: &[&str] = &[
    "keyword", "string", "comment", "number", "function", "variable", "added", "removed", "path",
    "command", "ref", "quantity", "tool", "error", "success", "warning", "question", "guide",
];

/// settings.look: what the pages draw with.
pub fn look(config: &Value) -> Value {
    let mut colors = Map::new();
    for name in COLORS {
        if let Some(Value::String(v)) = get(config, &format!("highlight.colors.{name}")) {
            let v = v.to_lowercase();
            let ok =
                v.len() == 7 && v.starts_with('#') && v[1..].bytes().all(|b| b.is_ascii_hexdigit());
            if ok {
                colors.insert((*name).into(), Value::String(v));
            }
        }
    }
    let mut hl = Map::new();
    for name in HIGHLIGHTS {
        // Every highlight defaults to on.
        let on = match get(config, &format!("highlight.{name}")) {
            Some(v) => truthy(Some(v)),
            None => true,
        };
        hl.insert((*name).into(), Value::Bool(on));
    }
    json!({
        "fonts": {
            "chat": get(config, "chat.font_size").cloned().unwrap_or(json!(14)),
            "matrix": get(config, "chat.matrix_font_size").cloned().unwrap_or(json!(13)),
        },
        "hl": hl,
        "colors": colors,
    })
}

/// sessionstat.transcript_path: the newest projects/*/<id>.jsonl.
fn transcript_stamp(sid: &str) -> Option<(PathBuf, u64, u128)> {
    if sid.len() != 36 || !sid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return None;
    }
    let mut best: Option<(PathBuf, u64, u128)> = None;
    for dir in std::fs::read_dir(sessions::projects_dir()).ok()?.flatten() {
        let p = dir.path().join(format!("{sid}.jsonl"));
        let Ok(meta) = std::fs::metadata(&p) else {
            continue;
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        if best.as_ref().is_none_or(|b| mtime > b.2) {
            best = Some((p, meta.len(), mtime));
        }
    }
    best
}

impl Shared {
    pub fn new(home: Option<&str>) -> Self {
        let boot: String = {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            format!("{:x}{:x}", t.as_secs(), std::process::id())
        };
        Shared {
            window_token: token_urlsafe(24),
            errors_cache: Mutex::new(None),
            tallies: Mutex::new(HashMap::new()),
            home: home.map(str::to_string),
            history: history::Index::new(),
            config: load_config(home),
            boot,
            snap: Mutex::new(None),
            stats: sessionstat::Reader::new(),
            doing: Mutex::new(HashMap::new()),
            convs: Mutex::new(HashMap::new()),
            paths: Mutex::new(HashMap::new()),
        }
    }

    /// convcache._stamp: stat the remembered path; look again (and
    /// remember) only when there is none or it has gone.
    fn stamp(&self, sid: &str) -> Option<(PathBuf, u64, u128)> {
        let known = self.paths.lock().unwrap().get(sid).cloned();
        if let Some(path) = known {
            if let Ok(meta) = std::fs::metadata(&path) {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                return Some((path, meta.len(), mtime));
            }
        }
        let found = transcript_stamp(sid)?;
        self.paths
            .lock()
            .unwrap()
            .insert(sid.to_string(), found.0.clone());
        Some(found)
    }

    /// convcache: the per-message items and the merged conversation,
    /// parsed again only when the transcript's size or mtime moves.
    fn conversation(&self, sid: &str) -> Result<Arc<Cached>, String> {
        let stamp = self.stamp(sid);
        if let Some(st) = &stamp {
            if let Some(hit) = self.convs.lock().unwrap().get(sid) {
                if &hit.stamp == st {
                    return Ok(Arc::clone(hit));
                }
            }
        }
        let chain = sessions::chain_json(sid)?;
        let chain = chain.as_array().cloned().unwrap_or_default();
        let raw = conversation::to_items(&conversation::messages(&chain), 0, false);
        let merged = conversation::merge_turns(raw.clone());
        let sig = signature(&merged);
        let merged_json = serde_json::to_string(&merged).map_err(|e| e.to_string())?;
        let entry = Arc::new(Cached {
            stamp: stamp.clone().unwrap_or_default(),
            raw,
            merged,
            merged_json,
            sig,
        });
        if stamp.is_some() {
            let mut convs = self.convs.lock().unwrap();
            if convs.len() >= 32 {
                convs.clear(); // a coarse version of convcache's LRU budget
            }
            convs.insert(sid.to_string(), Arc::clone(&entry));
        }
        Ok(entry)
    }

    fn api_conversation(&self, q: &HashMap<String, String>) -> (u16, Body) {
        let session = q.get("session").cloned().unwrap_or_default();
        if session.is_empty() {
            return (400, Body::Json(json!({"error": "session is required"})));
        }
        let int = |k: &str| {
            q.get(k)
                .and_then(|v| v.trim().parse::<i64>().ok())
                .unwrap_or(0)
                .max(0)
        };
        let limit = int("limit") as usize;
        let blocks = int("blocks");
        // _row(session, max_age=LOOKUP_AGE): a 15 s old snapshot will do,
        // and a fresh one (at most 2 s old) is asked only when it is missing.
        let hit = |rows: &[Row]| {
            rows.iter()
                .find(|r| {
                    r.get("sessionId").and_then(Value::as_str) == Some(session.as_str())
                        || r.get("id").and_then(Value::as_str) == Some(session.as_str())
                })
                .cloned()
        };
        let row = hit(&self.sessions(15.0)).or_else(|| hit(&self.sessions(2.0)));
        let sid = row
            .as_ref()
            .and_then(|r| r.get("sessionId").and_then(Value::as_str))
            .unwrap_or(&session)
            .to_string();
        let field = |k: &str| {
            row.as_ref()
                .and_then(|r| r.get(k))
                .cloned()
                .unwrap_or(Value::Null)
        };
        let cached = self.conversation(&sid);
        // The poll's usual case - the whole conversation, changed - is
        // answered from the items as serialised once at fill time, spliced
        // into the body, rather than copied and serialised on every poll.
        if let Ok(c) = &cached {
            let known = q.get("known").map(String::as_str) == Some(c.sig.as_str());
            if limit == 0 && blocks == 0 && !known {
                let text = format!(
                    "{{\"session\":{},\"items\":{},\"sig\":{},\"name\":{},\"cwd\":{}}}",
                    json!(session),
                    c.merged_json,
                    json!(c.sig),
                    field("name"),
                    field("cwd")
                );
                return (200, Body::Raw(text));
            }
        }
        let got = cached.map(|c| {
            if limit > 0 {
                let start = c.raw.len().saturating_sub(limit);
                let items = conversation::merge_turns(c.raw[start..].to_vec());
                let sig = signature(&items);
                (items, sig)
            } else {
                (c.merged.clone(), c.sig.clone())
            }
        });
        // Built in the Python handler's key order: c.conversation's body,
        // then `total`/`start`/`first_block` for a tail, and `sig` set last
        // (which leaves it in place where it was already there).
        let mut body = Map::new();
        body.insert("session".into(), json!(session));
        let (items, sig) = match got {
            Ok((items, sig)) => {
                body.insert("items".into(), Value::Null); // placed now, filled below
                body.insert("sig".into(), json!(sig));
                body.insert("name".into(), field("name"));
                body.insert("cwd".into(), field("cwd"));
                (items, sig)
            }
            Err(e) => {
                body.insert("items".into(), json!([]));
                body.insert("error".into(), json!(format!("RuntimeError: {e}")));
                (vec![], signature(&[]))
            }
        };
        if q.get("known").map(String::as_str) == Some(sig.as_str()) {
            return (
                200,
                Body::Json(json!({"session": session, "unchanged": true, "sig": sig})),
            );
        }
        if blocks > 0 {
            let (tail, start, first) = conversation::tail_blocks(&items, blocks);
            body.insert("items".into(), Value::Array(tail));
            body.insert("total".into(), json!(items.len()));
            body.insert("start".into(), json!(start));
            body.insert("first_block".into(), json!(first));
        } else {
            body.insert("items".into(), Value::Array(items));
        }
        body.insert("sig".into(), json!(sig));
        (200, Body::Json(Value::Object(body)))
    }

    // -- the session snapshot -------------------------------------------------

    fn now() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }

    /// Control.sessions(max_age): the rows, cached briefly.
    pub fn sessions(&self, max_age: f64) -> Vec<Row> {
        self.snapshot(max_age).1
    }

    /// (when the snapshot was taken, its rows). As registry.snapshot,
    /// `at` is read before `claude agents` runs.
    pub fn snapshot(&self, max_age: f64) -> (f64, Vec<Row>) {
        let now = Self::now();
        if let Some((at, rows)) = self.snap.lock().unwrap().as_ref() {
            if now - at < max_age {
                return (*at, rows.clone());
            }
        }
        let rows = self.build_snapshot();
        *self.snap.lock().unwrap() = Some((now, rows.clone()));
        (now, rows)
    }

    fn build_snapshot(&self) -> Vec<Row> {
        let st = registry::state();
        let creds = registry::credentials(&st);
        let mut rows = registry::merge(
            &registry::agents(std::time::Duration::from_secs(20)),
            &registry::session_files(),
            &creds,
        );
        procs::annotate(&mut rows);
        let empty = Map::new();
        let known = st
            .get("sessions")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let mut titled = Vec::new();
        for (i, row) in rows.iter_mut().enumerate() {
            let sid = row
                .get("sessionId")
                .map(crate::pyish::py_str)
                .unwrap_or_default();
            let Some(Value::Object(extra)) = known.get(&sid) else {
                continue;
            };
            if extra.is_empty() {
                continue;
            }
            overlay(row, extra);
            titled.push(i);
        }
        let ids: Vec<String> = titled
            .iter()
            .filter_map(|&i| {
                rows[i]
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        if let Value::Object(found) = sessions::titles_json(&ids) {
            for &i in &titled {
                let sid = rows[i]
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                rows[i].insert(
                    "window_title".into(),
                    found.get(&sid).cloned().unwrap_or(json!("")),
                );
            }
        }
        let queue = st
            .get("queue")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for row in rows.iter_mut() {
            self.decorate(row, &queue);
        }
        let dismissed = st
            .get("dismissed")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        rows.retain(|r| {
            let key = r.get("key").and_then(Value::as_str).unwrap_or("");
            !(dismissed.contains_key(key) && !truthy(r.get("pid")))
        });
        let hours = get(&self.config, "list.hide_finished_hours")
            .filter(|v| v.is_number())
            .and_then(Value::as_f64)
            .filter(|h| *h > 0.0)
            .unwrap_or(0.0);
        if hours > 0.0 {
            let cutoff = Self::now() - hours * 3600.0;
            rows.retain(|r| !(registry::finished(r) && finished_at(r) < cutoff));
        }
        rows
    }

    /// Control._decorate, less what only the Python dispatcher knows.
    fn decorate(&self, row: &mut Row, queue: &Map<String, Value>) {
        let sid = row
            .get("sessionId")
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        row.insert("key".into(), json!(sid));
        let state = row.get("state").and_then(Value::as_str).map(str::to_string);
        let ended = matches!(
            state.as_deref(),
            Some("done") | Some("failed") | Some("stopped")
        );
        if !ended {
            let line = self.doing_line(&sid);
            row.insert("doing".into(), json!(line));
        }
        if let Some(Value::Array(q)) = queue.get(&sid) {
            if !q.is_empty() {
                row.insert("queued".into(), json!(q.len()));
            }
        }
        let bg_idle = row.get("kind").and_then(Value::as_str) == Some("background")
            && !truthy(row.get("pid"))
            && !truthy(row.get("status"));
        if ended || bg_idle {
            row.insert("resumable".into(), json!(true));
        }
        row.insert(
            "resume_command".into(),
            json!(format!("claude --resume {sid}")),
        );
        let work = self.work(row);
        row.insert("work".into(), work);
    }

    /// Control._doing_line: activity.doing over the last 8 messages,
    /// remembered for DOING_EVERY (10 s).
    fn doing_line(&self, sid: &str) -> String {
        let now = Self::now();
        let hit = self.doing.lock().unwrap().get(sid).cloned();
        if let Some((at, line)) = &hit {
            if now - at < 10.0 {
                return line.clone();
            }
        }
        let line = match self.conversation(sid) {
            Ok(c) => {
                let start = c.raw.len().saturating_sub(8);
                activity::doing(&conversation::merge_turns(c.raw[start..].to_vec()))
            }
            Err(_) => hit.map(|h| h.1).unwrap_or_default(),
        };
        self.doing
            .lock()
            .unwrap()
            .insert(sid.to_string(), (now, line.clone()));
        line
    }

    /// Control._work: what the tile shows at a glance.
    fn work(&self, row: &Row) -> Value {
        let sid = row
            .get("sessionId")
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let stat = self
            .stats
            .read(&sid, Self::now())
            .unwrap_or_else(|| json!({}));
        let alive =
            row.get("pid").is_some_and(|p| p.is_u64() || p.is_i64()) && truthy(row.get("status"));
        let cache = stat.get("cache").cloned().unwrap_or_else(|| json!({}));
        let mut out = Map::new();
        out.insert(
            "cache_expires_at".into(),
            cache.get("expires_at").cloned().unwrap_or(Value::Null),
        );
        out.insert(
            "context_tokens".into(),
            stat.get("context_tokens").cloned().unwrap_or(Value::Null),
        );
        out.insert(
            "cold_tokens".into(),
            get(&self.config, "chat.cold_cache_tokens")
                .cloned()
                .unwrap_or(json!(0)),
        );
        out.insert(
            "resume_cost_usd".into(),
            costs::rewrite_cost(
                stat.get("model_id"),
                stat.get("context_tokens"),
                cache.get("ttl"),
            ),
        );
        out.insert("alive".into(), json!(alive));
        if alive {
            for name in ["agents", "shells", "monitors", "wakeups"] {
                let n = stat
                    .get(name)
                    .and_then(Value::as_array)
                    .map(|a| a.len())
                    .unwrap_or(0);
                out.insert(name.into(), json!(n));
            }
            let soonest = stat
                .get("wakeups")
                .and_then(Value::as_array)
                .map(|w| {
                    w.iter()
                        .filter_map(|x| x.get("fires_at").and_then(Value::as_f64))
                        .filter(|f| *f != 0.0)
                        .fold(None, |m: Option<f64>, f| Some(m.map_or(f, |m| m.min(f))))
                })
                .unwrap_or(None);
            out.insert(
                "wakes_at".into(),
                soonest.map(|f| json!(f)).unwrap_or(Value::Null),
            );
            let n = row
                .get("shell_processes")
                .filter(|v| truthy(Some(v)))
                .cloned()
                .unwrap_or(json!(0));
            out.insert("processes".into(), n);
        }
        Value::Object(out)
    }

    /// GET /api/sessions: the snapshot as it may leave the process.
    fn api_sessions(&self) -> Value {
        // The Stream Deck polls once a second; a snapshot STREAM_EVERY old
        // is as good for a key colour and saves a fork.
        let (at, rows) = self.snapshot(STREAM_EVERY);
        json!({"at": at, "sessions": public(rows)})
    }

    /// GET /api/stream: server-sent events, the session list whenever it
    /// changes - looked at every STREAM_EVERY seconds, never more often
    /// than STREAM_MIN_GAP, a keepalive after STREAM_KEEPALIVE of quiet,
    /// and closed after STREAM_SECONDS (the page reconnects). HTTP/1.0 and
    /// no length, as the Python handler answers.
    ///
    /// Python is also woken early by a hook event (Control.invalidate);
    /// this shadow receives no hooks, so it only looks on its clock.
    pub fn stream(&self, out: &mut dyn std::io::Write) {
        let head = "HTTP/1.0 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\n\
                    Cache-Control: no-store\r\n\r\n";
        if out
            .write_all(head.as_bytes())
            .and_then(|_| out.flush())
            .is_err()
        {
            return;
        }
        let start = Self::now();
        let (mut last, mut quiet, mut looked) = (String::new(), start, 0.0);
        while Self::now() < start + STREAM_SECONDS {
            let gap = STREAM_MIN_GAP - (Self::now() - looked);
            if gap > 0.0 {
                std::thread::sleep(std::time::Duration::from_secs_f64(gap));
            }
            looked = Self::now();
            let (at, rows) = self.snapshot(STREAM_EVERY);
            let rows = public(rows);
            let same = json_dumps_sorted(&json!({"sessions": rows}));
            let sent = if same != last {
                let body = json!({"at": at, "sessions": rows});
                let r = write!(out, "event: sessions\ndata: {body}\n\n").and_then(|_| out.flush());
                last = same;
                quiet = Self::now();
                r
            } else if Self::now() - quiet >= STREAM_KEEPALIVE {
                quiet = Self::now();
                out.write_all(b": keepalive\n\n").and_then(|_| out.flush())
            } else {
                Ok(())
            };
            if sent.is_err() {
                return; // the page went away
            }
            std::thread::sleep(std::time::Duration::from_secs_f64(STREAM_EVERY));
        }
    }

    /// Control.search: History, with which of its sessions are live.
    fn api_history(&self, q: &HashMap<String, String>) -> Value {
        let query = q.get("q").cloned().unwrap_or_default();
        let read = |sid: &str| {
            self.conversation(sid)
                .map(|c| c.merged.clone())
                .unwrap_or_default()
        };
        let mut got = self.history.search(&query, 40, &read);
        let live: std::collections::HashSet<String> = self
            .sessions(2.0)
            .iter()
            .filter(|r| truthy(r.get("pid")) || truthy(r.get("status")))
            .filter_map(|r| {
                r.get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        if let Some(Value::Array(results)) = got.get_mut("results") {
            for r in results.iter_mut() {
                let sid = r
                    .get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                r["live"] = json!(live.contains(&sid));
            }
        }
        got["ok"] = json!(true);
        got
    }

    /// Each notification channel on or off: config.json at start, with the
    /// switch kept in prefs (`/api/notify`, impctl) winning, as Control does.
    fn notify_enabled(&self) -> Vec<(&'static str, Map<String, Value>, bool)> {
        let st = registry::state();
        let prefs = st
            .get("prefs")
            .and_then(|p| p.get("notify"))
            .cloned()
            .unwrap_or(json!({}));
        settings::notify_channels(&self.config)
            .into_iter()
            .map(|(name, cfg)| {
                let on = match prefs.get(name) {
                    Some(v) => truthy(Some(v)),
                    None => truthy(cfg.get("enabled")),
                };
                (name, cfg, on)
            })
            .collect()
    }

    /// GET /api/notify: notify.status(). `last_error` and `sent` count
    /// what this process has pushed, and the shadow pushes nothing.
    fn api_notify(&self) -> Value {
        let mut out = Map::new();
        for (name, cfg, on) in self.notify_enabled() {
            let configured = if name == "ntfy" {
                truthy(cfg.get("topic"))
            } else {
                true
            };
            out.insert(
                name.into(),
                json!({"enabled": on, "delay": cfg.get("delay").cloned().unwrap_or(Value::Null),
                       "configured": configured}),
            );
        }
        out.insert("last_error".into(), Value::Null);
        out.insert("sent".into(), json!(0));
        Value::Object(out)
    }

    /// Control.catalogue, local part: the roots from config.json (else the
    /// folder the checkout sits in) and the folders sessions run in.
    fn catalogue(&self) -> Vec<Value> {
        let configured: Vec<String> = self
            .config
            .get("roots")
            .and_then(Value::as_array)
            .map(|r| {
                r.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let roots = if configured.is_empty() {
            // projects.DEFAULT_ROOTS: the folder the checkout sits in.
            let home = paths::config_home(self.home.as_deref());
            vec![home
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()]
        } else {
            configured
        };
        let local: Vec<String> = self
            .sessions(2.0)
            .iter()
            .filter(|r| !truthy(r.get("remote")))
            .filter_map(|r| {
                r.get("cwd")
                    .and_then(Value::as_str)
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
            })
            .collect();
        projects::discover(&roots, &local)
    }

    /// Control._row(id): the row whose sessionId or key is `id`.
    fn row(&self, id: &str) -> Option<Row> {
        let hit = |r: &Row| {
            r.get("sessionId").and_then(Value::as_str) == Some(id)
                || r.get("key").and_then(Value::as_str) == Some(id)
        };
        self.sessions(2.0).into_iter().find(hit)
    }

    /// Control.activity: one session's tool calls, from its conversation.
    /// (Python prefers the tool hooks' record when it has one; the shadow
    /// gets no hooks, so it always reads the conversation.)
    fn api_activity(&self, session: &str) -> Value {
        let hit = |rows: &[Row]| {
            rows.iter()
                .find(|r| {
                    r.get("sessionId").and_then(Value::as_str) == Some(session)
                        || r.get("key").and_then(Value::as_str) == Some(session)
                })
                .cloned()
        };
        let row = hit(&self.sessions(15.0)).or_else(|| hit(&self.sessions(2.0)));
        let sid = row
            .as_ref()
            .and_then(|r| r.get("sessionId").and_then(Value::as_str))
            .unwrap_or(session)
            .to_string();
        let items = match self.conversation(&sid) {
            Ok(c) => {
                let start = c.raw.len().saturating_sub(200);
                conversation::merge_turns(c.raw[start..].to_vec())
            }
            Err(e) => return json!({"ok": false, "error": format!("RuntimeError: {e}")}),
        };
        let calls = activity::timeline(&items);
        let start = calls.len().saturating_sub(200);
        json!({"ok": true, "source": "conversation", "timeline": calls[start..].to_vec(),
               "tally": activity::tally(&calls), "current": null})
    }

    /// Control.errors: tool failures across recent sessions.
    fn api_errors(&self, days: f64) -> Value {
        let now = Self::now();
        if let Some((at, d, result)) = self.errors_cache.lock().unwrap().as_ref() {
            if *d == days && now - at < 300.0 {
                return result.clone();
            }
        }
        let rows: Vec<Map<String, Value>> = self
            .history
            .list()
            .into_iter()
            .filter(|r| {
                now - r.get("modified").and_then(Value::as_f64).unwrap_or(0.0) <= days * 86400.0
            })
            .take(120)
            .collect();
        let mut per_project: indexmap::IndexMap<String, (i64, i64, i64)> =
            indexmap::IndexMap::new();
        let mut tallies = Vec::new();
        for r in &rows {
            let sid = r
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let size = r.get("size").cloned().unwrap_or(Value::Null);
            let cached = self
                .tallies
                .lock()
                .unwrap()
                .get(&sid)
                .filter(|c| c.0 == size)
                .map(|c| c.1.clone());
            let t = match cached {
                Some(t) => t,
                None => {
                    let Ok(c) = self.conversation(&sid) else {
                        continue;
                    };
                    let t = activity::tally(&activity::timeline(&c.merged));
                    self.tallies
                        .lock()
                        .unwrap()
                        .insert(sid.clone(), (size, t.clone()));
                    t
                }
            };
            let cwd = r
                .get("cwd")
                .filter(|v| truthy(Some(v)))
                .map(crate::pyish::py_str)
                .unwrap_or_else(|| "?".into());
            let cwd = cwd.replace('\\', "/");
            let name = cwd
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let name = if name.is_empty() {
                "?".to_string()
            } else {
                name
            };
            let p = per_project.entry(name).or_insert((0, 0, 0));
            p.0 += t["calls"].as_i64().unwrap_or(0);
            p.1 += t["errors"].as_i64().unwrap_or(0);
            p.2 += 1;
            tallies.push(t);
        }
        let mut out = activity::merge_tallies(&tallies);
        out.insert("ok".into(), json!(true));
        out.insert("days".into(), json!(days));
        out.insert("sessions".into(), json!(tallies.len()));
        out.insert(
            "by_project".into(),
            Value::Object(
                per_project
                    .into_iter()
                    .map(|(k, (c, e, s))| (k, json!({"calls": c, "errors": e, "sessions": s})))
                    .collect(),
            ),
        );
        out.insert("at".into(), json!(now));
        let result = Value::Object(out);
        *self.errors_cache.lock().unwrap() = Some((now, days, result.clone()));
        result
    }

    /// Control.changes: git status and diff in the session's folder.
    fn api_changes(&self, session: &str) -> Value {
        let Some(row) = self.row(session) else {
            return json!({"ok": false, "error": "no such session"});
        };
        let cwd = row.get("cwd").and_then(Value::as_str).unwrap_or("");
        changes::changes(cwd)
    }

    /// Control._known_project: a path the catalogue or a live session
    /// vouches for, never one taken on trust from the request.
    fn known_project(&self, path: &str) -> bool {
        if path.is_empty() {
            return false;
        }
        let Some(want) = paths::abs_normcase(path) else {
            return false;
        };
        let mut known: Vec<String> = self
            .catalogue()
            .iter()
            .filter_map(|p| p.get("path").and_then(Value::as_str).map(str::to_string))
            .collect();
        known.extend(
            self.sessions(2.0)
                .iter()
                .filter(|r| !truthy(r.get("remote")))
                .filter_map(|r| r.get("cwd").and_then(Value::as_str).map(str::to_string)),
        );
        known
            .iter()
            .any(|k| !k.is_empty() && paths::abs_normcase(k).as_deref() == Some(want.as_str()))
    }

    /// GET /api/settings: Control.settings_view.
    fn api_settings(&self) -> Value {
        let path = paths::config_home(self.home.as_deref()).join("config.json");
        let cfg = match settings::load(&path) {
            Ok(c) => c,
            Err(e) => {
                return json!({"ok": false, "error": format!("config.json cannot be read: {e}")})
            }
        };
        let mut out = settings::describe(&cfg);
        let enabled: HashMap<&str, bool> = self
            .notify_enabled()
            .into_iter()
            .map(|(n, _, on)| (n, on))
            .collect();
        if let Some(Value::Array(fields)) = out.get_mut("fields") {
            for f in fields.iter_mut() {
                let key = f["key"].as_str().unwrap_or("").to_string();
                if key == "notify.ntfy.enabled" || key == "notify.speak.enabled" {
                    let channel = key.split('.').nth(1).unwrap_or("");
                    f["value"] = json!(enabled.get(channel).copied().unwrap_or(false));
                }
            }
        }
        out["ok"] = json!(true);
        out["path"] = json!(path.to_string_lossy());
        out["waiting"] = json!([]);
        out["notify_error"] = Value::Null;
        out["shortcuts"] = settings::schema()["shortcuts"].clone();
        out
    }

    fn page(&self, page: &str, token: &str, with_look: bool) -> Body {
        let drawn = if with_look {
            Some(look(&self.config))
        } else {
            None
        };
        Body::Text(
            ui::with_meta(token, page, &self.boot, drawn.as_ref()),
            "text/html; charset=utf-8",
        )
    }

    pub fn handle(&self, method: &str, url: &str) -> (u16, Body) {
        let (raw_path, query) = url.split_once('?').unwrap_or((url, ""));
        // As the Python handler does: a trailing slash is the same path.
        let trimmed = raw_path.trim_end_matches('/');
        let path = if trimmed.is_empty() { "/" } else { trimmed };
        let q = parse_query(query);
        // Handler._arg: the first value of a query parameter, else "".
        let arg = |name: &str| q.get(name).cloned().unwrap_or_default();
        if method == "GET" {
            if let Some(name) = path.strip_prefix("/fonts/") {
                return match ui::font(name) {
                    Some(bytes) => (200, Body::Bytes(bytes, "font/woff2", "max-age=86400")),
                    None => (404, Body::Json(json!({"error": "no such font"}))),
                };
            }
        }
        match (method, path) {
            ("GET", "/") => (200, self.page(ui::SHELL, &self.window_token, true)),
            ("GET", "/matrix") => (200, self.page(ui::MATRIX, "", true)),
            ("GET", "/settings") => (200, self.page(ui::SETTINGS, &self.window_token, false)),
            ("GET", "/api/history") => (200, Body::Json(self.api_history(&q))),
            ("GET", "/api/events") => {
                // store.events(100): the newest hundred, newest first.
                let st = registry::state();
                let all = st
                    .get("events")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let recent: Vec<Value> = all.iter().rev().take(100).cloned().collect();
                (200, Body::Json(json!({"events": recent})))
            }
            ("GET", "/api/queue") => {
                // store.queued(): every session that has something queued.
                let st = registry::state();
                let q: Map<String, Value> = st
                    .get("queue")
                    .and_then(Value::as_object)
                    .map(|q| {
                        q.iter()
                            .filter(|(_, v)| truthy(Some(v)))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                (200, Body::Json(json!({"queue": q})))
            }
            ("GET", "/api/prefs") => {
                let st = registry::state();
                let prefs = st
                    .get("prefs")
                    .filter(|p| p.is_object() || p.is_array())
                    .cloned()
                    .unwrap_or(json!({}));
                (200, Body::Json(json!({"prefs": prefs})))
            }
            ("GET", "/api/decisions") => (200, Body::Json(json!({"decisions": decisions(60)}))),
            ("GET", "/api/settings") => (200, Body::Json(self.api_settings())),
            ("GET", "/api/projects") => (200, Body::Json(json!({"projects": self.catalogue()}))),
            ("GET", "/api/changes") => (200, Body::Json(self.api_changes(&arg("session")))),
            ("GET", "/api/activity") => (200, Body::Json(self.api_activity(&arg("session")))),
            ("GET", "/api/errors") => {
                let raw = arg("days");
                let raw = if raw.is_empty() { "7".to_string() } else { raw };
                let days = crate::pyish::py_float(&json!(raw))
                    .map(|d| d.clamp(0.1, 90.0))
                    .unwrap_or(7.0);
                (200, Body::Json(self.api_errors(days)))
            }
            ("GET", "/api/worktrees") => {
                let project = arg("project");
                let body = if self.known_project(&project) {
                    changes::worktrees(&project)
                } else {
                    json!({"ok": false, "error": format!("not a known project: {project}")})
                };
                (200, Body::Json(body))
            }
            ("GET", "/api/worktree_diff") => {
                let project = arg("project");
                let body = if self.known_project(&project) {
                    changes::worktree_diff(&project, &arg("path"))
                } else {
                    json!({"ok": false, "error": format!("not a known project: {project}")})
                };
                (200, Body::Json(body))
            }
            ("GET", "/api/notify") => (200, Body::Json(self.api_notify())),
            ("GET", "/api/quick") => (
                200,
                Body::Json(json!({"key": settings::value(&self.config, "quick.key"),
                                  "phrases": settings::value(&self.config, "quick.phrases"), "ok": true})),
            ),
            ("GET", "/api/health") => (
                200,
                Body::Json(
                    json!({"ok": true, "pid": std::process::id(), "boot": self.boot,
                                  "look": look(&self.config)}),
                ),
            ),
            ("GET", "/api/conversation") => self.api_conversation(&q),
            ("GET", "/api/sessions") => (200, Body::Json(self.api_sessions())),
            _ => (404, Body::Json(json!({"error": "no such path"}))),
        }
    }
}

/// Control.decisions: the permission hook's recent verdicts, newest first,
/// from the decisions.jsonl beside the state file.
fn decisions(limit: usize) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(paths::state_dir().join("decisions.jsonl")) else {
        return vec![];
    };
    // readlines() keeps each line's end; the last `limit` of them.
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..]
        .iter()
        .rev()
        .map(|l| crate::pyish::strip(l))
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

const STREAM_SECONDS: f64 = 600.0;
const STREAM_EVERY: f64 = 5.0;
const STREAM_MIN_GAP: f64 = 2.0;
const STREAM_KEEPALIVE: f64 = 15.0;

/// Row fields that never leave the process (dispatcher.PRIVATE_ROW_FIELDS).
const PRIVATE_ROW_FIELDS: &[&str] = &["token"];

fn public(rows: Vec<Row>) -> Vec<Value> {
    rows.into_iter()
        .map(|mut r| {
            for k in PRIVATE_ROW_FIELDS {
                r.shift_remove(*k);
            }
            Value::Object(r)
        })
        .collect()
}

/// dispatcher.finished_at: a finished job's last hook event, else its start.
fn finished_at(row: &Row) -> f64 {
    let num = |k: &str| {
        row.get(k)
            .filter(|v| v.is_number())
            .and_then(Value::as_f64)
            .filter(|v| *v > 0.0)
    };
    num("last_event_at")
        .or_else(|| num("startedAt").map(|s| s / 1000.0))
        .unwrap_or(0.0)
}

/// Control._overlay: fold what hooks recorded into a row. Read-only here:
/// where Python would also clear stale attention in the store, this only
/// leaves it off the row (Python's next snapshot does the same).
fn overlay(row: &mut Row, extra: &Map<String, Value>) {
    for field in [
        "socket",
        "name",
        "cwd",
        "last_notification",
        "last_event",
        "last_event_at",
        "last_failure",
        "last_failure_detail",
        "window_title",
        "hc",
    ] {
        if truthy(extra.get(field)) && !truthy(row.get(field)) {
            row.insert(field.into(), extra[field].clone());
        }
    }
    let dispatchable = |row: &Row| truthy(row.get("socket")) && truthy(row.get("token"));
    if registry::finished(row) {
        let d = dispatchable(row);
        row.insert("dispatchable".into(), json!(d));
        return;
    }
    let since = extra.get("attention_since").filter(|v| truthy(Some(v)));
    let kind = extra.get("attention_kind").and_then(Value::as_str);
    let failure = kind == Some("failure");
    let stale = !failure && registry::moved_on(row, since.and_then(Value::as_f64));
    if since.is_some() && stale {
        // cleared
    } else if since.is_some()
        && !failure
        && registry::still_running(row, extra.get("last_notification"))
    {
        // hidden while the work goes on
    } else if let Some(since) = since {
        row.insert("attention_since".into(), since.clone());
        row.insert("attention".into(), json!(true));
        row.insert(
            "attention_reason".into(),
            extra
                .get("attention_reason")
                .cloned()
                .unwrap_or(Value::Null),
        );
        row.insert(
            "attention_kind".into(),
            extra.get("attention_kind").cloned().unwrap_or(Value::Null),
        );
    }
    let d = dispatchable(row);
    row.insert("dispatchable".into(), json!(d));
}

/// A response body: a value to serialise, or JSON text already assembled.
pub enum Body {
    Json(Value),
    /// JSON text already assembled.
    Raw(String),
    /// Text of another type (a page).
    Text(String, &'static str),
    /// Bytes served as they are (a font), with their Cache-Control.
    Bytes(&'static [u8], &'static str, &'static str),
}

impl Body {
    /// (bytes, Content-Type, Cache-Control)
    pub fn into_parts(self) -> (Vec<u8>, &'static str, &'static str) {
        const JSON: &str = "application/json; charset=utf-8";
        match self {
            Body::Json(v) => (
                serde_json::to_vec(&v).unwrap_or_else(|_| b"{}".to_vec()),
                JSON,
                "no-store",
            ),
            Body::Raw(t) => (t.into_bytes(), JSON, "no-store"),
            Body::Text(t, ctype) => (t.into_bytes(), ctype, "no-store"),
            Body::Bytes(b, ctype, cache) => (b.to_vec(), ctype, cache),
        }
    }
}

/// secrets.token_urlsafe(n): n random bytes, base64url without padding.
fn token_urlsafe(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    getrandom::getrandom(&mut bytes).expect("the OS random source is unavailable");
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let v = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        let take = chunk.len() + 1;
        for i in 0..take {
            out.push(A[((v >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// parse_qs: the first value of each key, percent- and plus-decoded.
fn parse_query(q: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in q.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.entry(decode(k)).or_insert_with(|| decode(v));
    }
    out
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
                {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn serve(host: &str, port: u16, home: Option<&str>) -> i32 {
    let addr = format!("{host}:{port}");
    let server = match tiny_http::Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("imperatorium: cannot listen on {addr}: {e}");
            return 1;
        }
    };
    let shared = Arc::new(Shared::new(home));
    println!("imperatorium (native) listening on http://{addr}");
    for request in server.incoming_requests() {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            let method = request.method().as_str().to_string();
            let path = request
                .url()
                .split('?')
                .next()
                .unwrap_or("")
                .trim_end_matches('/')
                .to_string();
            if method == "GET" && path == "/api/stream" {
                let mut writer = request.into_writer();
                shared.stream(&mut *writer);
                return;
            }
            let (code, body) = shared.handle(&method, request.url());
            let (bytes, ctype, cache) = body.into_parts();
            // Content-Length, as the Python dispatcher sends: tiny_http goes
            // chunked above 32 KiB by default, and a conversation is often
            // several hundred.
            let resp = tiny_http::Response::from_data(bytes)
                .with_chunked_threshold(usize::MAX)
                .with_status_code(code)
                .with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()).unwrap(),
                )
                .with_header(
                    tiny_http::Header::from_bytes(&b"Cache-Control"[..], cache.as_bytes()).unwrap(),
                );
            let _ = request.respond(resp);
        });
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_python() {
        // python -c "import hashlib,json; print(hashlib.sha1(json.dumps([{'b':1,'a':'é'}],
        //   sort_keys=True).encode()).hexdigest()[:16])"
        assert_eq!(
            signature(&[json!({"b": 1, "a": "\u{e9}"})]),
            "09e09a7b616b14d8"
        );
    }

    #[test]
    fn query_strings() {
        let q = parse_query("session=a%2Db&known=x+y&blocks=400&session=ignored");
        assert_eq!(q["session"], "a-b");
        assert_eq!(q["known"], "x y");
        assert_eq!(q["blocks"], "400");
    }

    #[test]
    fn look_defaults() {
        let l = look(&json!({}));
        assert_eq!(l["fonts"]["chat"], 14);
        assert_eq!(l["hl"]["code"], true);
        let l = look(
            &json!({"chat": {"font_size": 15}, "highlight": {"mute": false, "colors": {"error": "#FF0000", "path": "red"}}}),
        );
        assert_eq!(l["fonts"]["chat"], 15);
        assert_eq!(l["hl"]["mute"], false);
        assert_eq!(l["colors"], json!({"error": "#ff0000"}));
    }
}
