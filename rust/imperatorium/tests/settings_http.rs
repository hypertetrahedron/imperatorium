//! test_settings.py's `Saving`, `Routes` and `Button`, against the binary:
//! what saving does to config.json and to the running dispatcher, who may
//! save, and the gear that opens the window. (`Checks` is the unit tests in
//! src/settings.rs `py_ported`.)
#![cfg(windows)]

mod common;
use common::*;
use serde_json::{json, Value};
use std::time::Duration;

fn config() -> Value {
    json!({
        "_comment": "This machine's settings.",
        "roots": [],
        "background": {"_comment": "Options for claude --bg.", "permission_mode": "acceptEdits"},
        "notify": {"ntfy": {"enabled": false, "topic": "", "delay": 60, "token": "s3cret"},
                   "speak": {"enabled": false, "delay": 5}},
        "budget": {"thresholds": [80, 95]},
        "remote": {"enabled": true, "hosts": [{"name": "devbox", "claude": "x"},
                                              {"name": "pi.local", "label": "pi", "tunnel": true}]},
    })
}

/// A dispatcher whose config.json is CONFIG, with remote left alone: the
/// config's "remote" block is the form's data here, never started (no host
/// is contacted: the first of the two tests that need hosts is below, with
/// its own config and no ssh).
fn start() -> Dispatcher {
    let mut c = config();
    c.as_object_mut().unwrap().remove("remote");
    Dispatcher::start(Options { config: Some(c), ..Options::new() })
}

fn on_disk(d: &Dispatcher) -> Value {
    read_json(&d.home.join("config.json"))
}

fn save(d: &Dispatcher, changes: Value) -> Value {
    let (code, got) = d.post("/api/settings", &json!({"changes": changes}));
    assert_eq!(code, 200, "{got}");
    got
}

fn field(d: &Dispatcher, key: &str) -> Value {
    let (_, view) = d.get("/api/settings");
    view["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == key)
        .unwrap_or_else(|| panic!("no field {key}"))
        .clone()
}

fn home_page(d: &Dispatcher) -> String {
    let (code, page) = d.get("/");
    assert_eq!(code, 200);
    page.as_str().expect("the page is HTML").to_string()
}

#[test]
fn a_save_writes_the_file_and_keeps_the_old_one() {
    let d = start();
    let before = on_disk(&d);
    let got = save(&d, json!({"voice.model": "small", "notify.ntfy.delay": 30}));
    assert_eq!(got["ok"], true, "{got}");
    let disk = on_disk(&d);
    assert_eq!(disk["voice"]["model"], "small");
    assert_eq!(disk["_comment"], "This machine's settings.");
    assert_eq!(read_json(&d.home.join("config.json.bak")), before);
    assert_eq!(got["restart"], json!(["dispatcher"]));
    let (_, view) = d.get("/api/settings");
    assert_eq!(view["waiting"], json!(["dispatcher"]));
}

#[test]
fn live_settings_apply_at_once() {
    let d = start();
    let got = save(
        &d,
        json!({"budget.thresholds": [50, 90], "background.model": "opus",
               "notify.speak.enabled": true, "notify.ntfy.delay": 15,
               "chat.cold_cache_tokens": 50000, "chat.matrix_status": false}),
    );
    assert_eq!(got["ok"], true, "{got}");
    assert_eq!(got["restart"], json!([]));
    // What the form shows is what the running dispatcher now holds.
    assert_eq!(field(&d, "budget.thresholds")["value"], json!([50, 90]));
    assert_eq!(field(&d, "background.model")["value"], "opus");
    assert_eq!(field(&d, "notify.ntfy.delay")["value"], 15);
    assert_eq!(field(&d, "chat.cold_cache_tokens")["value"], 50000);
    assert_eq!(field(&d, "chat.matrix_status")["value"], false);
    assert_eq!(field(&d, "notify.speak.enabled")["value"], true);
    // Kept where /api/notify keeps it, which wins at start.
    let kept = wait_until(Duration::from_secs(10), || {
        (d.state_json()["prefs"]["notify"]["speak"] == json!(true)).then_some(())
    });
    assert!(kept.is_some(), "state: {}", d.state_json());
    let (_, view) = d.get("/api/settings");
    assert_eq!(view["waiting"], json!([]));
}

#[test]
fn text_sizes_apply_at_once_and_reach_the_pages() {
    let d = start();
    assert!(home_page(&d).contains("<style id=lookvars>html:root{--chatz:1.0;--matrixz:1.0}</style>"));
    let got = save(&d, json!({"chat.font_size": 21, "chat.matrix_font_size": 26}));
    assert!(got["errors"].get("chat.matrix_font_size").is_some(), "{got}"); // above the 24 maximum
    let got = save(&d, json!({"chat.font_size": 21, "chat.matrix_font_size": 13}));
    assert_eq!(got["restart"], json!([]));
    assert!(
        home_page(&d).contains("<style id=lookvars>html:root{--chatz:1.5;--matrixz:1.0}</style>"),
        "{}",
        home_page(&d).chars().take(600).collect::<String>()
    );
}

#[test]
fn highlighting_applies_at_once() {
    let d = start();
    let page = home_page(&d);
    assert!(page.contains("\"hl-mute\"") && !page.contains("--hl-keyword"));
    let got = save(&d, json!({"highlight.mute": false, "highlight.colors.keyword": "#AA00FF"}));
    assert_eq!(got["restart"], json!([]));
    let page = home_page(&d);
    assert!(!page.contains("\"hl-mute\""), "mute is off");
    assert!(page.contains("--hl-keyword:#aa00ff"));
    assert_eq!(on_disk(&d)["highlight"]["colors"]["keyword"], "#aa00ff");
    let got = save(&d, json!({"highlight.colors.string": "red"}));
    assert!(got["errors"].get("highlight.colors.string").is_some(), "{got}");
    // Blank goes back to the theme's: the key leaves config.json.
    save(&d, json!({"highlight.colors.keyword": ""}));
    assert!(!home_page(&d).contains("--hl-keyword"));
    assert!(on_disk(&d)["highlight"].get("colors").map_or(true, |c| c.get("keyword").is_none()));
}

#[test]
fn a_toggle_made_through_api_notify_shows_as_it_is() {
    let d = start();
    let (_, got) = d.post("/api/notify", &json!({"channel": "speak", "enabled": true}));
    assert_eq!(got["ok"], true, "{got}");
    assert_eq!(field(&d, "notify.speak.enabled")["value"], true);
    // Saving something else does not switch it back off.
    save(&d, json!({"notify.speak.delay": 9}));
    assert_eq!(field(&d, "notify.speak.enabled")["value"], true);
}

#[test]
fn the_form_carries_the_last_notification_error() {
    let ntfy = FakeServer::start();
    ntfy.set_answer("/imptest", 403, b"{}");
    let mut c = config();
    c.as_object_mut().unwrap().remove("remote");
    c["notify"]["ntfy"] = json!({"enabled": true, "server": ntfy.url(""), "topic": "imptest", "delay": 0});
    let d = Dispatcher::start(Options { config: Some(c), ..Options::new() });
    let (_, view) = d.get("/api/settings");
    assert!(view["notify_error"].is_null());
    // A session asking for permission makes a push, which the server refuses.
    let hook = json!({"hook_event_name": "Notification", "session_id": "a1a1a1a1-1111-4111-8111-111111111111",
                      "cwd": d.root.join("work").to_string_lossy(), "notification_type": "permission_prompt",
                      "message": "Claude needs your permission to use Bash"});
    assert_eq!(d.post("/hook", &hook).0, 200);
    let seen = wait_until(Duration::from_secs(20), || {
        let (_, v) = d.get("/api/settings");
        v["notify_error"].as_str().map(str::to_string)
    })
    .expect("the refused push never showed as an error");
    assert!(seen.starts_with("ntfy:") && seen.contains("403"), "{seen}");
}

#[test]
fn errors_come_back_by_field_and_the_file_is_untouched() {
    let d = start();
    let before = on_disk(&d);
    let got = save(&d, json!({"notify.ntfy.delay": -5}));
    assert_eq!(got["ok"], false);
    assert!(got["errors"].get("notify.ntfy.delay").is_some(), "{got}");
    assert_eq!(on_disk(&d), before);
    assert!(!d.home.join("config.json.bak").exists());
}

#[test]
fn a_broken_file_is_not_overwritten() {
    let d = start();
    std::fs::write(d.home.join("config.json"), "{ not json").unwrap();
    assert_eq!(save(&d, json!({"notify.ntfy.delay": 5}))["ok"], false);
    assert_eq!(std::fs::read_to_string(d.home.join("config.json")).unwrap(), "{ not json");
}

/// Python: `control.config_path = None` made the window refuse. A dispatcher
/// here always has a home to read config.json from; with no config.json in
/// it the form reads {} and offers the defaults, and a save creates the
/// file. What is checked is that the missing file is not an error.
#[test]
fn without_a_config_file_the_form_shows_defaults_and_a_save_creates_it() {
    let d = Dispatcher::start(Options::new());
    assert!(!d.home.join("config.json").exists());
    let (_, view) = d.get("/api/settings");
    assert_eq!(view["ok"], true, "{view}");
    assert_eq!(field(&d, "notify.ntfy.server")["value"], "https://ntfy.sh");
    let got = save(&d, json!({"notify.ntfy.delay": 5}));
    assert_eq!(got["ok"], true, "{got}");
    assert_eq!(on_disk(&d)["notify"]["ntfy"]["delay"], 5);
}

// ------------------------------------------------------------------ routes

#[test]
fn the_page_and_the_form_are_served() {
    let d = start();
    let (code, page) = d.get("/settings");
    assert_eq!(code, 200);
    assert!(page.as_str().unwrap().contains("<title>Imperatorium Settings</title>"));
    let (_, view) = d.get("/api/settings");
    assert_eq!(view["ok"], true);
}

#[test]
fn the_windows_own_page_may_save_and_a_foreign_one_may_not() {
    let d = start();
    let token = d.window_token();
    let (_, body) = d.post_window("/api/settings", &json!({"changes": {"notify.ntfy.delay": 20}}), &token);
    assert_eq!(body["ok"], true, "{body}");
    let (code, _) = d.request(
        "/api/settings",
        Some(serde_json::to_vec(&json!({"changes": {"notify.ntfy.delay": 1}})).unwrap()),
        Some("application/json"),
        &[("Origin", "https://evil.example")],
    );
    assert_eq!(code, 403);
    assert_eq!(on_disk(&d)["notify"]["ntfy"]["delay"], 20);
}

#[test]
fn a_remote_hosts_tunnel_cannot_reach_it() {
    // The tunnel's listener on this machine, on a port of our own choosing.
    let local = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    assert!(![8792, 8793, 8794].contains(&local));
    let c = json!({"notify": {"ntfy": {"delay": 60}},
                   "remote": {"enabled": true,
                              "hosts": [{"name": "imptest-host.invalid",
                                         "tunnel": {"local_port": local, "remote_port": 9}}]}});
    // The tunnel's ssh is a program that does not exist: nothing is contacted.
    let d = Dispatcher::start(Options {
        config: Some(c),
        env: vec![("IMPERATORIUM_SSH".into(), "imp-test-no-such-ssh".into())],
        ..Options::new()
    });
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build();
    let url = format!("http://127.0.0.1:{local}/api/settings");
    let body = serde_json::to_vec(&json!({"changes": {"notify.ntfy.delay": 1}})).unwrap();
    let answered = wait_until(Duration::from_secs(30), || {
        match agent.post(&url).set("Content-Type", "application/json").send_bytes(&body) {
            Ok(r) => Some(r.status()),
            Err(ureq::Error::Status(code, _)) => Some(code),
            Err(_) => None, // not listening yet
        }
    })
    .unwrap_or_else(|| panic!("the hook-only listener never came up: {}", d.log_text()));
    assert_eq!(answered, 404);
    assert_eq!(
        read_json(&d.home.join("config.json"))["notify"]["ntfy"]["delay"],
        60
    );
}

// ------------------------------------------------------------------ button

fn script(d: &Dispatcher, page: &str, name: &str) -> String {
    let key = format!("/ui/{name}?v=");
    let at = page.find(&key).unwrap_or_else(|| panic!("{name} is not linked"));
    let end = page[at..].find('"').unwrap();
    let (code, body) = d.get(&page[at..at + end]);
    assert_eq!(code, 200);
    body.as_str().unwrap().to_string()
}

#[test]
fn the_gear_sits_left_of_the_matrix_button() {
    let d = start();
    let page = home_page(&d);
    assert!(page.find("id=setbtn").unwrap() < page.find("id=matrixbtn").unwrap());
    assert!(page.contains("aria-label=\"Settings\""));
    assert!(script(&d, &page, "app.js").contains("window.open(\"/settings\", \"imperatorium-settings\""));
}

#[test]
fn the_settings_script_parses() {
    let Some(node) = ["node", "node.exe"]
        .iter()
        .find(|n| std::process::Command::new(n).arg("--version").output().is_ok())
    else {
        eprintln!("node is not installed: skipped, as test_settings.py skipped it");
        return;
    };
    let d = start();
    let (_, page) = d.get("/settings");
    let js = script(&d, page.as_str().unwrap(), "settings.js");
    let (code, _, err) = run(
        std::process::Command::new(node).args(["-e", "new Function(require('fs').readFileSync(0, 'utf8'))"]),
        js.as_bytes(),
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
}

#[test]
fn the_settings_css_has_no_control_characters() {
    let d = start();
    let (_, page) = d.get("/settings");
    let css = script(&d, page.as_str().unwrap(), "settings.css");
    // \r is allowed: a checkout with CRLF line endings carries it.
    let bad: Vec<char> = css.chars().filter(|c| (*c as u32) < 32 && !"\n\t\r".contains(*c)).collect();
    assert!(bad.is_empty(), "{bad:?}");
    assert!(css.contains("content:\" \\2022\""));
}
