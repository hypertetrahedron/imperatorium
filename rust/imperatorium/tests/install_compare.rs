//! settings.json as `imperatorium install-hooks` writes it, against what
//! `hooks/install.py --native` wrote from the same starting file
//! (rust/conformance/install_compare.py): install, re-install and
//! uninstall, with and without the extras, line endings aside. All in a
//! temporary folder; ~/.claude is never touched.
#![cfg(windows)]

mod common;
use common::*;
use serde_json::Value;
use std::process::Command;

#[test]
fn install_hooks_writes_what_install_py_wrote() {
    let cases = read_jsonl(&golden_dir("install_compare").join("cases.jsonl"));
    assert_eq!(cases.len(), 80);
    let tmp = TempDir::new("install");
    let home = tmp.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("policy.json"),
        r#"{"park": {"enabled": true, "wait_seconds": 40, "background_wait_seconds": 300}}"#,
    )
    .unwrap();
    // The path recorded for the binary: a stand-in file in the temp folder
    // (install.py only checks that it is a file).
    let native = tmp.join("bin").join("imperatorium.exe");
    std::fs::create_dir_all(native.parent().unwrap()).unwrap();
    std::fs::write(&native, b"").unwrap();
    let module = repo().join("mod").join("imperatorium");
    let places = Places::new(&[("tmp", &tmp.path)]);
    let path = tmp.join("binary.json");
    let mut f = Failures::default();
    for case in &cases {
        let case = places.put(case);
        let _ = std::fs::remove_file(&path);
        if let Some(text) = case["start_text"].as_str() {
            std::fs::write(&path, text).unwrap();
        }
        let variant: Vec<&str> = case["variant"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let times = if case["twice"].as_bool().unwrap() {
            2
        } else {
            1
        };
        for _ in 0..times {
            let mut cmd = Command::new(bin());
            cmd.arg("install-hooks")
                .arg("--settings")
                .arg(&path)
                .arg("--native")
                .arg(&native)
                .arg("--home")
                .arg(&home);
            cmd.args(variant.iter().filter(|a| **a != "--mod"));
            if variant.contains(&"--mod") || variant.contains(&"--uninstall") {
                cmd.arg("--mod").arg(&module);
            }
            clean_env(&mut cmd, None);
            isolate(&mut cmd, Some(&tmp.path));
            let (code, _, err) = run(&mut cmd, b"");
            assert_eq!(
                code,
                0,
                "{} {variant:?}: {}",
                case["start_name"],
                String::from_utf8_lossy(&err)
            );
        }
        // Backups the install leaves beside the file are not compared.
        for e in std::fs::read_dir(&tmp.path).unwrap().flatten() {
            if e.file_name()
                .to_string_lossy()
                .starts_with("binary.json.imperatorium-")
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let got = std::fs::read(&path)
            .map(|b| String::from_utf8(b).unwrap().replace("\r\n", "\n"))
            .unwrap_or_else(|_| "(none)".into());
        let want = case["want"].as_str().unwrap();
        f.check(
            || {
                format!(
                    "{} {variant:?} {}",
                    case["start_name"],
                    if times == 2 { "twice" } else { "" }
                )
            },
            first_difference(
                &Value::String(want.into()),
                &Value::String(got.clone()),
                "$",
                true,
            )
            .map(|d| {
                let line = want.lines().zip(got.lines()).position(|(a, b)| a != b);
                format!("{d} (first differing line {line:?})")
            }),
        );
    }
    f.finish("install_compare");
}
