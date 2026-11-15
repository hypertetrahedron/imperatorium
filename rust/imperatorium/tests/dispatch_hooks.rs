//! Ports of test_dispatcher.py's HookIngest, IdleWhileRunning and
//! FailedTurns: what the hooks record and what the session list then says
//! (attention, credentials). Each test is named for its Python original.
#![cfg(windows)]
mod common;
use common::dispatch::*;
use serde_json::{json, Value};

fn attention(row: &Value) -> bool {
    row["attention"] == json!(true)
}

fn notify(rig: &Rig, kind: &str) -> Value {
    rig.hook("Notification", SID, json!({"notification_type": kind}));
    rig.row(SID)
}

// ------------------------------------------------------------ HookIngest

#[test]
fn session_start_records_the_token() {
    let rig = Rig::start();
    rig.start_hook(SID, "tok", r"\\.\pipe\x");
    assert_eq!(rig.tokens(), json!({SID: "tok"}));
}

#[test]
fn a_session_is_dispatchable_only_with_socket_and_token() {
    let rig = Rig::start();
    let socket = rig.live(SID, "idle", json!({}));
    assert_eq!(rig.row(SID)["dispatchable"], json!(false)); // a socket, but no token yet
    rig.start_hook(SID, "tok", &socket);
    assert_eq!(rig.row(SID)["dispatchable"], json!(true));
}

#[test]
fn a_rebound_socket_invalidates_the_stored_token() {
    // A resumed session keeps its id but binds a NEW inbox: the old token
    // against the fresh socket would report dispatchable and then fail at
    // the auth line.
    let rig = Rig::start();
    let socket = rig.live(SID, "idle", json!({}));
    rig.start_hook(SID, "tok", &socket);
    let new = rig.pipe();
    rig.touch(SID, json!({"messagingSocketPath": new}));
    let row = rig.row(SID);
    assert_eq!(row["dispatchable"], json!(false));
    assert_eq!(row["stale_credential"], json!(true));
    assert!(row.get("token").is_none());
}

#[test]
fn the_next_session_start_repairs_a_stale_credential() {
    let rig = Rig::start();
    let socket = rig.live(SID, "idle", json!({}));
    rig.start_hook(SID, "tok", &socket);
    let new = rig.pipe();
    rig.touch(SID, json!({"messagingSocketPath": new}));
    rig.start_hook(SID, "tok2", &new);
    assert_eq!(rig.row(SID)["dispatchable"], json!(true));
    // The row's token is private, so ask by what the pipe is shown.
    let got = rig.dispatch(json!({"session_id": SID, "prompt": "hi"}));
    assert_eq!(got["ok"], json!(true), "{got}");
    eventually("the auth line reaches the new pipe", || !rig.received(&new).is_empty());
    assert_eq!(auth_tokens(&rig.received(&new)), vec!["tok2".to_string()]);
}

#[test]
fn a_stop_event_can_register_a_session_start_never_saw() {
    // SessionStart fires once; every event carries the credential and any
    // of them can repair it.
    let rig = Rig::start();
    let socket = rig.live(SID, "idle", json!({}));
    rig.hook(
        "Stop",
        SID,
        json!({"messaging_token": "late", "messaging_socket": socket, "last_assistant_message": "done"}),
    );
    assert_eq!(rig.row(SID)["dispatchable"], json!(true));
    assert_eq!(rig.tokens()[SID], json!("late"));
}

#[test]
fn a_notification_also_carries_the_credential() {
    let rig = Rig::start();
    let socket = rig.live(SID, "idle", json!({}));
    rig.hook(
        "Notification",
        SID,
        json!({"notification_type": "permission_prompt", "messaging_token": "tok",
               "messaging_socket": socket}),
    );
    let row = rig.row(SID);
    assert_eq!(row["dispatchable"], json!(true));
    assert!(attention(&row));
}

#[test]
fn permission_prompt_raises_attention() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    rig.hook("Notification", SID, json!({"notification_type": "permission_prompt"}));
    let row = rig.row(SID);
    assert!(attention(&row));
    assert!(row.get("attention_since").is_some());
}

#[test]
fn stop_clears_attention() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    rig.hook("Notification", SID, json!({"notification_type": "permission_prompt"}));
    rig.hook("Stop", SID, json!({"last_assistant_message": "done"}));
    // The prompt is no longer what it waits on: its turn is complete.
    assert_eq!(rig.row(SID)["attention_kind"], json!("complete"));
}

#[test]
fn session_end_forgets_the_token() {
    let rig = Rig::start();
    rig.start_hook(SID, "tok", r"\\.\pipe\x");
    rig.hook("SessionEnd", SID, json!({}));
    let tokens = rig.tokens();
    assert!(tokens.is_null() || tokens == json!({}), "{tokens}");
}

#[test]
fn uninteresting_events_are_dropped() {
    let rig = Rig::start();
    let before = rig.events().len();
    let out = rig.hook("UserPromptSubmit", SID, json!({}));
    assert_eq!(out["ok"], json!(true));
    assert_eq!(out["ignored"], json!("UserPromptSubmit"));
    assert_eq!(rig.events().len(), before);
}

#[test]
fn tool_events_go_to_memory_not_the_store() {
    // One per tool call per session: the store rewrites its whole file on
    // every write, so these must never reach it.
    let rig = Rig::start();
    rig.start_hook(SID, "tok", r"\\.\pipe\x");
    let before = rig.events().len();
    let file = rig.state.clone();
    let snapshot = std::fs::read(&file).unwrap();
    let out = rig.hook(
        "PreToolUse",
        SID,
        json!({"tool_name": "Bash", "tool_input": {"command": "npm test"}, "tool_use_id": "t1"}),
    );
    assert_eq!(out["ok"], json!(true));
    assert_eq!(rig.events().len(), before);
    assert_eq!(std::fs::read(&file).unwrap(), snapshot);
    let (_, act) = rig.get(&format!("/api/activity?session={SID}"));
    assert_eq!(act["current"]["summary"], json!("npm test"), "{act}");
}

#[test]
fn an_event_without_a_session_id_is_dropped() {
    let rig = Rig::start();
    let (code, out) = rig.post("/hook", &json!({"hook_event_name": "Stop"}));
    assert_eq!(code, 200);
    assert_eq!(out["ok"], json!(true));
}

#[test]
fn stop_failure_keeps_its_error() {
    // The field is `error`; the test once asserted `error_type`, which
    // Claude Code has never sent.
    let rig = Rig::start();
    rig.hook("StopFailure", SID, json!({"error": "rate_limit"}));
    assert_eq!(rig.events()[0]["error"], json!("rate_limit"));
}

// ------------------------------------------------------ IdleWhileRunning
// `idle_prompt` arrives a minute after the prompt goes quiet, even while the
// session waits on a Monitor or a backgrounded command.

#[test]
fn idle_prompt_while_running_a_command_is_not_attention() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    assert!(!attention(&notify(&rig, "idle_prompt")));
}

#[test]
fn truly_idle_still_waits_for_you() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    assert!(attention(&notify(&rig, "idle_prompt")));
}

#[test]
fn it_comes_back_once_the_command_is_done() {
    // Hidden, not cleared: if the session then sits idle, it is right.
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    notify(&rig, "idle_prompt");
    rig.touch(SID, json!({"status": "idle"}));
    assert!(attention(&rig.row(SID)));
}

#[test]
fn a_permission_prompt_still_raises_it() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    assert!(attention(&notify(&rig, "permission_prompt")));
}

// ----------------------------------------------------------- FailedTurns
// A turn that died on an API error is exactly when a person is wanted.

fn fail(rig: &Rig, error: &str, detail: Option<&str>) {
    rig.hook(
        "StopFailure",
        SID,
        json!({"error": error, "error_details": detail,
               "last_assistant_message": "API Error: 529 Overloaded."}),
    );
}

fn working_again(rig: &Rig) {
    // Claude Code's own record of the moment it went busy: in the future,
    // so it is later than whatever raised attention.
    let later = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        + 600.0)
        * 1000.0;
    rig.touch(SID, json!({"status": "busy", "statusUpdatedAt": later}));
}

#[test]
fn an_overload_raises_attention_instead_of_clearing_it() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    fail(&rig, "overloaded", None);
    let row = rig.row(SID);
    assert!(attention(&row));
    assert_eq!(row["attention_reason"], json!("API overloaded - wait and retry"));
    assert_eq!(row["attention_kind"], json!("failure"));
}

#[test]
fn the_retry_does_not_wipe_the_flag() {
    // Claude Code retries a 529 by itself, so the session goes busy again
    // seconds later. That is not a person answering anything.
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    fail(&rig, "overloaded", None);
    working_again(&rig);
    assert!(attention(&rig.row(SID)), "a retry is not a resolution");
}

#[test]
fn a_permission_prompt_is_still_settled_by_working_again() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    rig.hook("Notification", SID, json!({"notification_type": "permission_prompt"}));
    working_again(&rig);
    assert!(!attention(&rig.row(SID)));
}

#[test]
fn finishing_cleanly_clears_a_failure() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    fail(&rig, "overloaded", None);
    rig.hook("Stop", SID, json!({"last_assistant_message": "done"}));
    assert!(!attention(&rig.row(SID)));
}

#[test]
fn the_quiet_failure_raises_nothing() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    fail(&rig, "max_output_tokens", None);
    let row = rig.row(SID);
    assert!(!attention(&row));
    assert_eq!(row["last_failure"], json!("max_output_tokens"));
}

#[test]
fn the_reason_and_detail_are_kept_for_the_window() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    fail(&rig, "invalid_request", Some("prompt is too long"));
    let row = rig.row(SID);
    assert_eq!(row["attention_reason"], json!("request too large - /compact or trim"));
    assert_eq!(row["last_failure"], json!("invalid_request"));
    assert!(row["last_failure_detail"].as_str().unwrap().contains("too long"));
}

#[test]
fn a_failure_still_records_the_credential() {
    // Every event carries the token, and a failing session is one you may
    // most want to reach.
    let rig = Rig::start();
    let socket = rig.live(SID, "busy", json!({}));
    rig.hook(
        "StopFailure",
        SID,
        json!({"error": "overloaded", "messaging_token": "tok", "messaging_socket": socket}),
    );
    assert_eq!(rig.tokens()[SID], json!("tok"));
}

#[test]
fn an_unknown_error_code_still_raises_something() {
    let rig = Rig::start();
    rig.live(SID, "busy", json!({}));
    fail(&rig, "some_future_code", None);
    let row = rig.row(SID);
    assert!(attention(&row));
    assert_eq!(row["attention_reason"], json!("API error"));
}
