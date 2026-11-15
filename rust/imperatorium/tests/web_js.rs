//! The window's JavaScript (imperatorium/web/*.js) has its tests in
//! imperatorium/web/tests/*.test.mjs, run by `node --test`. This runs them
//! under `cargo test`, so one command covers the window too.
//!
//! No node on PATH: the test says so and passes (set
//! IMPERATORIUM_REQUIRE_NODE=1 to make that a failure, as CI does).

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("checkout")
        .to_path_buf()
}

#[test]
fn window_javascript_tests_pass_under_node() {
    let dir = repo().join("imperatorium").join("web").join("tests");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".test.mjs"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no *.test.mjs in {}", dir.display());

    let out = match Command::new("node")
        .arg("--test")
        .args(&files)
        .current_dir(repo())
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            if std::env::var_os("IMPERATORIUM_REQUIRE_NODE").is_some() {
                panic!("node is required (IMPERATORIUM_REQUIRE_NODE) but not runnable: {e}");
            }
            eprintln!("skipped: node is not on PATH ({e}); the window's JavaScript tests did not run");
            return;
        }
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "node --test failed ({}):\n{text}",
        out.status
    );
    eprintln!(
        "{}",
        text.lines().filter(|l| l.starts_with("ℹ")).collect::<Vec<_>>().join("\n")
    );
}
