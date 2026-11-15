//! Ports of test_quick.py's Live and ComboOf: the page is given the
//! configured quick phrases, the Settings window the shortcut list, and the
//! page's own `comboOf` writes a key press the way the setting stores it.
//! (Saving a change and seeing it at once is settings_twin.rs, python-out/twins-a.)
#![cfg(windows)]
mod common;
use common::*;
use serde_json::{json, Value};
use std::process::Command;

#[test]
fn the_page_gets_the_configured_phrases() {
    let d = Dispatcher::start(Options {
        config: Some(json!({"quick": {"key": "alt+q", "phrases": ["one"]}})),
        ..Options::new()
    });
    let (code, got) = d.get("/api/quick");
    assert_eq!(code, 200);
    assert_eq!((got["key"].clone(), got["phrases"].clone()), (json!("alt+q"), json!(["one"])), "{got}");
    // and with no config, the defaults
    let d = Dispatcher::start(Options::new());
    let (_, got) = d.get("/api/quick");
    assert_eq!(got["key"], "ctrl+space");
    assert!(got["phrases"].as_array().unwrap().contains(&json!("implement as recommended")));
}

#[test]
fn the_settings_window_gets_the_shortcut_list() {
    let d = Dispatcher::start(Options::new());
    let (code, view) = d.get("/api/settings");
    assert_eq!(code, 200);
    let shortcuts = view["shortcuts"].as_array().expect("a shortcut list");
    assert!(shortcuts.len() >= 15, "{}", shortcuts.len());
    for s in shortcuts {
        for k in ["where", "keys", "what"] {
            assert!(s[k].is_string(), "{s}");
        }
    }
    let keys: Vec<&str> = shortcuts.iter().filter_map(|s| s["keys"].as_str()).collect();
    assert!(keys.iter().any(|k| k.eq_ignore_ascii_case("ctrl+k")), "{keys:?}");
    let sections: Vec<&str> = view["sections"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s[0].as_str().or_else(|| s["id"].as_str()))
        .collect();
    assert!(sections.contains(&"quick"), "{sections:?}");
}

fn node() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("node.exe"))
        .find(|p| p.is_file())
}

#[test]
fn combo_of_matches_what_the_setting_stores() {
    let Some(node) = node() else {
        eprintln!("node is not installed: skipped");
        return;
    };
    let shared = std::fs::read_to_string(repo().join("imperatorium").join("web").join("shared.js")).unwrap();
    let start = shared.find("function comboOf(").expect("comboOf in shared.js");
    let end = shared.find("// \"ctrl+space\" as people").expect("the comment after comboOf");
    let events = json!([
        {"code": "Space", "ctrlKey": true},
        {"code": "KeyQ", "ctrlKey": true, "shiftKey": true},
        {"code": "Digit1", "ctrlKey": true},
        {"code": "Period", "metaKey": true},
        {"code": "ControlLeft", "ctrlKey": true},
    ]);
    let js = format!(
        "{}process.stdout.write(JSON.stringify({events}.map(comboOf)));",
        &shared[start..end]
    );
    let mut cmd = Command::new(node);
    cmd.arg("-");
    let (code, out, err) = run(&mut cmd, js.as_bytes());
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    let got: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(got, json!(["ctrl+space", "ctrl+shift+q", "ctrl+1", "win+period", ""]));
    // each is what the setting stores, as the binary checks it
    let combos: Vec<Value> = got.as_array().unwrap()[..4].to_vec();
    let cases: Vec<Value> = combos
        .iter()
        .map(|c| json!({"key": "quick.key", "value": c, "current": null}))
        .collect();
    let answers = check_lines(&["settings-check"], &repo(), &cases);
    for (c, a) in combos.iter().zip(&answers) {
        assert_eq!(a, &json!({"ok": c}), "{c}");
    }
}
