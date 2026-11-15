//! Ports of test_finished.py: a finished background job is never "waiting
//! for you", can be taken off the list, and is hidden after a while when
//! `list.hide_finished_hours` says so.
//!
//! `claude agents --json` is the agents stub's (tests/common/dispatch.rs):
//! it lists a job as `state: done` until it is opened in Claude Code's
//! agent view, which is the case test_finished.py was written for.
#![cfg(windows)]
mod common;
use common::dispatch::*;
use serde_json::{json, Value};

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        * 1000.0
}

/// A background job that finished `age_hours` ago.
fn job(rig: &Rig, sid: &str, age_hours: f64, extra: Value) -> Value {
    let mut row = json!({"sessionId": sid, "id": &sid[..8.min(sid.len())], "kind": "background",
                         "state": "done", "name": "reply-with-the-single", "cwd": rig.cwd,
                         "startedAt": now_ms() - age_hours * 3_600_000.0, "key": sid});
    merge(&mut row, &extra);
    row
}

fn ids(rig: &Rig) -> Vec<String> {
    let mut v: Vec<String> = rig
        .rows()
        .iter()
        .filter_map(|r| r["sessionId"].as_str().map(str::to_string))
        .collect();
    v.sort();
    v
}

fn hiding_rows(rig: &Rig) -> Value {
    json!([
        job(rig, "a", 30.0, json!({})),                                   // finished, old
        job(rig, "b", 1.0, json!({})),                                    // finished, recent
        job(rig, "c", 99.0, json!({"state": "working"})),                 // still going
        {"sessionId": "d", "key": "d", "kind": "interactive", "pid": std::process::id(),
         "status": "idle", "cwd": rig.cwd, "startedAt": now_ms() - 99.0 * 3_600_000.0}
    ])
}

// -------------------------------------------------------------- Finished

#[test]
fn when_it_last_did_anything() {
    // finished_at: the last hook event, else when the job started, else 0.
    let rig = Rig::with_agents_config(json!({"list": {"hide_finished_hours": 24}}));
    let mut never = json!({"kind": "background", "state": "done", "sessionId": "n", "key": "n"});
    merge(&mut never, &json!({"cwd": rig.cwd}));
    rig.agents(json!([
        job(&rig, "old-started", 30.0, json!({})),
        job(&rig, "old-but-active", 30.0, json!({})),
        never
    ]));
    // A hook event now is "last did anything": 30 hours since it started
    // no longer counts against it.
    rig.hook("Notification", "old-but-active", json!({"notification_type": "idle_prompt"}));
    rig.invalidate();
    // No time at all is 0: older than any cutoff.
    assert_eq!(ids(&rig), vec!["old-but-active".to_string()]);
}

#[test]
fn the_setting_is_read_safely() {
    // Nothing, a negative number, a string, a boolean, null or a list all
    // mean "never hide"; only a positive number hides.
    for bad in [json!(-1), json!("24"), json!(true), json!(null), json!([24])] {
        let rig = Rig::with_agents_config(json!({"list": {"hide_finished_hours": bad}}));
        rig.agents(json!([job(&rig, "a", 30.0, json!({}))]));
        assert_eq!(ids(&rig), vec!["a".to_string()], "{bad}");
    }
    let rig = Rig::with_agents_config(json!({}));
    rig.agents(json!([job(&rig, "a", 30.0, json!({}))]));
    assert_eq!(ids(&rig), vec!["a".to_string()]);
    let rig = Rig::with_agents_config(json!({"list": {"hide_finished_hours": 24}}));
    rig.agents(json!([job(&rig, "a", 30.0, json!({}))]));
    assert!(ids(&rig).is_empty());
}

// ------------------------------------------------------------- Attention

fn stale_state(rig: &Rig, sid: &str) -> Value {
    rig.state_json()["sessions"][sid].clone()
}

#[test]
fn a_stale_notification_is_cleared_for_a_finished_job() {
    let rig = Rig::with_agents();
    rig.agents(json!([job(&rig, SID, 17.0 * 24.0, json!({}))]));
    rig.hook("Notification", SID, json!({"notification_type": "idle_prompt"}));
    assert!(stale_state(&rig, SID).get("attention_since").is_some());
    let row = rig.row(SID);
    assert_ne!(row["attention"], json!(true));
    assert!(stale_state(&rig, SID).get("attention_since").is_none());
}

#[test]
fn even_a_failure_flag_goes_once_the_job_is_done() {
    let rig = Rig::with_agents();
    rig.agents(json!([job(&rig, SID, 17.0 * 24.0, json!({}))]));
    rig.hook("StopFailure", SID, json!({"error": "overloaded"}));
    assert_ne!(rig.row(SID)["attention"], json!(true));
}

#[test]
fn a_blocked_job_still_asks() {
    let rig = Rig::with_agents();
    rig.agents(json!([job(&rig, SID, 17.0 * 24.0, json!({"state": "blocked"}))]));
    rig.hook("Notification", SID, json!({"notification_type": "idle_prompt"}));
    assert_eq!(rig.row(SID)["attention"], json!(true));
}

// --------------------------------------------------------------- Hiding

#[test]
fn off_by_default() {
    let rig = Rig::with_agents();
    rig.agents(hiding_rows(&rig));
    assert_eq!(ids(&rig), vec!["a", "b", "c", "d"]);
}

#[test]
fn only_old_finished_jobs_are_hidden() {
    let rig = Rig::with_agents_config(json!({"list": {"hide_finished_hours": 24}}));
    rig.agents(hiding_rows(&rig));
    assert_eq!(ids(&rig), vec!["b", "c", "d"]);
}

#[test]
fn it_is_a_setting_applied_at_once() {
    let rig = Rig::with_agents_config(json!({"background": {"model": "opus"},
                                             "list": {"hide_finished_hours": 24}}));
    rig.agents(hiding_rows(&rig));
    assert_eq!(ids(&rig), vec!["b", "c", "d"]);
    let fields = rig.get("/api/settings").1;
    let field = fields["fields"]
        .as_array()
        .and_then(|f| f.iter().find(|f| f["key"] == "list.hide_finished_hours"))
        .unwrap_or_else(|| panic!("no such field: {fields}"))
        .clone();
    assert_eq!(field["applies"], json!("live"), "{field}");
    // A negative number is refused; 48 is taken and applies at once.
    let (_, bad) = rig.post("/api/settings", &json!({"changes": {"list.hide_finished_hours": "-1"}}));
    assert_eq!(bad["ok"], json!(false), "{bad}");
    assert!(bad["errors"]["list.hide_finished_hours"].is_string(), "{bad}");
    let (_, got) = rig.post("/api/settings", &json!({"changes": {"list.hide_finished_hours": 48}}));
    assert_eq!(got["ok"], json!(true), "{got}");
    assert_eq!(got["restart"], json!([]));
    rig.invalidate();
    assert_eq!(ids(&rig), vec!["a", "b", "c", "d"], "30 hours is inside 48");
    let disk: Value =
        serde_json::from_str(&std::fs::read_to_string(rig.home.join("config.json")).unwrap()).unwrap();
    assert_eq!(disk["list"]["hide_finished_hours"], json!(48));
    // Kept out of the block that becomes `claude --bg` options.
    assert_eq!(disk["background"], json!({"model": "opus"}));
}

// ---------------------------------------------------------------- Window

#[test]
fn a_tile_with_no_process_can_be_taken_off_the_list() {
    let rig = Rig::start();
    let (code, js) = rig.get("/ui/app.js");
    assert_eq!(code, 200);
    let js = js.as_str().expect("the script is text").to_string();
    let a = js.find("function tile(s, short)").expect("the tile function");
    let body = &js[a..a + js[a..].find("const spend").unwrap()];
    // Dismiss no longer depends on the tile being archived.
    assert!(!body.contains("archived ? `<span data-arch=\"${esc(pkey(s))}\" title=\"unarchive\">↩</span>${"));
    assert!(body.contains("data-dismiss="));
    assert!(body.contains("s.state === \"working\" || RUNNING[s.status] ? \"\""));
}

#[test]
fn a_finished_job_reads_finished() {
    let rig = Rig::start();
    let (_, js) = rig.get("/ui/shared.js");
    assert!(js
        .as_str()
        .unwrap()
        .contains("if (s.state === \"done\") return [\"d-done\", \"finished\"];"));
}
