//! Ports of test_dispatcher.py's Origins, WindowOnly, ConversationRoute,
//! EventStream and TailBlocks: what the HTTP surface accepts and sends.
#![cfg(windows)]
mod common;
use common::dispatch::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::time::{Duration, Instant};

// --------------------------------------------------------------- Origins
// The window is a browser page, and browsers send Origin on every POST.

/// POST /api/focus (with nothing named it answers without touching
/// anything) with this Origin (`%PORT%` is the dispatcher's own port).
fn post_with(rig: &Rig, origin: Option<&str>) -> u16 {
    let origin = origin.map(|o| o.replace("%PORT%", &rig.port.to_string()));
    let headers: Vec<(&str, &str)> = origin.iter().map(|o| ("Origin", o.as_str())).collect();
    rig.request("/api/focus", Some(b"{}".to_vec()), Some("application/json"), &headers).0
}

#[test]
fn the_windows_own_origin_is_accepted() {
    // The bug: every Send from the window was refused as cross-origin.
    let rig = Rig::start();
    assert_eq!(post_with(&rig, Some("http://127.0.0.1:%PORT%")), 200);
    assert_eq!(post_with(&rig, Some("http://localhost:%PORT%")), 200);
}

#[test]
fn no_origin_is_accepted() {
    assert_eq!(post_with(&Rig::start(), None), 200);
}

#[test]
fn a_foreign_page_is_refused() {
    assert_eq!(post_with(&Rig::start(), Some("https://evil.example")), 403);
}

#[test]
fn the_same_host_on_another_port_is_refused() {
    // Any other local server - a dev server, another tool - is a different
    // origin, and a page there must not drive this.
    assert_eq!(post_with(&Rig::start(), Some("http://127.0.0.1:1")), 403);
}

#[test]
fn an_opaque_origin_is_refused() {
    assert_eq!(post_with(&Rig::start(), Some("null")), 403);
}

// ------------------------------------------------------------ WindowOnly
// Only the window's own page, with its token, may type as you.

fn call(rig: &Rig, path: &str, body: Value, origin: bool, token: Option<&str>) -> (u16, Value) {
    let o = format!("http://127.0.0.1:{}", rig.port);
    let mut headers: Vec<(&str, &str)> = vec![];
    if origin {
        headers.push(("Origin", &o));
    }
    if let Some(t) = token {
        headers.push(("X-Imperatorium-Window", t));
    }
    rig.request(path, Some(serde_json::to_vec(&body).unwrap()), Some("application/json"), &headers)
}

#[test]
fn a_plain_api_call_cannot_type() {
    let rig = Rig::start();
    let real = rig.window_token();
    let body = json!({"session_id": SID, "prompt": "i approve"});
    for (origin, token) in [(false, None), (false, Some(real.as_str())), (true, None), (true, Some("guessed"))] {
        let (code, _) = call(&rig, "/api/typed", body.clone(), origin, token);
        assert_eq!(code, 403, "{origin} {token:?}");
    }
}

#[test]
fn the_window_reaches_it() {
    let rig = Rig::start();
    let token = rig.window_token();
    let (code, got) = call(&rig, "/api/typed", json!({"session_id": SID, "prompt": "hi"}), true, Some(&token));
    assert_eq!(code, 200);
    assert!(got["error"].as_str().unwrap().contains("Settings"), "{got}"); // reached, and still off
}

#[test]
fn only_the_window_turns_it_on() {
    let rig = Rig::start();
    let (_, body) = call(&rig, "/api/settings", json!({"changes": {"chat.send_as_typed": true}}), false, None);
    assert!(body["errors"]["chat.send_as_typed"].is_string(), "{body}");
    // And it is still off for the window itself.
    let token = rig.window_token();
    let (_, got) = call(&rig, "/api/typed", json!({"session_id": SID, "prompt": "hi"}), true, Some(&token));
    assert!(got["error"].as_str().unwrap().contains("Settings"), "{got}");
}

#[test]
fn the_page_carries_the_token() {
    let rig = Rig::start();
    let token = rig.window_token();
    for path in ["/", "/settings"] {
        let (code, page) = rig.get(path);
        assert_eq!(code, 200);
        assert!(page.as_str().unwrap().contains(&token), "{path}");
    }
}

// ----------------------------------------------------- ConversationRoute
// The window polls this every 1.5 s for the whole conversation.

fn conv(rig: &Rig, query: &str) -> Value {
    rig.get(&format!("/api/conversation?session={SID}{query}")).1
}

#[test]
fn the_default_is_the_whole_conversation() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    let turns: Vec<(&str, String)> = (0..6)
        .map(|i| (if i % 2 == 0 { "user" } else { "assistant" }, format!("turn {i}")))
        .collect();
    let refs: Vec<(&str, &str)> = turns.iter().map(|(w, t)| (*w, t.as_str())).collect();
    rig.transcript(SID, &chat_lines(SID, &refs, 1));
    let got = conv(&rig, "");
    assert_eq!(got["items"].as_array().unwrap().len(), 6, "{got}");
    assert!(got["sig"].is_string());
}

fn two_turns(rig: &Rig) {
    rig.live(SID, "idle", json!({}));
    rig.transcript(SID, &chat_lines(SID, &[("user", "hi")], 1));
}

#[test]
fn an_unchanged_conversation_is_not_resent() {
    let rig = Rig::start();
    two_turns(&rig);
    let first = conv(&rig, "");
    assert_eq!(first["items"].as_array().unwrap().len(), 1, "{first}");
    let again = conv(&rig, &format!("&known={}", first["sig"].as_str().unwrap()));
    assert_eq!(again["unchanged"], json!(true), "{again}");
    assert!(again.get("items").is_none());
}

#[test]
fn a_changed_conversation_is_sent_in_full() {
    let rig = Rig::start();
    two_turns(&rig);
    let first = conv(&rig, "");
    rig.transcript(SID, &chat_lines(SID, &[("user", "hi"), ("assistant", "yo")], 1));
    let again = conv(&rig, &format!("&known={}", first["sig"].as_str().unwrap()));
    assert_eq!(again["items"].as_array().unwrap().len(), 2, "{again}");
    assert_ne!(again["sig"], first["sig"]);
}

#[test]
fn blocks_sends_only_the_newest_blocks() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    let texts: Vec<String> = (0..5).map(|i| i.to_string()).collect();
    let mut turns = vec![("user", "a")];
    turns.extend(texts.iter().map(|t| ("assistant", t.as_str())));
    rig.transcript(SID, &chat_lines(SID, &turns, 1));
    let whole = conv(&rig, "");
    assert_eq!(whole["items"].as_array().unwrap().len(), 2, "{whole}"); // replies merge
    let out = conv(&rig, "&blocks=3");
    let blocks: Vec<&str> = out["items"][0]["blocks"].as_array().unwrap().iter().filter_map(|b| b["text"].as_str()).collect();
    assert_eq!(blocks, vec!["2", "3", "4"], "{out}");
    assert_eq!((out["start"].clone(), out["first_block"].clone(), out["total"].clone()), (json!(1), json!(2), json!(2)));
    // The signature is the whole conversation's, so `known` still works.
    assert_eq!(out["sig"], whole["sig"]);
    let known = conv(&rig, &format!("&blocks=3&known={}", out["sig"].as_str().unwrap()));
    assert_eq!(known["unchanged"], json!(true));
}

// ----------------------------------------------------------- EventStream
// /api/stream pushes the session list, and a hook wakes it at once.

fn open_stream(rig: &Rig) -> BufReader<std::net::TcpStream> {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", rig.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(b"GET /api/stream HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    BufReader::new(s)
}

/// The next `data:` line of an open stream, as the list it carries.
fn next_list(r: &mut BufReader<std::net::TcpStream>) -> Value {
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line).expect("the stream went quiet");
        assert!(n > 0, "the stream ended");
        if let Some(data) = line.strip_prefix("data: ") {
            return serde_json::from_str(data.trim_end()).unwrap();
        }
    }
}

#[test]
fn the_list_is_pushed_and_a_hook_pushes_the_change() {
    let rig = Rig::start();
    let socket = rig.live(SID, "idle", json!({}));
    let mut r = open_stream(&rig);
    let mut head = String::new();
    loop {
        let mut l = String::new();
        r.read_line(&mut l).unwrap();
        if l.trim().is_empty() {
            break;
        }
        head.push_str(&l);
    }
    assert!(head.to_lowercase().contains("content-type: text/event-stream"), "{head}");
    let first = next_list(&mut r);
    let ids: Vec<&str> = first["sessions"].as_array().unwrap().iter().filter_map(|s| s["sessionId"].as_str()).collect();
    assert_eq!(ids, vec![SID]);
    assert_ne!(first["sessions"][0]["attention"], json!(true));
    let t0 = Instant::now();
    rig.start_hook(SID, "tok", &socket);
    rig.hook(
        "Notification",
        SID,
        json!({"notification_type": "permission_prompt", "message": "Claude needs your permission"}),
    );
    loop {
        let row = next_list(&mut r)["sessions"][0].clone();
        if row["attention"] == json!(true) {
            break;
        }
    }
    assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
}

#[test]
fn an_unchanged_list_is_not_resent() {
    let rig = Rig::start();
    rig.live(SID, "idle", json!({}));
    let mut s = std::net::TcpStream::connect(("127.0.0.1", rig.port)).unwrap();
    s.write_all(b"GET /api/stream HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    // The first list can take a while to build; after it, a good while of
    // looks (the timers run at 1/50) at an unchanged list must send nothing.
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut got = Vec::new();
    let mut buf = [0u8; 65536];
    while !String::from_utf8_lossy(&got).contains("event: sessions") {
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        got.extend_from_slice(&buf[..n]);
    }
    // Keepalive comments keep arriving, so look for a while, not for quiet.
    s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let until = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < until {
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(_) => {}
        }
    }
    let text = String::from_utf8_lossy(&got);
    assert_eq!(text.matches("event: sessions").count(), 1, "{text}");
}
