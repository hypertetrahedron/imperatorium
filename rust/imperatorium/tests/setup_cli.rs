//! test_setup.py's CommandLine: a bad `remote` or `service` usage is refused
//! with exit 2 before anything is touched (no ssh, no task, no config).
#![cfg(windows)]

mod common;
use common::setup::*;
use common::*;

#[test]
fn setup_bad_remote_and_service_usage_is_refused_without_touching_anything() {
    let tmp = TempDir::new("setup-usage");
    let home = tmp.join("home");
    let h = home.to_string_lossy().into_owned();
    for args in [
        &["remote", "bogus", "--home", &h][..],
        &["remote", "add", "--home", &h][..],
        &["remote", "remove", "--home", &h][..],
        &["service", "bogus", "--task-prefix", "ImpCovNever", "--home", &h][..],
    ] {
        let r = cli(args, None, &tmp.path);
        assert_eq!(r.code, 2, "{args:?}: {}{}", r.out, r.err);
        assert!(r.out.contains("usage") || r.err.contains("usage") || r.err.contains("invalid choice"), "{args:?}: {}{}", r.out, r.err);
    }
    assert!(!home.exists(), "a refused command wrote a config home");
}
