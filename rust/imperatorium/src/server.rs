//! `imperatorium serve`: the dispatcher's HTTP API, ported endpoint by
//! endpoint (native port phase 3). Until the cutover it runs beside the
//! Python dispatcher as a shadow (port 8811 by default), and
//! `rust/conformance/shadow.py` sends both the same requests and compares
//! the answers. It reads config.json, state.json and the transcripts, and
//! writes nothing.
//!
//! Ported so far: GET /api/health, GET /api/conversation. Anything else is
//! a 404, as the Python dispatcher answers an unknown path.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};

use crate::pyish::{json_dumps_sorted, truthy};
use crate::{conversation, paths, registry, sessions};

pub struct Shared {
    config: Value,
    boot: String,
    lookup: registry::Lookup,
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
            config: load_config(home),
            boot,
            lookup: registry::Lookup::new(),
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
        let row = self.lookup.find(&session, &self.config);
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

    pub fn handle(&self, method: &str, url: &str) -> (u16, Body) {
        let (path, query) = url.split_once('?').unwrap_or((url, ""));
        let q = parse_query(query);
        match (method, path) {
            ("GET", "/api/health") => (
                200,
                Body::Json(
                    json!({"ok": true, "pid": std::process::id(), "boot": self.boot,
                                  "look": look(&self.config)}),
                ),
            ),
            ("GET", "/api/conversation") => self.api_conversation(&q),
            _ => (404, Body::Json(json!({"error": "no such path"}))),
        }
    }
}

/// A response body: a value to serialise, or JSON text already assembled.
pub enum Body {
    Json(Value),
    Raw(String),
}

impl Body {
    pub fn into_text(self) -> String {
        match self {
            Body::Json(v) => serde_json::to_string(&v).unwrap_or_else(|_| "{}".into()),
            Body::Raw(t) => t,
        }
    }
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
            let (code, body) = shared.handle(&method, request.url());
            let text = body.into_text();
            // Content-Length, as the Python dispatcher sends: tiny_http goes
            // chunked above 32 KiB by default, and a conversation is often
            // several hundred.
            let resp = tiny_http::Response::from_string(text)
                .with_chunked_threshold(usize::MAX)
                .with_status_code(code)
                .with_header(
                    tiny_http::Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                )
                .with_header(
                    tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap(),
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
