//! The dispatcher's state file, written: a port of `imperatorium/store.py`.
//!
//! Used only when the binary is given a state path of its own (`--state`),
//! or is the dispatcher (`serve --live`); without either it is the
//! read-only shadow and nothing here runs. Only `--live` may open the live
//! state file: a test dispatcher that wrote it would fight the real one.

use crate::locks::LockExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Map, Value};

const MAX_EVENTS: usize = 400;

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub struct Store {
    pub path: PathBuf,
    data: Mutex<Map<String, Value>>,
}

fn empty() -> Map<String, Value> {
    json!({"tokens": {}, "events": [], "sessions": {}, "queue": {}, "prefs": {}, "dismissed": {}, "reviewed": {}})
        .as_object()
        .unwrap()
        .clone()
}

impl Store {
    /// Open (or start) the state at `path`. Err for the live state file.
    #[allow(dead_code)] // the tests; the server opens through open_as
    pub fn open(path: &Path) -> Result<Store, String> {
        Self::open_as(path, false)
    }

    /// `open`, or with `live` the live state file itself (`serve --live`).
    pub fn open_as(path: &Path, live_ok: bool) -> Result<Store, String> {
        let live = crate::paths::state_dir().join("state.json");
        let same = |a: &Path, b: &Path| {
            crate::paths::abs_normcase(&a.to_string_lossy())
                == crate::paths::abs_normcase(&b.to_string_lossy())
        };
        if !live_ok && same(path, &live) {
            return Err(format!(
                "{} is the live dispatcher's state; give this one its own",
                path.display()
            ));
        }
        let mut data = empty();
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(Value::Object(loaded)) = serde_json::from_str::<Value>(&text) {
                for key in [
                    "tokens",
                    "events",
                    "sessions",
                    "queue",
                    "prefs",
                    "dismissed",
                    "reviewed",
                ] {
                    if let Some(v) = loaded.get(key).filter(|v| v.is_object() || v.is_array()) {
                        data.insert(key.into(), v.clone());
                    }
                }
            }
        }
        Ok(Store {
            path: path.to_path_buf(),
            data: Mutex::new(data),
        })
    }

    /// A copy of everything, as `registry::state()` reads the file.
    pub fn snapshot(&self) -> Map<String, Value> {
        self.data.lk().clone()
    }

    /// Store._flush_locked: an atomic replace (`paths::write_atomic`); written
    /// in place rather than lose the state.
    fn flush(&self, data: &Map<String, Value>) {
        let dir = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let _ = std::fs::create_dir_all(&dir);
        let Ok(text) = serde_json::to_vec_pretty(data) else {
            return;
        };
        if crate::paths::write_atomic(&self.path, &text).is_ok() {
            return;
        }
        let _ = std::fs::write(&self.path, &text);
    }

    fn with<R>(&self, f: impl FnOnce(&mut Map<String, Value>) -> (R, bool)) -> R {
        let mut data = self.data.lk();
        let (r, dirty) = f(&mut data);
        if dirty {
            self.flush(&data);
        }
        r
    }

    fn table<'a>(data: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
        if !data.get(key).is_some_and(Value::is_object) {
            data.insert(key.into(), json!({}));
        }
        data.get_mut(key).unwrap().as_object_mut().unwrap()
    }

    fn record<'a>(data: &'a mut Map<String, Value>, sid: &str) -> &'a mut Map<String, Value> {
        let sessions = Self::table(data, "sessions");
        if !sessions.get(sid).is_some_and(Value::is_object) {
            sessions.insert(sid.into(), json!({}));
        }
        sessions.get_mut(sid).unwrap().as_object_mut().unwrap()
    }

    /// Store.register: merge `meta` (None values dropped) and the token.
    pub fn register(&self, sid: &str, token: Option<&Value>, meta: &Map<String, Value>) {
        if sid.is_empty() {
            return;
        }
        self.with(|data| {
            if let Some(t) = token.filter(|t| crate::pyish::truthy(Some(t))) {
                Self::table(data, "tokens").insert(sid.into(), t.clone());
            }
            let rec = Self::record(data, sid);
            for (k, v) in meta {
                if !v.is_null() {
                    rec.insert(k.clone(), v.clone());
                }
            }
            rec.insert("seen_at".into(), json!(now()));
            ((), true)
        })
    }

    /// Store.raise_attention: the clock starts once.
    pub fn raise_attention(&self, sid: &str, reason: &str, kind: &str, meta: &Map<String, Value>) {
        if sid.is_empty() {
            return;
        }
        self.with(|data| {
            let rec = Self::record(data, sid);
            for (k, v) in meta {
                if !v.is_null() {
                    rec.insert(k.clone(), v.clone());
                }
            }
            if !rec.contains_key("attention_since") {
                rec.insert("attention_since".into(), json!(now()));
            }
            rec.insert("attention_reason".into(), json!(reason));
            rec.insert("attention_kind".into(), json!(kind));
            rec.insert("seen_at".into(), json!(now()));
            ((), true)
        })
    }

    pub fn clear_attention(&self, sid: &str) {
        self.with(|data| {
            let Some(Value::Object(rec)) = Self::table(data, "sessions").get_mut(sid) else {
                return ((), false);
            };
            let mut dropped = false;
            for k in ["attention_since", "attention_reason", "attention_kind"] {
                if let Some(v) = rec.shift_remove(k) {
                    dropped |= !v.is_null();
                }
            }
            ((), dropped)
        })
    }

    pub fn forget(&self, sid: &str) {
        self.with(|data| {
            Self::table(data, "tokens").shift_remove(sid);
            Self::table(data, "sessions").shift_remove(sid);
            ((), true)
        })
    }

    pub fn session(&self, sid: &str) -> Option<Map<String, Value>> {
        self.data
            .lk()
            .get("sessions")?
            .get(sid)?
            .as_object()
            .cloned()
    }

    /// `add_event` for an event built with `json!({...})`.
    pub fn add_event_json(&self, event: Value) {
        let Value::Object(event) = event else {
            panic!("an event is a JSON object");
        };
        self.add_event(event);
    }

    pub fn add_event(&self, mut event: Map<String, Value>) {
        if !event.contains_key("at") {
            event.insert("at".into(), json!(now()));
        }
        self.with(|data| {
            if !data.get("events").is_some_and(Value::is_array) {
                data.insert("events".into(), json!([]));
            }
            let events = data.get_mut("events").unwrap().as_array_mut().unwrap();
            events.push(Value::Object(event));
            if events.len() > MAX_EVENTS {
                let cut = events.len() - MAX_EVENTS;
                events.drain(..cut);
            }
            ((), true)
        })
    }

    /// Store.dismiss: hide a tile with no process until it starts again;
    /// it leaves the archive too.
    /// Store.mark_reviewed: a waiting session you have looked at, and when.
    pub fn mark_reviewed(&self, sid: &str, at: f64) {
        if sid.is_empty() {
            return;
        }
        const KEEP: usize = 500;
        self.with(|data| {
            let seen = Self::table(data, "reviewed");
            seen.insert(sid.into(), json!(at));
            if seen.len() > KEEP {
                let mut by_age: Vec<(String, f64)> =
                    seen.iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0))).collect();
                by_age.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                for (k, _) in by_age.into_iter().take(seen.len() - KEEP) {
                    seen.shift_remove(&k);
                }
            }
            ((), true)
        })
    }

    pub fn reviewed(&self) -> Map<String, Value> {
        self.with(|data| (Self::table(data, "reviewed").clone(), false))
    }

    pub fn dismiss(&self, key: &str) {
        self.with(|data| {
            Self::table(data, "dismissed").insert(key.into(), json!(now()));
            let prefs = Self::table(data, "prefs");
            if let Some(Value::Array(archived)) = prefs.get("archived") {
                if archived.iter().any(|k| k.as_str() == Some(key)) {
                    let kept: Vec<Value> = archived
                        .iter()
                        .filter(|k| k.as_str() != Some(key))
                        .cloned()
                        .collect();
                    prefs.insert("archived".into(), Value::Array(kept));
                }
            }
            ((), true)
        })
    }

    /// Store.set_prefs: None removes a key.
    pub fn set_prefs(&self, changes: &Map<String, Value>) -> Value {
        self.with(|data| {
            let prefs = Self::table(data, "prefs");
            for (k, v) in changes {
                if v.is_null() {
                    prefs.shift_remove(k);
                } else {
                    prefs.insert(k.clone(), v.clone());
                }
            }
            (Value::Object(prefs.clone()), true)
        })
    }

    /// Store.enqueue: hold a prompt for a session. The id is random, not the
    /// clock, so two queued in one tick can be cancelled apart.
    pub fn enqueue(&self, sid: &str, prompt: &str, as_user: bool) -> Value {
        let mut raw = [0u8; 6];
        getrandom::getrandom(&mut raw).expect("random bytes");
        let id: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let mut item = json!({"id": id, "prompt": prompt, "at": now()});
        if as_user {
            item["as_user"] = json!(true);
        }
        self.with(|data| {
            let queue = Self::table(data, "queue");
            match queue.get_mut(sid) {
                Some(Value::Array(q)) => q.push(item.clone()),
                _ => {
                    queue.insert(sid.into(), json!([item.clone()]));
                }
            }
            (item, true)
        })
    }

    /// Store.queued(sid): one session's queue.
    pub fn queued(&self, sid: &str) -> Vec<Value> {
        self.snapshot()
            .get("queue")
            .and_then(|q| q.get(sid))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    /// Store.queued(): every non-empty queue's key, in order.
    pub fn queued_keys(&self) -> Vec<String> {
        self.snapshot()
            .get("queue")
            .and_then(Value::as_object)
            .map(|q| {
                q.iter()
                    .filter(|(_, v)| v.as_array().is_some_and(|a| !a.is_empty()))
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Store.take_queued: pop the oldest queued prompt.
    pub fn take_queued(&self, sid: &str) -> Option<Value> {
        self.with(|data| {
            let queue = Self::table(data, "queue");
            let Some(Value::Array(q)) = queue.get_mut(sid) else {
                return (None, false);
            };
            if q.is_empty() {
                return (None, false);
            }
            let item = q.remove(0);
            if q.is_empty() {
                queue.shift_remove(sid);
            }
            (Some(item), true)
        })
    }

    /// Store.requeue_front: put back a prompt whose delivery failed, saying why.
    pub fn requeue_front(&self, sid: &str, item: &Value, error: Option<&str>) {
        let mut item = item.clone();
        if let Some(e) = error.filter(|e| !e.is_empty()) {
            item["error"] = json!(e);
        }
        self.with(|data| {
            let queue = Self::table(data, "queue");
            match queue.get_mut(sid) {
                Some(Value::Array(q)) => q.insert(0, item),
                _ => {
                    queue.insert(sid.into(), json!([item]));
                }
            }
            ((), true)
        })
    }

    /// Store.unqueue: drop one queued prompt; whether it was there.
    pub fn unqueue(&self, sid: &str, item_id: &Value) -> bool {
        self.with(|data| {
            let queue = Self::table(data, "queue");
            let q = queue
                .get(sid)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let kept: Vec<Value> = q
                .iter()
                .filter(|i| i.get("id") != Some(item_id))
                .cloned()
                .collect();
            if kept.len() == q.len() {
                return (false, false);
            }
            if kept.is_empty() {
                queue.shift_remove(sid);
            } else {
                queue.insert(sid.into(), Value::Array(kept));
            }
            (true, true)
        })
    }

    pub fn undismiss(&self, key: &str) {
        self.with(|data| {
            let gone = Self::table(data, "dismissed")
                .shift_remove(key)
                .is_some_and(|v| !v.is_null());
            ((), gone)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_merges_and_attention_starts_once() {
        let dir = std::env::temp_dir().join(format!("imp-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = Store::open(&dir.join("state.json")).unwrap();
        let meta = |v: Value| v.as_object().unwrap().clone();
        s.register(
            "a",
            Some(&json!("tok")),
            &meta(json!({"cwd": "C:/w", "name": null})),
        );
        assert!(!s.session("a").unwrap().contains_key("name"));
        s.raise_attention("a", "needs permission", "notification", &Map::new());
        let first = s.session("a").unwrap()["attention_since"].clone();
        s.raise_attention("a", "again", "notification", &Map::new());
        assert_eq!(s.session("a").unwrap()["attention_since"], first);
        s.clear_attention("a");
        assert!(!s.session("a").unwrap().contains_key("attention_since"));
        let reread = Store::open(&dir.join("state.json")).unwrap().snapshot();
        assert_eq!(reread["tokens"]["a"], "tok");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_live_state_is_refused() {
        assert!(Store::open(&crate::paths::state_dir().join("state.json")).is_err());
    }
}

/// Ported from the Python suite (test_features.py `QueueStore`).
#[cfg(test)]
mod features_tests {
    use super::*;

    fn open(label: &str) -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("imp-featstore-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (Store::open(&dir.join("state.json")).unwrap(), dir)
    }

    fn prompts(q: &[Value]) -> Vec<String> {
        q.iter().map(|i| i["prompt"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn features_the_queue_is_fifo_and_an_empty_one_leaves_the_map() {
        let (s, dir) = open("fifo");
        assert!(s.take_queued("nobody").is_none());
        s.enqueue("s1", "first", false);
        s.enqueue("s1", "second", false);
        assert_eq!(prompts(&s.queued("s1")), ["first", "second"]);
        assert_eq!(s.take_queued("s1").unwrap()["prompt"], "first");
        assert_eq!(s.queued("s1").len(), 1);
        s.take_queued("s1");
        assert_eq!(s.snapshot()["queue"], json!({}));
        assert!(s.queued_keys().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn features_requeue_front_puts_it_back_first_with_the_error() {
        let (s, dir) = open("requeue");
        s.enqueue("s1", "first", false);
        let item = s.take_queued("s1").unwrap();
        s.enqueue("s1", "second", false);
        s.requeue_front("s1", &item, Some("delivery failed"));
        let got = s.queued("s1");
        assert_eq!(got[0]["prompt"], "first");
        assert_eq!(got[0]["error"], "delivery failed");
        assert_eq!(got[1]["prompt"], "second");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn features_ids_are_unique_so_unqueue_removes_one_item_by_id() {
        let (s, dir) = open("unqueue");
        // Queued back to back, as in one clock tick.
        let a = s.enqueue("s1", "x", false);
        let b = s.enqueue("s1", "y", false);
        assert_ne!(a["id"], b["id"]);
        assert!(s.unqueue("s1", &a["id"]));
        assert_eq!(prompts(&s.queued("s1")), ["y"]);
        assert!(!s.unqueue("s1", &json!("no-such-id")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn features_the_queue_survives_a_reload_and_prefs_round_trip() {
        let (s, dir) = open("reload");
        s.enqueue("s1", "persisted", false);
        let changes = |v: Value| v.as_object().unwrap().clone();
        s.set_prefs(&changes(json!({"pinned": ["a", "b"], "group": "x"})));
        assert_eq!(s.snapshot()["prefs"], json!({"pinned": ["a", "b"], "group": "x"}));
        s.set_prefs(&changes(json!({"group": null})));
        assert_eq!(s.snapshot()["prefs"], json!({"pinned": ["a", "b"]}));
        let reloaded = Store::open(&dir.join("state.json")).unwrap();
        assert_eq!(reloaded.queued("s1")[0]["prompt"], "persisted");
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// test_core.py's StoreState, Reviewed.test_store_keeps_when and
/// AttentionBookkeeping.
#[cfg(test)]
mod core_tests {
    use super::*;

    struct Dir(PathBuf);
    impl Dir {
        fn new(label: &str) -> Dir {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let d = std::env::temp_dir().join(format!("imp-store-core-{label}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Dir(d)
        }
        fn state(&self) -> PathBuf {
            self.0.join("state.json")
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn meta(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn reviewed_keeps_when() {
        let d = Dir::new("reviewed");
        let s = Store::open(&d.state()).unwrap();
        s.mark_reviewed("a", 123.0);
        let again = Store::open(&d.state()).unwrap();
        assert_eq!(again.reviewed()["a"], json!(123.0));
        assert_eq!(again.reviewed().len(), 1);
    }

    #[test]
    fn register_merges_and_ignores_none() {
        let d = Dir::new("register");
        let s = Store::open(&d.state()).unwrap();
        s.register("sid", Some(&json!("t")), &meta(json!({"cwd": "C:/x", "name": "first"})));
        s.register("sid", None, &meta(json!({"cwd": null, "name": "second"})));
        let rec = s.session("sid").unwrap();
        assert_eq!(rec["cwd"], "C:/x");
        assert_eq!(rec["name"], "second");
        assert_eq!(s.snapshot()["tokens"]["sid"], "t");
    }

    #[test]
    fn clear_attention_removes_the_key_and_tolerates_strangers() {
        let d = Dir::new("clear");
        let s = Store::open(&d.state()).unwrap();
        s.register("sid", None, &meta(json!({"attention_since": 123.0})));
        s.clear_attention("sid");
        assert!(!s.session("sid").unwrap().contains_key("attention_since"));
        s.clear_attention("unknown-sid"); // must not panic
        assert!(s.session("unknown-sid").is_none());
    }

    #[test]
    fn forget_drops_token_and_record() {
        let d = Dir::new("forget");
        let s = Store::open(&d.state()).unwrap();
        s.register("sid", Some(&json!("t")), &Map::new());
        s.forget("sid");
        let snap = s.snapshot();
        assert!(snap["tokens"].as_object().unwrap().is_empty());
        assert!(snap["sessions"].as_object().unwrap().is_empty());
        // and the file says so too
        let reread = Store::open(&d.state()).unwrap().snapshot();
        assert!(reread["tokens"].as_object().unwrap().is_empty());
    }

    #[test]
    fn events_are_capped() {
        let d = Dir::new("events");
        let s = Store::open(&d.state()).unwrap();
        for i in 0..520 {
            s.add_event_json(json!({"kind": format!("e{i}")}));
        }
        let events = s.snapshot()["events"].as_array().unwrap().clone();
        assert_eq!(events.len(), 400);
        // Stored oldest first (the API reverses them): the newest is last.
        assert_eq!(events.last().unwrap()["kind"], "e519");
        assert_eq!(events[0]["kind"], "e120");
    }

    #[test]
    fn a_corrupt_state_file_does_not_crash_startup() {
        let d = Dir::new("corrupt");
        std::fs::write(d.state(), "{not json").unwrap();
        let s = Store::open(&d.state()).unwrap();
        assert!(s.snapshot()["tokens"].as_object().unwrap().is_empty());
        // and it is usable afterwards
        s.register("a", Some(&json!("t")), &Map::new());
        assert_eq!(Store::open(&d.state()).unwrap().snapshot()["tokens"]["a"], "t");
    }

    #[test]
    fn repeated_failures_do_not_restart_the_clock() {
        // Three 529s in four minutes: the age on the tile has to mean
        // "stuck since", not "failed again just now".
        let d = Dir::new("failures");
        let s = Store::open(&d.state()).unwrap();
        s.raise_attention("sid", "API overloaded - wait and retry", "failure", &Map::new());
        let first = s.session("sid").unwrap()["attention_since"].clone();
        std::thread::sleep(std::time::Duration::from_millis(5));
        s.raise_attention("sid", "API unavailable - retry", "failure", &Map::new());
        let rec = s.session("sid").unwrap();
        assert_eq!(rec["attention_since"], first);
        assert_eq!(rec["attention_reason"], "API unavailable - retry");
        assert_eq!(rec["attention_kind"], "failure");
    }

    #[test]
    fn clearing_drops_the_reason_with_the_flag() {
        let d = Dir::new("clearing");
        let s = Store::open(&d.state()).unwrap();
        s.raise_attention("sid", "API overloaded - wait and retry", "failure", &Map::new());
        s.clear_attention("sid");
        let rec = s.session("sid").unwrap();
        for key in ["attention_since", "attention_reason", "attention_kind"] {
            assert!(!rec.contains_key(key), "{key}");
        }
    }

    #[test]
    fn clearing_keeps_everything_else() {
        let d = Dir::new("keeps");
        let s = Store::open(&d.state()).unwrap();
        s.raise_attention("sid", "why", "failure", &meta(json!({"cwd": "Y:/x"})));
        s.clear_attention("sid");
        assert_eq!(s.session("sid").unwrap()["cwd"], "Y:/x");
    }
}
