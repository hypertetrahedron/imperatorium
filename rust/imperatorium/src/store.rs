//! The dispatcher's state file, written: a port of `imperatorium/store.py`.
//!
//! Used only when the binary is given a state path of its own (`--state`);
//! without one it is the read-only shadow and nothing here runs. A store
//! refuses the live state file outright: until the cutover that file
//! belongs to the Python dispatcher.

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
    json!({"tokens": {}, "events": [], "sessions": {}, "queue": {}, "prefs": {}, "dismissed": {}})
        .as_object()
        .unwrap()
        .clone()
}

impl Store {
    /// Open (or start) the state at `path`. Err for the live state file.
    pub fn open(path: &Path) -> Result<Store, String> {
        let live = crate::paths::state_dir().join("state.json");
        let same = |a: &Path, b: &Path| {
            crate::paths::abs_normcase(&a.to_string_lossy())
                == crate::paths::abs_normcase(&b.to_string_lossy())
        };
        if same(path, &live) {
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
        self.data.lock().unwrap().clone()
    }

    /// Store._flush_locked: a temporary file beside it, then an atomic
    /// replace, retried; written in place rather than lose the state.
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
        let tmp = dir.join(format!(".state-{}.tmp", std::process::id()));
        if std::fs::write(&tmp, &text).is_ok() {
            for attempt in 0..5 {
                if std::fs::rename(&tmp, &self.path).is_ok() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
            }
            let _ = std::fs::remove_file(&tmp);
        }
        let _ = std::fs::write(&self.path, &text);
    }

    fn with<R>(&self, f: impl FnOnce(&mut Map<String, Value>) -> (R, bool)) -> R {
        let mut data = self.data.lock().unwrap();
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
            .lock()
            .unwrap()
            .get("sessions")?
            .get(sid)?
            .as_object()
            .cloned()
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
