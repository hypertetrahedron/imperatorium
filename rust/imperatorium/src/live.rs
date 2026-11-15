//! Per-session tool events from hooks, newest last, bounded: a port of
//! `activity.Live`. In memory only, as in Python.

use crate::locks::LockExt;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use crate::pyish::{py_str, take, truthy};

const MAX_EVENTS: usize = 200;
pub const TOOL_EVENTS: &[&str] = &["PreToolUse", "PostToolUse", "PostToolUseFailure"];

/// activity.summarise_input, the hooks' version.
fn summarise_input(input: Option<&Value>) -> String {
    let Some(Value::Object(ti)) = input else {
        let empty = json!("");
        return take(
            &py_str(if truthy(input) {
                input.unwrap()
            } else {
                &empty
            }),
            200,
        );
    };
    for key in [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "prompt",
        "query",
        "description",
    ] {
        if let Some(v) = ti.get(key).filter(|v| truthy(Some(v))) {
            return take(&py_str(v), 200);
        }
    }
    let mut keys: Vec<&String> = ti.keys().collect();
    keys.sort();
    keys.iter()
        .take(6)
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Default)]
pub struct Live {
    events: Mutex<HashMap<String, VecDeque<Map<String, Value>>>>,
}

impl Live {
    /// Live.ingest: True when the event was a tool event (taken or skipped).
    pub fn ingest(&self, key: &str, payload: &Map<String, Value>, now: f64) -> bool {
        let event = payload
            .get("hook_event_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !TOOL_EVENTS.contains(&event) {
            return false;
        }
        // A subagent's calls are its own story.
        if truthy(payload.get("agent_id")) {
            return true;
        }
        let tid = payload.get("tool_use_id").cloned().unwrap_or(Value::Null);
        let mut all = self.events.lk();
        let events = all.entry(key.to_string()).or_default();
        let fresh = |start: Value| {
            json!({"id": tid, "tool": payload.get("tool_name").cloned().unwrap_or(Value::Null),
                   "summary": summarise_input(payload.get("tool_input")),
                   "start": start, "end": null, "ok": null, "error": null})
            .as_object()
            .unwrap()
            .clone()
        };
        if event == "PreToolUse" {
            events.push_back(fresh(json!(now)));
            while events.len() > MAX_EVENTS {
                events.pop_front();
            }
            return true;
        }
        let at = match events.iter().rposition(|e| e.get("id") == Some(&tid)) {
            Some(i) => i,
            None => {
                // The Pre was missed (dispatcher restarted mid-call).
                events.push_back(fresh(Value::Null));
                while events.len() > MAX_EVENTS {
                    events.pop_front();
                }
                events.len() - 1
            }
        };
        let e = &mut events[at];
        e.insert("end".into(), json!(now));
        let ok = event == "PostToolUse";
        e.insert("ok".into(), json!(ok));
        if !ok {
            let src = [payload.get("error"), payload.get("tool_response")]
                .into_iter()
                .flatten()
                .find(|v| truthy(Some(v)));
            e.insert(
                "error".into(),
                json!(crate::activity::first_line_n(src, 200)),
            );
        }
        true
    }

    pub fn events(&self, key: &str) -> Vec<Value> {
        self.events
            .lk()
            .get(key)
            .map(|q| q.iter().cloned().map(Value::Object).collect())
            .unwrap_or_default()
    }

    /// Live.current: the call still running, or None.
    pub fn current(&self, key: &str, now: f64) -> Value {
        let all = self.events.lk();
        let Some(last) = all.get(key).and_then(|q| q.back()) else {
            return Value::Null;
        };
        if !last.get("end").is_none_or(Value::is_null) {
            return Value::Null;
        }
        let Some(start) = last.get("start").and_then(Value::as_f64) else {
            return Value::Null;
        };
        // A Pre with no Post for ten minutes is a lost Post, not a long call.
        if now - start > 600.0 {
            return Value::Null;
        }
        json!({"tool": last.get("tool").cloned().unwrap_or(Value::Null),
               "summary": last.get("summary").cloned().unwrap_or(Value::Null), "since": start})
    }

    pub fn forget(&self, key: &str) {
        self.events.lk().remove(key);
    }
}

/// Ported from the Python suite (test_features.py `Live`).
#[cfg(test)]
mod features_tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn pre(input: Value, extra: Value) -> Map<String, Value> {
        let mut m = obj(json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
                               "tool_input": input, "tool_use_id": "t1"}));
        for (k, v) in extra.as_object().into_iter().flatten() {
            m.insert(k.clone(), v.clone());
        }
        m
    }

    #[test]
    fn features_pre_then_post_pairs_into_one_finished_call() {
        let live = Live::default();
        live.ingest("k", &pre(json!({"command": "npm test"}), json!({})), 100.0);
        live.ingest(
            "k",
            &obj(json!({"hook_event_name": "PostToolUse", "tool_use_id": "t1"})),
            105.0,
        );
        let events = live.events("k");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["ok"], true);
        assert_eq!(live.current("k", 110.0), Value::Null);
    }

    #[test]
    fn features_a_running_call_is_current_until_it_ends_or_goes_stale() {
        let live = Live::default();
        live.ingest("k", &pre(json!({"command": "npm test"}), json!({})), 100.0);
        let cur = live.current("k", 105.0);
        assert_eq!(cur["tool"], "Bash");
        assert_eq!(cur["since"], 100.0);
        // Stuck for ten minutes: a lost Post, not a long call.
        assert_eq!(live.current("k", 100.0 + 601.0), Value::Null);
    }

    #[test]
    fn features_a_failure_event_is_recorded_not_ok() {
        let live = Live::default();
        live.ingest("k", &pre(json!({}), json!({})), 1.0);
        live.ingest(
            "k",
            &obj(json!({"hook_event_name": "PostToolUseFailure", "tool_use_id": "t1",
                        "error": "boom\nmore"})),
            2.0,
        );
        let e = &live.events("k")[0];
        assert_eq!(e["ok"], false);
        assert_eq!(e["error"], "boom");
    }

    #[test]
    fn features_a_subagents_calls_a_non_tool_event_and_forget() {
        let live = Live::default();
        let got = live.ingest("k", &pre(json!({}), json!({"agent_id": "sub1"})), 1.0);
        assert!(got);
        assert!(live.events("k").is_empty());
        assert!(!live.ingest("k", &obj(json!({"hook_event_name": "Stop"})), 1.0));
        live.ingest("k", &pre(json!({}), json!({})), 1.0);
        assert_eq!(live.events("k").len(), 1);
        live.forget("k");
        assert!(live.events("k").is_empty());
    }
}
