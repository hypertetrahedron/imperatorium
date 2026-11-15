//! The dispatcher's side of the Imperatorium mod (the ports of
//! test_modlink.py's ControlTests and RouteTests): the two endpoints and
//! their token, which prompts speak as the person, delivery through the mod
//! instead of the inbox, the queue, the usage a beat carries, the status
//! line, and the policy question. Every dispatcher is the harness's own.
#![cfg(windows)]

mod common;
use common::dispatch::{merge, Rig, OTHER, SID};
use common::*;
use serde_json::{json, Value};
use std::time::Duration;

const TOKEN_HEADER: &str = "X-Imperatorium-Mod";

fn mod_token(d: &Dispatcher) -> String {
    let path = d.state.parent().unwrap().join("mod-token");
    std::fs::read_to_string(path).unwrap().trim().to_string()
}

fn mod_post(d: &Dispatcher, path: &str, body: &Value, token: Option<&str>) -> (u16, Value) {
    let headers: Vec<(&str, &str)> = token.map(|t| vec![(TOKEN_HEADER, t)]).unwrap_or_default();
    d.request(
        path,
        Some(serde_json::to_vec(body).unwrap()),
        Some("application/json"),
        &headers,
    )
}

/// One beat of `sid`'s mod, as the mod sends it; the answer's body.
fn beat(rig: &Rig, sid: &str, extra: Value) -> Value {
    let mut body = json!({"session_id": sid, "cwd": rig.cwd});
    merge(&mut body, &extra);
    let (code, got) = mod_post(&rig.d, "/api/mod/beat", &body, Some(&mod_token(&rig.d)));
    assert_eq!(code, 200, "{got}");
    got
}

fn delivered(rig: &Rig, sid: &str) -> Vec<Value> {
    beat(rig, sid, json!({}))["deliver"].as_array().cloned().unwrap_or_default()
}

fn as_window(rig: &Rig, body: Value) -> Value {
    let token = rig.d.window_token();
    rig.d.post_window("/api/dispatch", &body, &token).1
}

#[test]
fn a_live_mod_takes_the_prompt_instead_of_the_socket() {
    let rig = Rig::start();
    let socket = rig.live(SID, "idle", json!({}));
    beat(&rig, SID, json!({}));
    let out = as_window(&rig, json!({"prompt": "run the tests", "session_id": SID}));
    assert_eq!(out["ok"], true, "{out}");
    assert!(rig.received(&socket).is_empty(), "nothing through the inbox socket");
    let got = delivered(&rig, SID);
    assert_eq!(got[0]["text"], "run the tests");
    assert_eq!(got[0]["as_user"], true);
}

#[test]
fn only_the_window_speaks_as_the_person() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    beat(&rig, SID, json!({}));
    let out = rig.dispatch(json!({"prompt": "from a script", "session_id": SID}));
    assert_eq!(out["ok"], true, "{out}");
    let got = delivered(&rig, SID);
    assert_eq!(got[0]["as_user"], false);
    // Framed as someone else's, as a peer message is.
    let text = got[0]["text"].as_str().unwrap();
    assert!(text.contains("from a script"), "{text}");
    assert_ne!(text, "from a script");
}

#[test]
fn a_dispatch_from_the_window_speaks_as_the_person_and_one_without_its_token_does_not() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    beat(&rig, SID, json!({}));
    // The pending record is what the mod has not yet taken: it shows on the row.
    let out = rig.dispatch(json!({"prompt": "hi", "session_id": SID}));
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(rig.row(SID)["mod_pending"], 1);
    assert_eq!(delivered(&rig, SID)[0]["as_user"], false);
    let out = as_window(&rig, json!({"prompt": "hi again", "session_id": SID}));
    assert_eq!(out["ok"], true, "{out}");
    // The first is claimed and not yet accepted; the window's is new.
    let got = delivered(&rig, SID);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["as_user"], true);
}

#[test]
fn without_a_mod_the_socket_is_used_as_before() {
    let rig = Rig::start();
    let socket = rig.live(SID, "idle", json!({}));
    rig.start_hook(SID, "tok", &socket);
    let out = as_window(&rig, json!({"prompt": "hello", "session_id": SID}));
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(rig.received(&socket).len(), 1);
}

#[test]
fn a_live_mod_makes_a_session_reachable_and_says_busy() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    beat(&rig, SID, json!({"busy": true}));
    let row = rig.row(SID);
    assert_eq!(row["dispatchable"], true);
    assert_eq!(row["status"], "busy");
    assert_eq!(row["mod"]["busy"], true);
}

#[test]
fn the_queue_remembers_who_queued() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    beat(&rig, SID, json!({"busy": true}));
    let token = rig.d.window_token();
    let (_, q) = rig.d.post_window("/api/queue", &json!({"session_id": SID, "prompt": "after this"}), &token);
    assert_ne!(q["ok"], false, "{q}");
    let queued = &rig.d.state_json()["queue"][SID];
    assert_eq!(queued[0]["as_user"], true, "{queued}");
    // The turn ends: the sweep hands it to the mod, still as the person's.
    let got = wait_until(Duration::from_secs(20), || {
        let got = beat(&rig, SID, json!({"busy": false}))["deliver"].as_array().cloned().unwrap_or_default();
        (!got.is_empty()).then_some(got)
    })
    .expect("the queued prompt never reached the mod");
    assert_eq!((got[0]["text"].as_str(), got[0]["as_user"].as_bool()), (Some("after this"), Some(true)));
}

#[test]
fn usage_from_a_beat_reaches_the_meters() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    beat(&rig, SID, json!({"usage": {"rateLimits": [{"kind": "five_hour", "percentUsed": 33}],
                                    "context": {"percent": 10, "window": 200000}}}));
    let (_, usage) = rig.d.get("/api/usage");
    assert_eq!(usage["limits"]["five_hour"]["used"], 33.0, "{usage}");
}

#[test]
fn the_status_line_names_who_is_waiting() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({"name": "ll-1"}));
    rig.live(OTHER, "idle", json!({"name": "api"}));
    rig.hook("Notification", OTHER, json!({"notification_type": "permission_prompt"}));
    rig.invalidate();
    let line = beat(&rig, SID, json!({}))["status"].as_str().unwrap_or("").to_string();
    assert!(line.contains("1 waiting on you: api"), "{line:?}");
}

#[test]
fn the_policy_is_asked_only_once_auto_approval_is_on() {
    let rig = Rig::start();
    let policy = rig.d.root.join("policy.json");
    let req = json!({"session_id": SID, "cwd": rig.cwd, "tool_name": "Bash",
                     "tool_input": {"command": "git status"}});
    let check = |req: &Value| mod_post(&rig.d, "/api/mod/check", req, Some(&mod_token(&rig.d))).1;
    std::fs::write(&policy, json!({"enabled": true, "decision_format": "string"}).to_string()).unwrap();
    assert_eq!(check(&req)["verdict"], "ask");
    std::fs::write(&policy, json!({"enabled": true, "decision_format": "object"}).to_string()).unwrap();
    assert_eq!(check(&req)["verdict"], "allow");
    // Logged with the hook's verdicts, marked as the mod's.
    let decisions = rig.d.get("/api/decisions").1;
    assert_eq!(decisions["decisions"][0]["via"], "mod", "{decisions}");
    // The policy's own refusals stand: it never approves its own controls.
    let bad = json!({"session_id": SID, "cwd": rig.cwd, "tool_name": "Bash",
                     "tool_input": {"command": "rm policy.json"}});
    assert_ne!(check(&bad)["verdict"], "allow");
}

#[test]
fn a_beat_needs_the_token() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    let body = json!({"session_id": SID});
    assert_eq!(mod_post(&rig.d, "/api/mod/beat", &body, None).0, 403);
    assert_eq!(mod_post(&rig.d, "/api/mod/beat", &body, Some("wrong")).0, 403);
    let (code, out) = mod_post(&rig.d, "/api/mod/beat", &body, Some(&mod_token(&rig.d)));
    assert_eq!(code, 200);
    assert_eq!(out["ok"], true);
    assert_eq!(rig.row(SID)["mod"]["busy"], false, "the mod is live");
}

#[test]
fn a_check_needs_the_token() {
    let rig = Rig::start();
    let (code, _) = mod_post(&rig.d, "/api/mod/check", &json!({"tool_name": "Bash"}), None);
    assert_eq!(code, 403);
}
