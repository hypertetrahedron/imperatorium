//! Settings > Remote machines: Test and Add through the dispatcher
//! (`/api/remote/test`, `/api/remote/add`, `/api/remote/job`), against the
//! fake ssh in its sh mode as tests/remote_cli.rs does: the probe, the binary
//! push and the settings edit are the real ones, run by `sh` in a sandbox
//! HOME, and nothing leaves this machine. Needs `sh` (Git for Windows');
//! says it is skipped without one.
//!
//! The dispatcher is a test copy (no `--live`), so it never offers to
//! restart itself, and the config starts with no hosts, so nothing is polled.
#![cfg(windows)]

mod common;
use common::fakessh::FakeSsh;
use common::safety::which_on;
use common::*;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn wait_job(d: &Dispatcher) -> Value {
    let began = Instant::now();
    loop {
        let (code, j) = d.get("/api/remote/job");
        assert_eq!(code, 200, "{j}");
        if j["running"] == json!(false) {
            return j;
        }
        assert!(began.elapsed() < Duration::from_secs(90), "the add never finished: {j}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn the_window_tests_and_adds_a_machine() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    if which_on("sh", &path).is_none() {
        eprintln!("remote_settings_http: skipped (no sh on PATH)");
        return;
    }
    let tmp = TempDir::new("remote-settings");
    let fakehome = tmp.join("hosthome");
    let builds = tmp.join("builds");
    let musl = builds.join("x86_64-unknown-linux-musl");
    for dir in [fakehome.join(".local").join("bin"), fakehome.join(".claude"), musl.clone()] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(fakehome.join(".local").join("bin").join("claude"), "#!/bin/sh\necho \"2.1.300 (Claude Code)\"\n").unwrap();
    std::fs::write(musl.join("imperatorium"), b"#!/bin/sh\n# not really an ELF\n").unwrap();
    let ssh = FakeSsh::new();
    let mut env = ssh.env(Some("sh"));
    env.push(("FAKEHOST_HOME".into(), fakehome.to_string_lossy().into_owned()));
    env.push(("IMPERATORIUM_REMOTE_BUILDS".into(), builds.to_string_lossy().into_owned()));
    // Key setup writes to ~/.ssh: this test's own, never this machine's.
    let userhome = tmp.join("userhome");
    std::fs::create_dir_all(&userhome).unwrap();
    env.push(("USERPROFILE".into(), userhome.to_string_lossy().into_owned()));
    env.push(("HOME".into(), userhome.to_string_lossy().into_owned()));
    let d = Dispatcher::start(Options { config: Some(json!({"roots": []})), env, ..Options::new() });
    let token = d.window_token();

    // Only the window: each of these runs ssh to a host the caller names.
    for p in ["/api/remote/test", "/api/remote/add", "/api/remote/key", "/api/remote/restart"] {
        let (code, got) = d.post(p, &json!({"host": "fakehost"}));
        assert_eq!(code, 403, "{p}: {got}");
    }
    // Never something ssh would read as an option.
    let (_, got) = d.post_window("/api/remote/test", &json!({"host": "-oProxyCommand=calc"}), &token);
    assert_eq!(got["ok"], json!(false), "{got}");
    assert!(got["error"].as_str().unwrap().contains("'-'"), "{got}");
    let (_, got) = d.post_window("/api/remote/add", &json!({"host": ""}), &token);
    assert_eq!(got["ok"], json!(false), "{got}");

    // A new host: reachable, Claude Code there, binary and hooks not yet.
    let (code, got) = d.post_window("/api/remote/test", &json!({"host": "fakehost", "tunnel": true}), &token);
    assert_eq!(code, 200);
    assert_eq!((got["reachable"].clone(), got["ok"].clone(), got["configured"].clone()), (json!(true), json!(false), json!(false)), "{got}");
    let marks: Vec<(&str, &str)> = got["rows"].as_array().unwrap().iter().map(|r| (r["what"].as_str().unwrap(), r["mark"].as_str().unwrap())).collect();
    assert_eq!(marks, [("ssh", "ok"), ("claude", "ok"), ("binary", "--"), ("hooks", "MISSING")], "{got}");
    // Without a tunnel the hooks are not needed.
    let (_, got) = d.post_window("/api/remote/test", &json!({"host": "fakehost", "tunnel": false}), &token);
    assert_eq!(got["ok"], json!(true), "{got}");

    // A host that does not answer says why, and how to fix key login.
    let (_, got) = d.post_window("/api/remote/test", &json!({"host": "downhost"}), &token);
    assert_eq!((got["reachable"].clone(), got["ok"].clone()), (json!(false), json!(false)), "{got}");
    assert!(got["rows"][0]["detail"].as_str().unwrap().contains("Connection refused"), "{got}");
    assert_eq!(got["auth"], json!(false), "a host that does not answer is no key problem: {got}");

    // A refused login offers key setup: this machine's key, made once, and
    // the one command that copies it (the password goes to ssh, not here).
    let (_, got) = d.post_window("/api/remote/test", &json!({"host": "lockedhost"}), &token);
    assert_eq!((got["reachable"].clone(), got["auth"].clone()), (json!(false), json!(true)), "{got}");
    let (_, key) = d.post_window("/api/remote/key", &json!({"host": "lockedhost"}), &token);
    assert_eq!((key["ok"].clone(), key["made"].clone()), (json!(true), json!(true)), "{key}");
    let pubkey = userhome.join(".ssh").join("id_ed25519.pub");
    assert_eq!(std::path::PathBuf::from(key["key"].as_str().unwrap()), pubkey, "{key}");
    assert!(std::fs::read_to_string(&pubkey).unwrap().starts_with("ssh-ed25519 "));
    let cmd = key["command"].as_str().unwrap();
    assert!(cmd.contains("ssh lockedhost") && cmd.contains("authorized_keys") && cmd.contains("id_ed25519.pub"), "{cmd}");
    let before = std::fs::read(&pubkey).unwrap();
    let (_, again) = d.post_window("/api/remote/key", &json!({"host": "lockedhost"}), &token);
    assert_eq!(again["made"], json!(false), "{again}");
    assert_eq!(std::fs::read(&pubkey).unwrap(), before, "an existing key is never replaced");
    let (_, got) = d.post_window("/api/remote/key", &json!({"host": "-oX=y"}), &token);
    assert_eq!(got["ok"], json!(false), "{got}");

    let (_, j) = d.get("/api/remote/job");
    assert_eq!(j["job"], Value::Null, "{j}");

    // Add: the same five steps as `remote add`, with no questions.
    let (_, got) = d.post_window("/api/remote/add", &json!({"host": "fakehost", "label": "Fake", "tunnel": true}), &token);
    assert_eq!(got["ok"], json!(true), "{got}");
    let j = wait_job(&d);
    let log = j["log"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n");
    assert_eq!(j["added"], json!(true), "{log}");
    for want in ["== 1/5 SSH", "== 3/5 The binary on fakehost", "== 5/5 config.json", "added {"] {
        assert!(log.contains(want), "{want} not in\n{log}");
    }
    assert!(!log.contains("restart the dispatcher yourself"), "{log}");
    assert_eq!(j["can_restart"], json!(false), "a test dispatcher never restarts the real one");
    let cfg = read_json(&d.home.join("config.json"));
    let entry = &cfg["remote"]["hosts"][0];
    assert_eq!((entry["name"].clone(), entry["label"].clone(), entry["tunnel"].clone()), (json!("fakehost"), json!("Fake"), json!(true)), "{cfg}");
    assert!(fakehome.join(".claude").join("imperatorium").join("bin").join("imperatorium").exists());
    // The fleet is read at start: the form says it waits on a restart.
    let (_, view) = d.get("/api/settings");
    assert_eq!(view["waiting"], json!(["dispatcher"]), "{view}");
    assert_eq!(view["hosts"][0]["name"], json!("fakehost"), "{view}");

    // Now it tests clean, as configured.
    let (_, got) = d.post_window("/api/remote/test", &json!({"host": "fakehost"}), &token);
    assert_eq!((got["ok"].clone(), got["configured"].clone()), (json!(true), json!(true)), "{got}");

    // A failed add is not written and says why.
    let (_, got) = d.post_window("/api/remote/add", &json!({"host": "downhost"}), &token);
    assert_eq!(got["ok"], json!(true), "{got}");
    let j = wait_job(&d);
    let log = j["log"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n");
    assert_eq!(j["added"], json!(false), "{log}");
    assert!(log.contains("still cannot reach downhost") && log.contains("ssh-keygen"), "{log}");
    assert_eq!(read_json(&d.home.join("config.json"))["remote"]["hosts"].as_array().map(Vec::len), Some(1));

    // And the restart is refused here.
    let (_, got) = d.post_window("/api/remote/restart", &json!({}), &token);
    assert_eq!(got["ok"], json!(false), "{got}");
}
