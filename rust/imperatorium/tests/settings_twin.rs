//! Replays settings_twin (answers frozen at `last-python`, see
//! golden/README.md): every POST /api/settings the recording made, the
//! config.json and config.json.bak it left (byte for byte), and the GETs
//! that show what a save applies, each as Python answered
//! (golden/settings_twin/steps.jsonl).
#![cfg(windows)]

mod common;
#[path = "common/twins_a.rs"]
mod twins_a;
use common::*;
use serde_json::Value;
use twins_a::*;

#[test]
fn settings_saves_answer_and_write_as_python_did() {
    let ntfy = Ntfy::start();
    let (d, home) = start_notify(&ntfy, "twin-rs", None);
    let places = Places::new(&[("tmp", &d.root.path)]);
    let token = d.window_token();
    let steps = read_jsonl(&golden_dir("settings_twin").join("steps.jsonl"));
    assert_eq!(steps.len(), 31);

    // same_text(): the twin's own home, ntfy topic and fake server made neutral.
    let neutral = |v: &Value| -> Value {
        let text = v
            .to_string()
            .replace(&json_inner(&home.path.to_string_lossy()), "<twin-home>")
            .replace("twin-rs", "twin-<side>")
            .replace(&format!("127.0.0.1:{}/", ntfy.port), "127.0.0.1:8819/");
        serde_json::from_str(&text).unwrap()
    };
    let mut f = Failures::default();
    for s in &steps {
        let kind = s["step"].as_str().unwrap();
        match kind {
            "save" => {
                let body = places.put(&s["body"]);
                let (code, answer) = if s["window"] == true {
                    d.post_window("/api/settings", &body, &token)
                } else {
                    d.post("/api/settings", &body)
                };
                let got = neutral(&serde_json::json!([code, answer]));
                f.check(
                    || format!("POST /api/settings: {}", s["label"].as_str().unwrap_or("")),
                    first_difference(&places.put(&s["want"]), &got, "$", false),
                );
            }
            "file" => {
                let name = s["name"].as_str().unwrap();
                let got = std::fs::read(home.join(name)).ok().map(|b| {
                    String::from_utf8_lossy(&b)
                        .replace("twin-rs", "twin-<side>")
                        .replace(&format!("127.0.0.1:{}/", ntfy.port), "127.0.0.1:8819/")
                });
                let want = s["want"].as_str().map(|w| places.text(w));
                f.check(
                    || format!("{name} after {}", s["after"].as_str().unwrap()),
                    (want != got).then(|| {
                        format!("byte for byte:\n  want {want:?}\n  got  {got:?}")
                    }),
                );
            }
            "get" => {
                let path = s["path"].as_str().unwrap();
                let (code, mut got) = d.get(path);
                assert_eq!(code, 200, "GET {path}");
                let want = if let Some(key) = s.get("key").and_then(Value::as_str) {
                    got = got[key].clone();
                    s["want"].clone()
                } else {
                    got = neutral(&got);
                    places.put(&s["want"])
                };
                f.check(
                    || format!("GET {path} after {}", s["after"].as_str().unwrap()),
                    first_difference(&want, &got, "$", false),
                );
            }
            other => panic!("unknown step {other}"),
        }
    }
    f.finish("settings twin");
}
