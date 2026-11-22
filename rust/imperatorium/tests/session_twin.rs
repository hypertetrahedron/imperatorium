//! session_twin.py's recorded steps against the binary: resume, fork, new
//! and close, Send As Typed and the queue's refusals, dispatch and focus,
//! with fakecli standing in for claude.exe and wt.exe and five hidden
//! fakecli consoles as the running sessions (see tests/golden/README.md,
//! "session_twin.py").
//!
//! Safety, because a launch that misses the fakes opens a real Claude Code
//! window on the desktop: no step sends mode `launch`, nor `auto` with a
//! target that fails; before the first step, every way the dispatcher
//! finds `wt` and `claude` is checked to end at the fakes
//! (safety::assert_launches_are_fakes); deep links go to a file
//! (IMPERATORIUM_TEST_DEEPLINK_LOG) and the test stops at the first one; a
//! step the golden says launched stops the test if fakecli did not log it.
//! The consoles are killed on drop (a panic included), and the dispatcher's
//! drop stops everything running from its bin folder.
#![cfg(windows)]

mod common;
use common::console::{console_env, Console};
use common::safety::{assert_launches_are_fakes, deep_links};
use common::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

const GONE: &str = "c0c0c0c0-0000-4000-8000-000000000000";
const ROLES: &[(&str, char, bool)] = &[
    ("leaves", '1', false),
    ("stubborn", '2', true),
    ("replaced", '3', false),
    ("typing", '4', false),
    ("busy", '5', false),
];
/// Slower, not faster: the queue sweep (15 s) must not drain the queued
/// `/compact` before the end, as it did not in the recording. Nothing else
/// here waits on a scaled timer except the park, whose `wait` is divided by
/// it.
const SCALE: f64 = 10.0;

/// session_twin.py neutral(): real ids -> `<role>`, fresh ids ->
/// `<new-uuid>`, the OS's "cannot open" wording -> one placeholder.
fn neutral(text: &str, ids: &BTreeMap<String, String>) -> String {
    let mut t = text.to_string();
    for (real, name) in ids {
        t = t.replace(real.as_str(), name);
    }
    let uuid =
        regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}")
            .unwrap();
    let t = uuid.replace_all(&t, "<new-uuid>");
    let pipe = regex::Regex::new(r#""cannot open [^"]*""#).unwrap();
    pipe.replace_all(&t, "\"<cannot open the inbox>\"")
        .into_owned()
}

fn neutral_value(v: &Value, ids: &BTreeMap<String, String>) -> Value {
    serde_json::from_str(&neutral(&v.to_string(), ids)).unwrap()
}

/// The golden's `<role>` names back to this run's ids.
fn real_ids(v: &Value, ids: &BTreeMap<String, String>) -> Value {
    match v {
        Value::String(s) => {
            let mut t = s.clone();
            for (real, name) in ids {
                t = t.replace(name.as_str(), real);
            }
            Value::String(t)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| real_ids(x, ids)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), real_ids(x, ids)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// typing steps: `id` and `at` values as 0, wherever they are.
fn zero_ids(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().map(zero_ids).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| {
                    let x = if k == "id" || k == "at" {
                        json!(0)
                    } else {
                        zero_ids(x)
                    };
                    (k.clone(), x)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// fakecli's launches (not the consoles' keys and lines).
fn launches(d: &Dispatcher) -> usize {
    d.fake_log()
        .iter()
        .filter(|e| e["prog"] != "console")
        .count()
}

fn did_launch(want: &Value) -> bool {
    let a = &want[1];
    a["ok"] == json!(true)
        && (a["where"].is_string() || a["action"] == "background" || a["action"] == "new")
}

#[test]
fn session_twin_answers_as_python_did() {
    let began = Instant::now();
    let steps = read_jsonl(&golden_dir("session_twin").join("steps.jsonl"));
    // <tmp>: the dispatcher's home is <tmp>\rs-home, so the catalogue's
    // root is <tmp>, holding the folders the recording's held.
    let twin = TempDir::new("session-twin");
    for f in ["bin", "claude", "py", "py-home", "rs", "rs-home", "work"] {
        std::fs::create_dir_all(twin.join(f)).unwrap();
    }
    let work = twin.join("work");
    let guard = TempDir::new("session-twin-guard");
    let links = guard.join("deeplinks.txt");
    let ntfy = FakeServer::start();
    let pipe_tag = format!("imp-session-twin-nobody-{}", std::process::id());
    let config = json!({
        "notify": {"ntfy": {"enabled": true, "server": ntfy.url("/"), "topic": "twin-rs",
                            "delay": 1.5, "priority": "high", "token": "tk"},
                   "speak": {"enabled": false}},
        "background": {"model": "sonnet", "permission_mode": "plan"},
        "chat": {"send_as_typed": true},
    });
    let d = Dispatcher::start(Options {
        config: Some(config),
        fake_cli: vec!["claude.exe", "wt.exe"],
        home: Some(twin.join("rs-home")),
        env: vec![
            // A session's own variables, which must not reach a launch.
            ("AI_AGENT".into(), "claude-code".into()),
            ("CLAUDE_CODE_ENTRYPOINT".into(), "cli".into()),
            ("CLAUDE_PID".into(), "1".into()),
            ("IMPERATORIUM_TEST_TIME_SCALE".into(), SCALE.to_string()),
            (
                "IMPERATORIUM_TEST_DEEPLINK_LOG".into(),
                links.to_string_lossy().into_owned(),
            ),
        ],
        ..Options::new()
    });
    assert_launches_are_fakes(&d.bin_dir);
    let started = began.elapsed();

    // The sessions: hidden consoles running the fake claude, logging what is
    // typed into them to the dispatcher's fake log.
    let log = d.root.join("fake.jsonl");
    let env = console_env(&d.bin_dir, &[("FAKE_LOG", log.as_os_str())]);
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    ids.insert(GONE.into(), "<gone>".into());
    let mut sid = BTreeMap::new();
    let mut consoles: BTreeMap<&str, Console> = BTreeMap::new();
    for (role, digit, stubborn) in ROLES {
        let mut args = vec!["--fake-console"];
        if *stubborn {
            args.push("--stubborn");
        }
        let c = Console::start(&d.bin_dir.join("claude.exe"), &args, &env, &work);
        let id = format!(
            "{}bbbb-1111-4111-8111-111111111111",
            digit.to_string().repeat(4)
        );
        let mut rec = json!({"pid": c.pid, "cwd": work, "name": format!("twin-{role}"),
                             "kind": "interactive",
                             "status": if *role == "busy" { "busy" } else { "idle" }});
        if *role == "typing" {
            // Its inbox, which nothing serves.
            rec["messagingSocketPath"] = json!(format!(r"\\.\pipe\{pipe_tag}"));
        }
        d.session_record(&id, rec);
        ids.insert(id.clone(), format!("<{role}>"));
        sid.insert(*role, id);
        consoles.insert(*role, c);
    }
    // A record whose process is gone.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    std::fs::write(
        d.claude.join("sessions").join(format!("{GONE}.json")),
        json!({"sessionId": GONE, "cwd": work, "name": "twin-gone", "kind": "interactive",
               "status": "", "messagingSocketPath": null, "startedAt": now_ms})
        .to_string(),
    )
    .unwrap();

    let places = Places::new(&[("tmp", twin.path.as_path())]);
    let token = d.window_token();
    let mut f = Failures::default();
    let mut hooked = false;
    let stop_if_unsafe = |label: &str| {
        let opened = deep_links(&links);
        assert!(
            opened.is_empty(),
            "SAFETY: step {label:?} made the dispatcher open a deep link {opened:?}; stopped"
        );
    };
    let mut timings: Vec<(String, Duration)> = Vec::new();
    for s in &steps {
        let label = s["label"].as_str().unwrap_or("").to_string();
        let t0 = Instant::now();
        match s["step"].as_str().unwrap() {
            "post" => {
                let path = s["path"].as_str().unwrap();
                if path == "/api/dispatch" && !hooked {
                    // The typing console becomes reachable (a token, and an
                    // inbox that is not there), so routing can find it.
                    let (code, _) = d.post(
                        "/hook",
                        &json!({
                        "hook_event_name": "SessionStart", "session_id": sid["typing"],
                        "cwd": work, "source": "startup", "messaging_token": "tok",
                        "messaging_socket": format!(r"\\.\pipe\{pipe_tag}")}),
                    );
                    assert_eq!(code, 200);
                    hooked = true;
                }
                let body = real_ids(&places.put(&s["body"]), &ids);
                if let Some(mode) = body.get("mode").and_then(Value::as_str) {
                    assert!(
                        mode != "launch" && mode != "auto",
                        "SAFETY: step {label:?} would send mode {mode}"
                    );
                }
                let before = launches(&d);
                let (code, answer) = d.post(path, &body);
                let got = neutral_value(&json!([code, answer]), &ids);
                let want = places.put(&s["want"]);
                stop_if_unsafe(&label);
                if did_launch(&want) {
                    // Fire-and-forget: fakecli logs it a moment later. If it
                    // never does, the launch went somewhere else: stop.
                    let reached = wait_until(Duration::from_secs(15), || {
                        (launches(&d) > before).then_some(())
                    });
                    assert!(
                        reached.is_some(),
                        "SAFETY: step {label:?} launched, but not through fakecli; stopped. \
                         Answer: {got}"
                    );
                }
                f.check(
                    || format!("post {label:?} ({path})"),
                    first_difference(&want, &got, "$", false),
                );
            }
            "typing" => {
                let typing = &sid["typing"];
                let typed = |prompt: &str| {
                    d.post_window(
                        "/api/typed",
                        &json!({"session_id": typing, "prompt": prompt}),
                        &token,
                    )
                };
                let (code, answer) = match label.as_str() {
                    "typed, not from the window" => {
                        d.post("/api/typed", &json!({"session_id": typing, "prompt": "hi"}))
                    }
                    "typed" => typed("hello\nworld  !"),
                    "typed, a command" => typed("/model opus"),
                    "typed, /exit" => typed("/exit"),
                    "typed, a menu" => typed("/plugin"),
                    "typed, shell mode" => typed("!rm -rf build"),
                    "typed, blank" => typed(" \n "),
                    "typed, busy" => d.post_window(
                        "/api/typed",
                        &json!({"session_id": sid["busy"], "prompt": "hi"}),
                        &token,
                    ),
                    "typed, nobody" => d.post_window(
                        "/api/typed",
                        &json!({"session_id": "nobody", "prompt": "hi"}),
                        &token,
                    ),
                    "typed, parked" => std::thread::scope(|scope| {
                        // A permission request parked for 2 s (its wait is
                        // scaled), seen in /api/pending before typing.
                        let park = scope.spawn(|| {
                            d.post(
                                "/permission/park",
                                &json!({
                                "session_id": typing, "tool_name": "Bash",
                                "tool_input": {"command": "ls"}, "wait": 2.0 / SCALE}),
                            )
                        });
                        let seen = wait_until(Duration::from_secs(10), || {
                            let (_, p) = d.get("/api/pending");
                            p["pending"]
                                .as_array()
                                .filter(|a| a.iter().any(|x| x["session_id"] == json!(typing)))
                                .map(|_| ())
                        });
                        assert!(seen.is_some(), "the park never showed in /api/pending");
                        let got = typed("hi");
                        park.join().unwrap();
                        got
                    }),
                    "queue /exit" => d.post(
                        "/api/queue",
                        &json!({"session_id": typing, "prompt": "/exit"}),
                    ),
                    "queue /compact" => d.post(
                        "/api/queue",
                        &json!({"session_id": typing, "prompt": "/compact"}),
                    ),
                    other => panic!("a typing step this test does not know: {other:?}"),
                };
                stop_if_unsafe(&label);
                let got = zero_ids(&neutral_value(&json!([code, answer]), &ids));
                f.check(
                    || format!("typing {label:?}"),
                    first_difference(&places.put(&s["want"]), &got, "$", false),
                );
            }
            "launched" => {
                let want = places.put(&s["want"]);
                let n = want.as_array().unwrap().len();
                // Let the last fire-and-forget launches log.
                wait_until(Duration::from_secs(10), || {
                    (d.fake_log().len() >= n).then_some(())
                });
                let entries: Vec<Value> = d
                    .fake_log()
                    .into_iter()
                    .map(|mut e| {
                        if let Some(args) = e.get_mut("args").and_then(Value::as_array_mut) {
                            for a in args.iter_mut() {
                                let s = a.as_str().unwrap_or("").to_string();
                                if s.to_lowercase().ends_with("wt.exe") {
                                    *a =
                                        json!(Path::new(&s).file_name().unwrap().to_string_lossy());
                                }
                            }
                        }
                        e
                    })
                    .collect();
                let got = neutral_value(&Value::Array(entries.clone()), &ids);
                f.check(
                    || "launched (fakecli's log)".into(),
                    first_difference(&want, &got, "$", false),
                );
                let leaked: Vec<&Value> = entries
                    .iter()
                    .filter(|e| e["session_env"].as_array().is_some_and(|a| !a.is_empty()))
                    .collect();
                f.check(
                    || "session variables reached a launch".into(),
                    (!leaked.is_empty()).then(|| format!("{leaked:?}")),
                );
                let lines: Vec<String> = entries
                    .iter()
                    .filter_map(|e| e["line"].as_str().map(str::to_string))
                    .collect();
                let typed_want = ["hello world !", "/model opus", "rm -rf build"];
                let typed_got: Vec<&str> = lines
                    .iter()
                    .map(String::as_str)
                    .filter(|l| typed_want.contains(l))
                    .collect();
                f.check(
                    || "typed lines".into(),
                    (typed_got != typed_want)
                        .then(|| format!("{lines:?}, expected {typed_want:?} among them")),
                );
            }
            "events" => {
                let events: Vec<Value> = d.state_json()["events"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|e| json!([e.get("kind"), e.get("session_id"), e.get("summary")]))
                    .collect();
                let got = neutral_value(&Value::Array(events), &ids);
                f.check(
                    || "events (kind, session_id, summary)".into(),
                    first_difference(&places.put(&s["want"]), &got, "$", false),
                );
            }
            other => panic!("a step this test does not know: {other:?}"),
        }
        timings.push((label, t0.elapsed()));
    }
    // The closed consoles left (on /exit, or killed); the others are still
    // there.
    let alive: Vec<&str> = ["leaves", "stubborn", "replaced"]
        .into_iter()
        .filter(|r| !consoles.get(*r).unwrap().wait_exit(Duration::from_secs(5)))
        .collect();
    f.check(
        || "closed consoles exited".into(),
        (!alive.is_empty()).then(|| format!("still running: {alive:?}")),
    );
    f.check(
        || "deep links opened".into(),
        Some(deep_links(&links))
            .filter(|l| !l.is_empty())
            .map(|l| format!("{l:?}")),
    );
    timings.sort_by_key(|t| std::cmp::Reverse(t.1));
    eprintln!(
        "session_twin: {:.1} s in all ({:.1} s to start the dispatcher); slowest steps: {}",
        began.elapsed().as_secs_f64(),
        started.as_secs_f64(),
        timings
            .iter()
            .take(5)
            .map(|(l, t)| format!("{l:?} {:.2} s", t.as_secs_f64()))
            .collect::<Vec<_>>()
            .join(", ")
    );
    assert!(f.checked >= 50, "only {} checks", f.checked);
    drop(consoles);
    drop(d);
    f.finish("session_twin");
}
