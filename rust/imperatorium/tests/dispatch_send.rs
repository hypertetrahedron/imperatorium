//! Ports of test_dispatcher.py's Routing, SlashCommands, SendTyped and the
//! local half of FocusRouting: where a prompt goes (a live session's inbox,
//! a background session, a typed slash command) and what is refused.
//!
//! Safety: nothing here may open a real window, so no test lets a dispatch
//! reach the deep-link launch (`auto` with nothing live, `launch`); those
//! Python tests are ported as far as the refusal or the choice before it
//! (see docs/python-test-coverage/test_dispatcher.md). Sessions that are
//! typed into are fakecli consoles; `claude` and `wt` are fakecli too.
#![cfg(windows)]
mod common;
use common::dispatch::*;
use common::wait_until;
use serde_json::{json, Value};
use std::time::Duration;

const TEXT: &str = "project: laser ledger, do the thing";

fn fakes(o: &mut common::Options) {
    o.fake_cli = vec!["claude.exe", "wt.exe"];
}

/// A live session in the rig's laser-ledger, with its credential reported.
fn reachable(rig: &Rig, sid: &str, status: &str, extra: Value) -> String {
    let socket = rig.live(sid, status, extra);
    rig.start_hook(sid, &format!("tok-{sid}"), &socket);
    socket
}

fn nothing_sent(rig: &Rig, sockets: &[&String]) {
    for s in sockets {
        assert!(rig.received(s).is_empty(), "{:?}", rig.received(s));
    }
}

// --------------------------------------------------------------- Routing

#[test]
fn injects_into_a_live_dispatchable_session() {
    let rig = Rig::start();
    let socket = reachable(&rig, SID, "idle", json!({}));
    let out = rig.dispatch(json!({"text": TEXT}));
    assert_eq!(out["action"], json!("inject"), "{out}");
    eventually("it arrives", || !rig.received(&socket).is_empty());
    let sent = user_texts(&rig.received(&socket));
    assert_eq!(sent.len(), 1);
    assert!(sent[0].ends_with("\n\ndo the thing"), "{sent:?}");
}

#[test]
fn a_dispatched_prompt_says_it_has_no_sender_to_answer() {
    // Two sessions in one project: without this, the receiver took the
    // prompt for a peer's and answered the other session by SendMessage.
    let rig = Rig::start();
    let socket = reachable(&rig, SID, "idle", json!({}));
    rig.dispatch(json!({"text": TEXT}));
    eventually("it arrives", || !rig.received(&socket).is_empty());
    let sent = &user_texts(&rig.received(&socket))[0];
    assert!(sent.starts_with("[Delivered by Imperatorium, the session dispatcher"), "{sent}");
    assert!(sent.contains("do not use SendMessage"));
    assert!(sent.ends_with("do the thing"));
    assert_eq!(sent.rsplit("\n\n").next().unwrap(), "do the thing");
}

#[test]
fn inject_mode_refuses_rather_than_launching() {
    let rig = Rig::start();
    let out = rig.dispatch(json!({"text": TEXT, "mode": "inject"}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(rig.events().iter().all(|e| e["kind"] != "dispatch_launch"));
}

#[test]
fn dry_run_touches_nothing() {
    let rig = Rig::start();
    let socket = reachable(&rig, SID, "idle", json!({}));
    let out = rig.dispatch(json!({"text": TEXT, "mode": "dry-run"}));
    assert_eq!(out["action"], json!("dry-run"), "{out}");
    nothing_sent(&rig, &[&socket]);
    assert!(rig.events().iter().all(|e| e["kind"] != "dispatch_launch" && e["kind"] != "dispatch_inject"));
}

#[test]
fn an_unknown_mode_is_refused_not_launched() {
    // A typo fell through to launch and opened a terminal window
    // (persona runs, 2026-11-24).
    let rig = Rig::start();
    let out = rig.dispatch(json!({"text": TEXT, "mode": "sideways"}));
    assert_eq!(out["ok"], json!(false), "{out}");
    let error = out["error"].as_str().unwrap_or("");
    assert!(error.contains("auto, inject, launch, background, dry-run"), "{error}");
    assert!(error.contains("sideways"), "{error}");
    assert!(rig.events().iter().all(|e| !e["kind"].as_str().unwrap_or("").starts_with("dispatch")));
}

#[test]
fn a_dry_run_can_name_a_session() {
    // `imperatorium send -s SESSION --dry-run` always failed: a session
    // was honoured only in auto and inject modes.
    let rig = Rig::start();
    let socket = reachable(&rig, SID, "idle", json!({}));
    let out = rig.dispatch(json!({"session_id": SID, "prompt": "hi", "mode": "dry-run"}));
    assert_eq!(out["ok"], json!(true), "{out}");
    assert_eq!(out["action"], json!("dry-run"));
    assert_eq!(out["would"], json!("inject"));
    assert_eq!(out["reachable"], json!(true));
    let slash = rig.dispatch(json!({"session_id": SID, "prompt": "/compact", "mode": "dry-run"}));
    assert_eq!(slash["would"], json!("command"), "{slash}");
    let none = rig.dispatch(json!({"session_id": "no-such", "prompt": "hi", "mode": "dry-run"}));
    assert_eq!(none["ok"], json!(false), "{none}");
    nothing_sent(&rig, &[&socket]);
}

#[test]
fn a_picked_path_wins_over_a_shared_name() {
    // Two folders called bookoferrantpages: the picker's one opened the
    // other, because only the name was sent and it resolved first.
    let rig = Rig::start();
    let y = rig.projects.join("bookoferrantpages");
    std::fs::create_dir_all(&y).unwrap();
    let c = rig.projects.parent().unwrap().join(format!("elsewhere-{}", std::process::id())).join("bookoferrantpages");
    std::fs::create_dir_all(&c).unwrap();
    // The second is known as the folder a session runs in.
    rig.live(OTHER, "idle", json!({"cwd": c.to_string_lossy()}));
    let out = rig.dispatch(json!({"project": "bookoferrantpages", "path": c.to_string_lossy(),
                                  "prompt": "hi", "mode": "dry-run", "confirm": true}));
    let _ = std::fs::remove_dir_all(c.parent().unwrap());
    assert_eq!(out["action"], json!("dry-run"), "{out}");
    let got = out["project"]["path"].as_str().unwrap().to_lowercase().replace('/', "\\");
    assert_eq!(got, c.to_string_lossy().to_lowercase().replace('/', "\\"));
    assert_eq!(out["project"]["source"], json!("session"));
}

#[test]
fn a_path_not_in_the_catalogue_dispatches_nothing() {
    let rig = Rig::start();
    let out = rig.dispatch(json!({"path": "C:/nowhere", "prompt": "hi", "mode": "dry-run"}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(rig.events().iter().all(|e| !e["kind"].as_str().unwrap_or("").starts_with("dispatch")));
}

#[test]
fn an_unmatched_project_dispatches_nothing() {
    let rig = Rig::start();
    let socket = reachable(&rig, SID, "idle", json!({}));
    let out = rig.dispatch(json!({"text": "project: home assistant, why offline"}));
    assert_eq!(out["ok"], json!(false), "{out}");
    nothing_sent(&rig, &[&socket]);
    assert!(rig.events().iter().all(|e| !e["kind"].as_str().unwrap_or("").starts_with("dispatch")));
}

#[test]
fn a_failed_inject_falls_back_in_auto_mode() {
    // A session can die between the snapshot and the write. In `inject`
    // mode that is the answer (auto would go on to open a window, which a
    // test must not do): the error says the pipe could not be opened.
    let rig = Rig::start();
    let gone = format!(r"\\.\pipe\imp-dispatch-gone-{}", std::process::id());
    rig.live(SID, "idle", json!({"messagingSocketPath": gone}));
    rig.start_hook(SID, "tok", &gone);
    let out = rig.dispatch(json!({"text": TEXT, "mode": "inject"}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(out["error"].as_str().unwrap().contains("cannot open"), "{out}");
}

#[test]
fn a_stale_credential_is_never_injected_into() {
    let rig = Rig::start();
    let old = reachable(&rig, SID, "idle", json!({}));
    let new = rig.pipe();
    rig.touch(SID, json!({"messagingSocketPath": new}));
    let out = rig.dispatch(json!({"text": TEXT, "mode": "inject"}));
    assert_eq!(out["ok"], json!(false), "{out}");
    nothing_sent(&rig, &[&old, &new]);
    let out = rig.dispatch(json!({"session_id": SID, "prompt": "go"}));
    assert_eq!(out["ok"], json!(false));
    assert!(out["error"].as_str().unwrap().contains("credential"), "{out}");
    nothing_sent(&rig, &[&old, &new]);
}

fn bg_calls(rig: &Rig) -> Vec<Value> {
    rig.fake_log()
        .into_iter()
        .filter(|e| e["prog"] == "claude" && e["args"].as_array().is_some_and(|a| a.iter().any(|x| x == "--bg")))
        .collect()
}

fn args_of(call: &Value) -> Vec<String> {
    call["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect()
}

#[test]
fn background_mode_starts_a_session_and_never_opens_a_window() {
    let rig = Rig::with(json!({}), fakes);
    let out = rig.dispatch(json!({"text": TEXT, "mode": "background"}));
    assert_eq!(out["action"], json!("background"), "{out}");
    assert_eq!(out["bg_id"], json!("1f2e3d4c"));
    let calls = bg_calls(&rig);
    assert_eq!(calls.len(), 1);
    assert_eq!(args_of(&calls[0]).last().unwrap(), "do the thing");
    let want = rig.cwd.to_lowercase().replace('/', "\\");
    assert_eq!(calls[0]["cwd"].as_str().unwrap().to_lowercase().replace('/', "\\"), want);
    // No `wt` (no window), no deep link.
    assert!(rig.fake_log().iter().all(|e| e["prog"] != "wt"));
    assert!(rig.events().iter().all(|e| e["kind"] != "dispatch_launch"));
}

#[test]
fn background_mode_prefers_a_new_session_over_a_live_one() {
    // "Go and do this" is not "put it in the window I am watching".
    let rig = Rig::with(json!({}), fakes);
    let socket = reachable(&rig, SID, "idle", json!({}));
    let out = rig.dispatch(json!({"text": TEXT, "mode": "background"}));
    assert_eq!(out["action"], json!("background"), "{out}");
    nothing_sent(&rig, &[&socket]);
}

#[test]
fn background_options_from_config_are_passed_through() {
    let rig = Rig::with(
        json!({"background": {"model": "sonnet", "permission_mode": "acceptEdits"}}),
        fakes,
    );
    rig.dispatch(json!({"project": "laser-ledger", "prompt": "go", "mode": "background"}));
    let args = args_of(&bg_calls(&rig)[0]);
    let after = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].clone());
    assert_eq!(after("--model").as_deref(), Some("sonnet"), "{args:?}");
    assert_eq!(after("--permission-mode").as_deref(), Some("acceptEdits"), "{args:?}");
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// laser-ledger as a repository, with a feature branch in a checkout of its
/// own beside it (as the session in test_dispatcher.py lives in).
fn repo_with_a_feature_checkout(rig: &Rig) -> std::path::PathBuf {
    let main = rig.projects.join("laser-ledger");
    git(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(main.join("a.txt"), "a").unwrap();
    git(&main, &["add", "a.txt"]);
    git(&main, &["commit", "-q", "-m", "first"]);
    let feat = rig.projects.join("ll-feat");
    git(&main, &["worktree", "add", "-q", "-b", "feat", &feat.to_string_lossy()]);
    feat
}

#[test]
fn background_runs_in_a_worktree_of_the_chosen_sessions_branch() {
    // The session picked lives in a feature-branch checkout beside the
    // project; its worktree is made from there, and the session runs in it.
    let rig = Rig::with(json!({}), fakes);
    let feat = repo_with_a_feature_checkout(&rig);
    rig.live(SID, "idle", json!({"cwd": feat.to_string_lossy()}));
    let out = rig.dispatch(json!({"project": "laser-ledger", "prompt": "do the thing",
                                  "mode": "background", "session_id": SID}));
    assert_eq!(out["ok"], json!(true), "{out}");
    assert_eq!((out["branch"].clone(), out["base"].clone()), (json!("worktree-do-the-thing"), json!("feat")));
    let wt = out["worktree"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&wt).is_dir(), "{wt}");
    let calls = bg_calls(&rig);
    let norm = |s: &str| s.to_lowercase().replace('/', "\\");
    assert_eq!(norm(calls[0]["cwd"].as_str().unwrap()), norm(&wt));
}

#[test]
fn a_fork_runs_in_a_worktree_of_its_sessions_branch() {
    // A fork runs beside the session it came from: in a window or in the
    // background, it gets a worktree made from that session's branch.
    let rig = Rig::with(json!({}), fakes);
    let feat = repo_with_a_feature_checkout(&rig);
    rig.live(SID, "idle", json!({"cwd": feat.to_string_lossy()}));
    let norm = |s: &str| s.to_lowercase().replace('/', "\\");
    let out = rig.d.post("/api/resume", &json!({"session_id": SID, "where": "background", "fork": true})).1;
    assert_eq!(out["ok"], json!(true), "{out}");
    assert_eq!(out["base"], json!("feat"), "{out}");
    let wt = out["worktree"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&wt).is_dir(), "{wt}");
    assert!(wt.contains("fork-"), "{wt}");
    let calls = bg_calls(&rig);
    assert_eq!(norm(calls[0]["cwd"].as_str().unwrap()), norm(&wt));
    let fork = rig.events().into_iter().find(|e| e["kind"] == "fork").expect("a fork event");
    assert_eq!(norm(fork["worktree"].as_str().unwrap()), norm(&wt));
    // A second fork of it gets a worktree of its own.
    let out = rig.d.post("/api/resume", &json!({"session_id": SID, "where": "window", "fork": true})).1;
    assert_eq!(out["ok"], json!(true), "{out}");
    let wt2 = out["worktree"].as_str().unwrap().to_string();
    assert_ne!(norm(&wt2), norm(&wt));
    // The tab is started and let go, so its log line comes a moment later.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let tab = loop {
        if let Some(t) = rig.fake_log().into_iter().rfind(|e| e["prog"] == "wt") {
            break t;
        }
        assert!(std::time::Instant::now() < until, "no tab was opened");
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert_eq!(norm(tab["cwd"].as_str().unwrap()), norm(&wt2));
    // A plain resume does not make one.
    let before = std::fs::read_dir(rig.projects.join("laser-ledger").join(".claude").join("worktrees")).unwrap().count();
    let _ = rig.d.post("/api/resume", &json!({"session_id": SID, "where": "background"}));
    let after = std::fs::read_dir(rig.projects.join("laser-ledger").join(".claude").join("worktrees")).unwrap().count();
    assert_eq!(before, after);
}

#[test]
fn a_worktree_whose_session_never_started_is_removed() {
    // No `claude` on the PATH: the start fails, and the worktree made for
    // it goes again.
    let rig = Rig::start();
    git(&rig.projects.join("laser-ledger"), &["init", "-q", "-b", "main"]);
    std::fs::write(rig.projects.join("laser-ledger").join("a.txt"), "a").unwrap();
    git(&rig.projects.join("laser-ledger"), &["add", "a.txt"]);
    git(&rig.projects.join("laser-ledger"), &["commit", "-q", "-m", "first"]);
    let out = rig.dispatch(json!({"project": "laser-ledger", "prompt": "go", "mode": "background"}));
    assert_eq!(out["ok"], json!(false), "{out}");
    let made = rig.projects.join("laser-ledger").join(".claude").join("worktrees").join("go");
    assert!(!made.exists(), "{made:?} is still there");
}

#[test]
fn a_failed_background_start_is_reported_not_swallowed() {
    let rig = Rig::start(); // no claude on the stand-in PATH
    let out = rig.dispatch(json!({"project": "laser-ledger", "prompt": "go", "mode": "background"}));
    assert_eq!(out["ok"], json!(false));
    assert!(out["error"].as_str().unwrap().contains("PATH"), "{out}");
    assert!(rig.events().iter().all(|e| e["kind"] != "dispatch_launch"));
}

#[test]
fn missing_prompt_or_project_is_refused() {
    let rig = Rig::start();
    assert_eq!(rig.dispatch(json!({"text": "project: laser ledger,"}))["ok"], json!(false));
    assert_eq!(rig.dispatch(json!({"text": "just do something"}))["ok"], json!(false));
}

#[test]
fn idle_session_wins_over_busy_one() {
    let rig = Rig::start();
    let busy = rig.live(SID, "busy", json!({"name": "busy-one"}));
    rig.start_hook(SID, "t1", &busy);
    let idle = rig.live(OTHER, "idle", json!({"name": "idle-one"}));
    rig.start_hook(OTHER, "t2", &idle);
    let out = rig.dispatch(json!({"text": "project: laser ledger, go"}));
    assert_eq!(out["session"], json!(OTHER), "{out}");
    nothing_sent(&rig, &[&busy]);
}

#[test]
fn explicit_project_and_prompt_skip_parsing() {
    let rig = Rig::start();
    let out = rig.dispatch(json!({"project": "gem-trip", "prompt": "fix it", "mode": "dry-run"}));
    assert_eq!(out["project"]["name"], json!("gem-trip"), "{out}");
}

// ---------------------------------------------------------- FocusRouting
// Raising the window a session is in, rather than any window. The window
// titles of a real terminal are not something a test can stand in for, so
// only what is decided without one is ported: nothing raised is reported.

#[test]
fn nothing_raised_at_all_is_reported_not_papered_over() {
    let rig = Rig::start();
    // A process that does not exist has no terminal and no window.
    rig.live(SID, "idle", json!({"pid": 4_000_000_000u64}));
    let (_, out) = rig.post("/api/focus", &json!({"session_id": SID}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert_eq!(out["error"], json!("could not raise its window"));
}

#[test]
fn a_failed_focus_never_launches_a_second_session() {
    // A focus request that could not raise the window once opened a brand
    // new session on the same project instead.
    let rig = Rig::start();
    rig.live(SID, "idle", json!({"pid": 4_000_000_000u64}));
    let (_, out) = rig.post("/api/focus", &json!({"session_id": SID}));
    assert_eq!(out["ok"], json!(false));
    assert!(rig.events().iter().all(|e| e["kind"] != "dispatch_launch"));
    assert!(out.get("url").is_none(), "{out}");
}

// --------------------------------------------------------- SlashCommands
// A slash command is typed at the prompt, never sent to the inbox.

struct Ready {
    rig: Rig,
    socket: String,
    pid: u32,
}

fn ready_with(config: Value, status: &str) -> Ready {
    let rig = Rig::with(config, |_| {});
    let pid = rig.console(false);
    let socket = reachable(&rig, SID, status, json!({"pid": pid}));
    Ready { rig, socket, pid }
}

fn ready(status: &str) -> Ready {
    ready_with(json!({}), status)
}

fn typed(rig: &Rig) -> Vec<String> {
    rig.fake_log()
        .iter()
        .filter(|e| e["prog"] == "console")
        .filter_map(|e| e.get("line").and_then(Value::as_str).map(str::to_string))
        .collect()
}

#[test]
fn the_chat_types_it_into_an_idle_session() {
    let r = ready("idle");
    let out = r.rig.dispatch(json!({"prompt": "/compact", "session_id": SID}));
    assert_eq!(out["action"], json!("command"), "{out}");
    assert_eq!(typed(&r.rig), vec!["/compact"]);
    // An inbox copy would arrive as inert text.
    nothing_sent(&r.rig, &[&r.socket]);
    let _ = r.pid;
}

#[test]
fn a_busy_session_is_refused_not_typed_into() {
    let r = ready("busy");
    let out = r.rig.dispatch(json!({"prompt": "/compact", "session_id": SID}));
    assert_eq!(out["ok"], json!(false));
    assert!(out["error"].as_str().unwrap().contains("queue"), "{out}");
    assert!(typed(&r.rig).is_empty());
    nothing_sent(&r.rig, &[&r.socket]);
}

#[test]
fn a_parked_permission_prompt_is_never_answered_by_it() {
    let r = ready("idle");
    // A permission request parked for this session (the hook holds it until
    // somebody decides, or its wait runs out: 300 s at 1/50 is 6 s).
    let park = {
        let port = r.rig.port;
        std::thread::spawn(move || {
            let body = json!({"session_id": SID, "tool_name": "Bash", "tool_input": {"command": "x"},
                              "wait": 300, "cwd": "."});
            ureq::post(&format!("http://127.0.0.1:{port}/permission/park"))
                .set("Content-Type", "application/json")
                .send_string(&body.to_string())
                .ok();
        })
    };
    let id = wait_until(Duration::from_secs(10), || {
        r.rig.get("/api/pending").1["pending"].as_array().and_then(|p| p.first()).map(|p| p["id"].clone())
    })
    .expect("the request is parked");
    let out = r.rig.dispatch(json!({"prompt": "/clear", "session_id": SID}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(typed(&r.rig).is_empty());
    r.rig.post("/api/decide", &json!({"id": id, "verdict": "deny"}));
    park.join().unwrap();
}

#[test]
fn a_menu_command_is_refused() {
    let r = ready("idle");
    let out = r.rig.dispatch(json!({"prompt": "/plugin", "session_id": SID}));
    assert!(out["error"].as_str().unwrap().contains("menu"), "{out}");
    assert!(typed(&r.rig).is_empty());
}

#[test]
fn by_project_name_it_runs_in_that_project_and_never_launches() {
    let rig = Rig::start();
    let out = rig.dispatch(json!({"text": "project: laser ledger, /compact"}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(rig.events().iter().all(|e| e["kind"] != "dispatch_launch"));
    let pid = rig.console(false);
    reachable(&rig, SID, "idle", json!({"pid": pid}));
    let out = rig.dispatch(json!({"text": "project: laser ledger, /compact"}));
    assert_eq!(out["action"], json!("command"), "{out}");
    assert_eq!(typed(&rig), vec!["/compact"]);
}

#[test]
fn queued_it_is_typed_when_the_turn_ends() {
    let r = ready("busy");
    let (_, q) = r.rig.post("/api/queue", &json!({"session_id": SID, "prompt": "/compact"}));
    assert_eq!(q["ok"], json!(true), "{q}");
    assert!(typed(&r.rig).is_empty());
    r.rig.touch(SID, json!({"status": "idle"}));
    eventually("the sweep types it", || typed(&r.rig) == vec!["/compact"]);
    nothing_sent(&r.rig, &[&r.socket]);
}

#[test]
fn what_could_never_be_typed_is_not_queued() {
    let r = ready("busy");
    let (_, q) = r.rig.post("/api/queue", &json!({"session_id": SID, "prompt": "/permissions"}));
    assert_eq!(q["ok"], json!(false), "{q}");
    assert_eq!(r.rig.get("/api/queue").1["queue"], json!({}));
}

#[test]
fn an_ordinary_prompt_still_goes_to_the_inbox() {
    let r = ready("idle");
    let out = r.rig.dispatch(json!({"prompt": "explain /compact", "session_id": SID}));
    assert_eq!(out["action"], json!("inject"), "{out}");
    assert!(typed(&r.rig).is_empty());
    eventually("it arrives", || !r.rig.received(&r.socket).is_empty());
}

// -------------------------------------------------------------- SendTyped
// Send As Typed: off until turned on, and then typed as one line.

fn typed_call(rig: &Rig, prompt: &str) -> Value {
    let token = rig.window_token();
    rig.post_window("/api/typed", &json!({"session_id": SID, "prompt": prompt}), &token).1
}

#[test]
fn off_by_default() {
    let r = ready("idle");
    let out = typed_call(&r.rig, "i approve");
    assert!(out["error"].as_str().unwrap().contains("Settings"), "{out}");
    assert!(typed(&r.rig).is_empty());
}

#[test]
fn typed_as_one_line_into_an_idle_session() {
    let r = ready_with(json!({"chat": {"send_as_typed": true}}), "idle");
    let out = typed_call(&r.rig, "spend up to $40\nthen stop");
    assert_eq!((out["action"].clone(), out["cleaned"].clone()), (json!("typed"), json!(true)), "{out}");
    assert_eq!(typed(&r.rig), vec!["spend up to $40 then stop"]);
    nothing_sent(&r.rig, &[&r.socket]);
}

#[test]
fn never_into_a_busy_session() {
    let r = ready_with(json!({"chat": {"send_as_typed": true}}), "busy");
    assert_eq!(typed_call(&r.rig, "i approve")["ok"], json!(false));
    assert!(typed(&r.rig).is_empty());
}

#[test]
fn a_command_goes_the_command_way() {
    let r = ready_with(json!({"chat": {"send_as_typed": true}}), "idle");
    assert_eq!(typed_call(&r.rig, "/compact")["action"], json!("command"));
    assert!(typed_call(&r.rig, "/plugin")["error"].as_str().unwrap().contains("menu"));
    assert_eq!(typed(&r.rig), vec!["/compact"]);
}

#[test]
fn the_setting_applies_live() {
    let r = ready("idle");
    assert!(typed_call(&r.rig, "hello")["error"].as_str().unwrap().contains("Settings"));
    let token = r.rig.window_token();
    let (_, got) = r.rig.post_window("/api/settings", &json!({"changes": {"chat.send_as_typed": true}}), &token);
    assert_eq!(got["ok"], json!(true), "{got}");
    assert_eq!(typed_call(&r.rig, "hello")["action"], json!("typed"));
}
