//! /api/open_path: a click on a path in the chat opens the file with its
//! program, or shows it in its folder. Nothing is opened for real: the
//! dispatcher writes what it would open to IMPERATORIUM_TEST_OPEN_LOG.
#![cfg(windows)]
mod common;
use common::dispatch::*;
use serde_json::{json, Value};

struct Opened {
    rig: Rig,
    log: std::path::PathBuf,
}

impl Drop for Opened {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.log);
    }
}

fn rig() -> Opened {
    let log = std::env::temp_dir().join(format!(
        "imp-open-{}-{}.log",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let l = log.to_string_lossy().into_owned();
    let rig = Rig::with(json!({}), |o| o.env.push(("IMPERATORIUM_TEST_OPEN_LOG".into(), l)));
    rig.live(SID, "idle", json!({}));
    std::fs::write(std::path::Path::new(&rig.cwd).join("notes.md"), "# notes\n").unwrap();
    std::fs::write(std::path::Path::new(&rig.cwd).join("tool.exe"), "MZ").unwrap();
    Opened { rig, log }
}

impl Opened {
    fn open(&self, path: &str, reveal: bool) -> Value {
        let token = self.rig.window_token();
        let (code, out) = self.rig.post_window(
            "/api/open_path",
            &json!({"session_id": SID, "path": path, "reveal": reveal}),
            &token,
        );
        assert_eq!(code, 200, "{out}");
        out
    }
    fn opened(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

#[test]
fn only_the_window_opens_files() {
    let o = rig();
    let (code, _) = o.rig.post("/api/open_path", &json!({"session_id": SID, "path": "notes.md"}));
    assert_eq!(code, 403);
    assert!(o.opened().is_empty());
}

#[test]
fn a_relative_path_is_the_sessions_and_its_line_is_dropped() {
    let o = rig();
    let out = o.open("notes.md:3", false);
    assert_eq!(out["ok"], json!(true), "{out}");
    assert_eq!(out["how"], json!("open"));
    let want = std::path::Path::new(&o.rig.cwd).join("notes.md");
    assert_eq!(o.opened(), [format!("open {}", want.display())]);
    let out = o.open("notes.md", true);
    assert_eq!(out["how"], json!("reveal"), "{out}");
    assert_eq!(o.opened().len(), 2);
}

#[test]
fn a_program_is_shown_in_its_folder_never_run() {
    let o = rig();
    let out = o.open("tool.exe", false);
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(out["error"].as_str().unwrap().contains("is a program"), "{out}");
    assert!(o.opened().is_empty());
    let out = o.open("tool.exe", true);
    assert_eq!(out["how"], json!("reveal"), "{out}");
}

#[test]
fn a_missing_file_is_said() {
    let o = rig();
    let out = o.open("gone.txt", false);
    assert_eq!(out["ok"], json!(false), "{out}");
    assert!(out["error"].as_str().unwrap().starts_with("not found"), "{out}");
    assert!(o.opened().is_empty());
}
