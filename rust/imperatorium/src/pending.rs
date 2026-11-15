//! Permission decisions waiting for a human, from `imperatorium/pending.py`.
//!
//! The permission hook parks an `ask` here and blocks; the window answers.
//! A park that expires is a silent "ask": the terminal prompts as it would
//! have without any of this, so never answering is always safe.

use crate::locks::LockExt;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::pyish::{py_str, strip, take, truthy};

/// Long-poll cap; the hook's own timeout is the real deadline.
pub const MAX_HOLD: f64 = 600.0;
pub const QUESTION_TOOL: &str = "AskUserQuestion";
/// Releases the hook without deciding: the terminal prompts at once.
pub const PASS: &str = "pass";

#[derive(Default)]
pub struct Pending {
    items: Mutex<IndexMap<String, Map<String, Value>>>,
    changed: Condvar,
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn new_id() -> String {
    let mut raw = [0u8; 6];
    getrandom::getrandom(&mut raw).expect("random bytes");
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

impl Pending {
    /// Pending.park: register and wait up to `wait` seconds for a decision.
    /// (verdict, decided_by, answers); the verdict is None if nobody answered.
    pub fn park(&self, request: &Map<String, Value>, wait: f64) -> (Value, Value, Value) {
        let id = new_id();
        let get = |k: &str| request.get(k).cloned().unwrap_or(Value::Null);
        let mut item = json!({
            "id": id, "at": now(), "session_id": get("session_id"), "cwd": get("cwd"),
            "tool": get("tool_name"), "input": summarise(request.get("tool_input")),
            "reason": get("_reason"), "verdict": null, "decided_by": null,
        })
        .as_object()
        .unwrap()
        .clone();
        if item["tool"].as_str() == Some(QUESTION_TOOL) {
            if let Some(qs) = questions(request) {
                let joined: Vec<String> = qs
                    .iter()
                    .map(|q| q["question"].as_str().unwrap_or("").to_string())
                    .collect();
                item.insert("questions".into(), Value::Array(qs));
                item.insert("input".into(), json!(take(&joined.join(" / "), 400)));
            }
        }
        let mut items = self.items.lk();
        items.insert(id.clone(), item);
        self.changed.notify_all();
        // Python's min(max(nan, 0), 600) is nan, and a nan deadline is
        // already past: no wait.
        let hold = if wait.is_nan() {
            0.0
        } else {
            wait.clamp(0.0, MAX_HOLD)
        };
        let deadline = Instant::now() + Duration::from_secs_f64(crate::timing::secs(hold));
        loop {
            let decided = items.get(&id).is_some_and(|i| !i["verdict"].is_null());
            let left = deadline.saturating_duration_since(Instant::now());
            if decided || left.is_zero() {
                break;
            }
            items = self.changed.wait_timeout(items, left).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
        let item = items.get(&id).cloned().unwrap_or_default();
        let mut verdict = item.get("verdict").cloned().unwrap_or(Value::Null);
        if verdict.as_str() == Some(PASS) {
            verdict = Value::Null;
        }
        if verdict.is_null() {
            // Expired or passed: the window stops offering it.
            items.shift_remove(&id);
        }
        let who = item.get("decided_by").cloned().unwrap_or(Value::Null);
        let answers = item.get("answers").cloned().unwrap_or(Value::Null);
        (verdict, who, answers)
    }

    /// Pending.decide: whether the decision was taken.
    pub fn decide(&self, id: &Value, verdict: &Value, who: Value, answers: Option<&Value>) -> bool {
        let Some(v) = verdict
            .as_str()
            .filter(|v| ["allow", "deny", PASS].contains(v))
        else {
            return false;
        };
        let Some(id) = id.as_str() else { return false };
        let mut items = self.items.lk();
        let Some(item) = items.get_mut(id) else {
            return false;
        };
        if !item["verdict"].is_null() {
            return false;
        }
        if item["tool"].as_str() == Some(QUESTION_TOOL) && v == "allow" {
            // A question allowed without its answers would run on nothing.
            let Some(checked) = checked_answers(item.get("questions"), answers) else {
                return false;
            };
            item.insert("answers".into(), checked);
        }
        item.insert("verdict".into(), json!(v));
        item.insert("decided_by".into(), who);
        self.changed.notify_all();
        true
    }

    /// Pending.waiting: everything still awaiting an answer, oldest first.
    pub fn waiting(&self) -> Vec<Value> {
        let items = self.items.lk();
        let mut out: Vec<Value> = items
            .values()
            .filter(|i| i["verdict"].is_null())
            .map(|i| Value::Object(i.clone()))
            .collect();
        out.sort_by(|a, b| {
            let at = |v: &Value| v["at"].as_f64().unwrap_or(0.0);
            at(a)
                .partial_cmp(&at(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }
}

/// pending._summarise: the one line a person needs to decide.
fn summarise(input: Option<&Value>) -> String {
    let Some(Value::Object(input)) = input else {
        return String::new();
    };
    for key in ["command", "file_path", "path", "pattern", "url"] {
        if let Some(v) = input.get(key).filter(|v| truthy(Some(v))) {
            return take(&py_str(v), 400);
        }
    }
    String::new()
}

fn text_or_empty(v: Option<&Value>) -> String {
    match v {
        Some(v) if truthy(Some(v)) => py_str(v),
        _ => String::new(),
    }
}

/// pending._questions: an AskUserQuestion's questions in the window's shape,
/// or None when the input is not what the tool takes.
pub fn questions(request: &Map<String, Value>) -> Option<Vec<Value>> {
    let raw = request
        .get("tool_input")?
        .as_object()?
        .get("questions")?
        .as_array()?;
    if raw.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for q in raw {
        let q = q.as_object()?;
        if strip(&text_or_empty(q.get("question"))).is_empty() {
            return None;
        }
        let options: Vec<Value> = q
            .get("options")
            .filter(|v| truthy(Some(v)))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .map(|o| {
                json!({"label": text_or_empty(o.get("label")),
                            "description": text_or_empty(o.get("description"))})
            })
            .filter(|o| !o["label"].as_str().unwrap_or("").is_empty())
            .collect();
        out.push(json!({
            "question": py_str(&q["question"]),
            "header": text_or_empty(q.get("header")),
            "multiSelect": truthy(q.get("multiSelect")),
            "options": options,
        }));
    }
    Some(out)
}

/// pending._checked_answers: {question: answer}, one non-empty string each.
fn checked_answers(questions: Option<&Value>, answers: Option<&Value>) -> Option<Value> {
    let qs = questions?.as_array().filter(|q| !q.is_empty())?;
    let answers = answers?.as_object()?;
    let mut out = Map::new();
    for q in qs {
        let text = q["question"].as_str().unwrap_or("");
        let value = answers.get(text)?.as_str()?;
        if strip(value).is_empty() {
            return None;
        }
        out.insert(text.into(), json!(take(strip(value), 2000)));
    }
    Some(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn req(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn expires_quietly() {
        let p = Pending::default();
        let got = p.park(
            &req(json!({"tool_name": "Bash", "tool_input": {"command": "ls"}})),
            0.05,
        );
        assert_eq!(got, (Value::Null, Value::Null, Value::Null));
        assert!(p.waiting().is_empty());
    }

    #[test]
    fn a_question_needs_every_answer() {
        let p = Arc::new(Pending::default());
        let q = req(
            json!({"tool_name": QUESTION_TOOL, "tool_input": {"questions": [
            {"question": "Which?", "options": [{"label": "A"}, {"label": ""}, 3]}]}}),
        );
        let parked = Arc::clone(&p);
        let t = std::thread::spawn(move || parked.park(&q, 5.0));
        while p.waiting().is_empty() {
            std::thread::sleep(Duration::from_millis(5));
        }
        let item = p.waiting()[0].clone();
        assert_eq!(item["input"], "Which?");
        assert_eq!(
            item["questions"][0]["options"],
            json!([{"label": "A", "description": ""}])
        );
        assert!(!p.decide(&item["id"], &json!("allow"), json!("window"), None));
        assert!(!p.decide(
            &item["id"],
            &json!("allow"),
            json!("window"),
            Some(&json!({"Which?": " "}))
        ));
        assert!(p.decide(
            &item["id"],
            &json!("allow"),
            json!("deck"),
            Some(&json!({"Which?": " A "}))
        ));
        assert!(!p.decide(&item["id"], &json!("deny"), json!("window"), None));
        assert_eq!(
            t.join().unwrap(),
            (json!("allow"), json!("deck"), json!({"Which?": "A"}))
        );
    }
}
