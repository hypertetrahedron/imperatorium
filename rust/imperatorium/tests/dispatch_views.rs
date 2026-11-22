//! Ports of test_dispatcher.py's RowLookup, Dictation, Awaited,
//! SessionStatus, WorkContext and CostRoute: the read-only views of one
//! session. Where Python stubbed the transcript reader, a transcript is
//! written for the session.
#![cfg(windows)]
mod common;
use common::dispatch::*;
use common::wait_until;
use serde_json::{json, Value};
use std::time::Duration;

/// A transcript with one reply and a background shell `b1` running `make`.
fn with_a_shell(rig: &Rig, sid: &str, input: u64) {
    let mut lines = chat_lines(sid, &[("user", "build it"), ("assistant", "on it")], input);
    lines.extend(background_shell(sid, 1, &format!("{sid}-1"), "b1", "make", input));
    rig.transcript(sid, &lines);
}

// ------------------------------------------------------------ RowLookup
// Finding a session's row for the chat must not wait on `claude agents`.

fn one_session_agents(rig: &Rig) -> Value {
    json!([{"sessionId": SID, "cwd": rig.cwd, "kind": "interactive", "name": "ll-1", "status": "idle"}])
}

#[test]
fn a_few_seconds_old_snapshot_is_good_enough() {
    let rig = Rig::with_agents();
    rig.agents(one_session_agents(&rig));
    rig.rows();
    let calls = rig.agent_calls();
    assert!(calls >= 1);
    rig.get(&format!("/api/conversation?session={SID}"));
    rig.get(&format!("/api/session_status?session={SID}"));
    assert_eq!(rig.agent_calls(), calls, "the chat's lookups ran `claude agents` again");
}

#[test]
fn an_unknown_session_looks_in_a_fresh_snapshot() {
    let rig = Rig::with_agents();
    rig.agents(json!([]));
    rig.rows();
    let calls = rig.agent_calls();
    // It appeared since. A snapshot under two seconds old is still used, so
    // the lookup finds it once that one is old enough, with one more run.
    std::fs::write(
        rig.bin_dir.join("agents.json"),
        serde_json::to_vec(&one_session_agents(&rig)).unwrap(),
    )
    .unwrap();
    let mut name = Value::Null;
    eventually("the new session is found", || {
        name = rig.get(&format!("/api/conversation?session={SID}")).1["name"].clone();
        name == json!("ll-1")
    });
    assert_eq!(rig.agent_calls(), calls + 1);
}

#[test]
fn an_old_snapshot_is_rebuilt() {
    // Fifteen seconds is how old a snapshot may be for the chat's lookups.
    let rig = Rig::with_agents();
    rig.agents(one_session_agents(&rig));
    rig.rows();
    let calls = rig.agent_calls();
    let start = std::time::Instant::now();
    assert!(wait_until(Duration::from_secs(40), || {
        rig.get(&format!("/api/conversation?session={SID}"));
        (rig.agent_calls() > calls).then_some(())
    })
    .is_some());
    assert!(start.elapsed() >= Duration::from_secs(14), "rebuilt after {:?}", start.elapsed());
}

// ------------------------------------------------------------- Dictation
// Dictation runs in a child process, and only when config asks for it.

#[test]
fn no_voice_block_means_no_dictation_process() {
    let rig = Rig::start();
    let (_, warm) = rig.post("/api/voice/warm", &json!({}));
    assert_eq!(warm["ok"], json!(false));
    assert_eq!(warm["error"], json!("dictation is not available"));
    let (code, said) = rig.request("/api/transcribe", Some(b"x".to_vec()), Some("audio/webm"), &[]);
    assert_eq!(code, 200);
    assert_eq!(said["ok"], json!(false));
    assert_eq!(said["error"], json!("dictation is not available"));
}

#[test]
fn a_voice_block_makes_a_worker_that_starts_nothing_without_preload() {
    let rig = Rig::with(json!({"voice": {"preload": false}}), |_| {});
    let (_, warm) = rig.post("/api/voice/warm", &json!({}));
    assert_eq!(warm["ok"], json!(false));
    // A worker exists (it says what it lacks), and nothing is running.
    let why = warm["error"].as_str().unwrap();
    assert!(why.starts_with("dictation needs"), "{warm}");
    assert_ne!(why, "dictation is not available");
}

#[test]
fn disabled_voice_makes_no_worker() {
    let rig = Rig::with(json!({"voice": {"enabled": false}}), |_| {});
    let (_, warm) = rig.post("/api/voice/warm", &json!({}));
    assert_eq!(warm["error"], json!("dictation is not available"));
}

// --------------------------------------------------------------- Awaited
// The panel under the chat: output of what a live session waits on.

#[test]
fn unknown_session() {
    let rig = Rig::start();
    assert_eq!(rig.get("/api/awaited?session=nope").1["ok"], json!(false));
}

#[test]
fn a_live_session_lists_its_tasks() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    with_a_shell(&rig, SID, 1);
    let got = rig.get(&format!("/api/awaited?session={SID}")).1;
    let ids: Vec<&str> = got["tasks"].as_array().unwrap().iter().filter_map(|t| t["id"].as_str()).collect();
    assert_eq!(ids, vec!["b1"], "{got}");
}

#[test]
fn an_ended_session_waits_on_nothing() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({"pid": null}));
    with_a_shell(&rig, SID, 1);
    assert_eq!(rig.get(&format!("/api/awaited?session={SID}")).1["tasks"], json!([]));
}

#[test]
fn status_does_not_carry_the_id_list() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    with_a_shell(&rig, SID, 1);
    let got = rig.get(&format!("/api/session_status?session={SID}")).1;
    assert!(got["stat"].get("known_tasks").is_none(), "{got}");
    assert!(got["stat"]["shells"].is_array());
}

// --------------------------------------------------------- SessionStatus
// The panel's route: transcript numbers, and none of its background work
// once the session's process is gone.

#[test]
fn session_status_for_an_unknown_session() {
    let rig = Rig::start();
    assert_eq!(rig.get("/api/session_status?session=nope").1["ok"], json!(false));
}

#[test]
fn a_live_session_keeps_its_background_work() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    with_a_shell(&rig, SID, 1);
    let got = rig.get(&format!("/api/session_status?session={SID}")).1;
    assert_eq!(got["alive"], json!(true), "{got}");
    assert_eq!(got["stat"]["shells"].as_array().unwrap().len(), 1);
    assert!(got.get("shell_processes").is_some());
}

#[test]
fn an_ended_session_has_nothing_running() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({"pid": null}));
    with_a_shell(&rig, SID, 1);
    let got = rig.get(&format!("/api/session_status?session={SID}")).1;
    assert_eq!(got["alive"], json!(false), "{got}");
    assert_eq!((got["stat"]["agents"].clone(), got["stat"]["shells"].clone()), (json!([]), json!([])));
    assert_eq!(got["stat"]["tokens"]["input"], json!(2)); // what it did stays (two replies of 1)
}

#[test]
fn every_local_row_carries_its_work_at_a_glance() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    with_a_shell(&rig, SID, 7);
    let stat = rig.get(&format!("/api/session_status?session={SID}")).1["stat"].clone();
    assert!(stat["cache"]["expires_at"].is_number(), "{stat}");
    let work = rig.row(SID)["work"].clone();
    assert_eq!(
        work,
        json!({"cache_expires_at": stat["cache"]["expires_at"], "cache_ttl": stat["cache"]["ttl"], "context_tokens": 7,
               "cold_tokens": 0, "resume_cost_usd": work["resume_cost_usd"], "alive": true,
               "agents": 0, "shells": 1, "monitors": 0, "wakeups": 0, "wakes_at": null,
               "processes": 0})
    );
    assert!(work["resume_cost_usd"].is_number());
}

#[test]
fn an_ended_rows_work_has_no_counts() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({"pid": null}));
    assert_eq!(
        rig.row(SID)["work"],
        json!({"cache_expires_at": null, "context_tokens": null, "cold_tokens": 0,
               "resume_cost_usd": null, "alive": false})
    );
}

// ----------------------------------------------------------- WorkContext

#[test]
fn the_tile_row_carries_the_context_size() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    rig.transcript(SID, &chat_lines(SID, &[("user", "hi"), ("assistant", "yo")], 230_000));
    let work = rig.row(SID)["work"].clone();
    assert_eq!(work["context_tokens"], json!(230_000));
    let stat = rig.get(&format!("/api/session_status?session={SID}")).1["stat"].clone();
    assert_eq!(work["cache_expires_at"], stat["cache"]["expires_at"]);
}

// ------------------------------------------------------------- CostRoute

#[test]
fn costs_come_from_the_history_and_errors_are_answers() {
    // The history is read from the transcripts: the window asked for is the
    // one answered (7 days, clamped to 1..90), as an answer, never a 500.
    let rig = Rig::start();
    rig.transcript(SID, &chat_lines(SID, &[("user", "hi"), ("assistant", "yo")], 100));
    let (code, got) = rig.get("/api/costs?days=7");
    assert_eq!(code, 200);
    assert_eq!(got["window_days"], json!(7), "{got}");
    assert_eq!(rig.get("/api/costs?days=900").1["window_days"], json!(90));
    assert_eq!(rig.get("/api/costs?days=nonsense").1["window_days"], json!(30));
}
