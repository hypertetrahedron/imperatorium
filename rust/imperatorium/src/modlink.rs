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
/// A mod silent this long is gone; its prompts go the old way.
pub const LIVE_FOR: f64 = 8.0;
/// A prompt handed out and never accepted is offered again after this.
pub const CLAIM_FOR: f64 = 30.0;
pub const SETTLED_KEEP: usize = 20;

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
}

pub struct ModLink {
    pub token: String,
    inner: Mutex<Inner>,
}

impl ModLink {
    pub fn new(token: String) -> Self {
        ModLink {
            token,
            inner: Mutex::default(),
        }
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
}
