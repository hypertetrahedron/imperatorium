//! The hook processes as Claude Code sees them (rust/conformance/
//! hooks_blackbox.py): each recorded case runs `imperatorium hook|statusline|
//! permission` with the same stdin, environment and policy against a fake
//! dispatcher answering /permission/park as recorded, and must match what
//! the Python hooks did byte for byte: exit code, stdout, every request
//! (path and body), their Content-Types, and the audit log
//! (decisions.jsonl) minus its timestamps.
#![cfg(windows)]

mod common;
use common::*;
use serde_json::{json, Value};
use std::process::Command;

fn audit(state: &std::path::Path) -> Value {
    let log = state.join("imperatorium").join("decisions.jsonl");
    let text = std::fs::read_to_string(log).unwrap_or_default();
    Value::Array(
        text.lines()
            .map(|l| {
                let mut r: Value = serde_json::from_str(l).unwrap();
                r.as_object_mut().unwrap().remove("at");
                r
            })
            .collect(),
    )
}

#[test]
fn hooks_behave_as_the_python_hooks_did() {
    let cases = read_jsonl(&golden_dir("hooks_blackbox").join("cases.jsonl"));
    assert!(cases.len() > 40);
    let fake = FakeServer::start();
    let mut f = Failures::default();
    for case in &cases {
        let state = TempDir::new("hook");
        let places = Places::new(&[("state", &state.path)]);
        let case = places.put(case);
        let which = case["which"].as_str().unwrap();
        let park = &case["park"];
        fake.set_answer(
            "/permission/park",
            park[0].as_u64().unwrap() as u16,
            park[1].as_str().unwrap().as_bytes(),
        );
        let mut cmd = Command::new(bin());
        cmd.arg(which).current_dir(repo());
        clean_env(&mut cmd, None);
        cmd.env("IMPERATORIUM_ENDPOINT", fake.url("/hook"))
            .env("IMPERATORIUM_PARK_ENDPOINT", fake.url("/permission/park"))
            .env("LOCALAPPDATA", &state.path)
            // Without IMPERATORIUM_POLICY the binary would look in
            // %APPDATA%\imperatorium: this machine's own policy.
            .env("APPDATA", state.join("roaming"))
            .env("XDG_CONFIG_HOME", state.join("config"))
            .env("XDG_STATE_HOME", &state.path)
            .env("CLAUDE_CONFIG_DIR", state.join("claude"));
        match &case["policy"] {
            Value::Null => {}
            p => {
                let path = state.join("policy.json");
                let text = match p {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                std::fs::write(&path, text).unwrap();
                cmd.env("IMPERATORIUM_POLICY", &path);
            }
        }
        for (k, v) in case["env"].as_object().unwrap() {
            cmd.env(k, v.as_str().unwrap());
        }
        fake.take();
        let (code, stdout, stderr) = run(&mut cmd, case["stdin"].as_str().unwrap().as_bytes());
        let seen = fake.take();
        let mut types: Vec<String> = seen.iter().filter_map(|(_, _, c, _)| c.clone()).collect();
        types.sort();
        types.dedup();
        let got = json!({
            "code": code,
            "stdout": String::from_utf8_lossy(&stdout),
            "requests": seen.iter().map(|(_, p, _, b)| json!([p, String::from_utf8_lossy(b)])).collect::<Vec<_>>(),
            "content_types": types,
            "audit": audit(&state.path),
        });
        let want = &case["want"];
        let label = || {
            format!(
                "{} {} {:.100} (stderr {:.200})",
                case["test"].as_str().unwrap(),
                which,
                case["stdin"].as_str().unwrap(),
                String::from_utf8_lossy(&stderr)
            )
        };
        let diff = ["code", "stdout", "requests", "content_types"]
            .iter()
            .find_map(|k| first_difference(&want[*k], &got[*k], k, true))
            .or_else(|| first_difference(&want["audit"], &got["audit"], "audit", false));
        f.check(label, diff);
    }
    f.finish("hooks_blackbox");
}
