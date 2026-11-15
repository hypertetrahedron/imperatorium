//! The harness itself (tests/common): dispatchers on free ports side by
//! side, the stand-in PATH, the fake CLI, cleanup on drop. (twin.py's
//! goldens are replayed in tests/twin.rs.)
#![cfg(windows)]

mod common;
use common::*;
use serde_json::json;

#[test]
fn dispatchers_run_side_by_side_on_free_ports() {
    let a = std::thread::spawn(|| Dispatcher::start(Options::new()));
    let b = Dispatcher::start(Options::new());
    let a = a.join().unwrap();
    assert_ne!(a.port, b.port);
    for d in [&a, &b] {
        assert!(!(8792..=8820).contains(&d.port), "a fixed port: {}", d.port);
        let (code, health) = d.get("/api/health");
        assert_eq!(code, 200, "{health}");
    }
    // Each keeps its own state.
    let hook = json!({"hook_event_name": "SessionStart", "session_id": "aaaaaaaa-0000-4000-8000-000000000001",
                      "cwd": "C:\\work\\harness"});
    assert_eq!(a.post("/hook", &hook).0, 200);
    let has = |d: &Dispatcher| {
        wait_until(std::time::Duration::from_secs(10), || {
            d.state_json()["sessions"]
                .get("aaaaaaaa-0000-4000-8000-000000000001")
                .map(|_| ())
        })
        .is_some()
    };
    assert!(has(&a));
    assert!(b.state_json()["sessions"]
        .get("aaaaaaaa-0000-4000-8000-000000000001")
        .is_none());
    let port = a.port;
    drop(a);
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "still serving after drop"
    );
}

#[test]
fn the_stand_in_path_finds_only_the_fakes() {
    let d = Dispatcher::start(Options {
        fake_cli: vec!["claude.exe", "wt.exe"],
        ..Options::new()
    });
    let path = stand_in_path(Some(&d.bin_dir));
    let dirs: Vec<_> = std::env::split_paths(&path).collect();
    assert_eq!(dirs[0], d.bin_dir);
    for name in ["claude.exe", "wt.exe", "hc.exe"] {
        let found: Vec<_> = dirs.iter().filter(|p| p.join(name).is_file()).collect();
        assert!(
            found.len() <= 1 && found.iter().all(|p| **p == d.bin_dir),
            "{name} at {found:?}"
        );
    }
    // Through the dispatcher: `claude agents` is fakecli's (it prints []),
    // so the session list has nothing from a real claude.
    let (code, sessions) = d.get("/api/sessions");
    assert_eq!(code, 200);
    assert!(sessions["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r.get("pid").is_none()));
}
