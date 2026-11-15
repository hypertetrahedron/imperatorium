//! Ports of test_public.py: what leaves the dispatcher over HTTP is never a
//! session's inbox token (until 2026-11-12 /api/sessions and /api/stream
//! returned it to whatever asked 127.0.0.1:8792, and with it the power to
//! put a prompt straight into that session).
#![cfg(windows)]
mod common;
use common::dispatch::*;
use serde_json::json;
use std::io::{BufRead, BufReader, Write};

const SECRET: &str = "0123456789abcdef0123456789abcdef";

/// A dispatchable session (socket + token) and one with neither.
fn rig_with_a_token() -> (Rig, String) {
    let rig = Rig::start();
    let socket = rig.live(SID, "idle", json!({}));
    rig.live(OTHER, "idle", json!({"name": "two", "messagingSocketPath": null}));
    rig.start_hook(SID, SECRET, &socket);
    (rig, socket)
}

#[test]
fn the_session_list_has_no_tokens() {
    let (rig, socket) = rig_with_a_token();
    rig.invalidate();
    let (code, body) = rig.get("/api/sessions");
    assert_eq!(code, 200);
    assert!(!body.to_string().contains(SECRET));
    let rows = body["sessions"].as_array().unwrap();
    let one = rows.iter().find(|r| r["sessionId"] == SID).unwrap();
    assert!(one.get("token").is_none());
    assert_eq!(one["dispatchable"], json!(true)); // what the window needs stays
    assert_eq!(one["socket"], json!(socket));
}

#[test]
fn the_stream_has_no_tokens() {
    let (rig, _) = rig_with_a_token();
    let mut s = std::net::TcpStream::connect(("127.0.0.1", rig.port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    s.write_all(b"GET /api/stream HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut r = BufReader::new(s);
    let mut got = String::new();
    loop {
        let mut line = String::new();
        r.read_line(&mut line).expect("the first list");
        got.push_str(&line);
        if line.starts_with("data:") {
            break;
        }
    }
    assert!(got.contains("event: sessions"), "{got}");
    assert!(!got.contains(SECRET));
    assert!(got.contains("\"dispatchable\":true"), "{got}");
}

#[test]
fn the_dispatcher_keeps_its_own_copy() {
    // Serving the list never takes the token out of the dispatcher's own
    // rows (the public copy is a copy): it still authenticates a prompt.
    let (rig, socket) = rig_with_a_token();
    rig.get("/api/sessions");
    assert_eq!(rig.tokens()[SID], json!(SECRET));
    let got = rig.dispatch(json!({"session_id": SID, "prompt": "hi"}));
    assert_eq!(got["ok"], json!(true), "{got}");
    eventually("the prompt reaches its pipe", || !rig.received(&socket).is_empty());
    assert_eq!(auth_tokens(&rig.received(&socket)), vec![SECRET.to_string()]);
}

#[test]
fn public_snapshot_copies() {
    // Asking for the list twice, with a dispatch between, never loses a
    // row's own token: the first copy sent out did not alias the rows.
    let (rig, socket) = rig_with_a_token();
    let first = rig.get("/api/sessions").1;
    assert!(!first.to_string().contains(SECRET));
    rig.invalidate();
    assert!(!rig.get("/api/sessions").1.to_string().contains(SECRET));
    let got = rig.dispatch(json!({"session_id": SID, "prompt": "again"}));
    assert_eq!(got["ok"], json!(true), "{got}");
    eventually("it still authenticates", || !rig.received(&socket).is_empty());
    assert_eq!(auth_tokens(&rig.received(&socket)), vec![SECRET.to_string()]);
}
