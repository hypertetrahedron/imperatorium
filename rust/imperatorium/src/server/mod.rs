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
mod route;
mod snapshot;
use self::snapshot::{matrix_active, remote_key, Snap};
mod conv;
pub use self::conv::signature;
use self::conv::Cached;
mod dispatch;
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

    fn now() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
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
