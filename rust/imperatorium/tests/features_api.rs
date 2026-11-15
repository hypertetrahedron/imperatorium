//! test_features.py's dispatcher behaviour, through the binary's HTTP API:
//! the prompt queue, the statusline and the plan line, the idle check after a
//! Stop, resume / new / dismiss, the history, activity and error views, the
//! parked-request waits and the bundled pages. Each test has a dispatcher of
//! its own (port 0, temp folders, a Claude config folder of its own, fakecli
//! for wt and claude); session records carry this process's pid, so they
//! count as running, and nothing here ever asks for one to be closed.
#![cfg(windows)]

mod common;
use common::features::*;
use common::*;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const SCALE: &str = "0.05";
const SID: &str = "5e000000-0000-4000-8000-000000000001";

fn scaled() -> Vec<(&'static str, &'static str)> {
    vec![("IMPERATORIUM_TEST_TIME_SCALE", SCALE)]
}

fn sid(n: u32) -> String {
    format!("5e000000-0000-4000-8000-{n:012}")
}

// ------------------------------------------------------------- the queue

#[test]
fn features_queue_delivers_holds_requeues_and_sweeps() {
    let fx = Fx::start("queue", vec![], scaled(), &[]);

    // An idle session whose pipe is listening gets the prompt at once.
    let idle = (sid(1), Pipe::new("idle"));
    fx.record(&idle.0, json!({"status": "idle", "messagingSocketPath": idle.1.path}));
    fx.session_start(&idle.0, Some(&idle.1.path), Some("tok-idle"));
    let out = fx.post("/api/queue", json!({"session_id": idle.0, "prompt": "do the thing"}));
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["waiting"], 0);
    let got = idle.1.take_n(1, Duration::from_secs(10));
    assert_eq!(got.len(), 1, "delivered once");
    assert!(got[0].starts_with("{\"type\": \"auth\", \"token\": \"tok-idle\"}\n"), "{}", got[0]);
    assert!(got[0].contains("do the thing"));
    assert_eq!(fx.queue(), json!({}), "an empty queue leaves the map");

    // A busy one holds its prompts until a Stop, one per turn.
    let busy = (sid(2), Pipe::new("busy"));
    fx.record(&busy.0, json!({"status": "busy", "messagingSocketPath": busy.1.path}));
    fx.session_start(&busy.0, Some(&busy.1.path), Some("tok-busy"));
    fx.post("/api/queue", json!({"session_id": busy.0, "prompt": "first"}));
    let second = fx.post("/api/queue", json!({"session_id": busy.0, "prompt": "second"}));
    assert_eq!(second["waiting"], 2);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(busy.1.count(), 0, "nothing sent to a busy session");
    let first_stop = fx.stop(&busy.0);
    assert!(first_stop["delivered"].is_string(), "{first_stop}");
    let got = busy.1.take_n(1, Duration::from_secs(10));
    assert!(got[0].contains("first") && !got[0].contains("second"), "{got:?}");
    assert_eq!(fx.queue()[&busy.0].as_array().unwrap().len(), 1);
    assert!(fx.stop(&busy.0)["delivered"].is_string());
    let got = busy.1.take_n(1, Duration::from_secs(10));
    assert!(got[0].contains("second"), "{got:?}");
    assert!(fx.queue().get(&busy.0).is_none());
    // A third Stop has nothing left to deliver.
    assert!(fx.stop(&busy.0).get("delivered").is_none());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(busy.1.count(), 0);

    // A delivery that fails keeps the prompt, with the error.
    let gone = sid(3);
    let nobody = format!("\\\\.\\pipe\\imp-feat-{}-nobody", std::process::id());
    fx.record(&gone, json!({"status": "idle", "messagingSocketPath": nobody}));
    fx.session_start(&gone, Some(&nobody), Some("tok-gone"));
    let out = fx.post("/api/queue", json!({"session_id": gone, "prompt": "x"}));
    assert_eq!(out["ok"], true);
    let q = fx.queue();
    let held = q[&gone].as_array().expect("still queued");
    assert_eq!(held.len(), 1);
    assert!(held[0]["error"].as_str().unwrap().contains("pipe"), "{held:?}");

    // Queued before the session had a credential: the sweep catches it once
    // the session registers.
    let late = (sid(4), Pipe::new("late"));
    fx.record(&late.0, json!({"status": "idle", "messagingSocketPath": late.1.path}));
    fx.hook(json!({"hook_event_name": "Notification", "session_id": "poke", "cwd": "C:/x"}));
    let out = fx.post("/api/queue", json!({"session_id": late.0, "prompt": "waiting"}));
    assert_eq!(out["ok"], true);
    assert_eq!(out["waiting"], 1);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(late.1.count(), 0, "not dispatchable yet");
    fx.session_start(&late.0, Some(&late.1.path), Some("tok-late"));
    let got = late.1.take_n(1, Duration::from_secs(30));
    assert_eq!(got.len(), 1, "the sweep delivered it");
    assert!(got[0].contains("tok-late") && got[0].contains("waiting"));
}

// ------------------------------------------------- statusline and plan

fn statusline(fx: &Fx, sid: &str, extra: Value) {
    let mut body = json!({"hook_event_name": "imperatorium_statusline", "session_id": sid});
    for (k, v) in extra.as_object().into_iter().flatten() {
        body[k] = v.clone();
    }
    assert_eq!(fx.hook(body)["ok"], true);
}

fn plan(fx: &Fx) -> Value {
    fx.d.get("/api/usage").1["plan"].clone()
}

#[test]
fn features_a_statusline_event_updates_usage_not_the_event_log() {
    let fx = Fx::start("ingest", vec![], vec![], &[]);
    fx.record(SID, json!({}));
    fx.session_start(SID, None, None);
    let before = fx.events().len();
    statusline(
        &fx,
        SID,
        json!({"context_window": {"used_percentage": 55},
               "rate_limits": {"five_hour": {"used_percentage": 10, "resets_at": 4102444800u64}}}),
    );
    let row = wait_until(Duration::from_secs(10), || {
        fx.row(SID).filter(|r| r["usage"]["context_pct"] == 55.0)
    })
    .expect("the usage reaches the session's row");
    assert_eq!(row["usage"]["context_pct"], 55.0);
    assert_eq!(fx.events().len(), before, "a statusline is not an event");
    assert_eq!(fx.d.get("/api/usage").1["limits"]["five_hour"]["used"], 10.0);
}

fn home_with(account: &TempDir, org: Option<&str>) {
    let path = account.join(".claude.json");
    match org {
        Some(o) => std::fs::write(path, json!({"oauthAccount": {"organizationType": o}}).to_string()).unwrap(),
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[test]
fn features_plan_names_the_login_only_where_there_are_no_meters() {
    let account = TempDir::new("featacct");
    let profile = account.path.to_string_lossy().into_owned();
    let fx = Fx::start("plan", vec![], vec![("USERPROFILE", &profile)], &[]);

    // A Pro or Max login before its first reading shows nothing.
    home_with(&account, Some("claude_max"));
    statusline(&fx, "s1", json!({"model": {"id": "claude-opus-5-5"}}));
    assert_eq!(plan(&fx), Value::Null);
    // The account plan reads only the organization type.
    home_with(&account, Some("claude_enterprise"));
    assert_eq!(plan(&fx)["label"], "Claude Enterprise");
    home_with(&account, Some("claude_team"));
    assert_eq!(plan(&fx)["label"], "Claude Team");
    home_with(&account, Some("claude_max"));
    assert_eq!(plan(&fx), Value::Null);
    std::fs::write(account.join(".claude.json"), "{broken").unwrap();
    assert_eq!(plan(&fx), Value::Null);
    home_with(&account, None);
    assert_eq!(plan(&fx), Value::Null);
}

#[test]
fn features_plan_with_bedrock_sessions_and_beside_real_windows() {
    // Every session on Bedrock: the login does not apply to them.
    let account = TempDir::new("featacct2");
    home_with(&account, Some("claude_enterprise"));
    let profile = account.path.to_string_lossy().into_owned();
    let fx = Fx::start("plan-bedrock", vec![], vec![("USERPROFILE", &profile)], &[]);
    statusline(&fx, "s1", json!({"imperatorium_provider": "bedrock"}));
    assert_eq!(
        plan(&fx),
        json!({"label": "AWS Bedrock", "providers": ["bedrock"], "alongside": false})
    );

    // Beside real windows only the uncovered providers are named.
    let team = TempDir::new("featacct3");
    home_with(&team, Some("claude_team"));
    let profile = team.path.to_string_lossy().into_owned();
    let fx = Fx::start("plan-windows", vec![], vec![("USERPROFILE", &profile)], &[]);
    statusline(
        &fx,
        "s1",
        json!({"rate_limits": {"five_hour": {"used_percentage": 10, "resets_at": 4102444800u64}}}),
    );
    assert_eq!(plan(&fx), Value::Null);
    statusline(&fx, "s2", json!({"imperatorium_provider": "bedrock"}));
    assert_eq!(
        plan(&fx),
        json!({"label": "AWS Bedrock", "providers": ["bedrock"], "alongside": true})
    );
}

// ------------------------------------------------------- idle after stop

#[test]
fn features_an_idle_session_after_stop_is_waiting_for_you_once() {
    // IDLE_AFTER (60 s) is scaled to three seconds.
    let fx = Fx::start("idle", vec![], scaled(), &[]);
    let (a, b, c, e) = (sid(11), sid(12), sid(13), sid(14));
    fx.record(&a, json!({"status": "idle"}));
    fx.record(&b, json!({"status": "idle"}));
    fx.record(&c, json!({"status": "busy"}));
    fx.record(&e, json!({"status": "idle"}));
    for s in [&a, &b, &c, &e] {
        fx.session_start(s, None, None);
    }
    fx.stop(&a);
    // Two turns end in a row: only the latest one's check counts.
    fx.stop(&b);
    std::thread::sleep(Duration::from_millis(30));
    fx.stop(&b);
    fx.stop(&c);
    // A real idle_prompt arrives first.
    fx.stop(&e);
    fx.hook(json!({"hook_event_name": "Notification", "session_id": e,
                   "cwd": fx.proj_a.to_string_lossy(), "notification_type": "idle_prompt"}));

    let raised = |sid: &str| -> usize {
        fx.events()
            .iter()
            .filter(|ev| ev["kind"] == "idle_prompt" && ev["session_id"] == sid && ev["summary"] == "raised by the dispatcher")
            .count()
    };
    wait_until(Duration::from_secs(60), || (raised(&a) >= 1).then_some(())).expect("a minute after Stop (scaled)");
    let row = fx.row(&a).unwrap();
    assert_eq!(row["attention"], true);
    assert_eq!(row["last_notification"], "idle_prompt");
    // The others' checks were due at the same moment; give them a beat.
    wait_until(Duration::from_secs(20), || (raised(&b) >= 1).then_some(())).expect("b waits too");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(raised(&a), 1);
    assert_eq!(raised(&b), 1, "the first turn's stale check raised nothing");
    assert_eq!(raised(&c), 0, "a busy session is not waiting for you");
    assert_eq!(fx.row(&c).unwrap()["attention"], false);
    assert_eq!(raised(&e), 0, "a real idle_prompt is not announced again");
}

#[test]
fn features_a_session_that_scheduled_its_own_next_turn_is_not_waiting() {
    // /loop's ScheduleWakeup: idle for up to an hour, then carries on.
    let wake = "5ca1e000-0000-4000-8000-000000000001";
    let cwd = TempDir::new("featwake");
    let fires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        + 1200.0;
    let seed = Seed {
        sid: wake,
        cwd: &cwd.path,
        steps: vec![Step::User("watch CI"), Step::Wakeup("w1", fires)],
        custom_title: None,
        age: 5,
    };
    let fx = Fx::start("wakeup", vec![], scaled(), &[seed]);
    fx.record(wake, json!({"status": "idle", "cwd": cwd.path.to_string_lossy()}));
    fx.session_start(wake, None, None);
    fx.stop(wake);
    // Past the (scaled) minute, and then some.
    std::thread::sleep(Duration::from_secs(5));
    let row = fx.row(wake).unwrap();
    assert_eq!(row["attention"], false, "{row}");
    assert_eq!(row["work"]["wakeups"], 1, "{row}");
    assert!(row["work"]["wakes_at"].as_f64().unwrap() > fires - 100.0);
    assert!(!fx.events().iter().any(|e| e["kind"] == "idle_prompt"));
}

// ------------------------------------------- resume, new, close, dismiss

fn wt_args(fx: &Fx, n: usize) -> Vec<Vec<String>> {
    fx.launched_n("wt", n)
        .iter()
        .map(|e| e["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect())
        .collect()
}

#[test]
fn features_resume_and_fork_launch_through_wt_and_claude() {
    // Claude Code's own variables are set in the dispatcher's environment.
    let fx = Fx::start(
        "resume",
        vec!["wt.exe", "claude.exe"],
        vec![("CLAUDECODE", "1"), ("CLAUDE_PID", "7"), ("KEEP_ME", "1")],
        &[],
    );
    let work = fx.proj_a.to_string_lossy().into_owned();
    let say = |body: Value| fx.post("/api/resume", body);

    let out = say(json!({"session_id": ""}));
    assert_eq!((out["ok"].clone(), out["error"].clone()), (json!(false), json!("session id is required")));
    let out = say(json!({"session_id": "sid0"}));
    assert_eq!(out["ok"], false); // nothing says where it ran
    assert!(out["error"].as_str().unwrap().contains("gone"), "{out}");
    let gone = fx.proj_a.join("gone").to_string_lossy().into_owned();
    let out = say(json!({"session_id": "sid0", "cwd": gone}));
    assert!(out["error"].as_str().unwrap().contains("gone"), "{out}");
    let out = say(json!({"session_id": "sid1", "cwd": work, "where": "teleport"}));
    assert_eq!((out["ok"].clone(), out["error"].clone()), (json!(false), json!("where must be window or background")));
    assert!(fx.d.fake_log().is_empty(), "nothing was launched yet");

    // A window: the terminal command, in the folder, no fork.
    let out = say(json!({"session_id": "sid1", "cwd": work, "where": "window"}));
    assert_eq!((out["ok"].clone(), out["action"].clone()), (json!(true), json!("resume")));
    let calls = wt_args(&fx, 1);
    assert_eq!(calls.len(), 1);
    let args = &calls[0];
    assert_eq!(args[..4], ["-w", "0", "new-tab", "-d"]);
    assert!(args.contains(&"--resume".to_string()) && args.contains(&"sid1".to_string()));
    assert!(!args.contains(&"--fork-session".to_string()));
    assert_eq!(norm_path(fx.launched_n("wt", 1)[0]["cwd"].as_str().unwrap()), norm_path(&work));

    // A fork adds --fork-session.
    let out = say(json!({"session_id": "sid1", "cwd": work, "where": "window", "fork": true}));
    assert_eq!(out["action"], "fork");
    assert!(wt_args(&fx, 2)[1].contains(&"--fork-session".to_string()));

    // In the background: claude --bg with the prompt and --resume.
    let out = say(json!({"session_id": "sid1", "cwd": work, "where": "background", "prompt": "go"}));
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["id"], "1f2e3d4c");
    let bg = fx.launched_n("claude", 1);
    let bg: Vec<String> = bg[0]["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
    assert!(bg.contains(&"--bg".to_string()) && bg.contains(&"--resume".to_string()) && bg.contains(&"sid1".to_string()), "{bg:?}");
    assert!(bg.contains(&"go".to_string()), "{bg:?}");

    // A new session: a plain claude in a tab with a fresh id each time.
    let a = fx.post("/api/new", json!({"cwd": work}));
    let b = fx.post("/api/new", json!({"cwd": work}));
    assert_eq!((a["ok"].clone(), a["action"].clone()), (json!(true), json!("new")));
    assert_ne!(a["session_id"], b["session_id"]);
    let all = wt_args(&fx, 4);
    let last = &all[all.len() - 1];
    let n = last.len();
    assert_eq!(last[n - 3..], ["claude", "--session-id", b["session_id"].as_str().unwrap()]);
    assert!(!last.contains(&"--resume".to_string()));
    let out = fx.post("/api/new", json!({"cwd": gone}));
    assert!(out["error"].as_str().unwrap().contains("gone"), "{out}");
    // No project to start in.
    let out = fx.post("/api/new", json!({"session_id": "nope-nope"}));
    assert_eq!(out["ok"], false);

    // Neither launch carried the calling session's identity, but kept the rest.
    for e in fx.d.fake_log() {
        assert_eq!(e["session_env"], json!([]), "{e}");
    }
}

fn norm_path(p: &str) -> String {
    p.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

#[test]
fn features_launching_needs_wt_and_claude_on_the_path() {
    // No stand-ins at all: wt.exe and claude.exe are not found.
    let fx = Fx::start("nopath", vec![], vec![], &[]);
    let work = fx.proj_a.to_string_lossy().into_owned();
    let out = fx.post("/api/resume", json!({"session_id": "sid1", "cwd": work, "where": "window"}));
    assert_eq!(out["ok"], false);
    assert!(out["error"].as_str().unwrap().contains("wt.exe"), "{out}");
    // A background start that fails comes back as an error, not a crash.
    let out = fx.post("/api/resume", json!({"session_id": "sid1", "cwd": work, "where": "background"}));
    assert_eq!(out["ok"], false, "{out}");
    assert!(out["error"].as_str().is_some_and(|e| !e.is_empty()));
}

#[test]
fn features_resume_of_running_and_dead_sessions_and_new_beside_one() {
    let dead = "dead0000-0000-4000-8000-00000000000d";
    let fx_cwd = TempDir::new("featdead");
    std::fs::create_dir_all(&fx_cwd.path).unwrap();
    let seed = Seed {
        sid: dead,
        cwd: &fx_cwd.path,
        steps: vec![Step::User("old work"), Step::Text("done")],
        custom_title: Some("an old session"),
        age: 600,
    };
    let fx = Fx::start("running", vec!["wt.exe", "claude.exe"], vec![], &[seed]);
    let run = sid(21);
    fx.record(&run, json!({"status": "idle"}));
    fx.session_start(&run, None, None);

    // Still running: not into a window...
    let out = fx.post("/api/resume", json!({"session_id": run, "where": "window"}));
    assert_eq!(out["ok"], false);
    assert!(out["error"].as_str().unwrap().contains("still running"), "{out}");
    assert!(fx.launched("wt").is_empty());
    // ...but forking it is allowed, and logged.
    let out = fx.post("/api/resume", json!({"session_id": run, "where": "window", "fork": true}));
    assert_eq!(out["ok"], true, "{out}");
    assert!(wt_args(&fx, 1)[0].contains(&"--fork-session".to_string()));
    assert!(fx.events().iter().any(|e| e["kind"] == "fork"));

    // A dead session's folder comes from the history.
    let out = fx.post("/api/resume", json!({"session_id": dead}));
    assert_eq!(out["ok"], true, "{out}");
    let calls = fx.launched_n("wt", 2);
    assert_eq!(norm_path(calls[1]["cwd"].as_str().unwrap()), norm_path(&fx_cwd.path.to_string_lossy()));

    // A new session beside the selected one starts in its folder.
    let out = fx.post("/api/new", json!({"session_id": run}));
    assert_eq!(out["ok"], true, "{out}");
    assert!(out.get("closed").is_none());
    let calls = fx.launched_n("wt", 3);
    assert_eq!(norm_path(calls[2]["cwd"].as_str().unwrap()), norm_path(&fx.proj_a.to_string_lossy()));
    assert!(fx.events().iter().any(|e| e["kind"] == "new"));

    // New-and-close with a session that has no process launches nothing.
    let idle_none = sid(22);
    fx.record(&idle_none, json!({"pid": null}));
    fx.session_start(&idle_none, None, None);
    let before = fx.launched("wt").len();
    let out = fx.post("/api/new", json!({"session_id": idle_none, "close": true}));
    assert_eq!(out["ok"], false, "{out}");
    assert_eq!(fx.launched("wt").len(), before);
}

#[test]
fn features_close_and_dismiss_refuse_what_has_no_process_or_still_runs() {
    let fx = Fx::start("dismiss", vec!["wt.exe", "claude.exe"], vec![], &[]);
    // Unknown sessions: answered 200, not ok.
    for path in ["/api/resume", "/api/close", "/api/dismiss"] {
        let out = fx.post(path, json!({"session_id": "nope-nope"}));
        assert_eq!(out["ok"], false, "{path} {out}");
    }
    // Known by its hooks only, no process: nothing to close.
    let hooked = sid(31);
    fx.record(&hooked, json!({"pid": null}));
    fx.session_start(&hooked, None, Some("tok-hooked"));
    let out = fx.post("/api/close", json!({"session_id": hooked}));
    assert_eq!(out["ok"], false, "{out}");

    // Dismissing it takes it off the list, out of the archive and tokens.
    fx.post("/api/prefs", json!({"archived": [hooked, "other"]}));
    assert!(fx.row(&hooked).is_some());
    let out = fx.post("/api/dismiss", json!({"session_id": hooked}));
    assert_eq!(out, json!({"ok": true, "how": "dismissed"}));
    assert!(fx.row(&hooked).is_none());
    let state = wait_until(Duration::from_secs(10), || {
        let s = fx.d.state_json();
        if s["dismissed"].get(&hooked).is_some() {
            Some(s)
        } else {
            None
        }
    })
    .expect("dismissal persisted");
    assert_eq!(state["prefs"]["archived"], json!(["other"]));
    assert!(state["tokens"].get(&hooked).is_none());
    assert!(fx.events().iter().any(|e| e["kind"] == "dismiss"));

    // It comes back on a SessionStart...
    fx.session_start(&hooked, None, None);
    assert!(fx.row(&hooked).is_some());
    // ...and a running session cannot be dismissed at all.
    let running = sid(32);
    fx.record(&running, json!({"status": "idle"}));
    fx.session_start(&running, None, None);
    let out = fx.post("/api/dismiss", json!({"session_id": running}));
    assert_eq!(out["ok"], false, "{out}");
    assert!(fx.row(&running).is_some());

    // A dismissed session comes back when it runs again.
    let later = sid(33);
    fx.record(&later, json!({"pid": null}));
    fx.session_start(&later, None, None);
    fx.post("/api/dismiss", json!({"session_id": later}));
    assert!(fx.row(&later).is_none());
    fx.record(&later, json!({"status": "idle"})); // with a process again
    wait_until(Duration::from_secs(15), || fx.row(&later)).expect("listed again once it has a process");
    // Listing it again clears the dismissal, with no hook needed, so it
    // stays listed after its process ends (as Python's _undismissed did).
    wait_until(Duration::from_secs(10), || fx.d.state_json()["dismissed"].get(&later).is_none().then_some(()))
        .expect("the dismissal is cleared when the row comes back");
    fx.record(&later, json!({"pid": null}));
    wait_until(Duration::from_secs(15), || fx.row(&later).filter(|r| r["pid"].is_null()))
        .expect("still listed once its process has ended");
    // A hook clears it too (the other session's here).
    fx.session_start(&later, None, None);
    wait_until(Duration::from_secs(10), || {
        let s = fx.d.state_json();
        (s["dismissed"].get(&later).is_none() && s["dismissed"].get(&hooked).is_none()).then_some(())
    })
    .expect("no longer held dismissed");
}

// ------------------------------------- history, activity, errors, pages

fn history_seeds<'a>(a: &'a std::path::Path, b: &'a std::path::Path) -> Vec<Seed<'a>> {
    vec![
        Seed {
            sid: "11111111-aaaa-4aaa-8aaa-000000000001",
            cwd: a,
            steps: vec![
                Step::User("fix the build"),
                Step::Tool("toolu_1", "Bash", json!({"command": "npm test"}), true, "ECONNREFUSED talking to host"),
            ],
            custom_title: Some("Raspberry Pi tunnel"),
            age: 100,
        },
        Seed {
            sid: "11111111-aaaa-4aaa-8aaa-000000000002",
            cwd: b,
            steps: vec![
                Step::User("session nine"),
                Step::Text("a leaky pipe here"),
                Step::Tool("toolu_2", "Read", json!({"file_path": "f.py"}), false, "ok"),
            ],
            custom_title: None,
            age: 50,
        },
    ]
}

fn history_fx(label: &str) -> (Fx, TempDir) {
    // The project folders live under the config folder, which Fx makes: build
    // the seeds against a folder of their own that Fx's names mirror.
    let base = TempDir::new(&format!("feathist-{label}"));
    let (a, b) = (base.join("proj-a"), base.join("proj-b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let fx = Fx::start(label, vec![], vec![], &history_seeds(&a, &b));
    (fx, base) // the transcripts name these folders: keep them while the test runs
}

#[test]
fn features_history_lists_newest_first_and_searches_title_then_text() {
    let (fx, _base) = history_fx("history");
    let (code, all) = fx.d.get("/api/history");
    assert_eq!(code, 200);
    let ids: Vec<&str> = all["results"].as_array().unwrap().iter().map(|r| r["session_id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["11111111-aaaa-4aaa-8aaa-000000000002", "11111111-aaaa-4aaa-8aaa-000000000001"]);
    assert_eq!(all["complete"], true);
    assert_eq!(all["results"][0]["where"], Value::Null);
    let (_, hit) = fx.d.get("/api/history?q=tunnel");
    assert_eq!(hit["results"].as_array().unwrap().len(), 1);
    assert_eq!(hit["results"][0]["where"], "title");
    let (_, hit) = fx.d.get("/api/history?q=leaky");
    assert_eq!(hit["results"].as_array().unwrap().len(), 1);
    assert_eq!(hit["results"][0]["where"], "text");
    assert!(hit["results"][0]["snippet"].as_str().unwrap().contains("leaky"));
}

#[test]
fn features_activity_falls_back_to_the_conversation_and_errors_aggregate() {
    let (fx, _base) = history_fx("activity");
    let one = "11111111-aaaa-4aaa-8aaa-000000000001";
    let (code, got) = fx.d.get(&format!("/api/activity?session={one}"));
    assert_eq!(code, 200);
    assert_eq!(got["source"], "conversation");
    assert_eq!(got["tally"]["errors"], 1);
    assert_eq!(got["tally"]["by_category"], json!({"network": 1}));
    assert_eq!(got["timeline"][0]["tool"], "Bash");

    let (code, errs) = fx.d.get("/api/errors?days=30");
    assert_eq!(code, 200);
    assert_eq!(errs["ok"], true);
    assert_eq!(errs["sessions"], 2);
    assert_eq!(errs["errors"], 1);
    assert_eq!(errs["by_project"]["proj-a"]["errors"], 1);
    assert_eq!(errs["by_project"]["proj-b"]["errors"], 0);
    // A second ask is the cached answer, to the last field.
    let (_, again) = fx.d.get("/api/errors?days=30");
    assert_eq!(again, errs);
    // And a fresh dispatcher has no calls but still answers.
    let fresh = Fx::start("errors-empty", vec![], vec![], &[]);
    let (code, body) = fresh.d.get("/api/errors");
    assert_eq!(code, 200);
    assert!(body.get("calls").is_some());
}

fn raw_status(port: u16, path: &str) -> u16 {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").unwrap();
    let mut text = String::new();
    let _ = s.read_to_string(&mut text);
    text.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0)
}

#[test]
fn features_pages_fonts_and_matrix_are_served() {
    let a = Fx::start("pages-a", vec![], vec![], &[]);
    let b = Fx::start("pages-b", vec![], vec![], &[]);
    // The pages and /api/health name the same dispatcher.
    let boot = a.d.get("/api/health").1["boot"].as_str().unwrap().to_string();
    assert_ne!(boot, b.d.get("/api/health").1["boot"].as_str().unwrap());
    for path in ["/", "/matrix", "/settings"] {
        let page = ureq::get(&a.d.url(path)).call().unwrap().into_string().unwrap();
        assert!(page.contains(&format!("<meta name=imp-boot content=\"{boot}\">")), "{path}");
    }
    // The chat matrix page and its data.
    let resp = ureq::get(&a.d.url("/matrix")).call().unwrap();
    assert!(resp.content_type().contains("text/html"));
    assert!(resp.into_string().unwrap().contains("<title>Chat Matrix</title>"));
    let (code, body) = a.d.get("/api/matrix");
    assert_eq!(code, 200);
    assert_eq!(body["tiles"], json!([]));
    assert!(body.get("spend").is_some());
    // Bundled fonts: type and caching.
    let resp = ureq::get(&a.d.url("/fonts/next-latin.woff2")).call().unwrap();
    assert_eq!(resp.header("Content-Type"), Some("font/woff2"));
    assert!(resp.header("Cache-Control").unwrap().contains("max-age"));
    let mut head = [0u8; 4];
    std::io::Read::read_exact(&mut resp.into_reader(), &mut head).unwrap();
    assert_eq!(&head, b"wOF2");
    // Only fonts in the folder.
    for name in ["nope.woff2", "..%2fui.py", "../ui.py", "OFL-AtkinsonHyperlegibleNext.txt"] {
        assert_eq!(raw_status(a.d.port, &format!("/fonts/{name}")), 404, "{name}");
    }
}

// ------------------------------------------------ parked-request waits

#[test]
fn features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows() {
    // Holds run at 0.02 of their length: 20 s is 0.4 s, 300 s is 6 s.
    let fx = Fx::start("park", vec![], vec![("IMPERATORIUM_TEST_TIME_SCALE", "0.02")], &[]);
    fx.record("bg", json!({"kind": "background", "status": "busy"}));
    fx.record("i", json!({"kind": "interactive", "status": "idle"}));
    fx.session_start("bg", None, None);
    fx.session_start("i", None, None);
    assert_eq!(fx.row("bg").unwrap()["kind"], "background");

    let ask = |session: &str, tool: &str, extra: Value| -> std::thread::JoinHandle<f64> {
        let url = fx.d.url("/permission/park");
        let mut body = json!({"session_id": session, "tool_name": tool,
                              "tool_input": {"command": "ls"}});
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        std::thread::spawn(move || {
            let started = Instant::now();
            let r = ureq::post(&url)
                .set("Content-Type", "application/json")
                .timeout(Duration::from_secs(60))
                .send_string(&body.to_string())
                .unwrap();
            assert_eq!(r.status(), 200);
            started.elapsed().as_secs_f64()
        })
    };
    let waits = json!({"wait": 20, "wait_background": 300});
    let qwaits = json!({"wait": 20, "wait_background": 300, "wait_question": 120});
    let jobs = [
        ("background gets the long wait", ask("bg", "Bash", waits.clone())),
        ("interactive gets the short wait", ask("i", "Bash", waits.clone())),
        ("unknown gets the short wait", ask("?", "Bash", waits.clone())),
        ("an older hook keeps its wait", ask("bg", "Bash", json!({"wait": 90}))),
        ("interactive question", ask("i", "AskUserQuestion", qwaits.clone())),
        ("background question", ask("bg", "AskUserQuestion", qwaits.clone())),
        ("other tools ignore the question wait", ask("i", "Bash", qwaits.clone())),
    ];
    let t: Vec<(&str, f64)> = jobs.into_iter().map(|(n, j)| (n, j.join().unwrap())).collect();
    let by = |name: &str| t.iter().find(|(n, _)| *n == name).unwrap().1;
    assert!(by("background gets the long wait") > 5.0, "{t:?}"); // 300 s
    for short in ["interactive gets the short wait", "unknown gets the short wait", "other tools ignore the question wait"] {
        assert!(by(short) < 2.0, "{short}: {t:?}"); // 20 s
    }
    let older = by("an older hook keeps its wait"); // 90 s
    assert!(older > 1.3 && older < 4.5, "{t:?}");
    let q = by("interactive question"); // 120 s
    assert!(q > 1.9 && q < 4.5, "{t:?}");
    assert!(by("background question") > 5.0, "{t:?}");
}
