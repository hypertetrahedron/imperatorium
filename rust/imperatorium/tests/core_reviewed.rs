//! Port of test_core.py's Reviewed.test_mark_reviewed_refuses_work_in_progress
//! through POST /api/reviewed: a session that waits for you can be marked as
//! looked at; one still working, or waiting on its own tasks, cannot.
#![cfg(windows)]
mod common;
use common::*;
use serde_json::{json, Value};
use std::time::Duration;

const DONE: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const BUSY: &str = "aaaaaaaa-0000-4000-8000-000000000002";

fn row(d: &Dispatcher, sid: &str) -> Value {
    let (code, body) = d.get("/api/sessions");
    assert_eq!(code, 200);
    body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["sessionId"] == sid)
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn reviewed_refuses_work_in_progress() {
    let d = Dispatcher::start(Options::new());
    // A finished turn: idle, and its Stop hook has arrived.
    d.session_record(DONE, json!({"status": "idle", "name": "done-one", "cwd": r"C:\work\core"}));
    assert_eq!(
        d.post("/hook", &json!({"hook_event_name": "Stop", "session_id": DONE, "cwd": r"C:\work\core"})).0,
        200
    );
    // One that is waiting on a permission: the person has not even seen it.
    d.session_record(BUSY, json!({"status": "waiting", "name": "busy-one", "cwd": r"C:\work\core"}));
    let done = wait_until(Duration::from_secs(10), || {
        Some(row(&d, DONE)).filter(|r| r["attention"] == true)
    })
    .unwrap_or_else(|| panic!("never waiting: {}", row(&d, DONE)));
    assert_eq!(done["attention_kind"], "complete");
    assert_eq!(row(&d, BUSY)["attention"], true);

    // Work in progress is not marked, and says why.
    let (code, got) = d.post("/api/reviewed", &json!({"session_id": BUSY}));
    assert_eq!(code, 200);
    assert_eq!((got["reviewed"].clone(), got["in_progress"].clone()), (json!(false), json!(true)), "{got}");
    assert!(d.state_json()["reviewed"].get(BUSY).is_none());
    assert_eq!(row(&d, BUSY)["attention"], true);

    // A finished one is.
    let (code, got) = d.post("/api/reviewed", &json!({"session_id": DONE}));
    assert_eq!((code, got["reviewed"].clone()), (200, json!(true)), "{got}");
    assert!(d.state_json()["reviewed"].get(DONE).is_some());
    let after = row(&d, DONE);
    assert_eq!((after["attention"].clone(), after["reviewed"].clone()), (json!(false), json!(true)));

    // A session nobody has heard of is not a crash.
    let (_, got) = d.post("/api/reviewed", &json!({"session_id": "nope"}));
    assert_eq!(got["ok"], false);
}
