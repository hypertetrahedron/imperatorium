//! Ports of test_dispatcher.py's RemoteHooks, RemoteWork, RemoteRouting,
//! HookOnlyListener and RemoteConversation: sessions on another machine.
//!
//! The host `pi` is reached through a stub `ssh` (IMPERATORIUM_SSH, see
//! tests/common/dispatch.rs `Rig::remote`): its polls answer a file, an
//! injection is logged, a `claude --bg` prints an id. Hooks arrive as the
//! host's tunnel delivers them, on the hook-only listener.
#![cfg(windows)]
mod common;
use common::dispatch::*;
use serde_json::{json, Value};

const RSID: &str = OTHER;
const RCWD: &str = "/home/les/src/die-reader";
const RSOCK: &str = "/tmp/cc-msg-r.sock";

fn remote_live(rig: &Rig, status: &str, socket: &str) -> Value {
    rig.remote_rows(
        json!([{"sessionId": RSID, "cwd": RCWD, "kind": "interactive", "pid": 112015,
                "status": status, "name": "die-1"}]),
        &[json!({"sessionId": RSID, "messagingSocketPath": socket, "pid": 112015})],
    );
    rig.remote_row(RSID)
}

fn remote_hook(rig: &Rig, event: &str, extra: Value) -> Value {
    let mut body = json!({"messaging_socket": RSOCK, "messaging_token": "rtok"});
    merge(&mut body, &extra);
    let (code, got) = rig.remote_hook(event, RSID, body);
    assert_eq!(code, 200, "{got}");
    got
}

fn row(rig: &Rig) -> Value {
    rig.remote_row(RSID)
}

fn attention(r: &Value) -> bool {
    r["attention"] == json!(true)
}

// ----------------------------------------------------------- RemoteHooks

#[test]
fn a_remote_session_becomes_dispatchable_once_its_hook_reports() {
    let rig = Rig::remote(json!({}));
    assert_eq!(remote_live(&rig, "idle", RSOCK)["dispatchable"], json!(false));
    remote_hook(&rig, "SessionStart", json!({}));
    assert_eq!(row(&rig)["dispatchable"], json!(true));
    // Its token is kept under the host's key.
    assert_eq!(rig.tokens()[format!("pi/{RSID}")], json!("rtok"));
}

#[test]
fn remote_records_are_kept_under_the_host() {
    let rig = Rig::remote(json!({}));
    remote_hook(&rig, "SessionStart", json!({}));
    let state = rig.state_json();
    assert!(state["sessions"].get(format!("pi/{RSID}")).is_some(), "{state}");
    assert!(state["sessions"].get(RSID).is_none(), "{state}");
}

#[test]
fn a_host_cannot_forget_a_local_session() {
    // Anything on that host can post to its end of the tunnel. It must not
    // be able to make a session on this machine unreachable.
    let rig = Rig::remote(json!({}));
    let socket = rig.live(SID, "idle", json!({}));
    rig.start_hook(SID, "tok", &socket);
    rig.remote_hook("SessionEnd", SID, json!({}));
    assert_eq!(rig.tokens()[SID], json!("tok"));
}

#[test]
fn a_host_cannot_repoint_a_local_session() {
    let rig = Rig::remote(json!({}));
    let socket = rig.live(SID, "idle", json!({}));
    rig.start_hook(SID, "tok", &socket);
    rig.remote_hook("Stop", SID, json!({"messaging_socket": "/tmp/evil", "messaging_token": "evil"}));
    let local = rig.row(SID);
    assert_eq!(local["dispatchable"], json!(true));
    assert_eq!(local["socket"], json!(socket));
    assert_eq!(rig.tokens()[SID], json!("tok"));
    assert_eq!(rig.tokens()[format!("pi/{SID}")], json!("evil")); // its own record, not ours
}

#[test]
fn a_remote_permission_prompt_raises_attention() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "busy", RSOCK);
    remote_hook(&rig, "SessionStart", json!({}));
    remote_hook(&rig, "Notification", json!({"notification_type": "permission_prompt"}));
    let r = row(&rig);
    assert!(attention(&r), "{r}");
    assert!(r["attention_reason"].as_str().is_some_and(|s| !s.is_empty()));
}

#[test]
fn a_remote_stop_clears_it() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "idle", RSOCK);
    remote_hook(&rig, "Notification", json!({"notification_type": "permission_prompt"}));
    remote_hook(&rig, "Stop", json!({}));
    // The permission prompt is cleared; what is left is a complete turn.
    assert_eq!(row(&rig)["attention_reason"], json!("complete"));
}

#[test]
fn a_rebound_remote_socket_invalidates_the_token() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "idle", "/tmp/new.sock");
    remote_hook(&rig, "SessionStart", json!({"messaging_socket": "/tmp/old.sock"}));
    let r = row(&rig);
    assert_eq!(r["dispatchable"], json!(false));
    assert_eq!(r["stale_credential"], json!(true));
}

#[test]
fn remote_events_say_which_host() {
    let rig = Rig::remote(json!({}));
    remote_hook(&rig, "SessionStart", json!({}));
    assert_eq!(rig.events()[0]["host"], json!("pi"));
}

// ------------------------------------------------------------ RemoteWork

#[test]
fn a_remote_row_carries_the_numbers_its_host_read() {
    // The stub host has no channel to read transcripts with (that is a
    // second ssh, `imperatorium remote-channel`), so what is checked here is
    // that a remote row has a `work` of its own and no `stat` leaks out.
    let rig = Rig::remote(json!({}));
    let r = remote_live(&rig, "idle", RSOCK);
    assert!(r.get("stat").is_none());
    assert_eq!(r["work"]["alive"], json!(true), "{r}");
    assert_eq!(r["work"]["processes"], json!(0));
}

#[test]
fn without_them_the_work_is_empty_rather_than_local() {
    // A transcript of the same id exists on THIS machine: a remote row
    // must not be read from it.
    let rig = Rig::remote(json!({}));
    rig.transcript(RSID, &chat_lines(RSID, &[("user", "hi"), ("assistant", "yo")], 5));
    let r = remote_live(&rig, "idle", RSOCK);
    assert_eq!(r["work"]["cache_expires_at"], Value::Null, "{r}");
    assert_eq!(r["work"]["context_tokens"], Value::Null);
}

#[test]
fn refresh_reaches_the_fleet() {
    let rig = Rig::remote(json!({}));
    assert_eq!(rig.post("/api/remote/refresh", &json!({})).1, json!({"ok": true, "hosts": 1}));
}

// --------------------------------------------------------- RemoteRouting

fn sent_over_ssh(rig: &Rig) -> String {
    rig.injected().join("\n")
}

fn started(rig: &Rig) -> Vec<String> {
    rig.ssh_log().into_iter().filter(|l| l.contains("--bg")).collect()
}

#[test]
fn a_prompt_to_a_remote_session_goes_over_ssh() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "idle", RSOCK);
    remote_hook(&rig, "SessionStart", json!({}));
    rig.invalidate();
    let out = rig.dispatch(json!({"prompt": "go", "session_id": RSID}));
    assert_eq!(out["ok"], json!(true), "{out}");
    let sent = sent_over_ssh(&rig);
    assert!(sent.contains(RSOCK), "{sent}");
    assert!(sent.contains("inbox-send"));
    assert!(sent.contains("{\"type\": \"auth\", \"token\": \"rtok\"}"), "{sent}");
    assert!(sent.contains("do not use SendMessage"), "{sent}");
    assert!(sent.contains("\\n\\ngo\""), "{sent}");
}

#[test]
fn a_remote_session_without_a_token_is_refused() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "idle", RSOCK);
    let out = rig.dispatch(json!({"prompt": "go", "session_id": RSID}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(rig.injected().is_empty());
}

#[test]
fn a_remote_failure_is_reported() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "idle", RSOCK);
    remote_hook(&rig, "SessionStart", json!({}));
    std::fs::write(rig.ssh.as_ref().unwrap().join("fail.txt"), "Connection refused").unwrap();
    let out = rig.dispatch(json!({"prompt": "go", "session_id": RSID}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(out["error"].as_str().unwrap().contains("Connection refused"), "{out}");
}

#[test]
fn a_remote_project_injects_into_its_live_session() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "idle", RSOCK);
    remote_hook(&rig, "SessionStart", json!({}));
    rig.invalidate();
    let out = rig.dispatch(json!({"project": "pi/die-reader", "prompt": "go", "confirm": true}));
    assert_eq!(out["action"], json!("inject"), "{out}");
    assert!(started(&rig).is_empty());
}

#[test]
fn a_remote_project_with_nothing_live_goes_to_the_background() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "idle", RSOCK); // listed, but no hook has reported a token
    let out = rig.dispatch(json!({"project": "pi/die-reader", "prompt": "go", "confirm": true}));
    assert_eq!(out["action"], json!("remote"), "{out}");
    let bg = started(&rig);
    assert_eq!(bg.len(), 1, "{bg:?}");
    assert!(bg[0].contains(&format!("cd {RCWD} && c --bg")) && bg[0].ends_with(" go"), "{}", bg[0]);
}

#[test]
fn inject_mode_to_a_remote_project_refuses_rather_than_starting() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "idle", RSOCK);
    let out = rig.dispatch(json!({"project": "pi/die-reader", "prompt": "go", "mode": "inject", "confirm": true}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(started(&rig).is_empty());
}

#[test]
fn background_mode_never_injects_remotely() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "idle", RSOCK);
    remote_hook(&rig, "SessionStart", json!({}));
    rig.invalidate();
    rig.dispatch(json!({"project": "pi/die-reader", "prompt": "go", "mode": "background", "confirm": true}));
    assert!(rig.injected().is_empty());
    assert_eq!(started(&rig).len(), 1);
}

#[test]
fn a_local_project_never_picks_a_remote_session() {
    let rig = Rig::remote(json!({}));
    rig.remote_rows(
        json!([{"sessionId": RSID, "cwd": rig.cwd, "kind": "interactive", "pid": 112015,
                "status": "idle", "name": "die-1"}]),
        &[json!({"sessionId": RSID, "messagingSocketPath": RSOCK})],
    );
    rig.remote_row(RSID);
    remote_hook(&rig, "SessionStart", json!({}));
    rig.invalidate();
    // inject mode, so that nothing falls through to a window.
    let out = rig.dispatch(json!({"project": "laser-ledger", "prompt": "go", "mode": "inject"}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(rig.injected().is_empty());
}

#[test]
fn a_remote_session_is_never_focused_as_a_local_pid() {
    let rig = Rig::remote(json!({}));
    remote_live(&rig, "idle", RSOCK);
    let (_, out) = rig.post("/api/focus", &json!({"session_id": RSID}));
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(out["error"].as_str().unwrap().contains("on pi"), "{out}");
    assert!(out.get("url").is_none());
}

// ------------------------------------------------------- HookOnlyListener
// The tunnel lands here, so this is what every process on the host sees.

#[test]
fn hooks_are_accepted_and_tagged_with_the_listeners_host() {
    let rig = Rig::remote(json!({}));
    let (code, _) = rig.remote_hook(
        "SessionStart",
        RSID,
        json!({"messaging_socket": RSOCK, "messaging_token": "rtok"}),
    );
    assert_eq!(code, 200);
    assert!(rig.tokens().get(format!("pi/{RSID}")).is_some());
}

#[test]
fn dispatch_is_not_reachable_from_the_host() {
    let rig = Rig::remote(json!({}));
    let socket = rig.live(SID, "idle", json!({}));
    rig.start_hook(SID, "tok", &socket);
    let (code, _) = rig.tunnel_post("/api/dispatch", &json!({"session_id": SID, "prompt": "rm -rf"}));
    assert_eq!(code, 404);
    assert!(rig.received(&socket).is_empty());
}

#[test]
fn nothing_else_is_either() {
    let rig = Rig::remote(json!({}));
    for path in ["/api/focus", "/api/decide", "/permission/park"] {
        assert_eq!(rig.tunnel_post(path, &json!({})).0, 404, "{path}");
    }
}

// ----------------------------------------------------- RemoteConversation

#[test]
fn a_local_session_never_goes_to_a_host() {
    let rig = Rig::remote(json!({}));
    rig.live(SID, "idle", json!({}));
    rig.transcript(SID, &chat_lines(SID, &[("user", "hi")], 1));
    let before = rig.ssh_log().iter().filter(|l| l.contains("remote-channel")).count();
    let got = rig.get(&format!("/api/conversation?session={SID}")).1;
    assert_eq!(got["items"].as_array().unwrap().len(), 1, "{got}");
    let after = rig.ssh_log().iter().filter(|l| l.contains("remote-channel")).count();
    assert_eq!(before, after);
}
