//! Replays notify_twin (answers frozen at `last-python`, see golden/README.md):
//! the hooks and /api/notify calls the recording made,
//! the pushes the binary sends to a fake ntfy server (title, priority, tags,
//! token, message), the counters, and the events it keeps, each as Python
//! produced them (golden/notify_twin/steps.jsonl).
//!
//! The idle check is due 60 s after a Stop in production; the binary runs at
//! IMPERATORIUM_TEST_TIME_SCALE = SCALE, so it is due after 60 * SCALE s, and
//! this test's own waits are scaled the same.
#![cfg(windows)]

mod common;
#[path = "common/twins_a.rs"]
mod twins_a;
use common::*;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use twins_a::*;

const SCALE: f64 = 0.1;
const TOPIC: &str = "twin-rs";

#[test]
fn notifications_push_and_count_as_python_did() {
    let ntfy = Ntfy::start();
    let (d, _home) = start_notify(&ntfy, TOPIC, Some(SCALE));
    let places = Places::new(&[("tmp", &d.root.path)]);
    for (sid, name) in [
        ("a1a1a1a1-1111-4111-8111-111111111111", "twin-asks"),
        ("a2a2a2a2-2222-4222-8222-222222222222", "twin-answered"),
        ("a3a3a3a3-3333-4333-8333-333333333333", "twin-fails"),
        ("a4a4a4a4-4444-4444-8444-444444444444", "twin-quiet"),
        ("a5a5a5a5-5555-4555-8555-555555555555", "twin-later"),
    ] {
        d.session_record(
            sid,
            json!({"cwd": d.root.join("work").to_string_lossy(), "name": name,
                   "kind": "interactive", "status": "idle",
                   "messagingSocketPath": r"\\.\pipe\imp-notify-twin-nobody",
                   "startedAt": std::time::SystemTime::now()
                       .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64}),
        );
    }
    let started = Instant::now();
    let steps = read_jsonl(&golden_dir("notify_twin").join("steps.jsonl"));
    let sent = |d: &Dispatcher| d.get("/api/notify").1["sent"].as_u64().unwrap_or(0);
    let local = local_hhmm(4102444800);
    let clock = regex::Regex::new(r"resets \d\d:\d\d").unwrap();
    let mut f = Failures::default();
    let mut idle_after = None;

    for s in &steps {
        let kind = s["step"].as_str().unwrap();
        match kind {
            "post" => {
                let path = s["path"].as_str().unwrap();
                let (code, answer) = d.post(path, &places.put(&s["body"]));
                f.check(
                    || format!("POST {path} {}", s["body"]),
                    first_difference(&s["want"], &json!([code, answer]), "$", false),
                );
            }
            "sleep" => {
                let secs = s["seconds"].as_f64().unwrap();
                std::thread::sleep(Duration::from_secs_f64(secs * SCALE));
                if secs >= 3.0 {
                    // The pushes due so far (three) have gone out: the script
                    // slept to be sure; poll for it instead.
                    wait_until(Duration::from_secs(10), || (sent(&d) >= 3).then_some(()));
                }
            }
            "idle push" => {
                let t = Instant::now();
                let n = wait_until(Duration::from_secs(30), || {
                    (ntfy.to(TOPIC).len() >= 5).then_some(())
                });
                idle_after = Some(t.elapsed());
                f.check(
                    || "idle push".into(),
                    n.is_none().then(|| {
                        format!("only {} of 5 pushes arrived: {:?}", ntfy.to(TOPIC).len(), ntfy.to(TOPIC))
                    }),
                );
                std::thread::sleep(Duration::from_secs_f64(2.0 * SCALE));
            }
            "get" => {
                let path = s["path"].as_str().unwrap();
                let (_, got) = d.get(path);
                f.check(
                    || format!("GET {path}"),
                    first_difference(&s["want"], &got, "$", false),
                );
            }
            "pushes" => {
                let text = s["want"].to_string().replace("<local HH:MM of 4102444800>", &local);
                let mut want: Vec<Value> = serde_json::from_str::<Value>(&text)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .clone();
                let mut got = ntfy.to(TOPIC);
                want.sort_by_key(|v| v.to_string());
                got.sort_by_key(|v| v.to_string());
                f.check(
                    || "pushes".into(),
                    first_difference(&Value::Array(want), &Value::Array(got), "$", false),
                );
            }
            "events" => {
                let tuples = |state: &Value| -> Vec<Value> {
                    let mut v: Vec<Value> = state["events"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .map(|e| json!([e["kind"], e["session_id"], e["summary"]]))
                        .collect();
                    v.sort_by_key(|x| x.to_string());
                    v
                };
                let mut want: Vec<Value> = s["want"].as_array().unwrap().clone();
                want.sort_by_key(|x| x.to_string());
                // The state file is written a moment after the events happen.
                let got = wait_until(Duration::from_secs(10), || {
                    let g = tuples(&d.state_json());
                    (g.len() >= want.len()).then_some(g)
                })
                .unwrap_or_else(|| tuples(&d.state_json()));
                // The golden's summary carries its recording machine's local
                // time for 4102444800 (the pushes use a placeholder): neutral.
                let neutral = |v: Vec<Value>| {
                    serde_json::from_str::<Value>(&clock.replace_all(&Value::Array(v).to_string(), "resets HH:MM"))
                        .unwrap()
                };
                f.check(
                    || "events".into(),
                    first_difference(&neutral(want), &neutral(got), "$", false),
                );
            }
            other => panic!("unknown step {other}"),
        }
    }
    eprintln!(
        "notify twin: idle push {:?} after the idle step began; whole replay {:?}",
        idle_after.unwrap(),
        started.elapsed()
    );
    f.finish("notify twin");
}
