//! `imperatorium install-hooks --mod`: the mod's folder in
//! CLAUDE_CODE_PLUGIN_DIRS (the ports of test_modlink.py's InstallTests that
//! the install_compare golden does not reach). A temp settings.json only.
#![cfg(windows)]

mod common;
use common::*;
use serde_json::{json, Value};
use std::process::Command;

const DIRS: &str = "CLAUDE_CODE_PLUGIN_DIRS";

struct Case {
    tmp: TempDir,
}

impl Case {
    fn new(settings: &Value) -> Case {
        let tmp = TempDir::new("modinstall");
        std::fs::create_dir_all(tmp.join("home")).unwrap();
        std::fs::write(tmp.join("imperatorium.exe"), b"").unwrap();
        std::fs::write(tmp.join("settings.json"), serde_json::to_vec(settings).unwrap()).unwrap();
        Case { tmp }
    }

    fn run(&self, args: &[&str]) -> (i32, String) {
        let mut cmd = Command::new(bin());
        cmd.arg("install-hooks")
            .arg("--settings")
            .arg(self.tmp.join("settings.json"))
            .arg("--native")
            .arg(self.tmp.join("imperatorium.exe"))
            .arg("--home")
            .arg(self.tmp.join("home"))
            .args(args);
        clean_env(&mut cmd, None);
        isolate(&mut cmd, Some(&self.tmp.path));
        let (code, out, err) = run(&mut cmd, b"");
        (code, format!("{}{}", String::from_utf8_lossy(&out), String::from_utf8_lossy(&err)))
    }

    fn settings(&self) -> Value {
        read_json(&self.tmp.join("settings.json"))
    }
}

#[test]
fn uninstall_removes_only_ours() {
    let ours = r"C:\x\mod\imperatorium";
    let case = Case::new(&json!({"env": {DIRS: ours}}));
    let (code, said) = case.run(&["--uninstall", "--mod", ours]);
    assert_eq!(code, 0, "{said}");
    // Nothing else was in env, so env goes with it.
    assert!(case.settings().get("env").is_none(), "{}", case.settings());

    let case = Case::new(&json!({"env": {DIRS: format!(r"C:\a;{ours}")}}));
    let (code, said) = case.run(&["--uninstall", "--mod", ours]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(case.settings()["env"][DIRS], r"C:\a");
}

#[test]
fn a_strange_env_is_left_alone() {
    let case = Case::new(&json!({"env": "nope"}));
    let (code, said) = case.run(&["--mod", r"C:\x\mod\imperatorium"]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(case.settings()["env"], "nope");
    assert!(said.contains("SKIP"), "the refusal is said: {said}");
}
