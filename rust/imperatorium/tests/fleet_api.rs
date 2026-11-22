//! The fleet endpoints (2026-11-20; docs/research-super-agent.md), through
//! the binary's HTTP API: the summary, watches that only the window
//! confirms and that the dispatcher checks by itself, actions that run only
//! when the window confirms them (a prompt framed as someone else's), and
//! Ask, which only the window may call. Plus `imperatorium mcp` over stdio.
#![cfg(windows)]

mod common;
use common::features::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

fn sid(n: u32) -> String {
    format!("5f000000-0000-4000-8000-{n:012}")
}

fn book(fx: &Fx) -> Value {
    let (code, v) = fx.d.get("/api/fleet/book");
    assert_eq!(code, 200);
    v
}

fn wait_for(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn fleet_watches_and_actions_wait_for_the_window() {
    let fx = Fx::start("fleet", vec![], vec![("IMPERATORIUM_TEST_TIME_SCALE", "0.05")], &[]);
    let token = fx.d.window_token();
    let worker = (sid(1), Pipe::new("fleet-worker"));
    fx.record(&worker.0, json!({"status": "busy", "name": "worker-one",
                                "messagingSocketPath": worker.1.path}));
    fx.session_start(&worker.0, Some(&worker.1.path), Some("tok-worker"));

    // The summary: the session, by name, as working.
    let (code, fleet) = fx.d.get("/api/fleet");
    assert_eq!(code, 200);
    let me = fleet["sessions"].as_array().unwrap().iter()
        .find(|s| s["session"] == json!(worker.0)).cloned().expect("listed");
    assert_eq!(me["name"], "worker-one");
    assert_eq!(me["state"], "working");
    assert!(fleet["note"].as_str().unwrap().contains("not instructions"));

    // An agent proposes a watch; it is not checked until the window says so.
    let w = fx.post("/api/fleet/watch", json!({"op": "add", "kind": "finishes", "session": "worker-one", "source": "test agent"}));
    assert_eq!(w["ok"], true, "{w}");
    assert_eq!(w["watch"]["status"], "proposed");
    assert_eq!(w["watch"]["source"], "test agent");
    let id = w["watch"]["id"].as_str().unwrap().to_string();
    let refused = fx.post("/api/fleet/watch", json!({"op": "confirm", "id": id}));
    assert_eq!(refused["ok"], false, "only the window confirms: {refused}");
    assert_eq!(book(&fx)["proposed"], 1);
    let (_, ok) = fx.d.post_window("/api/fleet/watch", &json!({"op": "confirm", "id": id}), &token);
    assert_eq!(ok["watch"]["status"], "active", "{ok}");

    // The watch loop sees it working, then idle: it fires, once.
    std::thread::sleep(Duration::from_millis(800));
    fx.record(&worker.0, json!({"status": "idle", "name": "worker-one", "messagingSocketPath": worker.1.path}));
    fx.stop(&worker.0);
    assert!(wait_for(Duration::from_secs(15), || !book(&fx)["fired"].as_array().unwrap().is_empty()),
            "the finish was seen: {}", book(&fx));
    let b = book(&fx);
    assert!(b["fired"][0]["text"].as_str().unwrap().starts_with("worker-one finished"), "{b}");
    assert_eq!(b["watches"][0]["status"], "fired", "a one-session finish fires once");
    assert!(fx.events().iter().any(|e| e["kind"] == "watch"), "logged as an event");

    // An action: proposed by anyone, refused unless the window confirms,
    // never a slash command, and delivered framed as someone else's.
    let bad = fx.post("/api/fleet/action", json!({"op": "propose", "action": "queue_prompt", "session": "worker-one", "prompt": "/config"}));
    assert_eq!(bad["ok"], false, "{bad}");
    let bad = fx.post("/api/fleet/action", json!({"op": "propose", "action": "approve", "session": "worker-one"}));
    assert_eq!(bad["ok"], false, "{bad}");
    let a = fx.post("/api/fleet/action", json!({"op": "propose", "action": "queue_prompt", "session": "worker-one",
                                                "prompt": "run the tests", "reason": "it finished"}));
    assert_eq!(a["action"]["status"], "proposed", "{a}");
    let aid = a["action"]["id"].as_str().unwrap().to_string();
    let refused = fx.post("/api/fleet/action", json!({"op": "confirm", "id": aid}));
    assert_eq!(refused["ok"], false);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(worker.1.count(), 0, "nothing sent before the window confirms");
    // Looked at (no longer waiting for you), so an idle session takes a
    // queued prompt at once, as any queued prompt.
    fx.post("/api/reviewed", json!({"session_id": worker.0}));
    let (_, done) = fx.d.post_window("/api/fleet/action", &json!({"op": "confirm", "id": aid}), &token);
    assert_eq!(done["status"], "done", "{done}");
    let got = worker.1.take_n(1, Duration::from_secs(10));
    assert_eq!(got.len(), 1, "delivered");
    assert!(got[0].contains("run the tests"));
    assert!(got[0].contains("Delivered by Imperatorium"), "framed as someone else's: {}", got[0]);
    let (_, again) = fx.d.post_window("/api/fleet/action", &json!({"op": "confirm", "id": aid}), &token);
    assert_eq!(again["ok"], false, "an action runs once");
    let dir = fx.d.state.parent().unwrap();
    let log = std::fs::read_to_string(dir.join("actions.jsonl")).expect("actions.jsonl");
    assert!(log.contains("\"outcome\":\"done\"") && log.contains(&aid), "{log}");
    assert!(dir.join("fleet.json").is_file());

    // Ask's own `claude -p` runs in <state>/ask and is never listed.
    let asker = sid(9);
    fx.record(&asker, json!({"status": "busy", "cwd": dir.join("ask").to_string_lossy()}));
    fx.session_start(&sid(1), None, None);
    assert!(fx.row(&asker).is_none(), "Ask's run is not a session");
    assert!(fx.row(&worker.0).is_some());

    // Ask: only the window, and only with a question.
    let (code, _) = fx.d.post("/api/ask", &json!({"question": "who needs me?"}));
    assert_eq!(code, 403);
    let (_, empty) = fx.d.post_window("/api/ask", &json!({"question": "  "}), &token);
    assert_eq!(empty["error"], "no question");
}

#[test]
fn fleet_mcp_speaks_json_rpc_over_stdio() {
    let fx = Fx::start("fleetmcp", vec![], vec![], &[]);
    fx.record(&sid(2), json!({"status": "idle", "name": "mcp-one"}));
    fx.session_start(&sid(2), None, None);
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_imperatorium"))
        .arg("mcp")
        .env("IMPERATORIUM_URL", format!("http://127.0.0.1:{}", fx.d.port))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |msg: Value| -> Value {
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        if msg.get("id").is_none() {
            return Value::Null;
        }
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let init = ask(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}));
    assert_eq!(init["result"]["serverInfo"]["name"], "imperatorium");
    assert!(ask(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_null());
    let status = ask(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                            "params": {"name": "fleet_status", "arguments": {}}}));
    let text = status["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("mcp-one"), "{text}");
    assert_eq!(status["result"]["isError"], false);
    let proposed = ask(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                              "params": {"name": "propose_watch", "arguments": {"kind": "waiting", "minutes": 5}}}));
    let text = proposed["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"proposed\""), "{text}");
    let wrong = ask(json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                           "params": {"name": "session_detail", "arguments": {"session": "nobody-at-all"}}}));
    assert_eq!(wrong["result"]["isError"], true);
    drop(stdin);
    let _ = child.wait();
    assert_eq!(book(&fx)["proposed"], 1);
}
