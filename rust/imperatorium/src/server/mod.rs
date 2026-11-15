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

use crate::locks::{LockExt, RwLockExt};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};

use crate::pyish::{json_dumps_sorted, truthy};
use crate::registry::Row;
use crate::{
    activity, changes, conversation, costs, history, paths, procs, projects, registry, sessions,
    sessionstat, settings, ui,
};
mod spend;
mod prefs;
use self::prefs::{get, load_config, look};

pub struct Shared {
    /// Pushes and speech: counters, and what is scheduled.
    notifier: Arc<crate::notify::Notifier>,
    /// This Shared, for timers that outlive the request that set them.
    me: std::sync::OnceLock<std::sync::Weak<Shared>>,
    /// The Imperatorium mod in each session: its beats and the prompts
    /// waiting for it. Only with a store of our own: the token lives beside
    /// the state, and the live dispatcher's is not ours to answer for.
    modlink: Option<crate::modlink::ModLink>,
    /// Permission requests parked by the hook, waiting for the window.
    pending: crate::pending::Pending,
    /// Statusline readings and the account's windows (usage.Usage).
    usage: crate::usage::Usage,
    /// The state this dispatcher writes, when it was given one (`--state`);
    /// None is the read-only shadow, which reads the live state file.
    store: Option<crate::store::Store>,
    /// Tool events from hooks (activity.Live).
    live: crate::live::Live,
    /// Control.cost_history: the spend ledger, read incrementally.
    cost_history: Mutex<costs::History>,
    /// Control._errors (the whole answer, ERRORS_EVERY) and _tallies
    /// (each session's tally, until its transcript's size moves).
    errors_cache: Mutex<Option<(f64, f64, Value)>>,
    tallies: Mutex<HashMap<String, (Value, Value)>>,
    /// `--home`: where config.json is, read again for the Settings form.
    home: Option<String>,
    /// Each session's pull request, polled off the request path (prs.Tracker).
    prs: Arc<crate::prs::Tracker>,
    /// The dictation child (voice.Worker), when voice is on and the
    /// checkout has its venv. Only for a dispatcher with its own store.
    voice: Option<Arc<crate::voice::Worker>>,
    /// Handed to the pages when they load (ui._with_meta); never written down.
    window_token: String,
    history: history::Index,
    /// config.json as this process applies it: what was there at start, with
    /// the settings a save applies at once (Control._apply_settings).
    config_live: std::sync::RwLock<Value>,
    /// Processes a saved setting waits on (Control._restart_needed).
    restart_needed: Mutex<std::collections::BTreeSet<String>>,
    settings_lock: Mutex<()>,
    boot: String,
    /// The dispatcher's snapshot cache, the rows still holding their tokens
    /// - only `public()` copies leave the process.
    snap: Mutex<Option<Arc<Snap>>>,
    /// Held while a snapshot is built, so callers that find the cache stale
    /// together wait for one `claude agents` rather than each running one.
    snap_build: Mutex<()>,
    /// Bumped by invalidate(): a snapshot built before the last bump is
    /// stale, and the event streams wait on `changed` for the next one.
    changes: Mutex<u64>,
    changed: std::sync::Condvar,
    stats: sessionstat::Reader,
    /// _doing_line's cache: sid -> (when, line).
    doing: Mutex<HashMap<String, (f64, String)>>,
    /// convcache: the parsed conversations, least recently used first.
    convs: Mutex<indexmap::IndexMap<String, Arc<Cached>>>,
    /// convcache._paths: each session's transcript, found once.
    paths: Mutex<HashMap<String, PathBuf>>,
    /// Sessions on other machines (remote.Fleet): only for a dispatcher with
    /// a store of its own and hosts in config.json. Polled once serve() has
    /// the port.
    fleet: Option<Arc<crate::remote::Fleet>>,
    /// Host name -> its hook tunnel, started by serve().
    tunnels: Mutex<HashMap<String, Arc<crate::remote::Tunnel>>>,
    /// The newest release, asked every few hours (gap J3).
    updates: Arc<crate::update::Checker>,
    /// Delayed jobs (the idle check after Stop), all on one thread.
    timer: crate::timer::Timer,
}

/// dispatcher.remote_key: where a remote session's hook records live.
fn remote_key(host: &str, sid: &str) -> String {
    format!("{host}/{sid}")
}

/// One session snapshot: when it was taken, which invalidate() it
/// followed, its rows, and their public JSON, made at most once however
/// many streams and polls send it.
struct Snap {
    at: f64,
    generation: u64,
    rows: Arc<Vec<Row>>,
    public: std::sync::OnceLock<Arc<PublicSnap>>,
}

/// A snapshot as it leaves the process: `data` is `{"at", "sessions"}` as
/// sent, `same` the sessions alone, sorted, to tell whether anything moved.
struct PublicSnap {
    data: String,
    same: String,
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

/// sessionstat.transcript_path (the newest projects/*/<id>.jsonl, a miss
/// remembered for a moment so a poll for a session with no transcript yet
/// does not list every folder each time), with its size and mtime.
fn transcript_stamp(sid: &str) -> Option<(PathBuf, u64, u128)> {
    let p = sessionstat::transcript_path(sid)?;
    let meta = std::fs::metadata(&p).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some((p, meta.len(), mtime))
}

impl Shared {
    pub fn new(home: Option<&str>, state: Option<&str>, live: bool) -> Self {
        let store = state.map(
            |p| match crate::store::Store::open_as(std::path::Path::new(p), live) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("imperatorium: {e}");
                    std::process::exit(2);
                }
            },
        );
        let has_store = store.is_some();
        let boot: String = {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            format!("{:x}{:x}", t.as_secs(), std::process::id())
        };
        Shared {
            window_token: token_urlsafe(24),
            pending: Default::default(),
            notifier: Default::default(),
            me: Default::default(),
            modlink: store.as_ref().and_then(|s| {
                let dir = s
                    .path
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                match crate::modlink::load_token(&dir) {
                    Ok(t) => Some(crate::modlink::ModLink::new(t)),
                    Err(e) => {
                        eprintln!("imperatorium: no mod token: {e}");
                        None
                    }
                }
            }),
            usage: {
                let cfg = load_config(home);
                let thresholds: Vec<f64> = cfg
                    .get("budget")
                    .and_then(|b| b.get("thresholds"))
                    .and_then(Value::as_array)
                    .filter(|a| !a.is_empty())
                    .map(|a| a.iter().filter_map(Value::as_f64).collect())
                    .unwrap_or_else(|| crate::usage::DEFAULT_THRESHOLDS.to_vec());
                let dir = match &store {
                    Some(s) => s
                        .path
                        .parent()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(".")),
                    None => paths::state_dir(),
                };
                crate::usage::Usage::new(
                    thresholds,
                    Some(dir.join("limits.json")),
                    store.is_some(),
                    Self::now(),
                )
            },
            store,
            live: crate::live::Live::default(),
            cost_history: Mutex::new(costs::History::new()),
            errors_cache: Mutex::new(None),
            tallies: Mutex::new(HashMap::new()),
            prs: Default::default(),
            updates: Default::default(),
            timer: crate::timer::Timer::new(),
            voice: {
                let cfg = load_config(home);
                match cfg.get("voice").filter(|v| v.is_object()) {
                    Some(block) if has_store => {
                        let settings = crate::voice::settings_from(Some(block));
                        let dir = paths::config_home(home);
                        if truthy(settings.get("enabled")) && crate::voice::available(&dir, &settings) {
                            Some(crate::voice::Worker::new(settings, &dir))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            },
            home: home.map(str::to_string),
            history: history::Index::new(),
            config_live: std::sync::RwLock::new(load_config(home)),
            restart_needed: Default::default(),
            settings_lock: Default::default(),
            boot,
            snap: Mutex::new(None),
            snap_build: Mutex::new(()),
            changes: Mutex::new(0),
            changed: std::sync::Condvar::new(),
            stats: sessionstat::Reader::new(),
            doing: Mutex::new(HashMap::new()),
            convs: Mutex::new(indexmap::IndexMap::new()),
            paths: Mutex::new(HashMap::new()),
            fleet: {
                let hosts = crate::remote::load_hosts(&load_config(home));
                (has_store && !hosts.is_empty())
                    .then(|| crate::remote::Fleet::new(hosts, crate::remote::POLL_EVERY))
            },
            tunnels: Mutex::new(HashMap::new()),
        }
    }

    /// The state as store.py holds it: the store's, or the live file read.
    fn state(&self) -> Map<String, Value> {
        match &self.store {
            Some(s) => s.snapshot(),
            None => registry::state(),
        }
    }

    /// Where the state file is, and decisions.jsonl beside it.
    fn state_dir(&self) -> PathBuf {
        match &self.store {
            Some(s) => s
                .path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".")),
            None => paths::state_dir(),
        }
    }

    /// Control.on_hook: one hook event. `host` is set only by a host's
    /// tunnel listener, and a remote session's records are kept under a
    /// host-qualified key, so a host can only ever describe its own.
    fn on_hook(&self, payload: &Map<String, Value>, host: Option<&str>) -> Value {
        let Some(store) = &self.store else {
            return json!({"ok": false, "error": "a read-only shadow takes no hooks"});
        };
        let event = payload
            .get("hook_event_name")
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str)
            .unwrap_or_else(|| "unknown".into());
        let sid = payload
            .get("session_id")
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str);
        let qualified = |sid: &str| match host {
            Some(h) => remote_key(h, sid),
            None => sid.to_string(),
        };
        if let Some(sid) = &sid {
            if event == "imperatorium_statusline" || event == "ccontrol_statusline" {
                // Several a minute per session: memory only, never the store.
                if let Some(crossing) = self.usage.ingest(&qualified(sid), payload, Self::now()) {
                    self.budget_crossed(&crossing);
                }
                return json!({"ok": true});
            }
            if crate::live::TOOL_EVENTS.contains(&event.as_str()) {
                self.live.ingest(&qualified(sid), payload, Self::now());
                return json!({"ok": true});
            }
        }
        const INTERESTING: &[&str] = &[
            "SessionStart",
            "SessionEnd",
            "Notification",
            "Stop",
            "StopFailure",
            "SubagentStop",
            "TaskCompleted",
        ];
        let Some(sid) = sid.filter(|_| INTERESTING.contains(&event.as_str())) else {
            return json!({"ok": true, "ignored": event});
        };
        let key = qualified(&sid);
        let mut meta = Map::new();
        meta.insert(
            "cwd".into(),
            payload.get("cwd").cloned().unwrap_or(Value::Null),
        );
        meta.insert("last_event".into(), json!(event));
        meta.insert("last_event_at".into(), json!(Self::now()));
        let token = payload.get("messaging_token").cloned();
        if let Some(sock) = payload.get("messaging_socket").filter(|v| truthy(Some(v))) {
            meta.insert("socket".into(), sock.clone());
        }
        // herdr-core hosting comes with step 3; a SessionStart outside it
        // replaces the old terminal's record, as in Python.
        if host.is_none() && !truthy(payload.get("hc_terminal")) && event == "SessionStart" {
            meta.insert("hc".into(), json!({}));
        }
        let ev = |kind: &str, extra: Value| {
            let mut e = json!({"kind": kind, "session_id": sid})
                .as_object()
                .unwrap()
                .clone();
            if let Value::Object(x) = extra {
                e.extend(x);
            }
            if let Some(h) = host {
                e.insert("host".into(), json!(h));
            }
            store.add_event(e);
        };
        let cwd = payload.get("cwd").cloned().unwrap_or(Value::Null);
        match event.as_str() {
            "SessionStart" => {
                meta.insert(
                    "name".into(),
                    payload.get("session_name").cloned().unwrap_or(Value::Null),
                );
                store.register(&key, token.as_ref(), &meta);
                store.undismiss(&key);
                self.invalidate();
                ev("session_start", json!({"cwd": meta.get("cwd")}));
            }
            "SessionEnd" => {
                let gone = store.session(&key).unwrap_or_default();
                store.forget(&key);
                self.live.forget(&key);
                self.usage.forget(&key);
                if let (Some(link), None) = (&self.modlink, host) {
                    link.forget(&key);
                }
                self.invalidate();
                let cwd = if truthy(Some(&cwd)) {
                    cwd.clone()
                } else {
                    gone.get("cwd").cloned().unwrap_or(Value::Null)
                };
                ev(
                    "session_end",
                    json!({"cwd": cwd, "name": gone.get("name").cloned().unwrap_or(Value::Null)}),
                );
            }
            "Notification" => {
                let kind = payload
                    .get("notification_type")
                    .filter(|v| truthy(Some(v)))
                    .map(crate::pyish::py_str)
                    .unwrap_or_else(|| "notification".into());
                meta.insert("last_notification".into(), json!(kind));
                let caption = match kind.as_str() {
                    "permission_prompt" => Some("needs permission"),
                    "idle_prompt" => Some("waiting for you"),
                    "agent_needs_input" => Some("needs input"),
                    "elicitation_dialog" => Some("needs an answer"),
                    "elicitation_url_dialog" => Some("needs a URL"),
                    _ => None,
                };
                match caption {
                    Some(c) => {
                        store.raise_attention(&key, c, "notification", &meta);
                        if truthy(token.as_ref()) {
                            store.register(&key, token.as_ref(), &Map::new());
                        }
                        self.announce(&key, c);
                    }
                    None => store.register(&key, token.as_ref(), &meta),
                }
                self.invalidate();
                ev(&kind, json!({"cwd": cwd}));
            }
            "StopFailure" => {
                let summary = payload
                    .get("last_assistant_message")
                    .filter(|v| truthy(Some(v)))
                    .cloned();
                let error = payload.get("error").cloned().unwrap_or(Value::Null);
                let detail = payload
                    .get("error_details")
                    .filter(|v| truthy(Some(v)))
                    .cloned()
                    .or(summary);
                let detail_s = detail
                    .as_ref()
                    .map(crate::pyish::py_str)
                    .unwrap_or_default();
                let caption = registry::failure_caption(error.as_str(), &detail_s);
                let failure = if truthy(Some(&error)) {
                    error.clone()
                } else {
                    json!("unknown")
                };
                meta.insert("last_failure".into(), failure);
                meta.insert(
                    "last_failure_detail".into(),
                    json!(crate::pyish::take(&detail_s, 280)),
                );
                match caption {
                    Some(c) => {
                        store.raise_attention(&key, c, "failure", &meta);
                        self.announce(&key, c);
                    }
                    None => store.register(&key, None, &meta),
                }
                if truthy(token.as_ref()) {
                    store.register(&key, token.as_ref(), &Map::new());
                }
                self.invalidate();
                ev(
                    "stopfailure",
                    json!({"cwd": cwd, "error": error,
                                         "summary": crate::pyish::take(&detail_s, 280)}),
                );
            }
            _ => {
                let summary = payload
                    .get("last_assistant_message")
                    .filter(|v| truthy(Some(v)))
                    .map(crate::pyish::py_str)
                    .unwrap_or_default();
                store.register(&key, token.as_ref(), &meta);
                if ["Stop", "SubagentStop", "TaskCompleted"].contains(&event.as_str()) {
                    store.clear_attention(&key);
                }
                self.invalidate();
                ev(
                    &event.to_lowercase(),
                    json!({"cwd": cwd, "summary": crate::pyish::take(&summary, 280)}),
                );
                if event == "Stop" {
                    let stop_at = meta.get("last_event_at").cloned().unwrap_or(Value::Null);
                    if let Some(me) = self.me.get().and_then(std::sync::Weak::upgrade) {
                        let (k, at) = (key.clone(), stop_at);
                        let (h, s) = (host.map(str::to_string), sid.clone());
                        self.timer.after(
                            std::time::Duration::from_secs_f64(IDLE_AFTER),
                            move || {
                                me.idle_check(&k, &at, h.as_deref(), &s);
                            },
                        );
                    }
                    // The turn is over: one queued prompt per turn.
                    if let Some(id) = self.drain(store, &key) {
                        return json!({"ok": true, "delivered": id});
                    }
                }
            }
        }
        json!({"ok": true})
    }

    /// GET /api/matrix: Control.matrix - each running session's replies and
    /// the step it is on, plus what they have cost between them.
    fn api_matrix(&self) -> Value {
        let mut rows: Vec<Row> = self
            .sessions(2.0)
            .iter()
            .filter(|r| matrix_active(r))
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            let k = |r: &Row| {
                (
                    r.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_lowercase(),
                    r.get("key")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                )
            };
            k(a).cmp(&k(b))
        });
        let now = Self::now();
        let account = self.usage.summary(now);
        let limits = account["limits"].clone();
        let spent = self.spend(now, 1.0, &limits);
        let per: HashMap<String, Value> = spent["current"]["each"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| (e["sessionId"].as_str().unwrap_or("").to_string(), e.clone()))
            .collect();
        let mut tiles: Vec<Map<String, Value>> = Vec::new();
        for row in &rows {
            let sid = row
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let mut tile = Map::new();
            for k in [
                "key",
                "sessionId",
                "name",
                "cwd",
                "status",
                "state",
                "attention",
                "attention_kind",
                "attention_reason",
                "waitingFor",
                "last_notification",
                "background_tasks",
                "remote",
                "host",
                "work",
                "hc",
                "reviewed",
                "tasks",
                "waiting_tasks",
            ] {
                tile.insert(k.into(), row.get(k).cloned().unwrap_or(Value::Null));
            }
            let mine = per.get(&sid).cloned().unwrap_or(json!({}));
            tile.insert(
                "cost_usd".into(),
                mine.get("cost_usd").cloned().unwrap_or(Value::Null),
            );
            tile.insert(
                "context_pct".into(),
                row.get("usage")
                    .and_then(|u| u.get("context_pct"))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            let (items, error) = if truthy(row.get("remote")) {
                // The tail only, over SSH on every poll: the replies a tile
                // keeps are recent anyway (MATRIX_REMOTE_ITEMS).
                let got = self.remote_conversation(row, 150);
                (
                    got.get("items").and_then(Value::as_array).cloned().unwrap_or_default(),
                    got.get("error").filter(|e| truthy(Some(e))).map(crate::pyish::py_str),
                )
            } else {
                let got = match self.conversation(&sid) {
                    Ok(c) => (c.merged.clone(), None),
                    Err(e) => (vec![], Some(format!("RuntimeError: {e}"))),
                };
                tile.insert(
                    "tokens".into(),
                    mine.get("tokens").cloned().unwrap_or(Value::Null),
                );
                got
            };
            let live = crate::matrix::working(row) || truthy(row.get("attention"));
            tile.extend(crate::matrix::focus(&items, live));
            if let Some(e) = error {
                tile.insert("error".into(), json!(e));
            }
            tiles.push(tile);
        }
        let mut spend = crate::matrix::spend(&tiles, &limits);
        spend.insert("plan".into(), account["plan"].clone());
        spend.insert("global".into(), spent["global"].clone());
        spend.insert("models".into(), spent["current"]["models"].clone());
        spend.insert(
            "complete".into(),
            spent.get("complete").cloned().unwrap_or(json!(true)),
        );
        let always = truthy(Some(&settings::value(&self.config(), "chat.matrix_status")));
        json!({"tiles": tiles, "now": now, "spend": spend, "status_always": always})
    }

    /// Handler._post_api, for the routes that change only the dispatcher's
    /// own state. The rest (resume, new, close, worktree, settings, remote)
    /// act on sessions, terminals or files and come later in step 2.
    fn post_api(
        &self,
        store: &crate::store::Store,
        path: &str,
        p: &Map<String, Value>,
        from_window: bool,
    ) -> Value {
        match path {
            "/api/prefs" => {
                let allowed: Map<String, Value> = ["pinned", "archived", "group", "filter"]
                    .iter()
                    .filter_map(|k| p.get(*k).map(|v| (k.to_string(), v.clone())))
                    .collect();
                json!({"ok": true, "prefs": store.set_prefs(&allowed)})
            }
            "/api/notify" => {
                let channel = p.get("channel").cloned().unwrap_or(Value::Null);
                let name = channel.as_str().unwrap_or("");
                if name != "ntfy" && name != "speak" {
                    return json!({"ok": false, "error": format!("no such channel: {}",
                        if channel.is_null() { "None".to_string() } else { crate::pyish::py_str(&channel) })});
                }
                let mut saved = self
                    .state()
                    .get("prefs")
                    .and_then(|x| x.get("notify"))
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                saved.insert(name.into(), json!(truthy(p.get("enabled"))));
                let mut change = Map::new();
                change.insert("notify".into(), Value::Object(saved));
                store.set_prefs(&change);
                let mut out = self.api_notify();
                out["ok"] = json!(true);
                out
            }
            "/api/unqueue" => {
                let sid = p
                    .get("session")
                    .map(crate::pyish::py_str)
                    .unwrap_or_default();
                let ok = store.unqueue(&sid, p.get("id").unwrap_or(&Value::Null));
                self.invalidate();
                json!({"ok": ok})
            }
            "/api/dismiss" => self.dismiss_session(store, p.get("session_id")),
            "/api/reviewed" => {
                // Control.mark_reviewed.
                let sid = p.get("session_id").and_then(Value::as_str).unwrap_or("");
                let Some(row) = self.row(sid).filter(|_| !sid.is_empty()) else {
                    return json!({"ok": false, "error": "no such session"});
                };
                if !truthy(row.get("attention")) {
                    return json!({"ok": true, "reviewed": truthy(row.get("reviewed"))});
                }
                if registry::in_progress(&row) {
                    // Work still going cannot have been reviewed; the window
                    // checks too, but its copy of the row can be a second old.
                    return json!({"ok": true, "reviewed": false, "in_progress": true});
                }
                store.mark_reviewed(sid, Self::now());
                self.invalidate();
                json!({"ok": true, "reviewed": true})
            }
            "/api/queue" => {
                let prompt = p.get("prompt").and_then(Value::as_str).unwrap_or("");
                if crate::pyish::strip(prompt).is_empty() {
                    return json!({"ok": false, "error": "no prompt"});
                }
                let sid = p.get("session_id").and_then(Value::as_str).unwrap_or("");
                if self.row(sid).is_none() {
                    return json!({"ok": false, "error": "no such session"});
                }
                self.enqueue(store, sid, prompt, from_window)
            }
            _ => json!({"ok": false, "error": "no such path"}),
        }
    }

    /// Control._idle_check: the `idle_prompt` Claude Code no longer sends. A
    /// minute after Stop, a session that is idle, has had no hook since and
    /// has nothing running is waiting for you. True when it raised it.
    fn idle_check(&self, key: &str, stop_at: &Value, host: Option<&str>, sid: &str) -> bool {
        let Some(store) = &self.store else {
            return false;
        };
        let rec = store.session(key).unwrap_or_default();
        if rec.get("last_event_at") != Some(stop_at) || truthy(rec.get("attention_since")) {
            return false;
        }
        self.invalidate();
        let Some(row) = self.row(key) else {
            return false;
        };
        if row.get("status").and_then(Value::as_str) != Some("idle") {
            return false;
        }
        // Background work holds idle_prompt back; a pending wakeup is not
        // work, but the session will take its next turn by itself.
        let work = row.get("work").cloned().unwrap_or(json!({}));
        let busy = |k: &str| work.get(k).is_some_and(|v| truthy(Some(v)));
        if registry::still_running(&row, Some(&json!("idle_prompt")))
            || truthy(row.get("shell_processes"))
            || ["agents", "shells", "monitors", "wakeups"]
                .iter()
                .any(|k| busy(k))
        {
            return false;
        }
        let caption = "waiting for you";
        let mut meta = Map::new();
        meta.insert("last_notification".into(), json!("idle_prompt"));
        store.raise_attention(key, caption, "notification", &meta);
        self.announce(key, caption);
        self.invalidate();
        let mut e = json!({"kind": "idle_prompt", "session_id": if sid.is_empty() { key } else { sid },
                       "cwd": rec.get("cwd").cloned().unwrap_or(Value::Null),
                       "summary": "raised by the dispatcher"});
        if let Some(h) = host {
            e["host"] = json!(h);
        }
        store.add_event_json(e);
        true
    }

    /// Control._announce: tell the notifier a session started waiting,
    /// named by its project, which reads better aloud than `gem-trip-4`.
    fn announce(&self, key: &str, caption: &str) {
        let (Some(store), Some(me)) = (
            &self.store,
            self.me.get().and_then(std::sync::Weak::upgrade),
        ) else {
            return;
        };
        let rec = store.session(key).unwrap_or_default();
        let since = rec.get("attention_since").cloned().unwrap_or(Value::Null);
        let cwd = rec
            .get("cwd")
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let base = |p: &str| p.rsplit(['/', '\\']).next().unwrap_or("").to_string();
        let name = rec
            .get("name")
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str)
            .or_else(|| Some(base(cwd.trim_end_matches(['/', '\\']))).filter(|b| !b.is_empty()));
        let project = Some(base(cwd.replace('\\', "/").trim_end_matches('/')))
            .filter(|b| !b.is_empty())
            .or(name);
        let owner: Arc<dyn crate::notify::Waiting> = me;
        self.notifier
            .attention(owner, key, project, Some(caption.to_string()), since);
    }

    /// Control.mod_beat: what the session's mod says it is doing, its usage
    /// and what became of its prompts; answers the prompts to submit now and
    /// the line to show under the prompt.
    fn mod_beat(&self, link: &crate::modlink::ModLink, beat: &Map<String, Value>) -> Value {
        let sid = beat
            .get("session_id")
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        if sid.is_empty() {
            return json!({"ok": false, "error": "no session_id"});
        }
        let now = Self::now();
        let was = link.state(&sid, now);
        let give = link.beat(beat, now);
        let acks = beat
            .get("acks")
            .filter(|v| truthy(Some(v)))
            .and_then(Value::as_array);
        if let Some(store) = &self.store {
            for ack in acks.into_iter().flatten() {
                if ack.get("outcome").and_then(Value::as_str) == Some("dropped") {
                    let reason = ack
                        .get("reason")
                        .filter(|v| truthy(Some(v)))
                        .map(crate::pyish::py_str)
                        .unwrap_or_default();
                    let e = json!({"kind": "dispatch_dropped", "session_id": sid,
                                   "cwd": beat.get("cwd").cloned().unwrap_or(Value::Null),
                                   "summary": crate::pyish::take(&reason, 280)});
                    store.add_event_json(e);
                }
            }
        }
        if let Some(u) = beat.get("usage").filter(|u| u.is_object()) {
            let reading = crate::modlink::statusline_payload(u);
            if let Some(crossing) = self.usage.ingest(&sid, reading.as_object().unwrap(), now) {
                self.budget_crossed(&crossing);
            }
        }
        let busy = truthy(beat.get("busy"));
        if was.as_ref().is_none_or(|w| truthy(w.get("busy")) != busy)
            || !give.is_empty()
            || truthy(beat.get("acks"))
        {
            self.invalidate();
        }
        json!({"ok": true, "deliver": give, "status": self.mod_status_line(link, &sid)})
    }

    /// Control.mod_status_line: who else is waiting on you, and what is
    /// queued here. None: nothing to say.
    fn mod_status_line(&self, link: &crate::modlink::ModLink, sid: &str) -> Value {
        // Any snapshot from the last 15 s will do: a beat comes every two
        // seconds from every session.
        let rows = self.sessions(15.0);
        let waiting: Vec<&Row> = rows
            .iter()
            .filter(|r| {
                truthy(r.get("attention"))
                    && r.get("sessionId").and_then(Value::as_str) != Some(sid)
            })
            .collect();
        let mut parts = vec![];
        if !waiting.is_empty() {
            let names: Vec<String> = waiting
                .iter()
                .take(3)
                .map(|r| {
                    let name = r
                        .get("name")
                        .filter(|v| truthy(Some(v)))
                        .map(crate::pyish::py_str)
                        .unwrap_or_else(|| {
                            let cwd = r
                                .get("cwd")
                                .filter(|v| truthy(Some(v)))
                                .map(crate::pyish::py_str)
                                .unwrap_or_default();
                            let trimmed = cwd.trim_end_matches(['/', '\\']);
                            let base = trimmed.rsplit(['/', '\\']).next().unwrap_or("");
                            let base = if cfg!(windows) && base.len() == 2 && base.ends_with(':') {
                                ""
                            } else {
                                base
                            };
                            if base.is_empty() {
                                "?".to_string()
                            } else {
                                base.to_string()
                            }
                        });
                    crate::pyish::take(&name, 24)
                })
                .collect();
            let more = if waiting.len() > 3 {
                format!(" +{}", waiting.len() - 3)
            } else {
                String::new()
            };
            parts.push(format!(
                "{} waiting on you: {}{}",
                waiting.len(),
                names.join(", "),
                more
            ));
        }
        let queued = self
            .store
            .as_ref()
            .map(|s| s.queued(sid).len())
            .unwrap_or(0);
        let here = queued + link.pending(sid).len();
        if here > 0 {
            parts.push(format!("{here} queued here"));
        }
        if parts.is_empty() {
            Value::Null
        } else {
            json!(parts.join(" \u{b7} "))
        }
    }

    /// Control.mod_check: the permission policy's verdict on a tool call the
    /// session's mod asks about - the same policy.decide the hook runs, and
    /// only once auto-approval is on (`decision_format: "object"`).
    fn mod_check(&self, request: &Map<String, Value>) -> Value {
        let policy = std::fs::read_to_string(paths::policy_path(self.home.as_deref()))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        if policy.get("decision_format").and_then(Value::as_str) != Some("object") {
            return json!({"verdict": "ask", "reason": "auto-approval is off"});
        }
        let get = |k: &str| request.get(k).cloned().unwrap_or(Value::Null);
        let req = json!({"tool_name": get("tool_name"), "tool_input": get("tool_input"),
                         "cwd": get("cwd"), "session_id": get("session_id")});
        let got = crate::policy::decide(&req, &policy);
        let empty = Map::new();
        let input = req["tool_input"].as_object().unwrap_or(&empty);
        let detail = ["command", "file_path", "pattern"]
            .iter()
            .find_map(|k| input.get(*k).filter(|v| truthy(Some(v))))
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let record = json!({"at": Self::now(), "verdict": got.verdict, "reason": got.reason, "tier": got.tier,
                            "tool": req["tool_name"], "via": "mod", "detail": crate::pyish::take(&detail, 200),
                            "cwd": req["cwd"], "session_id": req["session_id"], "agent_id": get("agent_id")});
        // decisions.jsonl beside the state, as the hook writes its own.
        let path = self.state_dir().join("decisions.jsonl");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
        {
            use std::io::Write;
            let _ =
                f.write_all(format!("{}\n", crate::pyish::json_dumps(&record, false)).as_bytes());
        }
        json!({"verdict": got.verdict, "reason": got.reason, "tier": got.tier})
    }

    /// Control.resume: resume or fork a local session, in a new tab or in
    /// the background.
    fn resume(
        &self,
        store: &crate::store::Store,
        session_id: &str,
        where_: &str,
        fork: bool,
        prompt: Option<&str>,
        cwd: Option<String>,
    ) -> Value {
        let row = self.row(session_id);
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("remote"))) {
            return json!({"ok": false, "error": format!("resuming a session on {} is not supported yet",
                crate::pyish::py_str(r.get("host").unwrap_or(&Value::Null)))});
        }
        let text = |r: &Row, k: &str| {
            r.get(k)
                .filter(|v| truthy(Some(v)))
                .map(crate::pyish::py_str)
        };
        let mut cwd = cwd.or_else(|| row.as_ref().and_then(|r| text(r, "cwd")));
        if cwd.is_none() {
            cwd = self
                .history
                .list()
                .iter()
                .find(|h| h.get("session_id").and_then(Value::as_str) == Some(session_id))
                .and_then(|h| {
                    h.get("cwd")
                        .filter(|v| truthy(Some(v)))
                        .map(crate::pyish::py_str)
                });
        }
        if let Some(r) = &row {
            if (truthy(r.get("pid")) || truthy(r.get("status"))) && !fork && where_ == "window" {
                return json!({"ok": false, "error": "that session is still running - focus it, or fork it to branch off"});
            }
        }
        if where_ == "window" && self.hc_hosting() {
            return json!({"ok": false, "error": "herdr-core hosting is not ported yet"});
        }
        let options = self
            .config()
            .get("background")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let got =
            match crate::launch::resume(session_id, cwd.as_deref(), where_, fork, prompt, &options)
            {
                Ok(g) => g,
                Err(e) => return json!({"ok": false, "error": e}),
            };
        let e = json!({"kind": if fork { "fork" } else { "resume" }, "session_id": session_id,
                       "cwd": cwd, "summary": got["command"]});
        store.add_event_json(e);
        self.invalidate();
        got
    }

    /// Whether window sessions start in a herdr-core terminal (terminal.host).
    fn hc_hosting(&self) -> bool {
        settings::value(&self.config(), "terminal.host").as_str() == Some("hc")
    }

    /// Control.new_session: a fresh session in the same project, in a new
    /// tab; with `close`, the old one ends once the new one is launched.
    fn new_session(
        &self,
        store: &crate::store::Store,
        session_id: Option<&str>,
        cwd: Option<String>,
        close: bool,
    ) -> Value {
        let row = session_id.and_then(|s| self.row(s));
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("remote"))) {
            return json!({"ok": false, "error": format!("starting a session on {} is not supported yet",
                crate::pyish::py_str(r.get("host").unwrap_or(&Value::Null)))});
        }
        if close && !row.as_ref().is_some_and(|r| truthy(r.get("pid"))) {
            return json!({"ok": false, "error": NOT_RUNNING});
        }
        let cwd = cwd.or_else(|| {
            row.as_ref().and_then(|r| {
                r.get("cwd")
                    .filter(|v| truthy(Some(v)))
                    .map(crate::pyish::py_str)
            })
        });
        if self.hc_hosting() {
            return json!({"ok": false, "error": "herdr-core hosting is not ported yet"});
        }
        let sid = uuid4();
        let mut got = match crate::launch::new_session(cwd.as_deref(), &sid) {
            Ok(g) => g,
            Err(e) => return json!({"ok": false, "error": e}),
        };
        let e = json!({"kind": "new", "cwd": cwd, "summary": got["command"], "session_id": got["session_id"]});
        store.add_event_json(e);
        if close {
            got["closed"] = self.close_row(store, row.as_ref().unwrap());
        }
        self.invalidate();
        got
    }

    /// Control.close_session: end a local session and, with it, its tab.
    fn close_session(&self, store: &crate::store::Store, session_id: Option<&str>) -> Value {
        let row = session_id.and_then(|s| self.row(s));
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("remote"))) {
            return json!({"ok": false, "error": format!("closing a session on {} is not supported yet",
                crate::pyish::py_str(r.get("host").unwrap_or(&Value::Null)))});
        }
        let Some(row) = row.filter(|r| truthy(r.get("pid"))) else {
            return json!({"ok": false, "error": NOT_RUNNING});
        };
        let got = self.close_row(store, &row);
        self.invalidate();
        got
    }

    /// Control._close_row. (A herdr-core terminal is closed through hc,
    /// which comes with herdr-core hosting.)
    fn close_row(&self, store: &crate::store::Store, row: &Row) -> Value {
        let hosted = row
            .get("hc")
            .and_then(|h| h.get("address"))
            .is_some_and(|a| truthy(Some(a)));
        let got = if hosted {
            json!({"ok": false, "error": "closing a herdr-core terminal is not ported yet"})
        } else {
            crate::closing::close(row.get("pid").unwrap_or(&Value::Null))
        };
        let summary = got
            .get("how")
            .filter(|v| truthy(Some(v)))
            .or(got.get("error"))
            .cloned()
            .unwrap_or(Value::Null);
        let e = json!({"kind": "close", "cwd": row.get("cwd").cloned().unwrap_or(Value::Null),
                       "session_id": row.get("sessionId").cloned().unwrap_or(Value::Null), "summary": summary});
        store.add_event_json(e);
        got
    }

    /// Control.park_wait: how long a parked request is held. A background
    /// session has no terminal, so it gets `wait_background`; a question
    /// takes longer to answer than an Allow.
    fn park_wait(&self, p: &Map<String, Value>) -> f64 {
        let num = |k: &str| {
            p.get(k)
                .filter(|v| truthy(Some(v)))
                .and_then(crate::pyish::py_float)
                .unwrap_or(0.0)
        };
        let mut wait = num("wait");
        if p.get("tool_name").and_then(Value::as_str) == Some(crate::pending::QUESTION_TOOL)
            && p.get("wait_question").is_some_and(|v| !v.is_null())
        {
            wait = wait.max(
                p.get("wait_question")
                    .and_then(crate::pyish::py_float)
                    .unwrap_or(0.0),
            );
        }
        let Some(bg) = p.get("wait_background").filter(|v| !v.is_null()) else {
            return wait;
        };
        let sid = p.get("session_id").and_then(Value::as_str);
        let background = self
            .sessions(2.0)
            .iter()
            .find(|r| r.get("sessionId").and_then(Value::as_str) == sid && sid.is_some())
            .is_some_and(|r| r.get("kind").and_then(Value::as_str) == Some("background"));
        if background {
            return crate::pyish::py_float(bg).unwrap_or(0.0);
        }
        wait
    }

    /// Control.enqueue: hold `prompt` until the session's turn ends; one
    /// that is idle and reachable gets it at once.
    fn enqueue(
        &self,
        store: &crate::store::Store,
        session_id: &str,
        prompt: &str,
        as_user: bool,
    ) -> Value {
        let Some(row) = self.row(session_id) else {
            return json!({"ok": false, "error": "no such session"});
        };
        if crate::slash::is_command(prompt) {
            // Refuse now what could never be typed, rather than leave it at
            // the head of the queue failing on every sweep.
            if let Some(why) =
                crate::slash::refusal(prompt).or_else(|| crate::slash::unreachable(&row))
            {
                return json!({"ok": false, "error": why});
            }
        }
        let key = row
            .get("key")
            .and_then(Value::as_str)
            .filter(|k| !k.is_empty())
            .unwrap_or(session_id)
            .to_string();
        let item = store.enqueue(&key, prompt, as_user);
        self.invalidate();
        if row.get("status").and_then(Value::as_str) == Some("idle")
            && !truthy(row.get("attention"))
            && truthy(row.get("dispatchable"))
        {
            self.drain(store, &key);
        }
        json!({"ok": true, "queued": item, "session": session_id,
               "waiting": store.queued(&key).len()})
    }

    /// Control.drain: deliver the oldest queued prompt for `key`; its id.
    fn drain(&self, store: &crate::store::Store, key: &str) -> Option<Value> {
        if store.queued(key).is_empty() {
            return None;
        }
        self.invalidate();
        let row = self
            .sessions(2.0)
            .iter()
            .find(|r| r.get("key").and_then(Value::as_str) == Some(key))
            .cloned()?;
        if !truthy(row.get("dispatchable")) {
            return None; // the sweep retries once it registers
        }
        let item = store.take_queued(key)?;
        let prompt = item.get("prompt").and_then(Value::as_str).unwrap_or("");
        let error = if crate::slash::is_command(prompt) {
            let got = self.run_command(store, &row, prompt);
            if got["ok"] == json!(true) {
                None
            } else {
                Some(crate::pyish::py_str(&got["error"]))
            }
        } else {
            self.deliver(store, &row, prompt, truthy(item.get("as_user")))
        };
        if let Some(e) = error {
            store.requeue_front(key, &item, Some(&e));
            return None;
        }
        self.invalidate();
        item.get("id").cloned()
    }

    /// Control.sweep_queue: deliver to queued sessions that are idle and not
    /// waiting on you - a prompt queued before the session had a token, or a
    /// missed Stop.
    fn sweep_queue(&self) -> Vec<Value> {
        let Some(store) = &self.store else {
            return vec![];
        };
        let keys = store.queued_keys();
        if keys.is_empty() {
            return vec![];
        }
        self.invalidate();
        let rows: HashMap<String, Row> = self
            .sessions(2.0)
            .iter()
            .filter_map(|r| Some((r.get("key")?.as_str()?.to_string(), r.clone())))
            .collect();
        let mut out = vec![];
        for key in keys {
            let Some(row) = rows.get(&key) else { continue };
            if row.get("status").and_then(Value::as_str) == Some("idle")
                && !truthy(row.get("attention"))
                && truthy(row.get("dispatchable"))
            {
                if let Some(id) = self.drain(store, &key) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// Control._deliver: put the prompt in the session's inbox. None, or the
    /// error. (The mod's path, for sessions whose mod is live, comes with the
    /// mod link; remote hosts with phase 4.)
    fn deliver(
        &self,
        store: &crate::store::Store,
        row: &Row,
        prompt: &str,
        as_user: bool,
    ) -> Option<String> {
        let sid = row.get("sessionId").and_then(Value::as_str).unwrap_or("");
        if let Some(link) = self
            .modlink
            .as_ref()
            .filter(|l| !truthy(row.get("remote")) && l.live(sid, Self::now()))
        {
            // The mod submits it once the session is idle: as the person's
            // own words only when they sent it from the window.
            let text = if as_user {
                prompt.to_string()
            } else {
                crate::conversation::with_origin(prompt)
            };
            let id = link.offer(sid, &text, as_user, Self::now());
            let e = json!({"kind": "dispatch_mod", "session_id": sid,
                           "cwd": row.get("cwd").cloned().unwrap_or(Value::Null), "as_user": as_user,
                           "mod_prompt": id, "summary": crate::pyish::take(prompt, 280)});
            store.add_event_json(e);
            self.invalidate();
            return None;
        }
        let text = crate::conversation::with_origin(prompt);
        let socket = row.get("socket").and_then(Value::as_str).unwrap_or("");
        let token = row.get("token").and_then(Value::as_str);
        let host = truthy(row.get("remote"))
            .then(|| row.get("host").map(crate::pyish::py_str).unwrap_or_default());
        let sent = match &host {
            Some(h) => match self.fleet.as_ref().filter(|f| f.host(h).is_some()) {
                None => return Some(format!("host {h} is not configured")),
                Some(f) => f.inject(h, socket, token, &text),
            },
            None => crate::inbox::send(socket, token, &text),
        };
        if let Err(e) = sent {
            return Some(e);
        }
        let sid = row.get("sessionId").cloned().unwrap_or(Value::Null);
        let summary = crate::pyish::take(&crate::conversation::strip_origin(&text), 280);
        let mut e = json!({"kind": "dispatch_inject", "session_id": sid,
                       "cwd": row.get("cwd").cloned().unwrap_or(Value::Null), "summary": summary});
        if let Some(h) = host.filter(|h| !h.is_empty()) {
            e["host"] = json!(h);
        }
        store.add_event_json(e);
        None
    }

    /// Control.dispatch: route an utterance. mode is auto | inject | launch |
    /// background | dry-run. `session_id` or `path` skip the name matching:
    /// the window has already chosen. Only from the window does a prompt
    /// speak as the person (as_user).
    fn dispatch(
        &self,
        store: &crate::store::Store,
        p: &Map<String, Value>,
        as_user: bool,
    ) -> Value {
        let text = |k: &str| {
            p.get(k)
                .filter(|v| truthy(Some(v)))
                .map(crate::pyish::py_str)
        };
        let mode = match p.get("mode") {
            None => "auto".to_string(),
            Some(v) => crate::pyish::py_str(v),
        };
        let session_id = text("session_id");
        let mut prompt = text("prompt");
        let mut project = text("project");
        if let (Some(sid), Some(pr)) = (&session_id, &prompt) {
            if mode == "auto" || mode == "inject" {
                let Some(row) = self
                    .sessions(2.0)
                    .iter()
                    .find(|r| r.get("sessionId").and_then(Value::as_str) == Some(sid))
                    .cloned()
                else {
                    return json!({"ok": false, "error": "no such session"});
                };
                if crate::slash::is_command(pr) {
                    return self.run_command(store, &row, pr);
                }
                if !truthy(row.get("dispatchable")) {
                    // Claude Code relaunching a session (its daemon does, on an
                    // update) gives it a new inbox without a hook to say so.
                    if truthy(row.get("stale_credential")) {
                        return json!({"ok": false, "error":
                            "that session is not reachable (Claude Code restarted it, so its credential is out of date - it registers again on its next hook event)"});
                    }
                    return json!({"ok": false, "error":
                        "that session is not reachable (no credential yet - it registers on its next hook event)"});
                }
                if let Some(e) = self.deliver(store, &row, pr, as_user) {
                    return json!({"ok": false, "error": e});
                }
                return json!({"ok": true, "action": "inject", "session": sid,
                              "session_name": row.get("name").cloned().unwrap_or(Value::Null),
                              "host": row.get("host").cloned().unwrap_or(Value::Null)});
            }
        }
        if let (Some(t), None) = (text("text"), &prompt) {
            let (spoken, said) = crate::projects::parse_utterance(&t);
            prompt = Some(said).filter(|s| !s.is_empty());
            project = project.or(spoken);
        }
        let Some(prompt) = prompt else {
            return json!({"ok": false, "error": "no prompt"});
        };
        let res = if let Some(path) = text("path") {
            match self.exact_project(&path, text("host").as_deref()) {
                Some(r) => r,
                None => return json!({"ok": false, "error": format!("no project at {path}")}),
            }
        } else if let Some(name) = &project {
            crate::projects::resolve(name, &self.catalogue())
        } else {
            return json!({"ok": false, "error": "no project named"});
        };
        let heard = project
            .clone()
            .map(Value::from)
            .unwrap_or_else(|| res["match"]["name"].clone());
        let mut out = json!({"heard_project": heard, "prompt": prompt, "confidence": res["confidence"],
                             "score": res["score"], "candidates": res["candidates"]});
        let shown = crate::pyish::py_repr(&project.clone().map(Value::from).unwrap_or(Value::Null));
        let set = |out: &mut Value, more: Value| {
            if let Value::Object(more) = more {
                for (k, v) in more {
                    out[&k] = v;
                }
            }
        };
        if res["confidence"] == "none" {
            set(
                &mut out,
                json!({"ok": false, "error": format!("no project matched {shown}")}),
            );
            return out;
        }
        // A merely probable match is never acted on silently: a misheard
        // name costs a keystroke, not a prompt at the wrong repository.
        if res["confidence"] != "certain" && !truthy(p.get("confirm")) {
            set(
                &mut out,
                json!({"ok": false, "needs_confirmation": true,
                                 "error": format!("ambiguous project {shown}")}),
            );
            return out;
        }
        let m = res["match"].clone();
        out["project"] = m.clone();
        if mode == "dry-run" {
            set(&mut out, json!({"ok": true, "action": "dry-run"}));
            return out;
        }
        let path = m["path"].as_str().unwrap_or("").to_string();
        let remote = truthy(m.get("host"));
        if crate::slash::is_command(&prompt) {
            // Never a launch: a new session started on "/compact" would be a
            // stranger doing nothing useful.
            let target = if remote {
                None
            } else {
                self.session_for(&path)
            };
            let Some(target) = target else {
                set(
                    &mut out,
                    json!({"ok": false, "error":
                    "no running session on this machine for that project to run the command in"}),
                );
                return out;
            };
            let got = self.run_command(store, &target, &prompt);
            set(&mut out, got);
            return out;
        }
        let background_options = || -> Map<String, Value> {
            self.config()
                .get("background")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|(k, _)| {
                    ["model", "permission_mode", "agent", "name", "extra"].contains(&k.as_str())
                })
                .collect()
        };
        if remote {
            let name = crate::pyish::py_str(&m["host"]);
            let Some(host) = self
                .fleet
                .as_ref()
                .and_then(|f| f.host(&name).cloned())
            else {
                set(
                    &mut out,
                    json!({"ok": false, "error": format!("host {name} is not configured")}),
                );
                return out;
            };
            // A live session there whose hooks have reported in is reached
            // as a local one is. Otherwise - and always for launch or
            // background, with no window here to open - the work becomes a
            // background session over there.
            if mode == "auto" || mode == "inject" {
                let target = self.session_for_on(&path, Some(&name));
                let error = match &target {
                    Some(t) => self.deliver(store, t, &prompt, as_user),
                    None => Some("no dispatchable session for that project".to_string()),
                };
                match (error, target) {
                    (None, Some(t)) => {
                        set(
                            &mut out,
                            json!({"ok": true, "action": "inject", "host": name,
                                   "session": t.get("sessionId").cloned().unwrap_or(Value::Null),
                                   "session_name": t.get("name").cloned().unwrap_or(Value::Null)}),
                        );
                        return out;
                    }
                    (Some(e), _) if mode == "inject" => {
                        set(&mut out, json!({"ok": false, "error": e}));
                        return out;
                    }
                    _ => {}
                }
            }
            match crate::remote::start_background(&host, &path, &prompt, &background_options()) {
                Err(e) => set(&mut out, json!({"ok": false, "error": e})),
                Ok(started) => {
                    let e = json!({"kind": "dispatch_remote", "cwd": path,
                                   "summary": crate::pyish::take(&prompt, 280),
                                   "host": name, "bg_id": started["id"]});
                    store.add_event_json(e);
                    set(
                        &mut out,
                        json!({"ok": true, "action": "remote", "host": name, "bg_id": started["id"]}),
                    );
                }
            }
            return out;
        }
        if mode == "background" {
            // In a worktree of its own, made here from the branch the chosen
            // session has checked out (else the project's): left to Claude
            // Code it would branch from the remote's default branch, which on
            // a long-lived feature branch is the wrong code.
            let options = background_options();
            let mut source = path.clone();
            if let Some(row) = session_id.as_deref().and_then(|sid| self.row(sid)) {
                let cwd = row.get("cwd").and_then(Value::as_str).unwrap_or("");
                if !truthy(row.get("remote")) && !cwd.is_empty() && std::path::Path::new(cwd).is_dir() {
                    source = cwd.to_string();
                }
            }
            let name = crate::background::short_name(&prompt)
                .or_else(|| options.get("name").filter(|v| truthy(Some(v))).map(crate::pyish::py_str))
                .unwrap_or_else(|| "bg".into());
            let made = match crate::changes::make_worktree(&source, &name) {
                Ok(m) => m,
                Err(e) => {
                    set(&mut out, json!({"ok": false, "error": format!("could not make its worktree: {e}")}));
                    return out;
                }
            };
            let run_in = made
                .as_ref()
                .and_then(|m| m["path"].as_str())
                .unwrap_or(&path)
                .to_string();
            match crate::background::start(&prompt, &run_in, &options) {
                Err(e) => {
                    if let Some(m) = &made {
                        crate::changes::drop_worktree(m);
                    }
                    set(&mut out, json!({"ok": false, "error": e}));
                }
                Ok(started) => {
                    let mut e = json!({"kind": "dispatch_background", "cwd": path,
                                   "summary": crate::pyish::take(&prompt, 280), "bg_id": started["id"]});
                    let mut done = json!({"ok": true, "action": "background", "bg_id": started["id"]});
                    if let Some(m) = &made {
                        let base = match m["base"].as_str() {
                            Some(b) => b.to_string(),
                            None => m["start"].as_str().unwrap_or("").chars().take(12).collect(),
                        };
                        let extra = [("worktree", m["path"].clone()), ("branch", m["branch"].clone()),
                                     ("base", json!(base))];
                        for (k, v) in extra {
                            e[k] = v.clone();
                            done[k] = v;
                        }
                    }
                    store.add_event_json(e);
                    self.invalidate();
                    set(&mut out, done);
                }
            }
            return out;
        }
        if mode == "auto" || mode == "inject" {
            match self.session_for(&path) {
                Some(target) => match self.deliver(store, &target, &prompt, as_user) {
                    None => {
                        set(
                            &mut out,
                            json!({"ok": true, "action": "inject",
                            "session": target.get("sessionId").cloned().unwrap_or(Value::Null),
                            "session_name": target.get("name").cloned().unwrap_or(Value::Null)}),
                        );
                        return out;
                    }
                    Some(e) if mode == "inject" => {
                        set(&mut out, json!({"ok": false, "error": e}));
                        return out;
                    }
                    Some(_) => {} // auto: a window instead
                },
                None if mode == "inject" => {
                    set(
                        &mut out,
                        json!({"ok": false, "error": "no dispatchable session for that project"}),
                    );
                    return out;
                }
                None => {}
            }
        }
        match crate::deeplink::launch(&path, &prompt) {
            Err(e) => set(&mut out, json!({"ok": false, "error": e})),
            Ok(url) => {
                let e = json!({"kind": "dispatch_launch", "cwd": path, "summary": crate::pyish::take(&prompt, 280)});
                store.add_event_json(e);
                set(
                    &mut out,
                    json!({"ok": true, "action": "launch", "url": url}),
                );
            }
        }
        out
    }

    /// Control._exact_project: the catalogue entry at `path`, as a certain
    /// match. (The catalogue holds this machine's projects only until remote
    /// hosts are ported.)
    fn exact_project(&self, path: &str, host: Option<&str>) -> Option<Value> {
        let host = host.filter(|h| !h.is_empty());
        let want = match host {
            // A remote path is the host's, compared as written.
            Some(_) => path.to_string(),
            None => paths::abs_normcase(path)?,
        };
        let proj = self.catalogue().into_iter().find(|p| {
            let theirs = p.get("host").filter(|h| truthy(Some(h))).map(crate::pyish::py_str);
            theirs.as_deref() == host
                && match host {
                    Some(_) => p["path"].as_str() == Some(want.as_str()),
                    None => {
                        p["path"].as_str().and_then(paths::abs_normcase).as_deref()
                            == Some(want.as_str())
                    }
                }
        })?;
        Some(json!({"match": proj, "score": 1.0, "confidence": "certain", "candidates": [proj]}))
    }

    /// Control._session_for: the best dispatchable session in `path` on this
    /// machine - an idle one if there is one, since a busy one would queue it
    /// behind its own work.
    fn session_for(&self, path: &str) -> Option<Row> {
        self.session_for_on(path, None)
    }

    /// The same on `host` (None: here). A remote path is the host's POSIX
    /// path, compared as written less a trailing slash: normcase/abspath
    /// here would make it a Windows path on the current drive.
    fn session_for_on(&self, path: &str, host: Option<&str>) -> Option<Row> {
        let want = match host {
            Some(_) => path.trim_end_matches('/').to_string(),
            None => paths::abs_normcase(path)?,
        };
        let same = |cwd: &str| match host {
            Some(_) => cwd.trim_end_matches('/') == want,
            None => paths::abs_normcase(cwd).as_deref() == Some(want.as_str()),
        };
        let mut best: Option<Row> = None;
        for row in self.sessions(2.0).iter() {
            let Some(cwd) = row
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())
            else {
                continue;
            };
            if !truthy(row.get("dispatchable")) {
                continue;
            }
            let row_host = row.get("host").filter(|h| truthy(Some(h))).map(crate::pyish::py_str);
            if row_host.as_deref() != host || truthy(row.get("remote")) != host.is_some() {
                continue;
            }
            if !same(cwd) {
                continue;
            }
            let idle = |r: &Row| r.get("status").and_then(Value::as_str) == Some("idle");
            if best.is_none() || (idle(&row) && !idle(best.as_ref().unwrap())) {
                best = Some(row.clone());
            }
        }
        best
    }

    /// Control.transcribe: browser-recorded audio to text in the dictation
    /// child, where the model stays loaded.
    fn transcribe(&self, audio: &[u8]) -> Value {
        let Some(v) = &self.voice else {
            return json!({"ok": false, "error": "dictation is not available"});
        };
        match v.transcribe_bytes(audio, None) {
            Err(e) => json!({"ok": false, "error": format!("VoiceError: {e}")}),
            Ok(text) => {
                let mut out = v.where_().and_then(|w| w.as_object().cloned()).unwrap_or_default();
                out.insert("ok".into(), json!(true));
                out.insert("text".into(), json!(text));
                Value::Object(out)
            }
        }
    }

    /// Control.warm_voice: the mic was pressed - build the model while the
    /// person speaks, and say where the last load landed.
    fn warm_voice(&self) -> Value {
        let Some(v) = &self.voice else {
            return json!({"ok": false, "error": "dictation is not available"});
        };
        v.warm();
        let mut out = v.where_().and_then(|w| w.as_object().cloned()).unwrap_or_default();
        out.insert("ok".into(), json!(true));
        out.insert("running".into(), json!(v.running()));
        // And how long the window may record: it stops itself there.
        out.insert("max_seconds".into(), v.settings.get("max_seconds").cloned().unwrap_or(Value::Null));
        Value::Object(out)
    }

    /// Control.focus_row: raise the window this session is in. One
    /// WindowsTerminal.exe hosts every session, so the process tree finds
    /// the terminal and the session's title picks its window. Three routes,
    /// best first, and the answer says which was used.
    fn focus_row(&self, store: &crate::store::Store, row: &Row) -> Option<Value> {
        let sid = row.get("sessionId").and_then(Value::as_str).unwrap_or("").to_string();
        let pid_v = row.get("pid").cloned().unwrap_or(Value::Null);
        let pid = crate::pyish::py_number(row.get("pid")).map(|f| f as u32).unwrap_or(0);
        let info = crate::sessions::info_json(&sid);
        let want = [info.get("custom_title"), info.get("summary")]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .find(|t| !t.is_empty())
            .unwrap_or("")
            .to_string();
        let host = crate::winfocus::terminal_for(pid);
        let windows = host.map(|h| crate::winfocus::titled_windows(Some(h))).unwrap_or_default();
        let hwnd = if want.is_empty() { None } else { crate::winfocus::match_title(&want, &windows) };
        if let Some(hwnd) = hwnd.filter(|h| crate::winfocus::focus_hwnd(*h)) {
            let meta = json!({"window_hwnd": hwnd, "window_title": want});
            store.register(&sid, None, meta.as_object().unwrap());
            return Some(json!({"ok": true, "action": "focus", "session": sid, "pid": pid_v,
                               "how": "title", "title": want}));
        }
        // Its tab is not the one in front, so no title matches; a tab stays
        // in the window it opened in, so the window it was last seen in is
        // where it still is, just behind another tab.
        let remembered = store
            .session(&sid)
            .and_then(|r| r.get("window_hwnd").and_then(Value::as_i64))
            .filter(|h| *h != 0);
        if let Some(h) = remembered {
            if host.is_some() && crate::winfocus::owner_pid(h as isize) == host && crate::winfocus::focus_hwnd(h as isize) {
                return Some(json!({"ok": true, "action": "focus", "session": sid, "pid": pid_v,
                                   "how": "remembered", "title": want,
                                   "note": format!("its tab is not in front - look for {}",
                                                   crate::pyish::py_repr(&json!(want)))}));
            }
        }
        if crate::winfocus::focus_pid(pid) {
            return Some(json!({"ok": true, "action": "focus", "session": sid, "pid": pid_v,
                               "how": "terminal", "title": want,
                               "note": "raised its terminal; could not tell its window apart"}));
        }
        None
    }

    /// Control.focus: put a project's session in front, or open one when
    /// none is running, so a button press always does something.
    fn focus(&self, store: &crate::store::Store, project: Option<&Value>, session_id: Option<&Value>) -> Value {
        let rows = self.sessions(2.0);
        let mut row: Option<Row> = None;
        let mut path: Option<String> = None;
        let gone = |name: &Value, host: &Value| {
            json!({"ok": false, "error": format!("{} is on {} - there is no window here to raise",
                crate::pyish::py_str(name), crate::pyish::py_str(host))})
        };
        if truthy(session_id) {
            let sid = session_id.unwrap();
            row = rows.iter().find(|r| r.get("sessionId") == Some(sid)).cloned();
            if let Some(r) = &row {
                if truthy(r.get("remote")) {
                    // Its pid is a process on another machine.
                    let name = crate::pyish::or(r.get("name"), Some(sid)).unwrap().clone();
                    return gone(&name, r.get("host").unwrap_or(&Value::Null));
                }
            }
            path = row.as_ref().and_then(|r| r.get("cwd").and_then(Value::as_str).map(str::to_string));
        }
        if row.is_none() {
            if !truthy(project) {
                return json!({"ok": false, "error": "no project or session named"});
            }
            let project = project.unwrap();
            let res = crate::projects::resolve(&crate::pyish::py_str(project), &self.catalogue());
            if res["confidence"] == "none" {
                return json!({"ok": false, "error": format!("no project matched {}", crate::pyish::py_repr(project))});
            }
            if truthy(res["match"].get("host")) {
                return gone(&res["match"]["name"], &res["match"]["host"]);
            }
            let p = res["match"]["path"].as_str().unwrap_or("").to_string();
            let want = paths::abs_normcase(&p);
            let mut live: Vec<Row> = rows
                .iter()
                .filter(|r| {
                    let cwd = r.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty());
                    cwd.is_some_and(|c| paths::abs_normcase(c) == want) && truthy(r.get("pid"))
                })
                .cloned()
                .collect();
            // Whatever is asking for a human wins; otherwise the busy one.
            live.sort_by_key(|r| {
                (!truthy(r.get("attention")), r.get("status").and_then(Value::as_str) != Some("busy"))
            });
            row = live.into_iter().next();
            path = Some(p);
        }
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("pid"))) {
            if let Some(got) = self.focus_row(store, r) {
                return got;
            }
            // Running, so opening a window would start a second session on
            // the same project - worse than saying it could not be raised.
            return json!({"ok": false, "error": "could not raise its window",
                          "session": r.get("sessionId").cloned().unwrap_or(Value::Null),
                          "pid": r.get("pid").cloned().unwrap_or(Value::Null)});
        }
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            return json!({"ok": false, "error": "nothing to focus and nowhere to open"});
        };
        match crate::deeplink::launch(&path, "") {
            Err(e) => json!({"ok": false, "error": e}),
            Ok(url) => json!({"ok": true, "action": "launch", "url": url}),
        }
    }

    /// Control.run_command: type slash command `text` at the row's prompt.
    fn run_command(&self, store: &crate::store::Store, row: &Row, text: &str) -> Value {
        if let Some(why) = crate::slash::refusal(text) {
            return json!({"ok": false, "error": why});
        }
        // Status moves under us: decide on a fresh look.
        self.invalidate();
        let row = self.fresh_row(row);
        if let Some(why) = crate::slash::blocked(&row, &self.pending.waiting()) {
            return json!({"ok": false, "error": why});
        }
        if !self.type_into(&row, text) {
            return json!({"ok": false, "error": "could not type into its terminal"});
        }
        let sid = row.get("sessionId").cloned().unwrap_or(Value::Null);
        let t = crate::pyish::strip(text);
        let e = json!({"kind": "command", "session_id": sid,
                       "cwd": row.get("cwd").cloned().unwrap_or(Value::Null),
                       "summary": crate::pyish::take(t, 280)});
        store.add_event_json(e);
        self.invalidate();
        json!({"ok": true, "action": "command", "command": t, "session": sid,
               "session_name": row.get("name").cloned().unwrap_or(Value::Null)})
    }

    /// The row again, from a fresh look; the old one if it has gone.
    fn fresh_row(&self, row: &Row) -> Row {
        let id = row
            .get("key")
            .and_then(Value::as_str)
            .filter(|k| !k.is_empty())
            .or_else(|| row.get("sessionId").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();
        self.row(&id).unwrap_or_else(|| row.clone())
    }

    /// Control._type_into: one line and Enter at the row's prompt. (A
    /// herdr-core terminal is typed into through hc, which comes with
    /// herdr-core hosting.)
    fn type_into(&self, row: &Row, text: &str) -> bool {
        let hosted = row
            .get("hc")
            .and_then(|h| h.get("address"))
            .is_some_and(|a| truthy(Some(a)));
        if hosted && !truthy(row.get("remote")) {
            return false;
        }
        crate::slash::type_command(row.get("pid").unwrap_or(&Value::Null), text)
    }

    /// Control.send_typed: Send As Typed - the text typed at the session's
    /// prompt as one line, with the authority of the person at the
    /// terminal. Off until Settings turns it on; only from the window.
    fn send_typed(
        &self,
        store: &crate::store::Store,
        session_id: Option<&Value>,
        text: Option<&Value>,
    ) -> Value {
        if !truthy(Some(&settings::value(&self.config(), "chat.send_as_typed"))) {
            return json!({"ok": false, "error": "Send As Typed is off - turn it on in Settings, under Chat"});
        }
        let id = session_id
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str);
        let Some(row) = id.and_then(|i| self.row(&i)) else {
            return json!({"ok": false, "error": "no such session"});
        };
        let raw = text.and_then(Value::as_str).unwrap_or("");
        let (clean, changed) = crate::slash::clean_typed(raw);
        if let Some(why) = crate::slash::typed_refusal(&clean) {
            return json!({"ok": false, "error": why});
        }
        if crate::slash::is_command(&clean) {
            let mut got = self.run_command(store, &row, &clean);
            got["cleaned"] = json!(changed);
            return got;
        }
        let sid = row
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let name = row.get("name").cloned().unwrap_or(Value::Null);
        let mod_live = self
            .modlink
            .as_ref()
            .is_some_and(|l| l.live(&sid, Self::now()));
        if !truthy(row.get("remote")) && mod_live {
            self.deliver(store, &row, &clean, true);
            return json!({"ok": true, "action": "typed", "via": "mod", "cleaned": changed,
                          "session": sid, "session_name": name});
        }
        self.invalidate();
        let row = self.fresh_row(&row);
        if let Some(why) = crate::slash::blocked(&row, &self.pending.waiting()) {
            return json!({"ok": false, "error": why});
        }
        if !self.type_into(&row, &clean) {
            return json!({"ok": false, "error": "could not type into its terminal"});
        }
        let e = json!({"kind": "typed", "session_id": sid,
                       "cwd": row.get("cwd").cloned().unwrap_or(Value::Null),
                       "summary": crate::pyish::take(&clean, 280)});
        store.add_event_json(e);
        self.invalidate();
        json!({"ok": true, "action": "typed", "cleaned": changed, "session": sid, "session_name": name})
    }

    /// Control.dismiss_session: take a session with no process off the list.
    fn dismiss_session(&self, store: &crate::store::Store, session_id: Option<&Value>) -> Value {
        let id = session_id
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let Some(row) = (if id.is_empty() { None } else { self.row(&id) }) else {
            return json!({"ok": false, "error": "no such session"});
        };
        if truthy(row.get("remote")) {
            return json!({"ok": false, "error": format!("dismissing a session on {} is not supported yet",
                crate::pyish::py_str(row.get("host").unwrap_or(&Value::Null)))});
        }
        if truthy(row.get("pid")) {
            return json!({"ok": false, "error": "that session is still running - close it instead"});
        }
        let key = row
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let sid = row
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        store.dismiss(&key);
        store.forget(&sid);
        let e = json!({"kind": "dismiss", "cwd": row.get("cwd").cloned().unwrap_or(Value::Null),
                       "session_id": sid, "name": row.get("name").cloned().unwrap_or(Value::Null)});
        store.add_event_json(e);
        self.invalidate();
        json!({"ok": true, "how": "dismissed"})
    }

    /// Control.invalidate: the next look rebuilds the snapshot, and the
    /// window's event streams are woken to look.
    fn invalidate(&self) {
        *self.snap.lk() = None;
        *self.changes.lk() += 1;
        self.changed.notify_all();
    }

    fn generation(&self) -> u64 {
        *self.changes.lk()
    }

    /// Control.wait_change: until something invalidates after `seen`, or
    /// `timeout` seconds.
    fn wait_change(&self, seen: u64, timeout: f64) {
        let g = self.changes.lk();
        let _ = self
            .changed
            .wait_timeout_while(g, std::time::Duration::from_secs_f64(timeout), |g| *g == seen);
    }

    /// A POST, with the dispatcher's checks: no foreign Origin, a JSON body
    /// of 1..MAX_BODY bytes.
    // The request's parts, as Handler reads them off the request.
    #[allow(clippy::too_many_arguments)]
    pub fn post(
        &self,
        path: &str,
        origin: Option<&str>,
        ctype: Option<&str>,
        body: &[u8],
        port: u16,
        window: Option<&str>,
        mod_token: Option<&str>,
    ) -> (u16, Body) {
        const MAX_BODY: usize = 4 * 1024 * 1024;
        if let Some(o) = origin {
            if o != format!("http://127.0.0.1:{port}") && o != format!("http://localhost:{port}") {
                return (
                    403,
                    Body::Json(json!({"error": "cross-origin requests are refused"})),
                );
            }
        }
        if path == "/api/transcribe" {
            // Raw audio, not JSON (Handler._read_binary).
            if body.is_empty() || body.len() > MAX_BODY * 8 {
                return (413, Body::Json(json!({"error": "body out of range"})));
            }
            return (200, Body::Json(self.transcribe(body)));
        }
        let ctype = ctype.unwrap_or("").split(';').next().unwrap_or("").trim();
        if ctype != "application/json" {
            return (
                415,
                Body::Json(json!({"error": "expected application/json"})),
            );
        }
        if body.is_empty() || body.len() > MAX_BODY {
            return (
                413,
                Body::Json(json!({"error": format!("body must be 1..{MAX_BODY} bytes")})),
            );
        }
        let Ok(payload) = std::str::from_utf8(body)
            .map_err(|_| ())
            .and_then(|t| serde_json::from_str::<Value>(t).map_err(|_| ()))
        else {
            return (400, Body::Json(json!({"error": "body is not JSON"})));
        };
        let empty = Map::new();
        let object = payload.as_object();
        const API: &[&str] = &[
            "/api/queue",
            "/api/unqueue",
            "/api/worktree",
            "/api/resume",
            "/api/new",
            "/api/close",
            "/api/dismiss",
            "/api/reviewed",
            "/api/prefs",
            "/api/notify",
            "/api/settings",
            "/api/remote/refresh",
        ];
        if API.contains(&path) && object.is_none() {
            return (400, Body::Json(json!({"error": "expected an object"})));
        }
        let p = object.unwrap_or(&empty);
        let from_window = origin.is_some()
            && window.is_some_and(|w| w.as_bytes() == self.window_token.as_bytes());
        if path == "/api/mod/beat" || path == "/api/mod/check" {
            if object.is_none() {
                return (400, Body::Json(json!({"error": "expected an object"})));
            }
            let Some(link) = self.modlink.as_ref().filter(|l| l.authentic(mod_token)) else {
                return (
                    403,
                    Body::Json(json!({"ok": false, "error": "not the Imperatorium mod"})),
                );
            };
            let out = if path == "/api/mod/beat" {
                self.mod_beat(link, p)
            } else {
                self.mod_check(p)
            };
            return (200, Body::Json(out));
        }
        match path {
            "/permission/park" => {
                // The hook blocks here until a human answers or the wait
                // runs out; running out is a silent "ask".
                let wait = self.park_wait(p);
                let (verdict, who, answers) = self.pending.park(p, wait);
                let mut reply = json!({"verdict": verdict, "decided_by": who});
                if truthy(Some(&answers)) {
                    reply["answers"] = answers;
                }
                return (200, Body::Json(reply));
            }
            "/api/decide" => {
                let who = p
                    .get("who")
                    .filter(|v| truthy(Some(v)))
                    .cloned()
                    .unwrap_or(json!("window"));
                let ok = self.pending.decide(
                    p.get("id").unwrap_or(&Value::Null),
                    p.get("verdict").unwrap_or(&Value::Null),
                    who,
                    p.get("answers"),
                );
                return (if ok { 200 } else { 409 }, Body::Json(json!({"ok": ok})));
            }
            _ => {}
        }
        match path {
            "/hook" => (200, Body::Json(self.on_hook(p, None))),
            "/api/update/check" => (200, Body::Json(self.updates.check(&self.config()))),
            "/api/update/apply" => {
                if !from_window {
                    return (403, Body::Json(json!({"ok": false, "error": "only the Imperatorium window installs an update"})));
                }
                if self.store.is_none() {
                    return (200, Body::Json(json!({"ok": false, "error": "a read-only shadow changes nothing"})));
                }
                let log = self.state_dir().join("update.log");
                (200, Body::Json(self.updates.apply(&self.config(), &log)))
            }
            "/api/remote/refresh" => {
                let n = self.fleet.as_ref().map(|f| f.refresh()).unwrap_or(0);
                (200, Body::Json(json!({"ok": true, "hosts": n})))
            }
            "/api/dispatch" => {
                let Some(store) = &self.store else {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                };
                if object.is_none() {
                    return (400, Body::Json(json!({"error": "expected an object"})));
                }
                (200, Body::Json(self.dispatch(store, p, from_window)))
            }
            "/api/voice/warm" => (200, Body::Json(self.warm_voice())),
            "/api/kill_process" => {
                // Ends one process a session's shell started: only from the
                // window, and only one that is beneath that session.
                if !from_window {
                    return (403, Body::Json(json!({"ok": false, "error": "only the Imperatorium window"})));
                }
                let sid = p.get("session_id").and_then(Value::as_str).unwrap_or("");
                let want = p.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32;
                let Some(root) = self.row(sid).filter(|r| !truthy(r.get("remote")))
                    .and_then(|r| r.get("pid").and_then(Value::as_u64)) else {
                    return (200, Body::Json(json!({"ok": false, "error": "no such local session"})));
                };
                let under = crate::procs::details(root as u32)
                    .iter()
                    .any(|d| d.get("pid").and_then(Value::as_u64) == Some(want as u64));
                if want == 0 || !under {
                    return (200, Body::Json(json!({"ok": false, "error": "that process is not one this session started"})));
                }
                (200, Body::Json(match crate::procs::kill(want) {
                    Ok(()) => json!({"ok": true}),
                    Err(e) => json!({"ok": false, "error": e}),
                }))
            }
            "/api/dictate" => {
                if !from_window {
                    return (403, Body::Json(json!({"ok": false, "error": "dictation opens the microphone: only the Imperatorium window and its herdr mods"})));
                }
                // The mods' own microphone (voice.Recorder) is herdr-only.
                (200, Body::Json(json!({"ok": false, "error": "dictation from the herdr mods is not ported yet"})))
            }
            "/api/open_window" => {
                let (route, size) = match p.get("page").and_then(Value::as_str) {
                    Some("matrix") => ("/matrix", "1100,720"),
                    Some("window") => ("/", "1280,820"),
                    _ => return (400, Body::Json(json!({"ok": false, "error": "page is one of matrix, window"}))),
                };
                if !from_window {
                    return (403, Body::Json(json!({"ok": false, "error": "only the Imperatorium window and its herdr mods"})));
                }
                let url = format!("http://127.0.0.1:{port}{route}");
                (200, Body::Json(match crate::voice::open_window(&url, size) {
                    Err(e) => json!({"ok": false, "error": e}),
                    Ok(used) => json!({"ok": true, "url": url, "browser": used.map(|b| {
                        b.rsplit(['/', '\\']).next().unwrap_or("").to_string()
                    })}),
                }))
            }
            "/api/focus" => {
                if object.is_none() {
                    return (400, Body::Json(json!({"error": "expected an object"})));
                }
                let Some(store) = &self.store else {
                    return (200, Body::Json(json!({"ok": false, "error": "a read-only shadow changes nothing"})));
                };
                (200, Body::Json(self.focus(store, p.get("project"), p.get("session_id"))))
            }
            "/api/typed" => {
                if object.is_none() {
                    return (400, Body::Json(json!({"error": "expected an object"})));
                }
                if !from_window {
                    return (
                        403,
                        Body::Json(json!({"ok": false,
                        "error": "Send As Typed is only available from the Imperatorium window"})),
                    );
                }
                let Some(store) = &self.store else {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                };
                (
                    200,
                    Body::Json(self.send_typed(store, p.get("session_id"), p.get("prompt"))),
                )
            }
            "/api/resume" | "/api/new" | "/api/close" => {
                let Some(store) = &self.store else {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                };
                let text = |k: &str| {
                    p.get(k)
                        .filter(|v| truthy(Some(v)))
                        .map(crate::pyish::py_str)
                };
                let sid = text("session_id");
                let got = match path {
                    "/api/resume" => self.resume(
                        store,
                        sid.as_deref().unwrap_or(""),
                        text("where").as_deref().unwrap_or("window"),
                        truthy(p.get("fork")),
                        text("prompt").as_deref(),
                        text("cwd"),
                    ),
                    "/api/new" => {
                        self.new_session(store, sid.as_deref(), text("cwd"), truthy(p.get("close")))
                    }
                    _ => self.close_session(store, sid.as_deref()),
                };
                (200, Body::Json(got))
            }
            "/api/worktree" => {
                let Some(store) = &self.store else {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                };
                let project = p
                    .get("project")
                    .filter(|v| !v.is_null())
                    .map(crate::pyish::py_str);
                // Only a directory the catalogue or a live session vouches
                // for, never a path taken on trust from the request.
                let Some(project) = project.filter(|pr| self.known_project(pr)) else {
                    let shown = p
                        .get("project")
                        .map(crate::pyish::py_str)
                        .unwrap_or_else(|| "None".into());
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": format!("not a known project: {shown}")}),
                        ),
                    );
                };
                let path = p.get("path").map(crate::pyish::py_str).unwrap_or_default();
                let action = p
                    .get("action")
                    .map(crate::pyish::py_str)
                    .unwrap_or_else(|| "None".into());
                let got = crate::changes::finish(&project, &path, &action, truthy(p.get("force")));
                if got["ok"] == json!(true) {
                    let summary = format!("{} {}", action, crate::pyish::py_str(&got["branch"]));
                    let e = json!({"kind": format!("worktree_{action}"), "cwd": project, "summary": summary});
                    store.add_event_json(e);
                }
                (200, Body::Json(got))
            }
            "/api/settings" => {
                if self.store.is_none() {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                }
                // Turning Send As Typed on lets a message speak as you, so
                // only the Settings window may do it.
                let ch = p.get("changes");
                if ch
                    .and_then(|c| c.get("chat.send_as_typed"))
                    .is_some_and(|v| truthy(Some(v)))
                    && !from_window
                {
                    return (
                        200,
                        Body::Json(json!({"ok": false,
                        "errors": {"chat.send_as_typed": "can only be turned on from the Settings window"},
                        "error": "some values were not accepted"})),
                    );
                }
                (200, Body::Json(self.save_settings(ch)))
            }
            "/api/prefs" | "/api/notify" | "/api/unqueue" | "/api/dismiss" | "/api/reviewed" | "/api/queue" => {
                let Some(store) = &self.store else {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                };
                (200, Body::Json(self.post_api(store, path, p, from_window)))
            }
            _ => (404, Body::Json(json!({"error": "no such path"}))),
        }
    }

    /// convcache._stamp: stat the remembered path; look again (and
    /// remember) only when there is none or it has gone.
    fn stamp(&self, sid: &str) -> Option<(PathBuf, u64, u128)> {
        let known = self.paths.lk().get(sid).cloned();
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
            .lk()
            .insert(sid.to_string(), found.0.clone());
        Some(found)
    }

    /// convcache: the per-message items and the merged conversation,
    /// parsed again only when the transcript's size or mtime moves.
    fn conversation(&self, sid: &str) -> Result<Arc<Cached>, String> {
        let stamp = self.stamp(sid);
        if let Some(st) = &stamp {
            let mut convs = self.convs.lk();
            if let Some(i) = convs.get_index_of(sid) {
                if &convs[i].stamp == st {
                    // Used now: to the back of the eviction order.
                    let last = convs.len() - 1;
                    convs.move_index(i, last);
                    return Ok(Arc::clone(&convs[last]));
                }
            }
        }
        let raw = Self::read_items(sid)?;
        let merged = conversation::merge_turns_ref(&raw);
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
            let mut convs = self.convs.lk();
            convs.shift_remove(sid);
            while convs.len() >= CONVS_KEPT {
                convs.shift_remove_index(0); // the least recently used
            }
            convs.insert(sid.to_string(), Arc::clone(&entry));
        }
        Ok(entry)
    }

    /// A session's per-message items, straight from its transcript.
    fn read_items(sid: &str) -> Result<Vec<Value>, String> {
        let chain = match sessions::chain_json(sid)? {
            Value::Array(a) => a,
            _ => vec![],
        };
        Ok(conversation::to_items(&conversation::messages(&chain), 0, false))
    }

    /// The merged conversation's plain text, for a History search: from the
    /// cache when it holds this transcript as it is, else read - and not
    /// remembered, so a search over hundreds of sessions does not push out
    /// the ones the window is showing. None when it cannot be read.
    fn search_text(&self, sid: &str) -> Option<String> {
        let stamp = self.stamp(sid);
        if let Some(st) = &stamp {
            let hit = self.convs.lk().get(sid).filter(|c| &c.stamp == st).cloned();
            if let Some(c) = hit {
                return Some(history::plain_text(&c.merged));
            }
        }
        let raw = Self::read_items(sid).ok()?;
        Some(history::plain_text(&conversation::merge_turns(raw)))
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
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("remote"))) {
            // Read on the session's own machine (Handler: a remote body is
            // hashed here, and `blocks` cut from it as from a local one).
            let mut body = self.remote_conversation(r, limit);
            let items = body.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
            let sig = signature(&items);
            if q.get("known").map(String::as_str) == Some(sig.as_str()) {
                return (
                    200,
                    Body::Json(json!({"session": body.get("session").cloned().unwrap_or(Value::Null),
                                      "unchanged": true, "sig": sig})),
                );
            }
            if blocks > 0 {
                let (tail, start, first) = conversation::tail_blocks(&items, blocks);
                body.insert("total".into(), json!(items.len()));
                body.insert("items".into(), Value::Array(tail));
                body.insert("start".into(), json!(start));
                body.insert("first_block".into(), json!(first));
            }
            body.insert("sig".into(), json!(sig));
            return (200, Body::Json(Value::Object(body)));
        }
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
                let items = conversation::merge_turns_ref(&c.raw[start..]);
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

    /// Control.hosts_status: GET /api/hosts.
    fn hosts_status(&self) -> Value {
        let Some(fleet) = &self.fleet else {
            return json!({"hosts": [], "status": {}});
        };
        let tunnels = self.tunnels.lk();
        let hosts: Vec<Value> = fleet
            .hosts
            .iter()
            .map(|h| {
                let mut e = h.public();
                if let Some(t) = tunnels.get(&h.name) {
                    e.insert("tunnel_status".into(), t.status());
                }
                Value::Object(e)
            })
            .collect();
        json!({"hosts": hosts, "status": fleet.status()})
    }

    /// Control._remote_conversation: the same items, read on the session's
    /// own machine by the host's channel.
    fn remote_conversation(&self, row: &Row, limit: usize) -> Map<String, Value> {
        let f = |k: &str| row.get(k).cloned().unwrap_or(Value::Null);
        let mut out = json!({"session": f("sessionId"), "items": [], "remote": true,
                             "host": f("host"), "name": f("name"), "cwd": f("cwd")})
        .as_object()
        .unwrap()
        .clone();
        let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
        let Some(fleet) = self.fleet.as_ref().filter(|f| f.host(&host).is_some()) else {
            out.insert("error".into(), json!(format!("host {host} is not configured")));
            return out;
        };
        let sid = row.get("sessionId").map(crate::pyish::py_str).unwrap_or_default();
        let cwd = row.get("cwd").and_then(Value::as_str);
        match fleet.read(&host, &sid, limit, cwd) {
            Ok((items, title)) => {
                out.insert("items".into(), Value::Array(items));
                out.insert("title".into(), json!(title));
            }
            Err(e) => {
                out.insert("error".into(), json!(format!("reading on {host}: {e}")));
            }
        }
        out
    }

    // -- the session snapshot -------------------------------------------------

    fn now() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }

    /// Control.sessions(max_age): the rows, cached briefly.
    pub fn sessions(&self, max_age: f64) -> Arc<Vec<Row>> {
        self.snapshot(max_age).1
    }

    /// (when the snapshot was taken, its rows). As registry.snapshot,
    /// `at` is read before `claude agents` runs.
    pub fn snapshot(&self, max_age: f64) -> (f64, Arc<Vec<Row>>) {
        let s = self.snap_of(max_age);
        (s.at, Arc::clone(&s.rows))
    }

    /// The cached snapshot if it is younger than `max_age` and no
    /// invalidate() came since it was begun; else one built now. Builds are
    /// single-flight: whoever waited on another's build takes its rows.
    fn snap_of(&self, max_age: f64) -> Arc<Snap> {
        let fresh = |max_age: f64| {
            let s = self.snap.lk().clone()?;
            (Self::now() - s.at < max_age && s.generation == self.generation()).then_some(s)
        };
        if let Some(s) = fresh(max_age) {
            return s;
        }
        let _building = self.snap_build.lk();
        if let Some(s) = fresh(max_age) {
            return s;
        }
        let generation = self.generation();
        let now = Self::now();
        let rows = Arc::new(self.build_snapshot());
        let s = Arc::new(Snap {
            at: now,
            generation,
            rows,
            public: std::sync::OnceLock::new(),
        });
        *self.snap.lk() = Some(Arc::clone(&s));
        s
    }

    /// The snapshot's public JSON, serialised once per snapshot.
    fn public_snap(&self, max_age: f64) -> Arc<PublicSnap> {
        let s = self.snap_of(max_age);
        Arc::clone(s.public.get_or_init(|| {
            let mut body = json!({"at": s.at, "sessions": public(&s.rows)});
            let data = body.to_string();
            if let Some(o) = body.as_object_mut() {
                o.shift_remove("at");
            }
            Arc::new(PublicSnap {
                data,
                same: json_dumps_sorted(&body),
            })
        }))
    }

    fn build_snapshot(&self) -> Vec<Row> {
        let st = self.state();
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
            if overlay(row, extra) {
                if let Some(store) = &self.store {
                    store.clear_attention(&sid);
                }
            }
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
        rows.extend(self.remote_with_hooks(known, &creds));
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
        let hours = get(&self.config(), "list.hide_finished_hours")
            .filter(|v| v.is_number())
            .and_then(Value::as_f64)
            .filter(|h| *h > 0.0)
            .unwrap_or(0.0);
        if hours > 0.0 {
            let cutoff = Self::now() - hours * 3600.0;
            rows.retain(|r| !(registry::finished(r) && finished_at(r) < cutoff));
        }
        for r in rows.iter_mut() {
            registry::classify(r);
        }
        if let Some(store) = &self.store {
            registry::apply_reviewed(&mut rows, &store.reviewed());
        }
        rows
    }

    /// Control._remote_with_hooks: remote rows, with what that host's hooks
    /// reported folded in - kept under host-qualified keys, so nothing a
    /// host posts can overwrite, forget or re-point a session here.
    fn remote_with_hooks(
        &self,
        known: &Map<String, Value>,
        creds: &HashMap<String, (Value, Value)>,
    ) -> Vec<Row> {
        let Some(fleet) = &self.fleet else {
            return vec![];
        };
        let mut out = Vec::new();
        for mut row in fleet.rows() {
            let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
            let sid = row.get("sessionId").map(crate::pyish::py_str).unwrap_or_default();
            let key = remote_key(&host, &sid);
            let extra = known
                .get(&key)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if !truthy(row.get("socket")) && truthy(extra.get("socket")) {
                row.insert("socket".into(), extra["socket"].clone());
            }
            // registry.attach_credentials, for this one row.
            if let Some((token, recorded)) = creds.get(&key) {
                if truthy(Some(token)) {
                    let live = row.get("socket").cloned().unwrap_or(Value::Null);
                    if truthy(Some(recorded)) && truthy(Some(&live)) && *recorded == live {
                        row.insert("token".into(), token.clone());
                    } else {
                        row.insert("stale_credential".into(), json!(true));
                    }
                }
            }
            let d = truthy(row.get("socket")) && truthy(row.get("token"));
            row.insert("dispatchable".into(), json!(d));
            if !extra.is_empty() && overlay(&mut row, &extra) {
                if let Some(store) = &self.store {
                    store.clear_attention(&key);
                }
            }
            out.push(row);
        }
        out
    }

    /// Control._decorate, less what only the Python dispatcher knows.
    fn decorate(&self, row: &mut Row, queue: &Map<String, Value>) {
        let sid = row
            .get("sessionId")
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let remote = truthy(row.get("remote"));
        let key = if remote {
            remote_key(&row.get("host").map(crate::pyish::py_str).unwrap_or_default(), &sid)
        } else {
            sid.clone()
        };
        row.insert("key".into(), json!(key));
        let state = row.get("state").and_then(Value::as_str).map(str::to_string);
        let ended = matches!(
            state.as_deref(),
            Some("done") | Some("failed") | Some("stopped")
        );
        let current = self.live.current(&key, Self::now());
        if let Value::Object(c) = &current {
            let summary = activity::first_line(c.get("summary"));
            let tool = c
                .get("tool")
                .map(crate::pyish::py_str)
                .unwrap_or_else(|| "None".into());
            row.insert("doing".into(), json!(format!("{tool} {summary}")));
            row.insert(
                "doing_since".into(),
                c.get("since").cloned().unwrap_or(Value::Null),
            );
        } else if !ended && !remote {
            let line = self.doing_line(&sid);
            row.insert("doing".into(), json!(line));
        }
        if let Some(got) = self.usage.session(&key) {
            let mut u = Map::new();
            for k in [
                "context_pct",
                "context_size",
                "cost_usd",
                "model",
                "at",
                "duration_ms",
                "api_duration_ms",
                "lines_added",
                "lines_removed",
            ] {
                u.insert(k.into(), got.get(k).cloned().unwrap_or(Value::Null));
            }
            row.insert("usage".into(), Value::Object(u));
        }
        if !truthy(row.get("remote")) {
            if let Some(cwd) = row.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty()) {
                if let Some(pr) = self.prs.get(cwd).filter(|p| truthy(Some(p))) {
                    row.insert("pr".into(), pr);
                }
            }
        }
        if let Some(Value::Array(q)) = queue.get(&key) {
            if !q.is_empty() {
                row.insert("queued".into(), json!(q.len()));
            }
        }
        if !truthy(row.get("remote")) {
            if let Some(link) = &self.modlink {
                if let Some(m) = link.state(&sid, Self::now()) {
                    // The mod reaches the session without its socket, and
                    // knows the moment a turn starts or ends.
                    let pick = |k: &str| m.get(k).cloned().unwrap_or(Value::Null);
                    row.insert("mod".into(), json!({"busy": pick("busy"), "since": pick("since"),
                                                    "version": pick("version"), "mod_version": pick("mod_version")}));
                    row.insert("dispatchable".into(), json!(true));
                    let status = match row.get("status") {
                        None | Some(Value::Null) => Some(""),
                        Some(v) => v.as_str(),
                    };
                    if matches!(status, Some("") | Some("idle") | Some("busy"))
                        && !truthy(row.get("attention"))
                    {
                        let busy = truthy(m.get("busy"));
                        row.insert("status".into(), json!(if busy { "busy" } else { "idle" }));
                    }
                }
                let n = link.pending(&sid).len();
                if n > 0 {
                    row.insert("mod_pending".into(), json!(n));
                }
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
        // A remote row's numbers were read on its host by the poll that
        // found it (remote.Stats).
        let stat = if remote {
            Some(row.remove("stat").unwrap_or_else(|| json!({})))
        } else {
            None
        };
        let work = self.work(row, stat);
        row.insert("work".into(), work);
    }

    /// Control._doing_line: activity.doing over the last 8 messages,
    /// remembered for DOING_EVERY (10 s).
    fn doing_line(&self, sid: &str) -> String {
        let now = Self::now();
        let hit = self.doing.lk().get(sid).cloned();
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
            .lk()
            .insert(sid.to_string(), (now, line.clone()));
        line
    }

    /// Control._work: what the tile shows at a glance.
    fn work(&self, row: &Row, stat: Option<Value>) -> Value {
        let sid = row
            .get("sessionId")
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let stat = stat.unwrap_or_else(|| {
            self.stats
                .read(&sid, Self::now())
                .unwrap_or_else(|| json!({}))
        });
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
            get(&self.config(), "chat.cold_cache_tokens")
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
    fn api_sessions(&self) -> String {
        // The Stream Deck polls once a second; a snapshot STREAM_EVERY old
        // is as good for a key colour and saves a fork.
        self.public_snap(STREAM_EVERY).data.clone()
    }

    /// GET /api/stream: server-sent events, the session list whenever it
    /// changes - looked at every STREAM_EVERY seconds, never more often
    /// than STREAM_MIN_GAP, a keepalive after STREAM_KEEPALIVE of quiet.
    /// HTTP/1.0 and no length, as the Python handler answers.
    ///
    /// Unlike Python it never ends the stream itself: it runs until a write
    /// fails, which the keepalive makes happen within STREAM_KEEPALIVE of
    /// the page going away. Python closes after 600 s and the page
    /// reconnects, but tiny_http cannot close a keep-alive connection:
    /// dropping the writer leaves the socket open for a next request, so
    /// the browser would wait on a stream that never ends and never says
    /// anything again - the window froze ten minutes after each load
    /// (2026-11-13).
    ///
    /// As in Python, it is also woken early by whatever invalidates the
    /// snapshot (a hook event, a remote poll landing), then looks once
    /// STREAM_MIN_GAP has passed. Every stream sends the same snapshot's
    /// JSON, serialised once.
    ///
    /// `/api/stream?matrix=1` is the Chat Matrix's: not the session list but
    /// `event: matrix`, the whole of /api/matrix, sent once on connecting and
    /// again whenever it changes (see `stream_matrix`). Python answers it
    /// with the session list, which the matrix page does not count as a push
    /// and so keeps polling.
    pub fn stream(&self, out: &mut dyn std::io::Write, matrix: bool) {
        let head = "HTTP/1.0 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\n\
                    Cache-Control: no-store\r\n\r\n";
        if out
            .write_all(head.as_bytes())
            .and_then(|_| out.flush())
            .is_err()
        {
            return;
        }
        if matrix {
            return self.stream_matrix(out);
        }
        let (mut quiet, mut looked) = (Self::now(), 0.0);
        let mut last: Option<Arc<PublicSnap>> = None;
        loop {
            let gap = STREAM_MIN_GAP - (Self::now() - looked);
            if gap > 0.0 {
                std::thread::sleep(std::time::Duration::from_secs_f64(gap));
            }
            looked = Self::now();
            let seen = self.generation();
            let snap = self.public_snap(STREAM_EVERY);
            let moved = last
                .as_ref()
                .is_none_or(|l| !Arc::ptr_eq(l, &snap) && l.same != snap.same);
            let sent = if moved {
                let r = write!(out, "event: sessions\ndata: {}\n\n", snap.data)
                    .and_then(|_| out.flush());
                last = Some(snap);
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
            self.wait_change(seen, STREAM_EVERY);
        }
    }

    /// The matrix's stream: what its page would have polled for every
    /// MATRIX_EVERY seconds, sent only when it differs from the last send
    /// (`now` aside, which the page does not read). The same work as the
    /// poll, minus the requests, the parsing and the redraws when nothing
    /// has moved.
    fn stream_matrix(&self, out: &mut dyn std::io::Write) {
        const MATRIX_EVERY: f64 = 1.5;
        let (mut last, mut quiet) = (String::new(), Self::now());
        loop {
            let mut body = self.api_matrix();
            if let Some(o) = body.as_object_mut() {
                o.remove("now");
            }
            let same = json_dumps_sorted(&body);
            let sent = if same != last {
                last = same;
                quiet = Self::now();
                write!(out, "event: matrix\ndata: {body}\n\n").and_then(|_| out.flush())
            } else if Self::now() - quiet >= STREAM_KEEPALIVE {
                quiet = Self::now();
                out.write_all(b": keepalive\n\n").and_then(|_| out.flush())
            } else {
                Ok(())
            };
            if sent.is_err() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs_f64(MATRIX_EVERY));
        }
    }

    /// Control.search: History, with which of its sessions are live.
    fn api_history(&self, q: &HashMap<String, String>) -> Value {
        let query = q.get("q").cloned().unwrap_or_default();
        let read = |sid: &str| self.search_text(sid);
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
        let st = self.state();
        let prefs = st
            .get("prefs")
            .and_then(|p| p.get("notify"))
            .cloned()
            .unwrap_or(json!({}));
        settings::notify_channels(&self.config())
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
        out.insert("last_error".into(), json!(self.notifier.last_error()));
        out.insert("sent".into(), json!(self.notifier.sent()));
        Value::Object(out)
    }

    /// Control.catalogue, local part: the roots from config.json (else the
    /// folder the checkout sits in) and the folders sessions run in.
    fn catalogue(&self) -> Vec<Value> {
        let configured: Vec<String> = self
            .config()
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
        let rows = self.sessions(2.0);
        let local: Vec<String> = rows
            .iter()
            .filter(|r| !truthy(r.get("remote")))
            .filter_map(|r| {
                r.get("cwd")
                    .and_then(Value::as_str)
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
            })
            .collect();
        let mut found = projects::discover(&roots, &local);
        // A remote project is whatever a remote session has worked in,
        // host-qualified: the same name exists on more than one machine,
        // and firing at the wrong one is the wrong-repository mistake.
        let mut seen = std::collections::HashSet::new();
        for r in rows.iter() {
            let (Some(host), Some(cwd)) = (
                r.get("host").filter(|_| truthy(r.get("remote"))).map(crate::pyish::py_str),
                r.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty()),
            ) else {
                continue;
            };
            if !seen.insert((host.clone(), cwd.to_string())) {
                continue;
            }
            let trimmed = cwd.trim_end_matches('/');
            let leaf = trimmed.rsplit('/').next().filter(|l| !l.is_empty()).unwrap_or(cwd);
            found.push(json!({"name": format!("{host}/{leaf}"), "path": cwd,
                              "source": "remote", "host": host}));
        }
        found
    }

    /// Control._row(id): the row whose sessionId or key is `id`.
    fn row(&self, id: &str) -> Option<Row> {
        let hit = |r: &Row| {
            r.get("sessionId").and_then(Value::as_str) == Some(id)
                || r.get("key").and_then(Value::as_str) == Some(id)
        };
        self.sessions(2.0).iter().find(|r| hit(r)).cloned()
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
        let key = row
            .as_ref()
            .and_then(|r| r.get("key").and_then(Value::as_str))
            .unwrap_or(session)
            .to_string();
        let events = self.live.events(&key);
        let (calls, source) = if !events.is_empty() {
            (events, "hooks")
        } else if row.as_ref().is_some_and(|r| truthy(r.get("remote"))) {
            return json!({"ok": true, "source": "none", "timeline": [],
                          "tally": activity::tally(&[]),
                          "note": "remote sessions report activity only through tool hooks"});
        } else {
            let items = match self.conversation(&sid) {
                Ok(c) => {
                    let start = c.raw.len().saturating_sub(200);
                    conversation::merge_turns(c.raw[start..].to_vec())
                }
                Err(e) => return json!({"ok": false, "error": format!("RuntimeError: {e}")}),
            };
            (activity::timeline(&items), "conversation")
        };
        let start = calls.len().saturating_sub(200);
        json!({"ok": true, "source": source, "timeline": calls[start..].to_vec(),
               "tally": activity::tally(&calls), "current": self.live.current(&key, Self::now())})
    }

    /// Control.session_status: what the transcript and process tree add to
    /// the row - background work, prompt cache, tokens, spend.
    fn api_session_status(&self, session: &str) -> Value {
        let hit = |rows: &[Row]| {
            rows.iter()
                .find(|r| {
                    r.get("sessionId").and_then(Value::as_str) == Some(session)
                        || r.get("key").and_then(Value::as_str) == Some(session)
                })
                .cloned()
        };
        let Some(row) = hit(&self.sessions(15.0)).or_else(|| hit(&self.sessions(2.0))) else {
            return json!({"ok": false, "error": "no such session"});
        };
        if truthy(row.get("remote")) {
            return json!({"ok": true, "now": Self::now(),
                          "note": "remote sessions report only what their tile shows"});
        }
        let now = Self::now();
        let sid = row
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut stat = self.stats.read(&sid, now);
        if let Some(Value::Object(st)) = stat.as_mut() {
            st.shift_remove("known_tasks");
        }
        let (per, _) = self
            .cost_history
            .lk()
            .sessions(std::slice::from_ref(&sid), 1.0);
        let spend = per.get(&sid).cloned().unwrap_or(Value::Null);
        let pid = row
            .get("pid")
            .filter(|p| p.is_u64() || p.is_i64())
            .and_then(Value::as_u64);
        let alive = pid.is_some() && truthy(row.get("status"));
        if let (Some(Value::Object(st)), false) = (stat.as_mut(), alive) {
            if !st.is_empty() {
                for name in ["agents", "shells", "monitors", "wakeups"] {
                    st.insert(name.into(), json!([]));
                }
            }
        }
        let mut out = Map::new();
        out.insert("ok".into(), json!(true));
        out.insert("now".into(), json!(now));
        out.insert("stat".into(), stat.unwrap_or_else(|| json!({})));
        out.insert("spend".into(), spend);
        out.insert("alive".into(), json!(alive));
        if let (true, Some(pid)) = (alive, pid) {
            let n = procs::waiting_shells(&[pid as u32], &procs::process_table(), now)
                .get(&(pid as u32))
                .copied()
                .unwrap_or(0);
            out.insert("shell_processes".into(), json!(n));
        }
        Value::Object(out)
    }

    /// Control.awaited: the end of what each shell and monitor the session
    /// is waiting on has printed, and what each subagent has said and done.
    /// Empty once the session's process has gone.
    fn api_awaited(&self, session: &str) -> Value {
        let hit = |rows: &[Row]| {
            rows.iter()
                .find(|r| {
                    r.get("sessionId").and_then(Value::as_str) == Some(session)
                        || r.get("key").and_then(Value::as_str) == Some(session)
                })
                .cloned()
        };
        let Some(row) = hit(&self.sessions(15.0)).or_else(|| hit(&self.sessions(2.0))) else {
            return json!({"ok": false, "error": "no such session"});
        };
        let now = Self::now();
        let alive =
            row.get("pid").is_some_and(|p| p.is_u64() || p.is_i64()) && truthy(row.get("status"));
        if truthy(row.get("remote")) && alive {
            // Read on the session's own machine.
            let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
            let sid = row.get("sessionId").map(crate::pyish::py_str).unwrap_or_default();
            return match self.fleet.as_ref().map(|f| f.awaited(&host, &sid)) {
                Some(Ok(tasks)) => json!({"ok": true, "now": now, "tasks": tasks}),
                Some(Err(e)) => json!({"ok": false, "error": format!("reading on {host}: {e}")}),
                None => json!({"ok": true, "now": now, "tasks": []}),
            };
        }
        if truthy(row.get("remote")) || !alive {
            return json!({"ok": true, "now": now, "tasks": []});
        }
        let sid = row
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let stat = self.stats.read(&sid, now).unwrap_or_else(|| json!({}));
        let folder = crate::taskout::tasks_dir(&sid);
        let ran_agents = stat
            .get("finished")
            .and_then(Value::as_array)
            .is_some_and(|f| f.iter().any(|x| x.get("kind").and_then(Value::as_str) == Some("agents")));
        let agents = if truthy(stat.get("agents")) || ran_agents {
            crate::taskout::subagents_dir(sessionstat::transcript_path(&sid).as_deref())
        } else {
            None
        };
        let tasks = crate::taskout::awaited(&stat, folder.as_deref(), agents.as_deref());
        json!({"ok": true, "now": now, "tasks": tasks})
    }

    /// A subagent of `session` as a chat (taskout.subagent_view), read on the
    /// session's own machine.
    fn api_subagent(&self, session: &str, agent: &str) -> Value {
        let Some(row) = self.row(session) else {
            return json!({"ok": false, "error": "no such session"});
        };
        let sid = row.get("sessionId").map(crate::pyish::py_str).unwrap_or_default();
        if truthy(row.get("remote")) {
            let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
            return match self.fleet.as_ref().map(|f| f.subagent(&host, &sid, agent)) {
                Some(Ok(v)) => v,
                Some(Err(e)) => json!({"ok": false, "error": format!("reading on {host}: {e}")}),
                None => json!({"ok": false, "error": format!("host {host} is not configured")}),
            };
        }
        crate::taskout::subagent_view(&sid, agent, Self::now())
    }

    /// The session timeline (timeline.of_session), read on the session's own
    /// machine.
    fn api_timeline(&self, session: &str, known: &str) -> Body {
        let Some(row) = self.row(session) else {
            return Body::Json(json!({"ok": false, "error": "no such session"}));
        };
        let sid = row.get("sessionId").map(crate::pyish::py_str).unwrap_or_default();
        if truthy(row.get("remote")) {
            let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
            return Body::Json(match self.fleet.as_ref().map(|f| f.timeline(&host, &sid, known)) {
                Some(Ok(v)) => v,
                Some(Err(e)) => json!({"ok": false, "error": format!("reading on {host}: {e}")}),
                None => json!({"ok": false, "error": format!("host {host} is not configured")}),
            });
        }
        // Serialised from the timeline's cache as it stands, not a copy of it.
        let v = crate::timeline::of_session_shared(&sid, known);
        Body::Raw(serde_json::to_string(&*v).unwrap_or_else(|_| "{}".into()))
    }

    /// The processes beneath a local session's own, with their CPU and memory.
    fn api_processes(&self, session: &str) -> Value {
        let Some(row) = self.row(session) else {
            return json!({"ok": false, "error": "no such session"});
        };
        let Some(pid) = row.get("pid").and_then(Value::as_u64).filter(|_| !truthy(row.get("remote"))) else {
            return json!({"ok": true, "processes": []});
        };
        json!({"ok": true, "pid": pid, "processes": crate::procs::details(pid as u32)})
    }

    /// Control.changes: git status and diff in the session's folder.
    fn api_changes(&self, session: &str) -> Value {
        let Some(row) = self.row(session) else {
            return json!({"ok": false, "error": "no such session"});
        };
        if truthy(row.get("remote")) {
            let name = row.get("name").filter(|n| truthy(Some(n))).map(crate::pyish::py_str)
                .unwrap_or_else(|| session.to_string());
            let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
            return json!({"ok": false, "error":
                format!("{name} is on {host} - changes are only read on this machine so far")});
        }
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
            .filter(|p| !truthy(p.get("host")))
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

    fn page(&self, page: &ui::Page, token: &str, with_look: bool) -> Body {
        let drawn = if with_look {
            Some(look(&self.config()))
        } else {
            None
        };
        Body::Text(
            ui::with_meta(token, &page.linked(), &self.boot, drawn.as_ref()),
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
            if let Some(name) = path.strip_prefix("/ui/") {
                // The window's styles and scripts. A request carrying the
                // current ?v= can be kept for good; any other still
                // revalidates by ETag.
                return match (ui::asset(name), ui::version(name)) {
                    (Some((ctype, body)), Some(v)) => {
                        let cache = if arg("v") == v {
                            "public, max-age=31536000, immutable"
                        } else {
                            "no-cache"
                        };
                        (200, Body::Static(body, ctype, cache, v))
                    }
                    _ => (404, Body::Json(json!({"error": "no such file"}))),
                };
            }
            if let Some(name) = path.strip_prefix("/fonts/") {
                return match ui::font(name) {
                    Some(bytes) => (200, Body::Bytes(bytes, "font/woff2", "max-age=86400")),
                    None => (404, Body::Json(json!({"error": "no such font"}))),
                };
            }
        }
        match (method, path) {
            ("GET", "/") => (200, self.page(&ui::SHELL, &self.window_token, true)),
            ("GET", "/matrix") => (200, self.page(&ui::MATRIX, "", true)),
            ("GET", "/settings") => (200, self.page(&ui::SETTINGS, &self.window_token, false)),
            ("GET", "/api/history") => (200, Body::Json(self.api_history(&q))),
            // Remote hosts are phase 4: until then this dispatcher polls none.
            ("GET", "/api/hosts") => (200, Body::Json(self.hosts_status())),
            ("GET", "/api/update") => (200, Body::Json(self.updates.status(&self.config()))),
            ("GET", "/api/events") => {
                // store.events(100): the newest hundred, newest first.
                let st = self.state();
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
                let st = self.state();
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
                let st = self.state();
                let prefs = st
                    .get("prefs")
                    .filter(|p| p.is_object() || p.is_array())
                    .cloned()
                    .unwrap_or(json!({}));
                (200, Body::Json(json!({"prefs": prefs})))
            }
            ("GET", "/api/usage") => (200, Body::Json(self.api_usage())),
            ("GET", "/api/pending") => {
                (200, Body::Json(json!({"pending": self.pending.waiting()})))
            }
            ("GET", "/api/matrix") => (200, Body::Json(self.api_matrix())),
            ("GET", "/api/decisions") => (
                200,
                Body::Json(json!({"decisions": decisions(&self.state_dir(), 60)})),
            ),
            ("GET", "/api/settings") => (200, Body::Json(self.api_settings())),
            ("GET", "/api/projects") => (200, Body::Json(json!({"projects": self.catalogue()}))),
            ("GET", "/api/changes") => (200, Body::Json(self.api_changes(&arg("session")))),
            ("GET", "/api/activity") => (200, Body::Json(self.api_activity(&arg("session")))),
            ("GET", "/api/costs") => {
                let raw = arg("days");
                let raw = if raw.is_empty() {
                    "30".to_string()
                } else {
                    raw
                };
                // int(): digits with an optional sign and surrounding space.
                let days = crate::pyish::strip(&raw)
                    .parse::<i64>()
                    .map(|d| d.clamp(1, 90))
                    .unwrap_or(30);
                let got = self
                    .cost_history
                    .lk()
                    .summary(days, Self::now(), 8.0);
                (200, Body::Json(got))
            }
            ("GET", "/api/session_status") => {
                (200, Body::Json(self.api_session_status(&arg("session"))))
            }
            ("GET", "/api/awaited") => (200, Body::Json(self.api_awaited(&arg("session")))),
            ("GET", "/api/timeline") => (200, self.api_timeline(&arg("session"), &arg("sig"))),
            ("GET", "/api/subagent") => (200, Body::Json(self.api_subagent(&arg("session"), &arg("agent")))),
            ("GET", "/api/processes") => (200, Body::Json(self.api_processes(&arg("session")))),
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
                Body::Json(json!({"key": settings::value(&self.config(), "quick.key"),
                                  "phrases": settings::value(&self.config(), "quick.phrases"), "ok": true})),
            ),
            ("GET", "/api/health") => (
                200,
                Body::Json(
                    json!({"ok": true, "pid": std::process::id(), "boot": self.boot,
                                  "look": look(&self.config())}),
                ),
            ),
            ("GET", "/api/conversation") => self.api_conversation(&q),
            ("GET", "/api/sessions") => (200, Body::Raw(self.api_sessions())),
            _ => (404, Body::Json(json!({"error": "no such path"}))),
        }
    }
}

/// Control.decisions: the permission hook's recent verdicts, newest first,
/// from the decisions.jsonl beside the state file.
fn decisions(dir: &std::path::Path, limit: usize) -> Vec<Value> {
    let Some(text) = tail_lines(&dir.join("decisions.jsonl"), limit) else {
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

/// The end of a file holding at least its last `limit` lines (whole), read
/// backwards in chunks rather than the whole file; None when it cannot be
/// read or what is kept is not UTF-8, as read_to_string fails.
fn tail_lines(path: &std::path::Path, limit: usize) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut start = len;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = 64 * 1024u64;
    // More than `limit` newlines means the line before the last `limit` has
    // ended in the buffer, so they are all whole.
    while start > 0 && buf.iter().filter(|b| **b == b'\n').count() <= limit {
        let step = chunk.min(start);
        start -= step;
        f.seek(SeekFrom::Start(start)).ok()?;
        let mut piece = vec![0u8; step as usize];
        f.read_exact(&mut piece).ok()?;
        piece.extend_from_slice(&buf);
        buf = piece;
        chunk = chunk.saturating_mul(2);
    }
    if start > 0 {
        // Drop the partial line in front; a newline is never inside a
        // UTF-8 sequence, so what is left starts on a character.
        let cut = buf.iter().position(|b| *b == b'\n').map_or(0, |i| i + 1);
        buf.drain(..cut);
    }
    String::from_utf8(buf).ok()
}

/// Parsed conversations kept (convcache's budget, by count).
const CONVS_KEPT: usize = 32;

const STREAM_EVERY: f64 = 5.0;
const STREAM_MIN_GAP: f64 = 2.0;
const STREAM_KEEPALIVE: f64 = 15.0;

/// dispatcher.NOT_RUNNING.
const NOT_RUNNING: &str = "that session is not running here, so there is nothing to close";

/// str(uuid.uuid4()).
fn uuid4() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("random bytes");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// dispatcher.IDLE_AFTER: how long after Stop the idle check looks.
const IDLE_AFTER: f64 = 60.0;

impl crate::notify::Waiting for Shared {
    fn channels(&self) -> crate::notify::Channels {
        self.notify_enabled()
    }

    /// Control._still_waiting: through the session list first, which is
    /// where a prompt answered at the terminal is noticed.
    fn still_waiting(&self, key: &str, since: &Value) -> bool {
        self.invalidate();
        let _ = self.sessions(2.0);
        let Some(store) = &self.store else {
            return false;
        };
        truthy(Some(since))
            && store
                .session(key)
                .and_then(|r| r.get("attention_since").cloned())
                .as_ref()
                == Some(since)
    }
}

/// matrix.active: whether a session gets a tile - its process is up and it
/// has not ended.
fn matrix_active(row: &Row) -> bool {
    truthy(row.get("status"))
        && !matches!(
            row.get("state").and_then(Value::as_str),
            Some("done") | Some("failed") | Some("stopped")
        )
}

/// Row fields that never leave the process (dispatcher.PRIVATE_ROW_FIELDS).
const PRIVATE_ROW_FIELDS: &[&str] = &["token"];

fn public(rows: &[Row]) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            let mut r = r.clone();
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
/// Control._overlay. True when the recorded attention is settled and the
/// store should clear it (Python clears it right there).
fn overlay(row: &mut Row, extra: &Map<String, Value>) -> bool {
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
        // A finished background job waits for nobody, whatever its last
        // hook said.
        let d = dispatchable(row);
        row.insert("dispatchable".into(), json!(d));
        return truthy(extra.get("attention_since"));
    }
    let since = extra.get("attention_since").filter(|v| truthy(Some(v)));
    let kind = extra.get("attention_kind").and_then(Value::as_str);
    let failure = kind == Some("failure");
    let stale = !failure && registry::moved_on(row, since.and_then(Value::as_f64));
    let mut clear = false;
    if since.is_some() && stale {
        clear = true;
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
    clear
}

/// A response body: a value to serialise, or JSON text already assembled.
pub enum Body {
    Json(Value),
    /// JSON text already assembled.
    Raw(String),
    /// Text of another type (a page): revalidated by ETag, never stored
    /// without asking, as its token, boot id and look can change.
    Text(String, &'static str),
    /// A compiled-in text file (a style or script) with its Cache-Control
    /// and version, which is its ETag.
    Static(&'static str, &'static str, &'static str, &'static str),
    /// Bytes served as they are (a font), with their Cache-Control.
    Bytes(&'static [u8], &'static str, &'static str),
}

impl Body {
    /// (bytes, Content-Type, Cache-Control, ETag)
    pub fn into_parts(self) -> (Vec<u8>, &'static str, &'static str, Option<String>) {
        const JSON: &str = "application/json; charset=utf-8";
        match self {
            Body::Json(v) => (
                serde_json::to_vec(&v).unwrap_or_else(|_| b"{}".to_vec()),
                JSON,
                "no-store",
                None,
            ),
            Body::Raw(t) => (t.into_bytes(), JSON, "no-store", None),
            Body::Text(t, ctype) => {
                use sha2::{Digest, Sha256};
                let sum = Sha256::digest(t.as_bytes());
                let tag: String = sum.iter().take(8).map(|b| format!("{b:02x}")).collect();
                (t.into_bytes(), ctype, "no-cache", Some(tag))
            }
            Body::Static(t, ctype, cache, v) => {
                (t.as_bytes().to_vec(), ctype, cache, Some(v.to_string()))
            }
            Body::Bytes(b, ctype, cache) => (b.to_vec(), ctype, cache, None),
        }
    }
}

/// Whether an If-None-Match header names `etag` (or is `*`), weak or not.
fn etag_matches(header: &str, etag: &str) -> bool {
    header
        .split(',')
        .map(|t| t.trim().trim_start_matches("W/").trim_matches('"'))
        .any(|t| t == "*" || t == etag)
}

/// secrets.token_urlsafe(n): n random bytes, base64url without padding.
pub fn token_urlsafe(n: usize) -> String {
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

/// dispatcher.start_tunnels: a hook-only listener and a reverse tunnel for
/// each host that asks. Run after the main port is bound, so a second
/// dispatcher that failed to bind never fights the first for the hosts'
/// ports.
fn start_tunnels(shared: &Arc<Shared>, fleet: &Arc<crate::remote::Fleet>) {
    for h in &fleet.hosts {
        let Some((local, remote)) = h.tunnel else {
            continue;
        };
        let addr = format!("127.0.0.1:{local}");
        let listener = match tiny_http::Server::http(&addr) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("tunnel {}: cannot listen on {local}: {e}", h.name);
                continue;
            }
        };
        let (me, name) = (Arc::clone(shared), h.name.clone());
        std::thread::Builder::new()
            .name(format!("hooks-{name}"))
            .spawn(move || hook_only(listener, me, name))
            .ok();
        let tunnel = crate::remote::Tunnel::new(&h.name, local, remote);
        tunnel.start();
        shared.tunnels.lk().insert(h.name.clone(), tunnel);
        println!("tunnel {}: host 127.0.0.1:{remote} -> hooks on 127.0.0.1:{local}", h.name);
    }
}

/// HookOnlyHandler: what a host's tunnel lands on - `/hook`, and nothing
/// else. Everything else the dispatcher serves would let any process on
/// that host drive the sessions on this machine. The host is known from
/// which listener the connection came in on, never from the payload.
fn hook_only(server: tiny_http::Server, shared: Arc<Shared>, host: String) {
    let inflight = Arc::new(AtomicUsize::new(0));
    for request in server.incoming_requests() {
        let Some(slot) = Slot::take(&inflight, MAX_TUNNEL_HOOKS) else {
            busy(request);
            continue;
        };
        let (shared, host) = (Arc::clone(&shared), host.clone());
        std::thread::spawn(move || {
            let _slot = slot;
            hook_only_one(request, &shared, &host)
        });
    }
}

fn hook_only_one(mut request: tiny_http::Request, shared: &Arc<Shared>, host: &str) {
    {
        let path = request.url().split('?').next().unwrap_or("").trim_end_matches('/').to_string();
        let (code, body) = if request.method() != &tiny_http::Method::Post || path != "/hook" {
            (404, json!({"error": "no such path"}))
        } else {
            let ctype = request
                .headers()
                .iter()
                .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("Content-Type"))
                .map(|h| h.value.as_str().to_string())
                .unwrap_or_default();
            let mut raw = Vec::new();
            let _ = std::io::Read::read_to_end(
                &mut std::io::Read::take(request.as_reader(), 4 * 1024 * 1024 + 1),
                &mut raw,
            );
            if ctype.split(';').next().unwrap_or("").trim() != "application/json" {
                (415, json!({"error": "expected application/json"}))
            } else if raw.is_empty() || raw.len() > 4 * 1024 * 1024 {
                (413, json!({"error": format!("body must be 1..{} bytes", 4 * 1024 * 1024)}))
            } else {
                match serde_json::from_slice::<Value>(&raw) {
                    Err(_) => (400, json!({"error": "body is not JSON"})),
                    Ok(v) => {
                        let empty = Map::new();
                        let p = v.as_object().unwrap_or(&empty);
                        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            shared.on_hook(p, Some(host))
                        }))
                        .unwrap_or_else(|e| {
                            eprintln!("imperatorium: hook from {host} panicked: {}", panic_text(&*e));
                            json!({"ok": false, "error": "internal error"})
                        });
                        (200, out)
                    }
                }
            }
        };
        let resp = tiny_http::Response::from_string(body.to_string())
            .with_status_code(code)
            .with_header(json_header());
        let _ = request.respond(resp);
    }
}

pub fn serve(host: &str, port: u16, home: Option<&str>, state: Option<&str>, live: bool) -> i32 {
    let addr = format!("{host}:{port}");
    let server = match tiny_http::Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("imperatorium: cannot listen on {addr}: {e}");
            return 1;
        }
    };
    let shared = Arc::new(Shared::new(home, state, live));
    let _ = shared.me.set(Arc::downgrade(&shared));
    if shared.store.is_some() {
        // Only once the port is ours, as in Python: a second dispatcher that
        // failed to bind never runs a second set of gh calls or a model.
        shared.prs.start();
        if let Some(v) = &shared.voice {
            if truthy(v.settings.get("preload")) {
                v.warm();
            }
        }
        let warm = Arc::clone(&shared);
        std::thread::spawn(move || {
            // Control.warm_spend: read the transcripts once at start.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let now = Shared::now();
                let limits = warm.usage.summary(now)["limits"].clone();
                warm.spend(now, 120.0, &limits);
            }));
        });
        let me = Arc::downgrade(&shared);
        shared.updates.start(move || me.upgrade().map(|s| s.config()));
        let sweeper = Arc::clone(&shared);
        std::thread::spawn(move || {
            // A sweep that panics is tried again next time; each distinct
            // panic is reported once, not every 15 s.
            let mut seen = std::collections::HashSet::new();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(15));
                let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    sweeper.sweep_queue()
                }));
                if let Err(e) = got {
                    let text = panic_text(&*e);
                    if seen.len() < 64 && seen.insert(text.clone()) {
                        eprintln!("imperatorium: sweep_queue panicked: {text}");
                    }
                }
            }
        });
        if let Some(fleet) = &shared.fleet {
            // Each poll that lands wakes the window's streams.
            let me = Arc::downgrade(&shared);
            fleet.start(Arc::new(move || {
                if let Some(s) = me.upgrade() {
                    s.invalidate();
                }
            }));
            start_tunnels(&shared, fleet);
        }
    }
    println!("imperatorium (native) listening on http://{addr}");
    // A fixed pool answers the ordinary requests, fed by a bounded queue:
    // a burst (or a client opening connections in a loop) gets a 503 rather
    // than a thread each. The two kinds that hold a connection open for long
    // - the window's event stream and a parked permission request - get a
    // thread of their own, counted against caps of their own, so they can
    // never starve the pool.
    //
    // tiny_http 0.12 sets no socket timeouts and offers no way to reach the
    // socket it accepted, so a client that stalls mid-body holds its worker
    // until it goes away; the pool bounds how many it can hold, and the
    // body read is capped in size (MAX_BODY) as before.
    let (tx, rx) = std::sync::mpsc::sync_channel::<tiny_http::Request>(QUEUED);
    let rx = Arc::new(Mutex::new(rx));
    for i in 0..WORKERS {
        let (rx, shared) = (Arc::clone(&rx), Arc::clone(&shared));
        std::thread::Builder::new()
            .name(format!("http-{i}"))
            .spawn(move || loop {
                let next = rx.lk().recv();
                match next {
                    Ok(request) => answer(request, &shared, port),
                    Err(_) => return,
                }
            })
            .expect("cannot start an HTTP worker");
    }
    let (streams, parked) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    for request in server.incoming_requests() {
        let (method, path) = route_of(&request);
        let long = if method == "GET" && path == "/api/stream" {
            Some((&streams, MAX_STREAMS))
        } else if method == "POST" && path == "/permission/park" {
            Some((&parked, MAX_PARKED))
        } else {
            None
        };
        if let Some((count, cap)) = long {
            let Some(slot) = Slot::take(count, cap) else {
                busy(request);
                continue;
            };
            let shared = Arc::clone(&shared);
            let spawned = std::thread::Builder::new().spawn(move || {
                let _slot = slot;
                answer(request, &shared, port);
            });
            if let Err(e) = spawned {
                eprintln!("imperatorium: cannot start a thread: {e}");
            }
            continue;
        }
        match tx.try_send(request) {
            Ok(()) => {}
            Err(std::sync::mpsc::TrySendError::Full(r))
            | Err(std::sync::mpsc::TrySendError::Disconnected(r)) => busy(r),
        }
    }
    0
}

/// Workers answering ordinary requests, and how many may wait for one.
const WORKERS: usize = 32;
const QUEUED: usize = 256;
/// Open `/api/stream` feeds (a window holds one each) and parked
/// permission requests (one per session asking) allowed at once.
const MAX_STREAMS: usize = 32;
const MAX_PARKED: usize = 64;
/// In-flight `/hook` requests on one host's tunnel listener.
const MAX_TUNNEL_HOOKS: usize = 16;

/// A place in one of the capped counts, given back when dropped.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(count: &Arc<AtomicUsize>, cap: usize) -> Option<Slot> {
        let mut n = count.load(Ordering::Acquire);
        loop {
            if n >= cap {
                return None;
            }
            match count.compare_exchange_weak(n, n + 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Some(Slot(Arc::clone(count))),
                Err(now) => n = now,
            }
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// (method, path without query or trailing slash)
fn route_of(request: &tiny_http::Request) -> (String, String) {
    let path = request
        .url()
        .split('?')
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    (request.method().as_str().to_string(), path)
}

fn json_header() -> tiny_http::Header {
    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("a static header")
}

/// Over a cap: 503, and the connection is not held.
fn busy(request: tiny_http::Request) {
    let body = json!({"error": "the dispatcher is busy, try again"}).to_string();
    let resp = tiny_http::Response::from_string(body)
        .with_status_code(503)
        .with_header(json_header());
    let _ = request.respond(resp);
}

/// What a caught panic said.
fn panic_text(e: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = e.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = e.downcast_ref::<String>() {
        s.clone()
    } else {
        "a panic without a message".to_string()
    }
}

/// One request on the main listener, start to end. A panic in a handler
/// answers 500 rather than dropping the connection, and is logged.
fn answer(mut request: tiny_http::Request, shared: &Arc<Shared>, port: u16) {
    let (method, path) = route_of(&request);
    if method == "GET" && path == "/api/stream" {
        let matrix = request
            .url()
            .split_once('?')
            .is_some_and(|(_, q)| q.split('&').any(|kv| kv == "matrix=1"));
        let mut writer = request.into_writer();
        let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            shared.stream(&mut *writer, matrix)
        }));
        if let Err(e) = got {
            eprintln!("imperatorium: GET /api/stream panicked: {}", panic_text(&*e));
        }
        return;
    }
    let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if method == "POST" {
            let header = |name: &str| {
                request
                    .headers()
                    .iter()
                    .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
                    .map(|h| h.value.as_str().to_string())
            };
            let (origin, ctype) = (header("Origin"), header("Content-Type"));
            let window = header("X-Imperatorium-Window");
            let mod_token = header("X-Imperatorium-Mod");
            let mut body = Vec::new();
            let _ = std::io::Read::read_to_end(
                &mut std::io::Read::take(request.as_reader(), 32 * 1024 * 1024 + 1),
                &mut body,
            );
            let p = if path.is_empty() {
                "/".to_string()
            } else {
                path.clone()
            };
            shared.post(
                &p,
                origin.as_deref(),
                ctype.as_deref(),
                &body,
                port,
                window.as_deref(),
                mod_token.as_deref(),
            )
        } else {
            shared.handle(&method, request.url())
        }
    }));
    let (code, body) = got.unwrap_or_else(|e| {
        eprintln!("imperatorium: {method} {path} panicked: {}", panic_text(&*e));
        (500, Body::Json(json!({"error": "internal error"})))
    });
    let (bytes, ctype, cache, etag) = body.into_parts();
    let unchanged = code == 200
        && method == "GET"
        && etag.as_deref().is_some_and(|tag| {
            request
                .headers()
                .iter()
                .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("If-None-Match"))
                .is_some_and(|h| etag_matches(h.value.as_str(), tag))
        });
    let (code, bytes) = if unchanged { (304, Vec::new()) } else { (code, bytes) };
    // Content-Length, as the Python dispatcher sends: tiny_http goes
    // chunked above 32 KiB by default, and a conversation is often
    // several hundred.
    let mut resp = tiny_http::Response::from_data(bytes)
        .with_chunked_threshold(usize::MAX)
        .with_status_code(code)
        .with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes())
                .expect("a static header"),
        )
        .with_header(
            tiny_http::Header::from_bytes(&b"Cache-Control"[..], cache.as_bytes())
                .expect("a static header"),
        );
    if let Some(tag) = etag {
        if let Ok(h) = tiny_http::Header::from_bytes(&b"ETag"[..], format!("\"{tag}\"").as_bytes()) {
            resp.add_header(h);
        }
    }
    let _ = request.respond(resp);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_of_a_file_has_its_last_lines() {
        let dir = std::env::temp_dir().join(format!("imp-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("decisions.jsonl");
        // Long enough that the tail is read in more than one chunk.
        let mut text = String::new();
        for i in 0..5000 {
            text.push_str(&format!("{{\"n\": {i}, \"pad\": \"{}\"}}\n", "\u{e9}".repeat(i % 40)));
        }
        for (body, limit) in [
            (text.clone(), 60usize),
            (text.clone() + "{\"n\": \"no newline\"}", 60),
            (text.clone(), 1),
            (text.clone(), 10000),
            ("{\"a\": 1}\n\n{\"b\": 2}\n".to_string(), 60),
            (String::new(), 60),
        ] {
            std::fs::write(&p, &body).unwrap();
            let whole: Vec<&str> = body.split_inclusive('\n').collect();
            let want = whole[whole.len().saturating_sub(limit)..].concat();
            let got = tail_lines(&p, limit).unwrap();
            let got_lines: Vec<&str> = got.split_inclusive('\n').collect();
            assert_eq!(got_lines[got_lines.len().saturating_sub(limit)..].concat(), want);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn slots_are_capped_and_given_back() {
        let count = Arc::new(AtomicUsize::new(0));
        let a = Slot::take(&count, 2).unwrap();
        let _b = Slot::take(&count, 2).unwrap();
        assert!(Slot::take(&count, 2).is_none());
        drop(a);
        assert!(Slot::take(&count, 2).is_some());
        assert_eq!(count.load(Ordering::Acquire), 1);
    }

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
    fn etags_match_as_browsers_send_them() {
        assert!(etag_matches("\"abc\"", "abc"));
        assert!(etag_matches("W/\"abc\", \"def\"", "def"));
        assert!(etag_matches("*", "abc"));
        assert!(!etag_matches("\"abd\"", "abc"));
    }

    #[test]
    fn query_strings() {
        let q = parse_query("session=a%2Db&known=x+y&blocks=400&session=ignored");
        assert_eq!(q["session"], "a-b");
        assert_eq!(q["known"], "x y");
        assert_eq!(q["blocks"], "400");
    }

}
