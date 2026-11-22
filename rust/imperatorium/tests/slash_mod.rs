//! Slash commands from the window through the session's mod (2026-11-22):
//! run once by a mod that can (0.4.0), the output back in the send's
//! answer; a picker or view refused with what it is so the window can show
//! its own; the session's command list for the prompt box; and reads and
//! /config changes only from the window. The mod here is the test's: it
//! beats on a thread as the real one does. Every dispatcher is the
//! harness's own, and nothing here starts a real claude.
#![cfg(windows)]

mod common;
use common::dispatch::{merge, Rig, SID};
use serde_json::{json, Value};
use std::time::Duration;

fn mod_token(rig: &Rig) -> String {
    let path = rig.d.state.parent().unwrap().join("mod-token");
    std::fs::read_to_string(path).unwrap().trim().to_string()
}

fn beat_at(port: u16, token: &str, body: &Value) -> Value {
    let got = ureq::post(&format!("http://127.0.0.1:{port}/api/mod/beat"))
        .set("Content-Type", "application/json")
        .set("X-Imperatorium-Mod", token)
        .send_bytes(&serde_json::to_vec(body).unwrap())
        .unwrap();
    serde_json::from_str(&got.into_string().unwrap()).unwrap()
}

fn beat(rig: &Rig, extra: Value) -> Value {
    let mut body = json!({"session_id": SID, "cwd": rig.cwd, "mod_version": "0.4.0"});
    merge(&mut body, &extra);
    beat_at(rig.d.port, &mod_token(rig), &body)
}

/// A mod on a thread: beats until it is handed an op, answers it with
/// `answer(op)`, then beats once more to deliver it. The ops it was handed.
fn fake_mod(rig: &Rig, answer: fn(&Value) -> Value) -> std::thread::JoinHandle<Vec<Value>> {
    let (port, token, cwd) = (rig.d.port, mod_token(rig), rig.cwd.clone());
    std::thread::spawn(move || {
        let base = json!({"session_id": SID, "cwd": cwd, "mod_version": "0.4.0"});
        for _ in 0..80 {
            let reply = beat_at(port, &token, &base);
            if let Some(ops) = reply["ops"].as_array().filter(|o| !o.is_empty()) {
                let results: Vec<Value> = ops.iter().map(|o| {
                    let mut r = answer(o);
                    r["id"] = o["id"].clone();
                    r
                }).collect();
                let mut b = base.clone();
                b["results"] = json!(results);
                beat_at(port, &token, &b);
                return ops.clone();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        vec![]
    })
}

fn as_window(rig: &Rig, path: &str, body: Value) -> (u16, Value) {
    let token = rig.d.window_token();
    rig.d.post_window(path, &body, &token)
}

#[test]
fn a_command_goes_through_the_mod_and_its_output_comes_back() {
    let rig = Rig::start();
    let socket = rig.live(SID, "idle", json!({}));
    beat(&rig, json!({}));
    let m = fake_mod(&rig, |_| json!({"ok": true, "data": {"text": "Set model to Opus 5.5"}}));
    let (_, out) = as_window(&rig, "/api/dispatch", json!({"session_id": SID, "prompt": "/model opus", "mode": "auto"}));
    let ops = m.join().unwrap();
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["via"], "mod", "{out}");
    assert_eq!(out["output"], "Set model to Opus 5.5", "{out}");
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["op"], "command");
    assert_eq!(ops[0]["args"], json!({"command": "model", "args": "opus"}));
    assert!(rig.received(&socket).is_empty(), "nothing through the inbox");
    // Run once: never handed out again.
    assert!(beat(&rig, json!({}))["ops"].is_null());
}

#[test]
fn an_older_mod_gets_no_ops() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    beat(&rig, json!({"mod_version": "0.3.1"}));
    let (_, out) = as_window(&rig, "/api/session/op", json!({"session_id": SID, "op": "status"}));
    assert_eq!(out["ok"], false, "{out}");
    assert_eq!(out["no_mod"], true, "{out}");
}

#[test]
fn a_picker_or_a_view_is_refused_with_what_it_is() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    beat(&rig, json!({}));
    for (text, how, what) in [("/model", "picker", "model"), ("/cost", "view", "usage"),
                              ("/resume", "window", "history"), ("/rewind", "terminal", "ui")] {
        let (_, out) = as_window(&rig, "/api/dispatch", json!({"session_id": SID, "prompt": text, "mode": "auto"}));
        assert_eq!(out["ok"], false, "{text}: {out}");
        assert_eq!((out["how"].as_str(), out["what"].as_str()), (Some(how), Some(what)), "{text}: {out}");
        assert_eq!(out["show_window"], how == "terminal", "{text}: {out}");
    }
    assert!(beat(&rig, json!({}))["ops"].is_null(), "nothing was run");
}

#[test]
fn the_prompt_box_lists_the_sessions_commands_with_what_each_does() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    beat(&rig, json!({"commands": [
        {"name": "compact", "description": "Free up context", "source": "builtin"},
        {"name": "model", "description": "Set the AI model", "source": "builtin"},
        {"name": "deploy", "description": "Ship it", "source": "plugin"}]}));
    let (_, got) = rig.d.get(&format!("/api/slash/commands?session={SID}"));
    let list = got["commands"].as_array().unwrap();
    let find = |n: &str| list.iter().find(|c| c["name"] == n).cloned().unwrap_or(Value::Null);
    assert_eq!(got["from_mod"], true);
    assert_eq!(find("compact")["how"], "run");
    assert_eq!(find("model")["how"], "picker");
    assert_eq!(find("deploy")["source"], "plugin");
    assert_eq!(find("config")["aliases"], json!(["settings"]), "the tables' own are listed too");
    assert_eq!(find("login")["how"], "terminal");
}

#[test]
fn reads_and_config_changes_come_only_from_the_window() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    beat(&rig, json!({}));
    let (code, _) = rig.d.post("/api/session/op", &json!({"session_id": SID, "op": "config_set",
                                                       "args": {"key": "verbose", "value": true}}));
    assert_eq!(code, 403);
    assert!(beat(&rig, json!({}))["ops"].is_null(), "not passed to the mod");
    let m = fake_mod(&rig, |o| json!({"ok": true, "data": {"value": o["args"]["value"]}}));
    let (_, out) = as_window(&rig, "/api/session/op", json!({"session_id": SID, "op": "config_set",
                                                         "args": {"key": "verbose", "value": true}}));
    let ops = m.join().unwrap();
    assert_eq!(out, json!({"ok": true, "data": {"value": true}}));
    assert_eq!(ops[0]["args"], json!({"key": "verbose", "value": true}));
    let (_, out) = as_window(&rig, "/api/session/op", json!({"session_id": SID, "op": "rm -rf"}));
    assert_eq!(out["ok"], false);
}

#[test]
fn the_catalog_is_read_from_the_state_folder_and_never_probed_here() {
    let rig = Rig::start();
    let (_, got) = rig.d.get("/api/claude/catalog");
    assert_eq!(got["catalog"], Value::Null, "{got}");
    assert_eq!(got["probing"], false, "a test dispatcher never starts claude");
}
