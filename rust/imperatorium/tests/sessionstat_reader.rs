//! test_sessionstat.py's `ReaderTests`, through the dispatcher: the status
//! panel's incremental reader over a real transcript folder
//! (GET /api/session_status), since the reader finds files through
//! CLAUDE_CONFIG_DIR and a unit test would have to change the whole
//! process's environment.
#![cfg(windows)]

mod common;
use common::*;
use serde_json::{json, Value};
use std::io::Write;
use std::time::Duration;

const SID: &str = "0f0f0f0f-0000-4000-8000-000000000001";

fn line(prompt: &str) -> String {
    format!("{}\n", json!({"type": "user", "promptId": prompt, "message": {"content": "x"}}))
}

fn write(path: &std::path::Path, text: &str, append: bool) {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

fn status(d: &Dispatcher, sid: &str) -> Value {
    let (code, v) = d.get(&format!("/api/session_status?session={sid}"));
    assert_eq!(code, 200);
    v
}

fn register(d: &Dispatcher, sid: &str) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    d.session_record(sid, json!({"cwd": d.root.join("work").to_string_lossy(), "startedAt": now_ms,
                                 "status": "idle", "kind": "interactive"}));
    // The row is listed once the dispatcher has looked at the folder.
    wait_until(Duration::from_secs(20), || {
        (status(d, sid)["ok"] == json!(true)).then_some(())
    })
    .unwrap_or_else(|| panic!("session {sid} never listed: {}", status(d, sid)));
}

#[test]
fn appends_are_read_and_a_half_written_line_waits() {
    let d = Dispatcher::start(Options::new());
    let dir = d.claude.join("projects").join("Y--x");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{SID}.jsonl"));
    write(&path, &line("p1"), false);
    register(&d, SID);
    let prompts = |d: &Dispatcher| status(d, SID)["stat"]["prompts"].clone();
    assert_eq!(prompts(&d), 1);
    let half = line("p2");
    write(&path, &half[..10], true);
    assert_eq!(prompts(&d), 1);
    write(&path, &half[10..], true);
    assert_eq!(prompts(&d), 2);
    write(&path, &line("p9"), false); // rewritten shorter: start again
    assert_eq!(prompts(&d), 1);
}

#[test]
fn unknown_or_malformed_id() {
    let d = Dispatcher::start(Options::new());
    let unknown = "0f0f0f0f-0000-4000-8000-00000000dead";
    register(&d, unknown);
    let got = status(&d, unknown);
    assert_eq!(got["stat"], json!({}), "a session with no transcript has no numbers: {got}");
    // Not a session id at all: no such session, and nothing outside the
    // projects folder is looked at.
    assert_eq!(status(&d, "..%2F..%2Fetc%2Fpasswd")["ok"], false);
}
