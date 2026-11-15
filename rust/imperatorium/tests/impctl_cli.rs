//! test_impctl.py's command line, run against the binary: help covers every
//! command, sessions are found by the names people use, send reaches the
//! dispatcher as the right request, and a removal is never one careless
//! keystroke away. The dispatcher is a recording fake (IMPERATORIUM_URL);
//! nothing starts, stops or registers a real service.
#![cfg(windows)]

mod common;
use common::setup::*;
use common::*;
use serde_json::json;

#[test]
fn impctl_help_prints_each_command_with_its_parameters() {
    let tmp = TempDir::new("impctl-help");
    let r = cli(&["help"], None, &tmp.path);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("  send PROMPT... (-s SESSION | -p PROJECT) [--mode auto|inject|launch|background] [--dry-run]"), "{}", r.out);
    assert!(r.out.contains("  restart [all|dispatcher|streamdeck|hotkey] [--task-prefix P]"), "{}", r.out);
    assert!(r.out.contains("\nService\n"), "{}", r.out);
}

#[test]
fn impctl_no_arguments_print_the_help_too() {
    let tmp = TempDir::new("impctl-noargs");
    let r = cli(&[], None, &tmp.path);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("\nService\n") && r.out.contains("  sessions"), "{}", r.out);
}

#[test]
fn impctl_an_unknown_help_topic_is_an_error_not_a_crash() {
    let tmp = TempDir::new("impctl-topic");
    let r = cli(&["help", "bogus"], None, &tmp.path);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("no command"), "{}", r.err);
    assert!(!r.err.contains("panicked"), "{}", r.err);
}

#[test]
fn impctl_unknown_flags_are_refused() {
    let tmp = TempDir::new("impctl-flags");
    let server = fake_with_sessions();
    let r = cli(&["sessions", "--bogus"], Some(&base(&server)), &tmp.path);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("--bogus"), "{}", r.err);
}

#[test]
fn impctl_service_uninstall_without_yes_or_a_terminal_removes_nothing() {
    let tmp = TempDir::new("impctl-uninstall");
    // A prefix no real task carries, whatever happened next.
    let r = cli(&["service", "uninstall", "--task-prefix", "ImpCovNever", "--home", &tmp.path.to_string_lossy()], None, &tmp.path);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    assert!(r.err.contains("needs --yes"), "{}", r.err);
    assert!(!r.out.contains("remove task"), "{}", r.out);
}

#[test]
fn uninstall_without_yes_or_a_terminal_removes_nothing() {
    let tmp = TempDir::new("setup-uninstall");
    let settings = tmp.path.join("settings.json");
    let before = r#"{"statusLine": {"type": "command", "command": "imperatorium statusline"}}"#;
    std::fs::write(&settings, before).unwrap();
    // A settings file and a task prefix of its own, whatever happened next.
    let r = cli(
        &["uninstall", "--settings", &settings.to_string_lossy(), "--task-prefix", "ImpCovNever", "--home", &tmp.path.to_string_lossy()],
        None,
        &tmp.path,
    );
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    assert!(r.err.contains("needs --yes"), "{}", r.err);
    assert!(!r.out.contains("== "), "{}", r.out);
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), before);
}

#[test]
fn impctl_service_install_does_not_ask() {
    let tmp = TempDir::new("impctl-install");
    let r = cli(
        &["service", "install", "--dry-run", "--no-hotkey", "--task-prefix", "ImpCovNever", "--home", &tmp.path.to_string_lossy()],
        None,
        &tmp.path,
    );
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert!(r.out.contains("task ImpCovNeverDispatcher:"), "{}", r.out);
    assert!(!r.err.contains("--yes"), "{}", r.err);
}

#[test]
fn impctl_resume_falls_back_to_a_past_session_by_id_prefix() {
    let tmp = TempDir::new("impctl-resume");
    let server = fake_with_sessions();
    server.set_answer(
        "/api/history?q=42e2",
        200,
        json!({"results": [{"session_id": "42e2913a-1111", "cwd": "Y:/old"}]}).to_string().as_bytes(),
    );
    server.set_answer("/api/resume", 200, json!({"where": "window", "command": "claude --resume"}).to_string().as_bytes());
    let r = cli(&["resume", "42e2"], Some(&base(&server)), &tmp.path);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert_eq!(r.out.trim(), "window: claude --resume");
    let sent = posted(&server, "/api/resume");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["session_id"], json!("42e2913a-1111"));
    assert_eq!(sent[0]["cwd"], json!("Y:/old"));
    assert_eq!(sent[0]["where"], json!("window"));
}

#[test]
fn impctl_a_past_session_that_does_not_match_the_prefix_is_not_taken() {
    let tmp = TempDir::new("impctl-nomatch");
    let server = fake_with_sessions();
    let hit = json!({"results": [{"session_id": "42e2913a-1111", "cwd": "Y:/old"}]}).to_string();
    server.set_answer("/api/history?q=zzzz", 200, hit.as_bytes());
    server.set_answer("/api/history?q=", 200, hit.as_bytes());
    let r = cli(&["resume", "zzzz"], Some(&base(&server)), &tmp.path);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    assert!(r.err.contains("nor any past session id"), "{}", r.err);
    assert!(posted(&server, "/api/resume").is_empty());
}

#[test]
fn impctl_send_to_a_session_injects_by_its_full_id() {
    let tmp = TempDir::new("impctl-send");
    let server = fake_with_sessions();
    server.set_answer("/api/dispatch", 200, json!({"ok": true, "action": "inject", "session": "35aa0000-0000"}).to_string().as_bytes());
    let r = cli(&["send", "-s", "claude-lens", "hello", "there"], Some(&base(&server)), &tmp.path);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert_eq!(r.out.trim(), "inject -> 35aa0000-0000");
    let sent = posted(&server, "/api/dispatch");
    assert_eq!(sent, vec![json!({"prompt": "hello there", "mode": "auto", "session_id": "35aa0000-0000"})]);
}

#[test]
fn impctl_send_needs_exactly_one_target() {
    let tmp = TempDir::new("impctl-target");
    let server = fake_with_sessions();
    for args in [&["send", "hi"][..], &["send", "-s", "x", "-p", "y", "hi"][..]] {
        let r = cli(args, Some(&base(&server)), &tmp.path);
        assert_eq!(r.code, 1, "{args:?}: {}{}", r.out, r.err);
        assert!(r.err.contains("exactly one"), "{}", r.err);
    }
    assert!(posted(&server, "/api/dispatch").is_empty());
}

#[test]
fn impctl_a_refusal_exits_non_zero() {
    let tmp = TempDir::new("impctl-refused");
    let server = FakeServer::start();
    server.set_answer("/api/dispatch", 200, json!({"ok": false, "error": "nope"}).to_string().as_bytes());
    let r = cli(&["send", "-p", "x", "hi"], Some(&base(&server)), &tmp.path);
    assert_eq!(r.code, 1);
    assert!(r.out.contains("refused: nope"), "{}", r.out);
}

#[test]
fn impctl_a_dead_dispatcher_says_how_to_start_it() {
    let tmp = TempDir::new("impctl-down");
    let r = cli(&["sessions"], Some(&dead_base()), &tmp.path);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("imperatorium start"), "{}", r.err);
}
