//! A tile's ✕ (2026-11-23, user report): closing a background session
//! killed its process, Claude Code's supervisor started it again some 20 s
//! later, and the tile stayed, offering Resume - a second ✕ brought it back
//! to life. Now a `claude --bg` session is stopped with `claude stop <id>`
//! (which the supervisor honours), and every ✕ that ends a session takes
//! its tile off the list. Also: a background session can start with no
//! first message (`claude --bg` with no prompt waits idle).
//!
//! `claude` here is the agents stub (tests/common/dispatch.rs): it lists
//! agents.json and logs every call to stub.log, so `claude stop` is seen
//! without a real session.
#![cfg(windows)]
mod common;
use common::dispatch::*;
use serde_json::{json, Value};

fn bg_row(rig: &Rig, state: &str, pid: Option<u32>) -> Value {
    let mut row = json!({"sessionId": SID, "id": "b1abcde1", "kind": "background", "state": state,
                         "status": "idle", "cwd": rig.cwd, "name": "bg-job", "key": SID});
    if let Some(p) = pid {
        row["pid"] = json!(p);
    }
    row
}

fn listed(rig: &Rig) -> bool {
    rig.invalidate();
    rig.rows().iter().any(|r| r["sessionId"] == SID)
}

fn stops(rig: &Rig) -> Vec<String> {
    std::fs::read_to_string(rig.d.bin_dir.join("stub.log"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("stop "))
        .map(str::to_string)
        .collect()
}

#[test]
fn closing_a_background_session_stops_it_rather_than_killing_it() {
    let rig = Rig::with_agents();
    // Running, with this test's own pid: a kill would end the test.
    rig.agents(json!([bg_row(&rig, "blocked", Some(std::process::id()))]));
    assert!(listed(&rig));
    let (_, got) = rig.post("/api/close", &json!({"session_id": SID}));
    assert_eq!(got, json!({"ok": true, "how": "stopped"}), "{got}");
    assert_eq!(stops(&rig), vec!["stop b1abcde1"]);
    assert_eq!(rig.events()[0]["kind"], "close");
    assert_eq!(rig.events()[0]["summary"], "stopped");
    // Claude Code now lists it stopped, with no process: its tile is gone.
    rig.agents(json!([bg_row(&rig, "stopped", None)]));
    assert!(!listed(&rig), "the tile stayed after the ✕");
    assert!(rig.state_json()["dismissed"].get(SID).is_some());
}

#[test]
fn a_session_started_again_comes_back() {
    // The list hides what was closed only until it has a process again
    // (resumed from History, say).
    let rig = Rig::with_agents();
    rig.agents(json!([bg_row(&rig, "blocked", Some(std::process::id()))]));
    rig.post("/api/close", &json!({"session_id": SID}));
    rig.agents(json!([bg_row(&rig, "stopped", None)]));
    assert!(!listed(&rig));
    rig.agents(json!([bg_row(&rig, "blocked", Some(std::process::id()))]));
    assert!(listed(&rig));
}

#[test]
fn removing_a_background_tile_between_processes_stops_it_first() {
    // No process at this moment but not stopped: its supervisor may be
    // starting it again, so it is stopped before it leaves the list.
    let rig = Rig::with_agents();
    rig.agents(json!([bg_row(&rig, "blocked", None)]));
    let (_, got) = rig.post("/api/dismiss", &json!({"session_id": SID}));
    assert_eq!(got, json!({"ok": true, "how": "dismissed"}), "{got}");
    assert_eq!(stops(&rig), vec!["stop b1abcde1"]);
    assert!(!listed(&rig));
}

#[test]
fn a_finished_or_stopped_one_is_only_taken_off_the_list() {
    for state in ["done", "stopped"] {
        let rig = Rig::with_agents();
        rig.agents(json!([bg_row(&rig, state, None)]));
        let (_, got) = rig.post("/api/dismiss", &json!({"session_id": SID}));
        assert_eq!(got["ok"], json!(true), "{state}: {got}");
        assert!(stops(&rig).is_empty(), "{state}");
        assert!(!listed(&rig), "{state}");
    }
}

#[test]
fn a_new_background_session_needs_no_first_message() {
    let tmp = common::TempDir::new("bgnew");
    let (main, _feat) = common::git::bg_project(&tmp.path, "bn");
    let rig = common::Dispatcher::start(common::Options {
        fake_cli: vec!["claude.exe", "wt.exe"],
        home: Some(tmp.join("bn-home")),
        ..common::Options::new()
    });
    let token = rig.window_token();
    let (_, r) = rig.post_window("/api/spawn", &json!({"kind": "new", "where": "background",
        "cwd": main.to_string_lossy(), "prompt": ""}), &token);
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["bg_id"], json!("1f2e3d4c"), "{r}");
    let bg: Vec<Value> = rig
        .fake_log()
        .into_iter()
        .filter(|e| e["prog"] == "claude" && e["args"].as_array().is_some_and(|a| a.iter().any(|x| x == "--bg")))
        .collect();
    assert_eq!(bg.len(), 1, "{bg:?}");
    // No greeting: the last argument is --bg itself (or an option), never a prompt.
    let args: Vec<String> = bg[0]["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
    assert_eq!(args.last().map(String::as_str), Some("--bg"), "{args:?}");
}
