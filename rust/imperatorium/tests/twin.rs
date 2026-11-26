//! Replays tests/golden/twin/steps.jsonl: the hook events, the refused POSTs,
//! the /api POSTs, the session ids, the state file and the GETs, each answered
//! as the Python dispatcher answered when the file was recorded (frozen at the
//! git tag `last-python`; see golden/README.md). Compared as the recording
//! script did: POST answers as equal values, the state and the GETs by
//! first_difference (key order counts), clock readings masked.
#![cfg(windows)]

mod common;
use common::*;
use serde_json::{json, Value};
use std::time::Duration;

const CLOCKS: &[&str] = &[
    "seen_at",
    "last_event_at",
    "last_stop_at",
    "attention_since",
    "at",
    "start",
    "end",
    "since",
    "now",
];

/// twin.py masked(): numbers under a clock key become "<clock>".
fn masked(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| {
                    let clock = CLOCKS.contains(&k.as_str()) && x.is_number();
                    (k.clone(), if clock { json!("<clock>") } else { masked(x) })
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(masked).collect()),
        other => other.clone(),
    }
}

/// GET until the answer matches (usage and matrix were retried by the
/// script in case spend moved), or three tries.
fn get_masked(d: &Dispatcher, path: &str, want: &Value, retry: bool) -> (Value, Option<String>) {
    let mut got = Value::Null;
    let mut diff = None;
    for _ in 0..if retry { 3 } else { 1 } {
        let (code, body) = d.get(path);
        got = json!([code, masked(&body)]);
        diff = first_difference(want, &got[1], "$", true);
        if diff.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    (got, diff)
}

#[test]
fn twin_answers_as_python_did() {
    // Home is the checkout, CLAUDE_CONFIG_DIR an empty folder (Dispatcher's).
    let d = Dispatcher::start(Options {
        home: Some(repo()),
        ..Options::new()
    });
    let steps = read_jsonl(&golden_dir("twin").join("steps.jsonl"));
    let mut f = Failures::default();
    let mut n = [0usize; 6];
    for s in &steps {
        let kind = s["step"].as_str().unwrap();
        match kind {
            "hook" => {
                n[0] += 1;
                let (code, answer) = d.post("/hook", &s["body"]);
                f.check(
                    || format!("hook {} #{}", s["body"]["hook_event_name"], n[0]),
                    first_difference(&s["want"], &json!([code, answer]), "$", false),
                );
            }
            "refused" => {
                n[1] += 1;
                let raw: Vec<u8> = match s["raw"].as_str() {
                    // latin-1 text of the bytes
                    Some(t) => t.chars().map(|c| c as u32 as u8).collect(),
                    None => serde_json::to_vec(&s["body"]).unwrap(),
                };
                let origin = s["origin"].as_str().map(|o| o.replace("%d", &d.port.to_string()));
                let headers: Vec<(&str, &str)> =
                    origin.iter().map(|o| ("Origin", o.as_str())).collect();
                let (code, answer) = d.request(
                    s["path"].as_str().unwrap(),
                    Some(raw),
                    s["ctype"].as_str(),
                    &headers,
                );
                f.check(
                    || format!("refused #{} {} {}", n[1], s["ctype"], s["raw"]),
                    first_difference(&s["want"], &json!([code, answer]), "$", false),
                );
            }
            "post" => {
                n[2] += 1;
                let (code, answer) = d.post(s["path"].as_str().unwrap(), &s["body"]);
                f.check(
                    || format!("POST {} {}", s["path"], s["body"]),
                    first_difference(&s["want"], &json!([code, answer]), "$", false),
                );
            }
            "session ids" => {
                n[3] += 1;
                let (_, body) = d.get("/api/sessions");
                let mut ids: Vec<String> = body["sessions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| !r["remote"].as_bool().unwrap_or(false))
                    .map(|r| r["sessionId"].as_str().unwrap().to_string())
                    .collect();
                ids.sort();
                f.check(
                    || "/api/sessions ids after dismiss".to_string(),
                    first_difference(&s["want"], &json!(ids), "$", true),
                );
            }
            "state.json" => {
                n[4] += 1;
                let mut diff = None;
                for _ in 0..20 {
                    let mut state = d.state_json();
                    // `dismissed` is {key: when}: the values are clocks.
                    if let Some(m) = state.get_mut("dismissed").and_then(|x| x.as_object_mut()) {
                        for v in m.values_mut() {
                            *v = json!("<clock>");
                        }
                    }
                    // Recorded since the Python reference froze: when a
                    // StopFailure happened (2026-11-18), what a later prompt
                    // or reply is measured against to clear it.
                    if let Some(m) = state.get_mut("sessions").and_then(|x| x.as_object_mut()) {
                        for rec in m.values_mut().filter_map(|r| r.as_object_mut()) {
                            rec.shift_remove("last_failure_at");
                        }
                    }
                    diff = first_difference(&s["want"], &masked(&state), "state", true);
                    if diff.is_none() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                f.check(|| "state.json".to_string(), diff);
            }
            "get" => {
                n[5] += 1;
                let path = s["path"].as_str().unwrap();
                let retry = path == "/api/usage" || path == "/api/matrix";
                let (got, diff) = get_masked(&d, path, &s["want"], retry);
                assert_eq!(got[0], 200, "GET {path}");
                f.check(|| format!("GET {path}"), diff);
            }
            other => panic!("unknown twin step {other}"),
        }
    }
    assert_eq!(n, [32, 7, 11, 1, 1, 8], "steps replayed: {n:?}");
    f.finish("twin");
}
