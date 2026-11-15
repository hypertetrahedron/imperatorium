//! Ports of test_closing.py: ending a session asks it to exit first (Ctrl+C,
//! `/exit`, Enter typed into its console) and kills it only if it stays; a
//! tab's own shell is sent `exit` once claude has gone.
//!
//! The sessions here are fakecli consoles (`claude --fake-console`: reads
//! what is typed into it, as Claude Code does, and leaves on `/exit`) and,
//! for the tab, copies of cmd.exe named for the terminal, the shell and an
//! editor, so the process table looks like a tab's. POST /api/close is the
//! door in; fakecli's log shows what was typed.
#![cfg(windows)]
mod common;
use common::dispatch::*;
use serde_json::{json, Value};
use std::time::Duration;

fn close(rig: &Rig) -> Value {
    rig.post("/api/close", &json!({"session_id": SID})).1
}

/// What the fake console was typed: ("key", "ctrl-c") and ("line", text).
fn typed(rig: &Rig) -> Vec<(String, String)> {
    rig.fake_log()
        .iter()
        .filter(|e| e["prog"] == "console")
        .map(|e| match (e.get("key"), e.get("line")) {
            (Some(k), _) => ("key".to_string(), k.as_str().unwrap().to_string()),
            (_, Some(l)) => ("line".to_string(), l.as_str().unwrap().to_string()),
            _ => unreachable!(),
        })
        .collect()
}

fn exit_typed() -> Vec<(String, String)> {
    vec![("key".into(), "ctrl-c".into()), ("line".into(), "/exit".into())]
}

#[test]
fn exit_is_typed_and_the_tab_closes() {
    let rig = Rig::start();
    let pid = rig.console(false);
    rig.live(SID, "idle", json!({"pid": pid}));
    let got = close(&rig);
    assert_eq!(got, json!({"ok": true, "how": "exit", "tab_closed": true}));
    assert_eq!(typed(&rig), exit_typed()); // Ctrl+C, then /exit and Enter
    assert!(!running(pid, "claude.exe"));
    assert_eq!(rig.events()[0]["summary"], json!("exit"));
}

#[test]
fn one_that_will_not_exit_is_killed() {
    let rig = Rig::start();
    let pid = rig.console(true);
    rig.live(SID, "idle", json!({"pid": pid}));
    let got = close(&rig);
    assert_eq!(got, json!({"ok": true, "how": "killed", "tab_closed": false}));
    // It was asked first.
    assert_eq!(typed(&rig), exit_typed());
    assert!(!running(pid, "claude.exe"));
}

#[test]
fn one_already_gone_is_not_touched() {
    let rig = Rig::start();
    let mut done = std::process::Command::new("cmd").args(["/c", "exit"]).spawn().unwrap();
    let pid = done.id();
    done.wait().unwrap();
    rig.live(SID, "idle", json!({"pid": pid}));
    let got = close(&rig);
    assert_eq!(got["how"], json!("gone"), "{got}");
    assert!(typed(&rig).is_empty());
}

#[test]
fn no_pid_is_an_error() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({"pid": null}));
    assert_eq!(close(&rig)["ok"], json!(false));
    // And a session nobody has heard of.
    let (_, unknown) = rig.post("/api/close", &json!({"session_id": "nobody"}));
    assert_eq!(unknown["ok"], json!(false));
    assert_eq!(rig.post("/api/close", &json!({})).1["ok"], json!(false));
}

#[test]
fn a_tab_shell_is_sent_exit_after_claude_leaves() {
    let rig = Rig::start();
    let tab = Tab::start(&rig, "windowsterminal");
    rig.live(SID, "idle", json!({"pid": tab.claude}));
    let got = close(&rig);
    assert_eq!(got, json!({"ok": true, "how": "exit", "tab_closed": true}), "{got}");
    assert!(!running(tab.claude, "claude.exe"));
    // The shell was kept open by /k: it is gone because `exit` was typed.
    eventually("the tab's shell has gone", || !running(tab.shell, "pwsh.exe"));
}

#[test]
fn a_shell_under_something_else_is_left_alone() {
    let rig = Rig::start();
    let tab = Tab::start(&rig, "code");
    rig.live(SID, "idle", json!({"pid": tab.claude}));
    close(&rig);
    assert!(!running(tab.claude, "claude.exe"));
    // Only claude was typed into; the shell keeps running.
    std::thread::sleep(Duration::from_millis(800));
    assert!(running(tab.shell, "pwsh.exe"));
    assert_eq!(typed(&rig), exit_typed());
}

#[test]
fn control_keys_round_trip_by_name() {
    // The keys travel to the typing child by name (<ctrl-c>, <enter>) and
    // arrive as the characters they name: Ctrl+C as a character, Enter
    // ending the line. A slash command is typed the same way as /exit.
    let rig = Rig::start();
    let pid = rig.console(true);
    rig.live(SID, "idle", json!({"pid": pid}));
    let got = rig.dispatch(json!({"session_id": SID, "prompt": "/compact"}));
    assert_eq!(got["action"], json!("command"), "{got}");
    assert_eq!(
        typed(&rig),
        vec![("key".to_string(), "ctrl-c".to_string()), ("line".to_string(), "/compact".to_string())]
    );
}
