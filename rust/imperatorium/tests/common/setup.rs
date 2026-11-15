//! Helpers for the setup_* and impctl_* integration tests (the ports of
//! test_setup.py and test_impctl.py): the binary's command line run against
//! a recording fake dispatcher (IMPERATORIUM_URL), in a scratch HOME and
//! with a stand-in PATH. Nothing here reaches the live dispatcher, tasks
//! or ~/.claude.
#![allow(dead_code)]

use super::*;
use std::path::Path;
use std::process::{Command, Stdio};

/// What a CLI run printed.
pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
}

/// `imperatorium ARGS` with no terminal on stdin, a clean environment and
/// everything it could write pointed into `scratch`. `url` is the
/// dispatcher it talks to (IMPERATORIUM_URL); `None` leaves it unset.
pub fn cli(args: &[&str], url: Option<&str>, scratch: &Path) -> Ran {
    let mut cmd = Command::new(bin());
    cmd.args(args).current_dir(scratch);
    clean_env(&mut cmd, None);
    isolate(&mut cmd, Some(scratch));
    cmd.env("USERPROFILE", scratch).env("HOME", scratch);
    if let Some(u) = url {
        cmd.env("IMPERATORIUM_URL", u);
    }
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .expect("run imperatorium");
    Ran {
        code: out.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&out.stdout).into_owned(),
        err: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// The base URL of a fake dispatcher.
pub fn base(server: &FakeServer) -> String {
    format!("http://127.0.0.1:{}", server.port)
}

/// A port nothing listens on.
pub fn dead_base() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    format!("http://127.0.0.1:{port}")
}

/// The live sessions impctl's tests are written against.
pub fn rows() -> Value {
    serde_json::json!({"sessions": [
        {"sessionId": "9cbf54ae-0000", "name": "bookoferrantpages-d0", "cwd": "C:/b",
         "window_title": "AI has a pickaxe problem"},
        {"sessionId": "35692dd4-0000", "name": "central-control-a3", "cwd": "Y:/cc"},
        {"sessionId": "35aa0000-0000", "name": "claude-lens-33", "cwd": "C:/l"},
    ]})
}

/// A fake dispatcher that lists `rows()`.
pub fn fake_with_sessions() -> FakeServer {
    let s = FakeServer::start();
    s.set_answer("/api/sessions", 200, rows().to_string().as_bytes());
    s
}

/// The JSON bodies POSTed to `path` so far (consumes what was seen).
pub fn posted(server: &FakeServer, path: &str) -> Vec<Value> {
    server
        .take()
        .into_iter()
        .filter(|(m, p, _, _)| m == "POST" && p == path)
        .map(|(_, _, _, body)| serde_json::from_slice(&body).unwrap())
        .collect()
}
