//! worktree_twin.py replayed: merge / PR / discard of background-session
//! worktrees in a throwaway project (local bare origin, gh unauthenticated),
//! every answer, what is left in the repository, what reached the origin and
//! the events recorded, against tests/golden/worktree_twin/steps.jsonl.
#![cfg(windows)]

mod common;
use common::git::{git, worktree_project, write};
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

const SIDE: &str = "proj-rs";

/// The golden's `proj-<side>` for this run's project.
fn sided(v: &Value) -> Value {
    serde_json::from_str(&v.to_string().replace("proj-<side>", SIDE)).unwrap()
}

/// worktree_twin.py state().
fn left(main: &Path) -> Value {
    let mut worktrees: Vec<String> = git(main, &["worktree", "list", "--porcelain"])
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(|p| p.rsplit(|c| c == '/' || c == '\\').next().unwrap().to_string())
        .collect();
    worktrees.sort();
    let mut branches: Vec<String> = git(main, &["branch", "--format=%(refname:short)"])
        .split_whitespace()
        .map(String::from)
        .collect();
    branches.sort();
    let log: Vec<String> = git(main, &["log", "--format=%s", "main"])
        .lines()
        .map(String::from)
        .collect();
    json!({"worktrees": worktrees, "branches": branches, "log": log,
           "status": git(main, &["status", "--porcelain"])})
}

#[test]
fn worktree_actions_answer_as_python_did() {
    // Declared first: dropped last, after the dispatcher has let go.
    let tmp = TempDir::new("wt");
    let t0 = Instant::now();
    let main = worktree_project(&tmp.path, SIDE);
    let origin = tmp.join("origins").join(format!("{SIDE}.git"));
    let setup = t0.elapsed();
    let gh = tmp.join("gh");
    std::fs::create_dir_all(&gh).unwrap();
    let mut env: Vec<(String, String)> = common::git::GIT_ENV
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    for (k, v) in [
        ("GH_CONFIG_DIR", gh.to_string_lossy().into_owned()),
        ("GH_TOKEN", String::new()),
        ("GITHUB_TOKEN", String::new()),
        ("GH_PROMPT_DISABLED", "1".into()),
    ] {
        env.push((k.into(), v));
    }
    let d = Dispatcher::start(Options {
        config: Some(json!({"roots": [tmp.join("projects").to_string_lossy()]})),
        env,
        ..Options::new()
    });
    let places = Places::new(&[("tmp", &tmp.path)]);
    let steps = read_jsonl(&golden_dir("worktree_twin").join("steps.jsonl"));
    let mut f = Failures::default();
    let mut slowest = (std::time::Duration::ZERO, String::new());
    let mut requests = std::time::Duration::ZERO;
    for s in &steps {
        let label = s["label"].as_str().unwrap_or("").to_string();
        match s["step"].as_str().unwrap() {
            "worktree" => {
                let body = places.put(&sided(&s["body"]));
                let t = Instant::now();
                let (code, answer) = d.post(s["path"].as_str().unwrap(), &body);
                let took = t.elapsed();
                requests += took;
                if took > slowest.0 {
                    slowest = (took, label.clone());
                }
                eprintln!("  {took:>9.2?}  {label}");
                // The script compared json.dumps(sort_keys=True) text.
                let want = places.put(&sided(&s["want"]));
                f.check(
                    || format!("worktree: {label}"),
                    first_difference(&want, &json!([code, answer]), "$", false),
                );
            }
            "main checkout" => {
                if label == "main busy" {
                    write(&main.join("a.txt"), "being edited in main\n");
                } else {
                    git(&main, &["checkout", "--", "a.txt"]);
                }
            }
            "left" => f.check(
                || "what is left in the repository".to_string(),
                first_difference(&sided(&s["want"]), &left(&main), "$", false),
            ),
            "pushed" => {
                let got = git(&origin, &["branch", "--list", "agent-pr"]).trim().to_string();
                f.check(
                    || "pushed: agent-pr in the origin".to_string(),
                    first_difference(&s["want"], &json!(got), "$", false),
                );
            }
            "events" => {
                let state = d.state_json();
                let got: Vec<Value> = state["events"]
                    .as_array()
                    .map(|a| a.as_slice())
                    .unwrap_or(&[])
                    .iter()
                    .filter(|e| e["kind"].as_str().is_some_and(|k| k.starts_with("worktree_")))
                    .map(|e| json!([e["kind"], e["summary"]]))
                    .collect();
                f.check(
                    || "events: worktree_*".to_string(),
                    first_difference(&s["want"], &json!(got), "$", false),
                );
            }
            other => panic!("unknown step {other}"),
        }
    }
    eprintln!(
        "worktree_twin: project setup {setup:.2?}; {} requests took {requests:.2?} in all, slowest {:.2?} ({})",
        f.checked, slowest.0, slowest.1
    );
    assert!(f.checked == 21, "only {} checks", f.checked);
    f.finish("worktree_twin");
}
