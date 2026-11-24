//! POST /api/spawn (2026-11-22): where a new session runs and how it is
//! started, against the real dispatcher with fakecli as claude.exe and
//! wt.exe (nothing real starts). The rules themselves are unit-tested in
//! src/spawn.rs; this checks the wiring - the plan the window is shown,
//! the worktree made (and the uncommitted work copied into it), the command
//! line, the first message queued with its note, and the mid-turn cut.
#![cfg(windows)]

mod common;
use common::git::{bg_project, git};
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const A: &str = "7777aaaa-1111-4111-8111-111111111111";

fn calls(d: &Dispatcher, prog: &str, n: usize) -> Vec<Value> {
    wait_until(Duration::from_secs(15), || {
        let c: Vec<Value> = d
            .fake_log()
            .into_iter()
            .filter(|c| c["prog"] == prog && c["args"][0] != "agents")
            .collect();
        (c.len() >= n).then_some(c)
    })
    .unwrap_or_else(|| panic!("fewer than {n} {prog} calls: {:?}", d.fake_log()))
}

fn args(c: &Value) -> Vec<String> {
    c["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap_or("").to_string()).collect()
}

fn queued(d: &Dispatcher, sid: &str) -> Vec<String> {
    d.state_json()["queue"][sid]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|q| q["prompt"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn spawn_plans_and_starts_where_the_rules_say() {
    let tmp = TempDir::new("spawn");
    let (main, _feat) = bg_project(&tmp.path, "rs");
    let d = Dispatcher::start(Options {
        config: Some(json!({"background": {"permission_mode": "acceptEdits"}})),
        fake_cli: vec!["claude.exe", "wt.exe"],
        home: Some(tmp.join("rs-home")),
        ..Options::new()
    });
    let token = d.window_token();
    let m = main.to_string_lossy().into_owned();
    // Its transcript, for the mid-turn cut further down: two turns, the
    // second still running (written first: lookups cache a miss).
    let tdir = d.claude.join("projects").join("x");
    std::fs::create_dir_all(&tdir).unwrap();
    let lines = [
        json!({"type": "user", "uuid": "u1", "sessionId": A}),
        json!({"type": "assistant", "uuid": "a1", "message": {"stop_reason": "end_turn"}}),
        json!({"type": "user", "uuid": "u2"}),
        json!({"type": "assistant", "uuid": "a2", "message": {"stop_reason": "tool_use"}}),
    ];
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(tdir.join(format!("{A}.jsonl")), text).unwrap();
    d.session_record(A, json!({"cwd": m, "name": "parent", "kind": "interactive", "status": "idle"}));
    wait_until(Duration::from_secs(10), || {
        let (_, s) = d.get("/api/sessions");
        s.to_string().contains(A).then_some(())
    });

    // Only the window starts sessions this way.
    let (code, _) = d.post("/api/spawn", &json!({"kind": "new", "cwd": m, "plan": true}));
    assert_eq!(code, 403);

    // A live session is in the main checkout: a fresh one gets a worktree.
    let (_, plan) = d.post_window("/api/spawn", &json!({"kind": "new", "session_id": A, "plan": true}), &token);
    assert_eq!(plan["ok"], true, "{plan}");
    assert_eq!((plan["placement"].as_str(), plan["warning"].as_str()), (Some("worktree"), Some("")), "{plan}");

    // A fork of it, with uncommitted work in its folder: warned.
    std::fs::write(main.join("f.txt"), "one\nedited\n").unwrap();
    std::fs::write(main.join("new.txt"), "brand new\n").unwrap();
    let (_, plan) = d.post_window("/api/spawn", &json!({"kind": "fork", "session_id": A, "plan": true}), &token);
    assert_eq!((plan["placement"].as_str(), plan["warning"].as_str()), (Some("worktree"), Some("dirty")), "{plan}");
    assert_eq!(plan["dirty_count"], 2, "{plan}");
    assert_eq!(plan["mid_turn"], false);

    // A background session with nothing to do starts idle since 2026-11-23
    // (tests/close_background.rs), so it is no longer refused here.

    // Copy and continue: a tab on a fork in a new worktree that has the work.
    let (_, r) = d.post_window("/api/spawn", &json!({"kind": "fork", "where": "window", "session_id": A,
        "prompt": "Look at the parser", "copy": true}), &token);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["placement"], "worktree");
    assert_eq!(r["copied"], 2);
    let wt = r["worktree"].as_str().unwrap().to_string();
    assert!(wt.replace('\\', "/").contains(".claude/worktrees/look-at-the-parser"), "{wt}");
    assert_eq!(std::fs::read_to_string(Path::new(&wt).join("new.txt")).unwrap(), "brand new\n");
    assert!(std::fs::read_to_string(Path::new(&wt).join("f.txt")).unwrap().contains("edited"));
    let fork = r["session_id"].as_str().unwrap().to_string();
    let tab = args(&calls(&d, "wt", 1)[0]);
    let at = tab.iter().position(|a| a == "claude").unwrap();
    assert_eq!(tab[at..], ["claude", "--resume", A, "--fork-session", "--session-id", &fork]);
    assert_eq!(tab[tab.iter().position(|a| a == "-d").unwrap() + 1], wt);
    let first = queued(&d, &fork);
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(first[0].starts_with("[Imperatorium] You are a fork"), "{}", first[0]);
    assert!(first[0].contains("2 uncommitted file(s) were copied in") && first[0].ends_with("\n\nLook at the parser"));

    // "Use the same folder": a fresh tab in the main checkout, told it shares.
    let (_, r) = d.post_window("/api/spawn", &json!({"kind": "new", "where": "window", "session_id": A,
        "prompt": "Tidy the docs", "worktree": false}), &token);
    assert_eq!((r["ok"].as_bool(), r["placement"].as_str()), (Some(true), Some("here")), "{r}");
    let tab = args(&calls(&d, "wt", 2)[1]);
    assert_eq!(tab[tab.iter().position(|a| a == "-d").unwrap() + 1].to_lowercase(), m.to_lowercase());
    let sid = r["session_id"].as_str().unwrap();
    assert!(queued(&d, sid)[0].contains("Another session is working in this same folder"));

    // Mid-turn: the fork's first turn runs in print mode, cut at the end of
    // the last finished turn, in the background setting's permission mode.
    d.session_record(A, json!({"cwd": m, "name": "parent", "kind": "interactive", "status": "busy"}));
    let busy = wait_until(Duration::from_secs(10), || {
        let (_, p) = d.post_window("/api/spawn", &json!({"kind": "fork", "session_id": A, "plan": true}), &token);
        (p["mid_turn"] == true).then_some(p)
    });
    assert!(busy.is_some(), "the parent never showed as busy");
    git(&main, &["stash", "-u"]);
    let (_, r) = d.post_window("/api/spawn", &json!({"kind": "fork", "where": "background", "session_id": A,
        "prompt": "Write the changelog"}), &token);
    assert_eq!((r["ok"].as_bool(), r["first_turn"].as_str()), (Some(true), Some("headless")), "{r}");
    let fork = r["session_id"].as_str().unwrap().to_string();
    let print = calls(&d, "claude", 1)
        .into_iter()
        .map(|c| args(&c))
        .find(|a| a.first().map(String::as_str) == Some("-p"))
        .expect("a claude -p call");
    assert_eq!(print[..11], ["-p", "--resume", A, "--resume-session-at", "a1", "--fork-session", "--session-id",
                             &fork, "--output-format", "json", "--permission-mode"]);
    assert_eq!(print[11], "acceptEdits");
    assert!(print.last().unwrap().ends_with("\n\nWrite the changelog"));
    let done = wait_until(Duration::from_secs(10), || {
        d.state_json()["events"].as_array().cloned().unwrap_or_default().into_iter()
            .find(|e| e["kind"] == "fork_first_turn" && e["session_id"] == json!(fork))
    });
    assert_eq!(done.expect("the first turn's end is recorded")["ok"], true);

    // Mid-turn with nothing typed: refused, as the fork needs its message.
    let (_, r) = d.post_window("/api/spawn", &json!({"kind": "fork", "where": "window", "session_id": A}), &token);
    assert!(r["error"].as_str().unwrap().contains("mid-turn"), "{r}");
}
