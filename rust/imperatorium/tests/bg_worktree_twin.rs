//! bg_worktree_twin.py replayed: background dispatch into a worktree made
//! from the chosen session's branch. fakecli stands in for claude.exe (it
//! logs `claude --bg` and where it ran, and starts nothing). Compared with
//! tests/golden/bg_worktree_twin/steps.jsonl: every answer (paths neutral),
//! where `claude --bg` ran, the events recorded, and what is left in the
//! repository (worktrees, their branches, the base each noted, whether one
//! carries feat's work).
#![cfg(windows)]

mod common;
use common::git::{bg_project, git, git_raw};
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const SID: &str = "6666bbbb-1111-4111-8111-111111111111";

/// bg_worktree_twin.py neutral(): this run's checkouts and project name
/// made neutral, in the three spellings the answers use.
fn neutral(v: &Value, main: &Path, feat: &Path) -> Value {
    let mut text = v.to_string();
    for (real, name) in [(feat, "<feat>"), (main, "<proj>")] {
        let real = real.to_string_lossy().into_owned();
        let escaped = serde_json::to_string(&real).unwrap();
        for form in [escaped[1..escaped.len() - 1].to_string(), real.replace('\\', "/"), real] {
            text = text.replace(&form, name);
        }
    }
    serde_json::from_str(&text.replace("proj-rs", "proj-<side>")).unwrap()
}

/// What the dispatches left in the repository (bg_worktree_twin.py left()).
fn left(main: &Path) -> Value {
    let mut out = Vec::new();
    for block in git(main, &["worktree", "list", "--porcelain"]).trim().split("\n\n") {
        let field = |key: &str| {
            block
                .lines()
                .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(' ')))
                .unwrap_or("")
                .to_string()
        };
        let path = field("worktree");
        if !path.replace('\\', "/").contains(".claude/worktrees") {
            continue;
        }
        let branch = field("branch").replace("refs/heads/", "");
        let base = String::from_utf8_lossy(
            &git_raw(
                main,
                &["config", &format!("branch.{branch}.imperatoriumbase")],
            )
            .stdout,
        )
        .trim()
        .to_string();
        let p = Path::new(&path);
        out.push(json!({"leaf": p.file_name().unwrap().to_string_lossy(), "branch": branch,
                        "base": base, "has_feat_work": p.join("feat.txt").is_file()}));
    }
    Value::Array(out)
}

#[test]
fn background_dispatch_answers_as_python_did() {
    let tmp = TempDir::new("bgwt");
    let (main, feat) = bg_project(&tmp.path, "rs");
    // The project is found under <tmp>: the Python twin's default root was
    // the folder its home sat in; since 2026-11-24 an empty `roots` lists
    // no folders, so <tmp> is named.
    let d = Dispatcher::start(Options {
        config: Some(json!({"background": {"model": "sonnet"}, "roots": [tmp.path.to_string_lossy()]})),
        fake_cli: vec!["claude.exe"],
        home: Some(tmp.join("rs-home")),
        ..Options::new()
    });
    // The chosen session: in the feat checkout, alive as this test.
    d.session_record(
        SID,
        json!({"cwd": feat.to_string_lossy(), "name": "twin-feat", "kind": "interactive",
               "status": "idle"}),
    );
    let steps = read_jsonl(&golden_dir("bg_worktree_twin").join("steps.jsonl"));
    let places = Places::new(&[("tmp", &tmp.path)]);
    let mut f = Failures::default();
    let mut dispatches = 0;
    for s in &steps {
        let nb = |v: &Value| neutral(v, &main, &feat);
        match s["step"].as_str().unwrap() {
            "dispatch" => {
                dispatches += 1;
                let label = s["label"].as_str().unwrap();
                // The golden's body is neutral: put this run's paths back.
                let text = s["body"].to_string().replace("<sid>", SID);
                let body: Value = serde_json::from_str(&text).unwrap();
                let body = places.put(&body_paths(&body, &main));
                let (code, mut answer) = d.post(s["path"].as_str().unwrap(), &body);
                if answer.get("bg_id").is_some() {
                    answer["bg_id"] = json!("<id>");
                }
                f.check(
                    || format!("dispatch: {label}"),
                    first_difference(&s["want"], &nb(&json!([code, answer])), "$", false),
                );
            }
            "launched" => {
                let want = s["want"].as_array().unwrap().len();
                let calls = wait_until(Duration::from_secs(10), || {
                    let c: Vec<Value> = d
                        .fake_log()
                        .into_iter()
                        .filter(|c| c["prog"] == "claude" && c["args"][0] != "agents")
                        .collect();
                    (c.len() >= want).then_some(c)
                })
                .unwrap_or_default();
                let got: Vec<Value> = calls
                    .iter()
                    .map(|c| nb(&json!({"args": c["args"], "cwd": c["cwd"]})))
                    .collect();
                f.check(
                    || "claude --bg: args and cwd".to_string(),
                    first_difference(&s["want"], &json!(got), "$", false),
                );
            }
            "events" => {
                let got: Vec<Value> = d.state_json()["events"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|e| e["kind"] == "dispatch_background")
                    .map(|mut e| {
                        for k in ["at", "bg_id"] {
                            e.as_object_mut().unwrap().remove(k);
                        }
                        nb(&e)
                    })
                    .collect();
                f.check(
                    || "events: dispatch_background".to_string(),
                    first_difference(&s["want"], &json!(got), "$", false),
                );
            }
            "left" => f.check(
                || "what is left: worktrees, branches, bases".to_string(),
                first_difference(&s["want"], &nb(&left(&main)), "$", true),
            ),
            other => panic!("unknown step {other}"),
        }
    }
    assert_eq!(dispatches, 4);
    assert_eq!(f.checked, 7);
    f.finish("bg_worktree_twin");
}

/// A golden body names the project as `<proj>`: this run's main checkout.
fn body_paths(body: &Value, main: &Path) -> Value {
    let text = body.to_string().replace(
        "\"<proj>\"",
        &serde_json::to_string(&main.to_string_lossy()).unwrap(),
    );
    serde_json::from_str(&text).unwrap()
}
