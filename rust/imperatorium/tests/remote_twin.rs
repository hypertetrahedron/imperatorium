//! Ported from remote_twin.py (at `last-python`): the binary's remote hosts end to end against the fake
//! ssh (fakecli as ssh.exe; it replaces fakessh.py): rows from a host, its
//! hooks through the tunnel's hook-only listener, the credential they
//! bring, injection, the queue drained by a remote Stop, a remote
//! conversation, the host-qualified catalogue, and background dispatch
//! over there. Nothing leaves this machine: IMPERATORIUM_SSH names the
//! fake, and config.json has only the made-up host.
//!
//! No golden: each expectation is the script's (what dispatcher.py does with
//! the same inputs), checked as the script checked it.
#![cfg(windows)]

mod common;
use common::fakessh::FakeSsh;
use common::*;
use serde_json::{json, Value};
use sha2::Digest;
use std::time::{Duration, Instant};

const SID: &str = "11111111-2222-4333-8444-555555555555";
const SOCKET: &str = "/tmp/claude-fake/inbox.sock";

/// remote_twin.py call(): GET, or POST `body`; the status and the JSON ({}
/// when empty).
fn call(port: u16, path: &str, body: Option<&Value>) -> (u16, Value) {
    let url = format!("http://127.0.0.1:{port}{path}");
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let r = match body {
        Some(b) => agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(&b.to_string()),
        None => agent.get(&url).call(),
    };
    let resp = match r {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return (0, json!({"transport": e.to_string()})),
    };
    let code = resp.status();
    let text = resp.into_string().unwrap_or_default();
    let v = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    };
    (code, v)
}

struct Checks {
    failed: Vec<String>,
    n: usize,
}

impl Checks {
    fn check(&mut self, name: &str, ok: bool, got: impl FnOnce() -> String) {
        self.n += 1;
        eprintln!("{name:<58} {}", if ok { "ok" } else { "FAIL" });
        if !ok {
            let g: String = got().chars().take(600).collect();
            self.failed.push(format!("{name}\n     got: {g}"));
        }
    }

    fn finish(&self, what: &str) {
        assert!(
            self.failed.is_empty(),
            "{what}: {} of {} checks failed:\n  {}",
            self.failed.len(),
            self.n,
            self.failed.join("\n  ")
        );
        eprintln!("{what}: {} checks, all passed", self.n);
    }
}

fn row_of(port: u16, key: &str) -> Option<Value> {
    call(port, "/api/sessions", None).1["sessions"]
        .as_array()?
        .iter()
        .find(|r| r["key"] == key)
        .cloned()
}

fn has(v: &Value, s: &str) -> bool {
    v.to_string().contains(s)
}

#[test]
fn remote_host_end_to_end_through_a_fake_ssh() {
    let began = Instant::now();
    let ssh = FakeSsh::new();
    let tmp = TempDir::new("remote-twin");
    let builds = tmp.join("builds");
    let musl = builds.join("x86_64-unknown-linux-musl");
    std::fs::create_dir_all(&musl).unwrap();
    // Any bytes will do for the "build": the fake host reports their hash.
    std::fs::write(musl.join("imperatorium"), b"not really an ELF").unwrap();
    let sha: String = sha2::Sha256::digest(b"not really an ELF")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    std::fs::write(ssh.data.join("sha256"), sha).unwrap();
    let hooks = free_port();
    let mut env = ssh.env(None);
    env.push((
        "IMPERATORIUM_REMOTE_BUILDS".into(),
        builds.to_string_lossy().into_owned(),
    ));
    env.push(("IMPERATORIUM_TEST_TIME_SCALE".into(), "0.05".into()));
    let d = Dispatcher::start(Options {
        config: Some(json!({
            "remote": {"hosts": [{"name": "fakehost", "label": "Fake",
                                  "tunnel": {"local_port": hooks}}]},
            "roots": [tmp.path]})),
        env,
        ..Options::new()
    });
    let port = d.port;
    let key = format!("fakehost/{SID}");
    let mut c = Checks {
        failed: vec![],
        n: 0,
    };

    // The host's row, with its work from the stats channel.
    let row = wait_until(Duration::from_secs(30), || {
        let rows = call(port, "/api/sessions", None).1;
        rows["sessions"]
            .as_array()?
            .iter()
            .find(|r| r["remote"] == json!(true) && r["work"]["context_tokens"].is_number())
            .cloned()
    })
    .or_else(|| {
        call(port, "/api/sessions", None).1["sessions"]
            .as_array()?
            .iter()
            .find(|r| r["remote"] == json!(true))
            .cloned()
    });
    c.check("a host's session is on the board", row.is_some(), || {
        format!("{:?}", ssh.calls())
    });
    let row = row.unwrap_or(Value::Null);
    c.check(
        "keyed by host, labelled, remote",
        row["key"] == key.as_str()
            && row["host"] == "fakehost"
            && row["label"] == "Fake"
            && row["remote"] == json!(true),
        || row.to_string(),
    );
    c.check(
        "socket from the host's session file",
        row["socket"] == SOCKET,
        || row.to_string(),
    );
    c.check(
        "not dispatchable before its hooks bring a token",
        row["dispatchable"] == json!(false),
        || row.to_string(),
    );
    c.check(
        "work from the host's stats channel",
        row["work"]["context_tokens"] == json!(4242),
        || row["work"].to_string(),
    );

    let hosts = call(port, "/api/hosts", None).1;
    c.check(
        "/api/hosts lists it with its tunnel",
        hosts["hosts"][0]["name"] == "fakehost"
            && hosts["hosts"][0]["tunnel_status"]["local_port"] == json!(hooks)
            && hosts["status"]["fakehost"]["fails"] == json!(0),
        || hosts.to_string(),
    );

    // The host's hooks, through the hook-only listener.
    let listening = wait_until(Duration::from_secs(15), || {
        std::net::TcpStream::connect(("127.0.0.1", hooks))
            .ok()
            .map(|_| ())
    });
    c.check("the hook listener is up", listening.is_some(), String::new);
    let (code, body) = call(hooks, "/api/dispatch", Some(&json!({"text": "x"})));
    c.check(
        "the hook listener refuses everything but /hook",
        code == 404,
        || format!("{code} {body}"),
    );
    let start = json!({"hook_event_name": "SessionStart", "session_id": SID,
                       "cwd": "/home/fake/projects/fakeproj", "messaging_socket": SOCKET,
                       "messaging_token": "remote-tok", "session_name": "fakeproj-ab"});
    let r = call(hooks, "/hook", Some(&start)).1;
    c.check(
        "SessionStart through the tunnel",
        r["ok"] == json!(true),
        || r.to_string(),
    );
    let row = wait_until(Duration::from_secs(15), || {
        row_of(port, &key).filter(|r| r["dispatchable"] == json!(true))
    })
    .or_else(|| row_of(port, &key))
    .unwrap_or(Value::Null);
    c.check(
        "the token makes it dispatchable",
        row["dispatchable"] == json!(true),
        || row.to_string(),
    );
    c.check(
        "the token never leaves in /api/sessions",
        row.get("token").is_none(),
        || row.to_string(),
    );
    let ev = call(port, "/api/events", None).1["events"][0].clone();
    c.check(
        "its event names the host",
        ev["kind"] == "session_start" && ev["host"] == "fakehost" && ev["session_id"] == SID,
        || ev.to_string(),
    );
    // A local hook with the same id must not touch the remote record.
    call(
        port,
        "/hook",
        Some(&json!({"hook_event_name": "SessionEnd", "session_id": SID})),
    );
    let state = d.state_json();
    c.check(
        "a local SessionEnd cannot forget the host's session",
        state["sessions"].get(&key).is_some(),
        || {
            format!(
                "{:?}",
                state["sessions"]
                    .as_object()
                    .map(|o| o.keys().collect::<Vec<_>>())
            )
        },
    );

    // Injection by session.
    let before = ssh.calls().len();
    let r = call(
        port,
        "/api/dispatch",
        Some(
            &json!({"session_id": SID, "prompt": "do the thing", "confirm": true,
                     "mode": "auto"}),
        ),
    )
    .1;
    c.check(
        "dispatch to the remote session injects",
        r["ok"] == json!(true) && r["action"] == "inject" && r["host"] == "fakehost",
        || r.to_string(),
    );
    let sent: Vec<Value> = ssh.calls()[before..]
        .iter()
        .filter(|x| x["cmd"].as_str().unwrap_or("").contains("inbox-send"))
        .cloned()
        .collect();
    let frames: Vec<Value> = sent
        .first()
        .and_then(|s| s["stdin"].as_str())
        .unwrap_or("")
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    c.check(
        "inbox-send to the session's socket, quoted",
        sent.first().is_some_and(|s| {
            s["cmd"]
                .as_str()
                .unwrap_or("")
                .ends_with("inbox-send /tmp/claude-fake/inbox.sock")
        }),
        || format!("{sent:?}"),
    );
    c.check(
        "frames: the token first, then the prompt with its origin",
        frames.len() == 2
            && frames[0] == json!({"type": "auth", "token": "remote-tok"})
            && frames[1]["message"]["content"]
                .as_str()
                .is_some_and(|t| t.ends_with("do the thing")),
        || format!("{frames:?}"),
    );
    c.check(
        "the token is never on the command line",
        !ssh.calls()
            .iter()
            .any(|x| x["cmd"].as_str().unwrap_or("").contains("remote-tok")),
        String::new,
    );
    let ev = call(port, "/api/events", None).1["events"][0].clone();
    c.check(
        "dispatch_inject event names the host",
        ev["kind"] == "dispatch_inject" && ev["host"] == "fakehost",
        || ev.to_string(),
    );

    // Queue, drained by the host's Stop.
    let q = call(
        port,
        "/api/queue",
        Some(&json!({"session_id": SID, "prompt": "after this turn"})),
    )
    .1;
    let row = row_of(port, &key).unwrap_or(Value::Null);
    c.check(
        "queued under the host-qualified key",
        q["ok"] != json!(false) && row["queued"].as_i64().unwrap_or(0) >= 0,
        || q.to_string(),
    );
    let before = ssh.calls().len();
    let stop = call(
        hooks,
        "/hook",
        Some(&json!({"hook_event_name": "Stop", "session_id": SID,
                     "cwd": "/home/fake/projects/fakeproj", "last_assistant_message": "done"})),
    )
    .1;
    let later = ssh.calls()[before..]
        .iter()
        .any(|x| x["cmd"].as_str().unwrap_or("").contains("inbox-send"));
    let waiting = q["waiting"].as_i64().unwrap_or(0) != 0 || q["waiting"] == json!(true);
    c.check(
        "a queued prompt goes out on the host's Stop (or was sent at once)",
        later || !waiting,
        || json!({"stop": stop, "queue": q}).to_string(),
    );

    // Conversation and the refusals.
    let conv = call(port, &format!("/api/conversation?session={SID}"), None).1;
    c.check(
        "conversation read on the host",
        conv["remote"] == json!(true)
            && conv["title"] == "Fake remote work"
            && conv["items"].as_array().map(Vec::len) == Some(2)
            && conv["sig"].as_str().is_some_and(|s| !s.is_empty()),
        || conv.to_string(),
    );
    let sig = conv["sig"].as_str().unwrap_or("");
    let again = call(
        port,
        &format!("/api/conversation?session={SID}&known={sig}"),
        None,
    )
    .1;
    c.check(
        "an unchanged remote conversation is answered by its sig",
        again["unchanged"] == json!(true),
        || again.to_string(),
    );
    for (path, want) in [
        ("/api/changes", "changes are only read on this machine"),
        ("/api/activity", "only through tool hooks"),
        ("/api/session_status", "report only what their tile shows"),
    ] {
        let body = call(port, &format!("{path}?session={SID}"), None).1;
        c.check(
            &format!("refused as Python does: {path}"),
            has(&body, want),
            || body.to_string(),
        );
    }
    let (_, body) = call(port, "/api/focus", Some(&json!({"session_id": SID})));
    c.check(
        "focus refuses a remote session",
        has(&body, "no window here to raise"),
        || body.to_string(),
    );

    // Catalogue and dispatch by project.
    let projects = call(port, "/api/projects", None).1;
    let proj = projects["projects"]
        .as_array()
        .and_then(|a| a.iter().find(|p| p["host"] == "fakehost").cloned());
    c.check(
        "the catalogue has the host-qualified project",
        proj.as_ref()
            .is_some_and(|p| p["name"] == "fakehost/fakeproj" && p["source"] == "remote"),
        || format!("{proj:?}"),
    );
    let dry = call(
        port,
        "/api/dispatch",
        Some(&json!({"project": "fakehost/fakeproj", "prompt": "x", "mode": "dry-run"})),
    )
    .1;
    c.check(
        "dry run resolves it on the host",
        dry["ok"] == json!(true) && dry["project"]["host"] == "fakehost",
        || dry.to_string(),
    );
    let bg = call(
        port,
        "/api/dispatch",
        Some(
            &json!({"project": "fakehost/fakeproj", "path": "/home/fake/projects/fakeproj",
                     "host": "fakehost", "prompt": "run it over there", "confirm": true,
                     "mode": "background"}),
        ),
    )
    .1;
    c.check(
        "background dispatch starts claude --bg on the host",
        bg["ok"] == json!(true) && bg["action"] == "remote" && bg["bg_id"] == "5ac0ffee",
        || bg.to_string(),
    );
    let bgc: Vec<Value> = ssh
        .calls()
        .into_iter()
        .filter(|x| x["cmd"].as_str().unwrap_or("").contains("--bg"))
        .collect();
    c.check(
        "in the project folder, the claude path left to expand",
        bgc.last().is_some_and(|x| {
            x["cmd"]
                .as_str()
                .unwrap_or("")
                .starts_with("cd /home/fake/projects/fakeproj && $HOME/.local/bin/claude --bg")
        }),
        || format!("{:?}", bgc.last()),
    );
    c.check(
        "a tunnel was started (ssh -N -R to the host)",
        ssh.calls().iter().any(|x| x["tunnel"] == json!(true)),
        String::new,
    );
    let refresh = call(port, "/api/remote/refresh", Some(&json!({}))).1;
    c.check(
        "/api/remote/refresh",
        refresh == json!({"ok": true, "hosts": 1}),
        || refresh.to_string(),
    );
    call(
        hooks,
        "/hook",
        Some(&json!({"hook_event_name": "SessionEnd", "session_id": SID})),
    );
    let gone = wait_until(Duration::from_secs(10), || {
        d.state_json()["sessions"].get(&key).is_none().then_some(())
    });
    c.check(
        "the host's SessionEnd forgets its record",
        gone.is_some(),
        || {
            format!(
                "{:?}",
                d.state_json()["sessions"]
                    .as_object()
                    .map(|o| o.keys().cloned().collect::<Vec<_>>())
            )
        },
    );
    // Every ssh call went to the made-up host.
    let hosts_called: Vec<String> = ssh
        .calls()
        .iter()
        .filter_map(|x| x["host"].as_str().map(str::to_string))
        .filter(|h| h != "fakehost")
        .collect();
    c.check(
        "only the fake host was called",
        hosts_called.is_empty(),
        || format!("{hosts_called:?}"),
    );
    eprintln!("remote_twin: {:.1} s", began.elapsed().as_secs_f64());
    drop(d);
    c.finish("remote_twin");
}
