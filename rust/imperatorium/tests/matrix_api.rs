//! The chat matrix and the spend it shows (the ports of test_matrix.py's
//! Active, Spend and Method classes): /api/matrix and /api/usage on the
//! harness's own dispatcher, with made-up sessions and transcripts under its
//! CLAUDE_CONFIG_DIR. The account's plan is the temp home's own
//! `.claude.json`, never this machine's.
#![cfg(windows)]

mod common;
use common::dispatch::{merge, Rig, OTHER, SID};
use common::*;
use serde_json::{json, Value};

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// An ISO time as a transcript writes it.
fn iso(epoch: f64) -> String {
    let secs = epoch as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil_from_days
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// One assistant message with its usage, on claude-sonnet-4-5 (output at
/// $15 per million tokens).
fn spent(id: &str, input: u64, output: u64, at: f64) -> Value {
    json!({"type": "assistant", "cwd": "C:\\work", "timestamp": iso(at),
           "message": {"id": id, "model": "claude-sonnet-4-5", "content": [],
                       "usage": {"input_tokens": input, "output_tokens": output}}})
}

/// A rig on a temp home whose login is `plan` (an `organizationType`), or
/// has no plan at all.
fn rig_on(plan: Option<&str>, config: Value) -> (Rig, TempDir) {
    let home = TempDir::new("matrix-home");
    let account = match plan {
        Some(p) => json!({"oauthAccount": {"organizationType": p}}),
        None => json!({}),
    };
    std::fs::write(home.join(".claude.json"), account.to_string()).unwrap();
    let path = home.path.to_string_lossy().into_owned();
    let rig = Rig::with(config, |o| {
        o.env.push(("USERPROFILE".into(), path.clone()));
        o.env.push(("HOME".into(), path));
    });
    (rig, home)
}

fn statusline(rig: &Rig, sid: &str, five_hour: Value) {
    let (code, _) = rig.d.post(
        "/hook",
        &json!({"hook_event_name": "imperatorium_statusline", "session_id": sid,
                "rate_limits": {"five_hour": five_hour}}),
    );
    assert_eq!(code, 200);
}

fn window<'a>(usage: &'a Value, name: &str) -> &'a Value {
    usage["spend"]["global"]["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == name)
        .unwrap_or_else(|| panic!("no {name} window in {usage}"))
}

fn approx(a: &Value, b: f64, slack: f64) {
    let a = a.as_f64().unwrap_or_else(|| panic!("{a} is not a number"));
    assert!((a - b).abs() <= slack, "{a} is not within {slack} of {b}");
}

/// Before the five-hour window, inside the week; inside both; weeks old.
fn history(rig: &Rig, t: f64) {
    rig.transcript(
        SID,
        &[spent("old", 0, 1000, t - 4.5 * 3600.0), spent("new", 0, 2000, t - 3600.0), spent("ancient", 0, 4000, t - 20.0 * 86400.0)],
    );
}

#[test]
fn a_subscription_counts_its_windows_and_open_sessions_their_whole_life() {
    let (rig, _home) = rig_on(Some("claude_max"), json!({}));
    let t = now();
    history(&rig, t);
    rig.live(SID, "busy", json!({}));
    // The five-hour window resets in an hour, so it began four hours ago.
    statusline(&rig, SID, json!({"used_percentage": 30, "resets_at": t + 3600.0}));
    let (_, usage) = rig.d.get("/api/usage");
    let (five, week) = (window(&usage, "five_hour"), window(&usage, "seven_day"));
    assert_eq!(five["tokens"]["output"], 2000, "{usage}");
    approx(&five["since"], t - 4.0 * 3600.0, 5.0);
    approx(&five["resets_at"], t + 3600.0, 1.0);
    // No seven-day reading: the last seven days, its reset unknown.
    assert_eq!(week["tokens"]["output"], 3000, "{usage}");
    assert!(week["resets_at"].is_null());
    let cur = &usage["spend"]["current"];
    assert_eq!(cur["sessions"], 1);
    assert_eq!(cur["tokens"]["output"], 7000);
    approx(&cur["cost_usd"], 7000.0 * 15.0 / 1e6, 1e-9);
    assert_eq!(cur["each"][0]["sessionId"], SID);
    assert_eq!(usage["spend"]["complete"], true);
    // Split by model for the model mix, in the windows and the open sessions.
    assert_eq!(five["models"]["claude-sonnet-4-5"]["output"], 2000);
    assert_eq!(cur["models"]["claude-sonnet-4-5"]["output"], 7000);
    let (_, matrix) = rig.d.get("/api/matrix");
    assert_eq!(matrix["spend"]["models"]["claude-sonnet-4-5"]["output"], 7000, "{matrix}");
}

#[test]
fn a_window_waiting_after_its_reset_counts_from_the_reset() {
    let (rig, _home) = rig_on(Some("claude_max"), json!({}));
    let t = now();
    history(&rig, t);
    rig.live(SID, "idle", json!({}));
    statusline(&rig, SID, json!({"used_percentage": 50, "resets_at": t - 2.0 * 3600.0}));
    let (_, usage) = rig.d.get("/api/usage");
    let five = window(&usage, "five_hour");
    approx(&five["since"], t - 2.0 * 3600.0, 1.0);
    assert!(five["resets_at"].is_null());
    assert_eq!(five["tokens"]["output"], 2000, "{usage}"); // "new", an hour ago
}

#[test]
fn a_closed_session_counts_globally_but_not_as_current() {
    let (rig, _home) = rig_on(Some("claude_max"), json!({}));
    history(&rig, now());
    let (_, usage) = rig.d.get("/api/usage");
    assert_eq!(window(&usage, "seven_day")["tokens"]["output"], 3000, "{usage}");
    assert_eq!(usage["spend"]["current"]["sessions"], 0);
    assert_eq!(usage["spend"]["current"]["total_tokens"], 0);
}

#[test]
fn an_account_without_windows_counts_the_month() {
    let (rig, _home) = rig_on(None, json!({}));
    let t = now();
    history(&rig, t);
    let (_, usage) = rig.d.get("/api/usage");
    let names: Vec<&str> = usage["spend"]["global"]["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["month"], "{usage}");
    // The first of this month, local midnight: at most 31 days back, on a
    // quarter-hour boundary (every UTC offset is one).
    let since = window(&usage, "month")["since"].as_f64().unwrap();
    assert!(since <= t && t - since < 32.0 * 86400.0, "{since}");
    assert_eq!(since % 900.0, 0.0);
}

#[test]
fn usage_carries_the_spend() {
    let (rig, _home) = rig_on(Some("claude_max"), json!({}));
    let (_, usage) = rig.d.get("/api/usage");
    assert!(usage["spend"]["global"]["windows"].is_array(), "{usage}");
    assert!(usage["spend"].get("current").is_some());
}

#[test]
fn a_running_session_becomes_a_tile() {
    let (rig, _home) = rig_on(Some("claude_max"), json!({}));
    rig.live(SID, "busy", json!({}));
    let stamp = |s: u64| iso(now() - 60.0 + s as f64);
    let mut assistant = spent("m1", 3, 4, now());
    assistant["uuid"] = json!("a1");
    assistant["parentUuid"] = json!("u1");
    assistant["message"]["role"] = json!("assistant");
    assistant["message"]["content"] = json!([
        {"type": "text", "text": "on it"},
        {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "make"}}]);
    rig.transcript(SID, &[
        json!({"type": "user", "uuid": "u1", "parentUuid": null, "sessionId": SID, "timestamp": stamp(0),
               "message": {"role": "user", "content": "go"}}),
        assistant,
    ]);
    let (_, got) = rig.d.get("/api/matrix");
    let tiles = got["tiles"].as_array().unwrap();
    assert_eq!(tiles.len(), 1, "{got}");
    let tile = &tiles[0];
    assert_eq!(tile["sessionId"], SID);
    let replies: Vec<&str> = tile["replies"].as_array().unwrap().iter().map(|r| r["text"].as_str().unwrap()).collect();
    assert_eq!(replies, ["on it"]);
    assert_eq!(tile["current"]["name"], "Bash");
    assert_eq!(got["spend"]["total_tokens"], 7);
    assert_eq!(tile["tokens"]["output"], 4);
    assert!(tile["cost_usd"].as_f64().unwrap() > 0.0);
    assert!(got["spend"]["global"]["windows"].is_array());
    // The background-work counts ride along for the "waiting on" line.
    assert!(tile["work"].get("alive").is_some(), "{tile}");
}

#[test]
fn an_unreadable_transcript_still_gets_its_tile() {
    let (rig, _home) = rig_on(Some("claude_max"), json!({}));
    rig.live(SID, "idle", json!({}));
    // A transcript that cannot be read: held open by someone who shares it
    // with nobody.
    let path = rig.transcript(SID, &[json!({"type": "user", "message": {"role": "user", "content": "x"}})]);
    use std::os::windows::fs::OpenOptionsExt;
    let _held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
    let (_, got) = rig.d.get("/api/matrix");
    let tile = &got["tiles"][0];
    assert_eq!(tile["replies"], json!([]), "{got}");
    // The binary's reader treats a file it cannot open as no transcript,
    // so there is no `error` to see here (Python's raised and said "gone");
    // the error text is only set when the reader itself returns Err.
    assert_eq!(tile["sessionId"], SID);
}

#[test]
fn no_sessions_no_tiles() {
    let (rig, _home) = rig_on(Some("claude_max"), json!({}));
    let (_, got) = rig.d.get("/api/matrix");
    assert_eq!(got["tiles"], json!([]));
    assert_eq!(got["spend"]["sessions"], 0);
}

#[test]
fn the_status_tile_is_always_shown_unless_turned_off() {
    let (rig, _home) = rig_on(Some("claude_max"), json!({}));
    assert_eq!(rig.d.get("/api/matrix").1["status_always"], true);
    let (off, _home) = rig_on(Some("claude_max"), json!({"chat": {"matrix_status": false}}));
    assert_eq!(off.d.get("/api/matrix").1["status_always"], false);
}

#[test]
fn running_sessions_get_a_tile_and_ended_ones_do_not() {
    let rig = Rig::with_agents();
    let row = |sid: &str, extra: Value| {
        let mut r = json!({"sessionId": sid, "cwd": rig.cwd, "kind": "interactive", "pid": std::process::id()});
        merge(&mut r, &extra);
        r
    };
    rig.agents(json!([
        row(SID, json!({"status": "idle", "name": "running"})),
        row(OTHER, json!({"status": "idle", "state": "done", "name": "ended"})),
        row("33333333-3333-3333-3333-333333333333", json!({"name": "no-process"})),
    ]));
    let (_, got) = rig.d.get("/api/matrix");
    let ids: Vec<&str> = got["tiles"].as_array().unwrap().iter().map(|t| t["sessionId"].as_str().unwrap()).collect();
    assert_eq!(ids, [SID], "{got}");
}
