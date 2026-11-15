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

mod alerts;
mod conv;
mod dispatch;
mod hook;
mod prefs;
mod route;
mod snapshot;
mod spend;
mod views;

pub use self::conv::signature;
use self::conv::Cached;
use self::hook::{decisions, start_tunnels};
use self::prefs::{get, load_config, look};
use self::snapshot::{matrix_active, remote_key, Snap};

/// The dispatcher's state, shared by every request thread. Its methods are
/// spread over the child modules by what they do: `route` (GET and POST
/// dispatch), `hook` (tool hooks, the idle check, the mod's beats, tunnels),
/// `snapshot` (the session list, its cache and the event streams), `conv`
/// (the conversation cache and its views), `views` (read-only per-session
/// and fleet views), `prefs` (the Settings form), `spend` (budget, usage,
/// errors), `dispatch` (sending to sessions and the prompt queue) and
/// `alerts` (push and speech channels).
///
/// Locks. Each is held for a short critical section and none is held across
/// I/O on a request path except `snap_build`. Where two are ever held
/// together, the order is:
///
/// - `snap_build`, then `snap` and `changes`: `snap_of` holds `snap_build`
///   while the snapshot is built (which reads `config_live` and `doing` and
///   calls into `stats` and the other components' own locks), then stores
///   the result in `snap`. `snap` and `changes` are never held together:
///   `invalidate` takes them one after the other, and never `snap_build`,
///   so a build cannot hold up what invalidates it.
/// - `settings_lock`, then `config_live` (write), then, after that guard is
///   dropped, `snap`/`changes` (via `invalidate`) and `restart_needed`.
/// - `tunnels`, then each tunnel's own lock (`hosts_status` reads the
///   status while holding it).
///
/// `convs`, `paths`, `cost_history`, `errors_cache`, `tallies` and `doing`
/// are leaf locks: no other lock of this struct is taken while one is held
/// (`paths` and `convs` are taken one after the other, never nested), and
/// none is held across a call to `conversation()`. `changes` is paired with
/// the condvar `changed`, which waits on that mutex alone.
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
                        if truthy(settings.get("enabled")) {
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
                    .then(|| crate::remote::Fleet::new(hosts, crate::remote::poll_every()))
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

    fn now() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
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

pub fn serve(host: &str, port: u16, home: Option<&str>, state: Option<&str>, live: bool) -> i32 {
    let addr = format!("{host}:{port}");
    let server = match tiny_http::Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("imperatorium: cannot listen on {addr}: {e}");
            return 1;
        }
    };
    // `--port 0`: the system picks a free port (the integration tests run
    // many dispatchers side by side). Everything below, the Origin check
    // included, uses the port actually bound, and the "listening" line
    // below names it.
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(port);
    let addr = format!("{host}:{port}");
    let shared = Arc::new(Shared::new(home, state, live));
    let _ = shared.me.set(Arc::downgrade(&shared));
    if shared.store.is_some() {
        // Only once the port is ours, as in Python: a second dispatcher that
        // failed to bind never runs a second set of gh calls or a model.
        shared.prs.start();
        if let Some(v) = &shared.voice {
            if truthy(v.settings.get("preload")) && v.missing().is_none() {
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
                std::thread::sleep(std::time::Duration::from_secs_f64(crate::timing::t().queue_sweep));
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
