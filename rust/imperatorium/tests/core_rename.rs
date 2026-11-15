//! Ports of test_rename.py: what an install from before the rename of
//! ccontrol / Central Control (2026-10-20) left behind is still found.
//!
//! Since the clean break with the Python version (2026-11-15) a state folder
//! under the old name is neither used nor moved: an install that old is
//! uninstalled with its own version first. A statusline hook that still says
//! `ccontrol_statusline` is still heard.
#![cfg(windows)]
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

/// `imperatorium permission` once, with LOCALAPPDATA at `base`: it audits
/// the decision into the state folder, which says which folder it chose.
fn permission_audits_into(base: &Path) {
    let fake = FakeServer::start();
    let policy = base.join("policy.json");
    std::fs::write(&policy, br#"{"enabled": true, "defaults": {}}"#).unwrap();
    let mut cmd = Command::new(bin());
    cmd.arg("permission").current_dir(repo());
    clean_env(&mut cmd, None);
    cmd.env("IMPERATORIUM_ENDPOINT", fake.url("/hook"))
        .env("IMPERATORIUM_PARK_ENDPOINT", fake.url("/permission/park"))
        .env("LOCALAPPDATA", base)
        .env("APPDATA", base.join("roaming"))
        .env("XDG_CONFIG_HOME", base.join("config"))
        .env("XDG_STATE_HOME", base.join("xdg-state"))
        .env("CLAUDE_CONFIG_DIR", base.join("claude"))
        .env("IMPERATORIUM_POLICY", &policy);
    let request = json!({"hook_event_name": "PermissionRequest", "session_id": "s",
                         "tool_name": "Bash", "tool_input": {"command": "ls"}, "cwd": r"C:\work"});
    let (code, _, err) = run(&mut cmd, request.to_string().as_bytes());
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
}

fn old(base: &Path, name: &str) {
    let dir = base.join("ccontrol");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), "{}").unwrap();
}

#[test]
fn an_old_state_folder_is_left_alone() {
    // A clean break (2026-11-15): the pre-rename folder is neither used
    // nor moved; the hook writes under the new name.
    let t = TempDir::new("rename-old");
    old(&t.path, "state.json");
    permission_audits_into(&t.path);
    assert!(t.join("imperatorium").join("decisions.jsonl").is_file());
    assert!(!t.join("ccontrol").join("decisions.jsonl").exists());
    assert!(t.join("ccontrol").join("state.json").is_file());
}

#[test]
fn a_new_install_uses_the_new_name() {
    let t = TempDir::new("rename-new");
    permission_audits_into(&t.path);
    assert!(t.join("imperatorium").join("decisions.jsonl").is_file());
    assert!(!t.join("ccontrol").exists());
}

#[test]
fn once_the_new_folder_exists_the_old_one_is_ignored() {
    let t = TempDir::new("rename-both");
    std::fs::create_dir_all(t.join("imperatorium")).unwrap();
    old(&t.path, "state.json");
    permission_audits_into(&t.path);
    assert!(t.join("imperatorium").join("decisions.jsonl").is_file());
    assert!(!t.join("ccontrol").join("decisions.jsonl").exists());
}

/// A session's statusline reading carries the plan's rate limits; they land
/// in /api/usage whichever name the hook speaks.
fn limits_after(event: &str) -> Value {
    let d = Dispatcher::start(Options::new());
    let reading = json!({"hook_event_name": event, "session_id": "bbbbbbbb-0000-4000-8000-000000000001",
                         "cwd": r"C:\work", "model": {"display_name": "Opus", "id": "claude-opus-5-5"},
                         "rate_limits": {"five_hour": {"used_percentage": 41.5, "resets_at": 4102444800u64}}});
    let (code, got) = d.post("/hook", &reading);
    assert_eq!((code, got["ok"].clone()), (200, json!(true)), "{event}: {got}");
    let (code, body) = d.get("/api/usage");
    assert_eq!(code, 200, "{body}");
    body["limits"].clone()
}

#[test]
fn a_statusline_from_before_the_rename_still_reports() {
    let now = limits_after("imperatorium_statusline");
    let before = limits_after("ccontrol_statusline");
    assert_eq!(now["five_hour"]["used"], 41.5, "{now}");
    for k in ["used", "resets_at"] {
        assert_eq!(before["five_hour"][k], now["five_hour"][k], "{k}: {before} vs {now}");
    }
}
