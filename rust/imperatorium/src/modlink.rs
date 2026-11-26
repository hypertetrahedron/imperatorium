//! The dispatcher's side of the Imperatorium mod, from
//! `imperatorium/modlink.py`.
//!
//! The mod inside each Claude Code session beats to `POST /api/mod/beat`
//! every couple of seconds; the reply carries the prompts waiting for it,
//! which it submits once the session is idle. A prompt the person sent from
//! the window goes as their own words; anything else is framed under the
//! mod's name. Beats carry the token in the state folder's `mod-token`,
//! which keeps anything that merely reaches the port from posing as a mod.

use crate::locks::LockExt;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::pyish::{py_str, take, truthy};

pub const TOKEN_FILE: &str = "mod-token";
/// Beside the token: where this dispatcher listens (`http://127.0.0.1:8792`),
/// so the mod need not assume the default port.
pub const ENDPOINT_FILE: &str = "mod-endpoint";

/// The URL the mod reaches a dispatcher bound to `host:port` at.
pub fn endpoint_url(host: &str, port: u16) -> String {
    let at = match host {
        "" | "0.0.0.0" => "127.0.0.1".to_string(),
        "::" => "[::1]".to_string(),
        h if h.contains(':') && !h.starts_with('[') => format!("[{h}]"),
        h => h.to_string(),
    };
    format!("http://{at}:{port}")
}
/// A mod silent this long is gone; its prompts go the old way.
pub const LIVE_FOR: f64 = 8.0;
/// A prompt handed out and never accepted is offered again after this.
pub const CLAIM_FOR: f64 = 30.0;
pub const SETTLED_KEEP: usize = 20;
/// A stop the mod has not taken by then is dropped: handed out later it
/// would end a turn the person never meant to stop.
pub const STOP_FOR: f64 = 10.0;
/// A stop handed out and not answered is handed out again after this.
pub const STOP_CLAIM_FOR: f64 = 3.0;
/// The first mod that ends a turn when its beat's reply says `stop`.
pub const STOP_SINCE: (u64, u64, u64) = (0, 3, 0);

/// The first mod that runs the ops a beat's reply carries (`ops`): slash
/// commands, and reads of the session's context, status and settings.
pub const OPS_SINCE: (u64, u64, u64) = (0, 4, 0);
/// An op its mod has not answered by then is dropped.
pub const OP_FOR: f64 = 30.0;
/// A read handed out and not answered is handed out again after this. A
/// change (a command, a setting) is handed out once only: run twice, it
/// would do its work twice.
pub const OP_CLAIM_FOR: f64 = 5.0;
/// Ops that change the session; the rest only read.
const CHANGES: &[&str] = &["command", "config_set"];
const RESULTS_KEEP: usize = 100;

fn at_least(version: Option<&Value>, since: (u64, u64, u64)) -> bool {
    let v = version.and_then(Value::as_str).unwrap_or("");
    let mut parts = v.split(['.', '-']).map(|p| p.parse::<u64>().ok());
    match (parts.next().flatten(), parts.next().flatten(), parts.next().flatten()) {
        (Some(a), Some(b), c) => (a, b, c.unwrap_or(0)) >= since,
        _ => false,
    }
}

/// Pure: whether a mod of `version` ("0.3.0") can stop a turn.
pub fn can_stop(version: Option<&Value>) -> bool {
    at_least(version, STOP_SINCE)
}

/// Pure: whether a mod of `version` runs ops.
pub fn can_ops(version: Option<&Value>) -> bool {
    at_least(version, OPS_SINCE)
}

type Obj = Map<String, Value>;

/// modlink.load_token: the token in `folder`, made on first use.
pub fn load_token(folder: &Path) -> Result<String, String> {
    let path = folder.join(TOKEN_FILE);
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = crate::pyish::strip(&t);
        if t.chars().count() >= 32 {
            return Ok(t.to_string());
        }
    }
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let mut raw = [0u8; 32];
    getrandom::getrandom(&mut raw).map_err(|e| e.to_string())?;
    let token = base64url(&raw);
    write_private(&path, &format!("{token}\n")).map_err(|e| e.to_string())?;
    Ok(token)
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(text.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

fn base64url(raw: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in raw.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for i in 0..=chunk.len() {
            out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

fn is_num(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Number(_)))
}

/// modlink.statusline_payload: `$.session.usage()` as the statusline
/// payload usage.reading() reads.
pub fn statusline_payload(usage: &Value) -> Value {
    let empty = Map::new();
    let usage = usage.as_object().unwrap_or(&empty);
    let or_empty = |k: &str| {
        usage
            .get(k)
            .filter(|v| truthy(Some(v)))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    };
    let ctx = or_empty("context");
    let mut out = json!({"context_window": {}, "cost": {}, "rate_limits": {}});
    if is_num(ctx.get("percent")) {
        out["context_window"]["used_percentage"] = ctx["percent"].clone();
    }
    if is_num(ctx.get("window")) {
        out["context_window"]["context_window_size"] = ctx["window"].clone();
    }
    let cost = or_empty("cost");
    if is_num(cost.get("usd")) {
        out["cost"]["total_cost_usd"] = cost["usd"].clone();
    }
    if truthy(usage.get("model")) {
        out["model"] = json!({"display_name": py_str(&usage["model"])});
    }
    let windows = usage
        .get("rateLimits")
        .filter(|v| truthy(Some(v)))
        .and_then(Value::as_array);
    for w in windows.into_iter().flatten() {
        let Some(w) = w.as_object() else { continue };
        if !is_num(w.get("percentUsed")) {
            continue;
        }
        let mut entry = json!({"used_percentage": w["percentUsed"].clone()});
        if let Some(r) = epoch(w.get("resetsAt")) {
            entry["resets_at"] = json!(r);
        }
        let kind = w
            .get("kind")
            .filter(|v| truthy(Some(v)))
            .map(py_str)
            .unwrap_or_default();
        out["rate_limits"][kind] = entry;
    }
    out
}

/// modlink._epoch: an ISO 8601 time (as datetime.fromisoformat reads it,
/// "Z" for UTC) as epoch seconds, or None.
pub fn epoch(iso: Option<&Value>) -> Option<f64> {
    let s = iso?
        .as_str()
        .filter(|s| !s.is_empty())?
        .replace('Z', "+00:00");
    let b = s.as_bytes();
    let digits = |from: usize, n: usize| -> Option<u32> {
        let part = s.get(from..from + n)?;
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (y, mo, d) = (digits(0, 4)?, digits(5, 2)?, digits(8, 2)?);
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return None;
    }
    let (mut h, mut mi, mut sec, mut micro) = (0u32, 0u32, 0u32, 0u32);
    let mut i = 10;
    let mut offset: Option<i64> = None;
    if i < b.len() {
        // One separator character, then the time.
        i += 1;
        h = digits(i, 2)?;
        i += 2;
        if b.get(i) == Some(&b':') {
            mi = digits(i + 1, 2)?;
            i += 3;
            if b.get(i) == Some(&b':') {
                sec = digits(i + 1, 2)?;
                i += 3;
                if matches!(b.get(i), Some(b'.') | Some(b',')) {
                    let start = i + 1;
                    let mut end = start;
                    while end < b.len() && b[end].is_ascii_digit() {
                        end += 1;
                    }
                    if end == start {
                        return None;
                    }
                    let frac = &s[start..end.min(start + 6)];
                    micro = format!("{frac:0<6}").parse().ok()?;
                    i = end;
                }
            }
        }
        if i < b.len() {
            let sign = match b[i] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let rest = &s[i + 1..];
            let parts: Vec<&str> = if rest.contains(':') {
                rest.split(':').collect()
            } else {
                rest.as_bytes()
                    .chunks(2)
                    .map(|c| std::str::from_utf8(c).unwrap_or("x"))
                    .collect()
            };
            if parts.is_empty()
                || parts.len() > 3
                || parts
                    .iter()
                    .any(|p| p.len() != 2 || !p.bytes().all(|c| c.is_ascii_digit()))
            {
                return None;
            }
            let n: Vec<i64> = parts.iter().map(|p| p.parse().unwrap()).collect();
            if n[0] > 23 || n.get(1).is_some_and(|m| *m > 59) || n.get(2).is_some_and(|x| *x > 59) {
                return None;
            }
            offset =
                Some(sign * (n[0] * 3600 + n.get(1).unwrap_or(&0) * 60 + n.get(2).unwrap_or(&0)));
        }
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if y < 1
        || !(1..=12).contains(&mo)
        || d < 1
        || d > days_in[mo as usize - 1]
        || h > 23
        || mi > 59
        || sec > 59
    {
        return None;
    }
    match offset {
        Some(off) => {
            let whole = crate::paths::days_from_civil(y as i64, mo, d) * 86400
                + (h * 3600 + mi * 60 + sec) as i64
                - off;
            Some((whole as f64 * 1e6 + micro as f64) / 1e6)
        }
        None => {
            Some(crate::paths::local_to_epoch(y as i64, mo, d, h, mi, sec) + micro as f64 / 1e6)
        }
    }
}

#[derive(Default)]
struct Inner {
    mods: HashMap<String, Obj>,
    outbox: IndexMap<String, Vec<Obj>>,
    settled: HashMap<String, Vec<Obj>>,
    /// The stop asked for each session: (id, asked at, handed out at).
    stops: HashMap<String, (String, f64, Option<f64>)>,
    /// What came of each session's last stop: {id, outcome, reason, at}.
    stopped: HashMap<String, Obj>,
    /// The ops waiting for each session's mod: {id, op, args, at, given_at}.
    ops: HashMap<String, Vec<Obj>>,
    /// What each op came to, by id: ({ok, data | error}, when).
    results: HashMap<String, (Value, f64)>,
}

pub struct ModLink {
    pub token: String,
    inner: Mutex<Inner>,
    /// Woken when an op's result arrives.
    answered: std::sync::Condvar,
}

impl ModLink {
    pub fn new(token: String) -> Self {
        ModLink {
            token,
            inner: Mutex::default(),
            answered: std::sync::Condvar::new(),
        }
    }

    /// Ask the session's mod to run `op` (command, context, status,
    /// usage, config, config_set, settings) with `args`; the op's id.
    pub fn request_op(&self, sid: &str, op: &str, args: Value, now: f64) -> String {
        let mut raw = [0u8; 6];
        getrandom::getrandom(&mut raw).expect("random bytes");
        let id: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let item = json!({"id": id, "op": op, "args": args, "at": now, "given_at": null});
        self.inner
            .lk()
            .ops
            .entry(sid.to_string())
            .or_default()
            .push(item.as_object().unwrap().clone());
        id
    }

    /// The ops to hand the session's mod with this beat's reply.
    pub fn take_ops(&self, sid: &str, now: f64) -> Vec<Value> {
        let mut inner = self.inner.lk();
        let Some(list) = inner.ops.get_mut(sid) else {
            return vec![];
        };
        let mut expired = vec![];
        list.retain(|o| {
            let keep = now - o["at"].as_f64().unwrap_or(now) <= OP_FOR;
            if !keep {
                expired.push(py_str(&o["id"]));
            }
            keep
        });
        let mut give = vec![];
        for o in list.iter_mut() {
            let changes = CHANGES.contains(&o["op"].as_str().unwrap_or(""));
            match o.get("given_at").and_then(Value::as_f64) {
                Some(_) if changes => continue,
                Some(g) if now - g < OP_CLAIM_FOR => continue,
                _ => {}
            }
            o.insert("given_at".into(), json!(now));
            give.push(json!({"id": o["id"], "op": o["op"], "args": o["args"]}));
        }
        if list.is_empty() {
            inner.ops.remove(sid);
        }
        for id in expired {
            inner.results.insert(id, (json!({"ok": false, "error": "the session's mod did not answer in time"}), now));
        }
        give
    }

    /// Record what the mod's beat says its ops came to.
    fn take_results(&self, inner: &mut Inner, sid: &str, beat: &Obj, now: f64) {
        let Some(list) = beat.get("results").and_then(Value::as_array) else {
            return;
        };
        for r in list {
            let Some(id) = r.get("id").and_then(Value::as_str) else { continue };
            if let Some(ops) = inner.ops.get_mut(sid) {
                ops.retain(|o| o["id"].as_str() != Some(id));
            }
            let mut got = r.as_object().cloned().unwrap_or_default();
            got.remove("id");
            got.entry("ok").or_insert(json!(true));
            inner.results.insert(id.to_string(), (Value::Object(got), now));
        }
        if inner.results.len() > RESULTS_KEEP {
            let mut by_age: Vec<(String, f64)> =
                inner.results.iter().map(|(k, (_, t))| (k.clone(), *t)).collect();
            by_age.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (k, _) in by_age.iter().take(inner.results.len() - RESULTS_KEEP) {
                inner.results.remove(k);
            }
        }
        self.answered.notify_all();
    }

    /// What op `id` came to, once its mod has answered.
    pub fn result(&self, id: &str) -> Option<Value> {
        self.inner.lk().results.get(id).map(|(v, _)| v.clone())
    }

    /// Wait up to `limit` for op `id`'s result.
    pub fn wait_result(&self, id: &str, limit: std::time::Duration) -> Option<Value> {
        let until = std::time::Instant::now() + limit;
        let mut inner = self.inner.lk();
        loop {
            if let Some((v, _)) = inner.results.get(id) {
                return Some(v.clone());
            }
            let left = until.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return None;
            }
            inner = self
                .answered
                .wait_timeout(inner, left)
                .map(|(g, _)| g)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }

    /// The slash commands the session's mod last listed ($.command.list()).
    pub fn commands(&self, sid: &str) -> Option<Value> {
        self.inner
            .lk()
            .mods
            .get(sid)
            .and_then(|m| m.get("commands"))
            .filter(|c| c.is_array())
            .cloned()
    }

    /// ModLink.authentic, in constant time.
    pub fn authentic(&self, token: Option<&str>) -> bool {
        let (a, b) = (token.unwrap_or("").as_bytes(), self.token.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }

    /// ModLink.beat: record one beat, settle what it reports on, and answer
    /// the prompts to submit now.
    pub fn beat(&self, beat: &Obj, now: f64) -> Vec<Value> {
        let sid = beat
            .get("session_id")
            .filter(|v| truthy(Some(v)))
            .map(py_str)
            .unwrap_or_default();
        if sid.is_empty() {
            return vec![];
        }
        let mut inner = self.inner.lk();
        let known = inner.mods.get(&sid).cloned().unwrap_or_default();
        let or = |k: &str| match beat.get(k).filter(|v| truthy(Some(v))) {
            Some(v) => v.clone(),
            None => known.get(k).cloned().unwrap_or(Value::Null),
        };
        let get = |k: &str| beat.get(k).cloned().unwrap_or(Value::Null);
        let state = json!({
            "at": now,
            "since": known.get("since").cloned().unwrap_or(json!(now)),
            "cwd": or("cwd"),
            "busy": truthy(beat.get("busy")),
            "turn_started": get("turn_started"),
            "interactive": get("interactive"),
            "surfaces": beat.get("surfaces").filter(|v| truthy(Some(v))).cloned().unwrap_or(json!([])),
            "version": get("version"),
            "mod_version": get("mod_version"),
            "usage": or("usage"),
            "commands": or("commands"),
        });
        inner
            .mods
            .insert(sid.clone(), state.as_object().unwrap().clone());
        let acks = beat
            .get("acks")
            .filter(|v| truthy(Some(v)))
            .and_then(Value::as_array);
        for ack in acks.into_iter().flatten() {
            settle(&mut inner, &sid, ack, now);
        }
        for s in beat.get("stopped").and_then(Value::as_array).into_iter().flatten() {
            let Some(id) = s.get("id").and_then(Value::as_str) else { continue };
            if inner.stops.get(&sid).is_some_and(|(want, _, _)| want == id) {
                inner.stops.remove(&sid);
            }
            let mut got = s.as_object().cloned().unwrap_or_default();
            got.insert("at".into(), json!(now));
            inner.stopped.insert(sid.clone(), got);
        }
        self.take_results(&mut inner, &sid, beat, now);
        let mut give = vec![];
        for item in inner.outbox.get_mut(&sid).into_iter().flatten() {
            if truthy(item.get("accepted_at")) {
                continue; // in the engine's queue: never handed out twice
            }
            if let Some(c) = item
                .get("claimed_at")
                .filter(|v| truthy(Some(v)))
                .and_then(Value::as_f64)
            {
                if now - c < CLAIM_FOR {
                    continue;
                }
            }
            item.insert("claimed_at".into(), json!(now));
            give.push(json!({"id": item["id"], "text": item["text"], "as_user": truthy(item.get("as_user"))}));
        }
        give
    }

    /// Ask the session's mod to end its running turn; the stop's id. A
    /// newer ask replaces one not yet answered.
    pub fn request_stop(&self, sid: &str, now: f64) -> String {
        let mut raw = [0u8; 6];
        getrandom::getrandom(&mut raw).expect("random bytes");
        let id: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        self.inner.lk().stops.insert(sid.to_string(), (id.clone(), now, None));
        id
    }

    /// The stop to hand the session's mod with this beat's reply, if one is
    /// waiting and fresh; again when the last hand-out went unanswered.
    pub fn take_stop(&self, sid: &str, now: f64) -> Option<String> {
        let mut inner = self.inner.lk();
        let (id, at, claimed) = inner.stops.get(sid).cloned()?;
        if now - at > STOP_FOR {
            inner.stops.remove(sid);
            return None;
        }
        if claimed.is_some_and(|c| now - c < STOP_CLAIM_FOR) {
            return None;
        }
        inner.stops.insert(sid.to_string(), (id.clone(), at, Some(now)));
        Some(id)
    }

    /// What the mod said came of the session's last stop.
    pub fn stop_result(&self, sid: &str) -> Option<Obj> {
        self.inner.lk().stopped.get(sid).cloned()
    }

    pub fn forget(&self, sid: &str) {
        self.inner.lk().mods.remove(sid);
    }

    /// ModLink.state: the last beat of a live mod.
    pub fn state(&self, sid: &str, now: f64) -> Option<Obj> {
        let inner = self.inner.lk();
        let got = inner.mods.get(sid)?;
        (now - got["at"].as_f64().unwrap_or(0.0) <= LIVE_FOR).then(|| got.clone())
    }

    pub fn live(&self, sid: &str, now: f64) -> bool {
        self.state(sid, now).is_some()
    }

    /// ModLink.offer: queue `text` for the session's mod; the prompt's id.
    pub fn offer(&self, sid: &str, text: &str, as_user: bool, now: f64) -> String {
        let mut raw = [0u8; 6];
        getrandom::getrandom(&mut raw).expect("random bytes");
        let id: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let item =
            json!({"id": id, "text": text, "as_user": as_user, "at": now, "claimed_at": null});
        let mut inner = self.inner.lk();
        inner
            .outbox
            .entry(sid.to_string())
            .or_default()
            .push(item.as_object().unwrap().clone());
        id
    }

    /// ModLink.pending(sid): offered and not yet settled.
    pub fn pending(&self, sid: &str) -> Vec<Obj> {
        self.inner
            .lk()
            .outbox
            .get(sid)
            .cloned()
            .unwrap_or_default()
    }

    #[allow(dead_code)] // the window's record of what became of them comes later
    pub fn settled(&self, sid: &str) -> Vec<Obj> {
        self.inner
            .lk()
            .settled
            .get(sid)
            .cloned()
            .unwrap_or_default()
    }
}

fn settle(inner: &mut Inner, sid: &str, ack: &Value, now: f64) {
    let Some(ack) = ack.as_object() else { return };
    let Some(box_) = inner.outbox.get_mut(sid) else {
        return;
    };
    let want = ack.get("id").unwrap_or(&Value::Null);
    let Some(pos) = box_.iter().position(|i| &i["id"] == want) else {
        return;
    };
    let mut item = match ack.get("outcome").and_then(Value::as_str) {
        Some("accepted") => {
            box_[pos].insert("accepted_at".into(), json!(now));
            return;
        }
        Some("submitted") => {
            let mut item = box_.remove(pos);
            item.insert("outcome".into(), json!("submitted"));
            item
        }
        Some("dropped") => {
            let mut item = box_.remove(pos);
            item.insert("outcome".into(), json!("dropped"));
            let reason = ack
                .get("reason")
                .filter(|v| truthy(Some(v)))
                .map(py_str)
                .unwrap_or_default();
            item.insert("reason".into(), json!(take(&reason, 300)));
            item
        }
        _ => return,
    };
    item.insert("settled_at".into(), json!(now));
    if box_.is_empty() {
        inner.outbox.shift_remove(sid);
    }
    let kept = inner.settled.entry(sid.to_string()).or_default();
    kept.push(item);
    let excess = kept.len().saturating_sub(SETTLED_KEEP);
    kept.drain(..excess);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_reads_what_fromisoformat_reads() {
        let e = |s: &str| epoch(Some(&json!(s)));
        assert_eq!(e("2100-01-01T00:00:00.000Z"), Some(4102444800.0));
        assert_eq!(e("2100-01-01T01:30:00+01:30"), Some(4102444800.0));
        assert_eq!(e("2100-01-01T00:00:00.25Z"), Some(4102444800.25));
        assert_eq!(e("2100-02-30T00:00:00Z"), None);
        assert_eq!(e("soon"), None);
        assert_eq!(e(""), None);
        assert_eq!(epoch(Some(&json!(5))), None);
    }

    #[test]
    fn a_prompt_is_handed_out_once_then_settles() {
        let m = ModLink::new("t".repeat(40));
        let id = m.offer("s", "hi", true, 0.0);
        let beat = |acks: Value, at: f64| {
            m.beat(
                json!({"session_id": "s", "acks": acks})
                    .as_object()
                    .unwrap(),
                at,
            )
        };
        assert_eq!(beat(json!([]), 1.0).len(), 1);
        assert!(beat(json!([]), 2.0).is_empty());
        assert_eq!(beat(json!([]), 40.0).len(), 1); // never accepted: again
        assert!(beat(json!([{"id": id, "outcome": "accepted"}]), 41.0).is_empty());
        assert!(beat(json!([]), 80.0).is_empty());
        beat(json!([{"id": id, "outcome": "submitted"}]), 81.0);
        assert!(m.pending("s").is_empty());
        assert_eq!(m.settled("s")[0]["outcome"], "submitted");
        assert!(
            m.authentic(Some(&"t".repeat(40))) && !m.authentic(Some("t")) && !m.authentic(None)
        );
    }
}

/// What test_modlink.py pinned down for the link itself.
#[cfg(test)]
mod python_tests {
    use super::*;

    fn temp(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("imp-modlink-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn obj(v: Value) -> Obj {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn the_token_is_made_once_and_kept() {
        let dir = temp("token");
        let first = ModLink::new(load_token(&dir).unwrap());
        let again = ModLink::new(load_token(&dir).unwrap());
        assert_eq!(again.token, first.token);
        assert!(first.token.chars().count() >= 32);
        assert!(first.authentic(Some(&first.token)));
        assert!(!first.authentic(Some("nope")));
        assert!(!first.authentic(None));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_beat_makes_a_mod_live_until_it_stops() {
        let link = ModLink::new("t".repeat(40));
        assert!(!link.live("s", 1000.0));
        link.beat(&obj(json!({"session_id": "s", "busy": true})), 1000.0);
        assert!(link.live("s", 1000.0));
        assert_eq!(link.state("s", 1000.0).unwrap()["busy"], true);
        assert!(!link.live("s", 1000.0 + LIVE_FOR + 1.0));
        assert!(link.state("s", 1000.0 + LIVE_FOR + 1.0).is_none());
    }

    #[test]
    fn a_prompt_is_handed_out_once_accepted() {
        let link = ModLink::new("t".repeat(40));
        let pid = link.offer("s", "do it", true, 1000.0);
        let beat = |acks: Value, at: f64| link.beat(&obj(json!({"session_id": "s", "acks": acks})), at);
        let got = beat(json!([]), 1000.0);
        assert_eq!(got, [json!({"id": pid, "text": "do it", "as_user": true})]);
        // Claimed: not handed out again on the next beat...
        assert!(beat(json!([]), 1001.0).is_empty());
        // ...unless the mod never accepted it (it reloaded mid-beat).
        assert_eq!(beat(json!([]), 1000.0 + CLAIM_FOR + 1.0).len(), 1);
        // Accepted: pending, never handed out again, however long it waits.
        beat(json!([{"id": pid, "outcome": "accepted"}]), 1040.0);
        assert!(beat(json!([]), 1040.0 + 10.0 * CLAIM_FOR).is_empty());
        assert_eq!(link.pending("s").len(), 1);
        // Its turn started: settled.
        beat(json!([{"id": pid, "outcome": "submitted"}]), 1400.0);
        assert!(link.pending("s").is_empty());
        assert_eq!(link.settled("s")[0]["outcome"], "submitted");
    }

    #[test]
    fn a_stop_is_handed_out_until_answered_and_never_late() {
        let link = ModLink::new("t".into());
        let id = link.request_stop("s", 100.0);
        assert_eq!(link.take_stop("s", 100.5).as_deref(), Some(id.as_str()));
        // Handed out: not again at once, again if unanswered.
        assert_eq!(link.take_stop("s", 101.0), None);
        assert_eq!(link.take_stop("s", 104.0).as_deref(), Some(id.as_str()));
        link.beat(&obj(json!({"session_id": "s", "stopped": [{"id": id, "outcome": "aborted"}]})), 105.0);
        assert_eq!(link.take_stop("s", 110.0), None);
        assert_eq!(link.stop_result("s").unwrap()["outcome"], "aborted");
        // Never taken in time: dropped, not handed out late.
        link.request_stop("s", 200.0);
        assert_eq!(link.take_stop("s", 211.0), None);
        assert_eq!(link.take_stop("s", 212.0), None);
    }

    #[test]
    fn only_a_new_enough_mod_can_stop() {
        assert!(can_stop(Some(&json!("0.3.0"))));
        assert!(can_stop(Some(&json!("1.0"))));
        assert!(!can_stop(Some(&json!("0.2.0"))));
        assert!(!can_stop(Some(&json!("x"))));
        assert!(!can_stop(None));
    }

    #[test]
    fn a_dropped_prompt_says_why() {
        let link = ModLink::new("t".repeat(40));
        let pid = link.offer("s", "x", false, 1000.0);
        link.beat(&obj(json!({"session_id": "s"})), 1000.0);
        link.beat(
            &obj(json!({"session_id": "s", "acks": [{"id": pid, "outcome": "dropped",
                "reason": "the session was stopped and nobody has sent input since"}]})),
            1001.0,
        );
        assert!(link.pending("s").is_empty());
        assert!(link.settled("s")[0]["reason"].as_str().unwrap().contains("stopped"));
        assert_eq!(link.settled("s")[0]["outcome"], "dropped");
    }

    #[test]
    fn the_settled_record_is_bounded() {
        let link = ModLink::new("t".repeat(40));
        for i in 0..(SETTLED_KEEP + 5) {
            let pid = link.offer("s", &format!("p{i}"), false, 1000.0);
            link.beat(&obj(json!({"session_id": "s", "acks": [{"id": pid, "outcome": "submitted"}]})), 1000.0);
        }
        assert_eq!(link.settled("s").len(), SETTLED_KEEP);
    }

    #[test]
    fn usage_becomes_a_statusline_payload() {
        let got = statusline_payload(&json!({
            "context": {"tokens": 5000, "window": 200000, "percent": 2.5},
            "rateLimits": [{"kind": "five_hour", "percentUsed": 40, "resetsAt": "2026-11-03T20:00:00Z"},
                           {"kind": "seven_day", "percentUsed": "junk"}],
            "cost": {"usd": 1.5}, "model": "Opus 5.5"}));
        assert_eq!(got["context_window"], json!({"used_percentage": 2.5, "context_window_size": 200000}));
        assert_eq!(got["cost"], json!({"total_cost_usd": 1.5}));
        assert_eq!(got["model"], json!({"display_name": "Opus 5.5"}));
        assert_eq!(got["rate_limits"]["five_hour"]["used_percentage"], 40);
        assert_eq!(got["rate_limits"]["five_hour"]["resets_at"].as_f64(), Some(1793736000.0));
        assert!(got["rate_limits"].get("seven_day").is_none());
        assert_eq!(statusline_payload(&Value::Null)["rate_limits"], json!({}));
    }

    fn beat_with(m: &ModLink, extra: Value, at: f64) -> Vec<Value> {
        let mut b = json!({"session_id": "s"});
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        m.beat(b.as_object().unwrap(), at)
    }

    #[test]
    fn ops_need_mod_0_4() {
        assert!(can_ops(Some(&json!("0.4.0"))));
        assert!(can_ops(Some(&json!("1.0"))));
        assert!(!can_ops(Some(&json!("0.3.1"))));
        assert!(!can_ops(None));
    }

    #[test]
    fn a_command_is_handed_out_once_and_its_output_comes_back() {
        let m = ModLink::new("t".repeat(40));
        let id = m.request_op("s", "command", json!({"command": "model", "args": "opus"}), 0.0);
        let given = m.take_ops("s", 1.0);
        assert_eq!(given, vec![json!({"id": id, "op": "command", "args": {"command": "model", "args": "opus"}})]);
        // Never again, even long after: run twice, it would act twice.
        assert!(m.take_ops("s", 20.0).is_empty());
        assert_eq!(m.result(&id), None);
        beat_with(&m, json!({"results": [{"id": id, "ok": true, "data": {"text": "Set model to Opus"}}]}), 21.0);
        assert_eq!(m.result(&id).unwrap()["data"]["text"], "Set model to Opus");
        assert_eq!(m.wait_result(&id, std::time::Duration::from_millis(1)).unwrap()["ok"], true);
    }

    #[test]
    fn a_read_is_handed_out_again_when_unanswered_and_dropped_late() {
        let m = ModLink::new("t".repeat(40));
        let id = m.request_op("s", "context", json!({}), 0.0);
        assert_eq!(m.take_ops("s", 0.0).len(), 1);
        assert!(m.take_ops("s", 1.0).is_empty());
        assert_eq!(m.take_ops("s", OP_CLAIM_FOR + 0.5).len(), 1);
        assert!(m.take_ops("s", OP_FOR + 1.0).is_empty());
        assert_eq!(m.result(&id).unwrap()["ok"], false);
    }

    #[test]
    fn a_waiting_request_wakes_when_the_answer_arrives() {
        let m = std::sync::Arc::new(ModLink::new("t".repeat(40)));
        let id = m.request_op("s", "status", json!({}), 0.0);
        let (m2, id2) = (std::sync::Arc::clone(&m), id.clone());
        let t = std::thread::spawn(move || m2.wait_result(&id2, std::time::Duration::from_secs(5)));
        std::thread::sleep(std::time::Duration::from_millis(50));
        beat_with(&m, json!({"results": [{"id": id, "data": {"model": "opus"}}]}), 1.0);
        let got = t.join().unwrap().unwrap();
        assert_eq!(got, json!({"ok": true, "data": {"model": "opus"}}));
    }

    #[test]
    fn the_command_list_is_kept_from_the_beat_that_sent_it() {
        let m = ModLink::new("t".repeat(40));
        beat_with(&m, json!({"commands": [{"name": "compact", "source": "builtin"}]}), 0.0);
        beat_with(&m, json!({}), 2.0);
        assert_eq!(m.commands("s").unwrap(), json!([{"name": "compact", "source": "builtin"}]));
    }
}
