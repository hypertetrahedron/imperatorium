//! Post a message into a running Claude Code session's inbox, as
//! `imperatorium/inbox.py` does: a named pipe on Windows, a Unix domain
//! socket elsewhere, the auth line first, the whole payload built before
//! connecting (Claude Code drops a connection idle for 30 seconds).

use serde_json::{json, Value};

use crate::pyish::{json_dumps, strip};

/// Stay well under Claude Code's ~1,000,000 character message cap.
pub const MAX_CONTENT: usize = 900_000;

/// inbox._frames: the exact bytes written, auth first.
pub fn frames(token: Option<&str>, content: &str) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        out.extend(json_dumps(&json!({"type": "auth", "token": t}), false).into_bytes());
        out.push(b'\n');
    }
    let msg = json!({"type": "user", "message": {"role": "user", "content": content}});
    out.extend(json_dumps(&msg, false).into_bytes());
    out.push(b'\n');
    out
}

/// inbox.send: Ok, or why the message could not be handed over. Delivery to
/// the inbox is not delivery to Claude; the session's own controls decide.
pub fn send(socket: &str, token: Option<&str>, content: &str) -> Result<(), String> {
    if strip(content).is_empty() {
        return Err("content must be a non-empty string".into());
    }
    let n = content.chars().count();
    if n > MAX_CONTENT {
        return Err(format!("content is {n} chars; the cap is {MAX_CONTENT}"));
    }
    if cfg!(windows) && token.is_none_or(str::is_empty) {
        return Err("a token is required on native Windows".into());
    }
    let payload = frames(token, content);
    write(socket, &payload)
}

#[cfg(windows)]
fn write(path: &str, payload: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if !path.starts_with("\\\\") {
        return Err(format!(
            "not a named-pipe path: {}",
            crate::pyish::py_repr(&Value::from(path))
        ));
    }
    // Write only: Claude Code's pipe grants others read-only, and nothing is
    // ever read back.
    let mut f = match std::fs::OpenOptions::new().write(true).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(format!(
                "access denied opening {path}: the session's pipe does not grant this \
                 process write access. The usual cause is a Claude Code session \
                 running elevated (Run as administrator) or as another user while \
                 the dispatcher runs unelevated; start the session unelevated as \
                 the same user."
            ))
        }
        Err(e) => return Err(format!("cannot open {path}: {e}")),
    };
    f.write_all(payload)
        .map_err(|e| format!("write to {path} failed: {e}"))
}

#[cfg(unix)]
fn write(path: &str, payload: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let mut s = UnixStream::connect(path).map_err(|e| format!("send to {path} failed: {e}"))?;
    let t = Some(std::time::Duration::from_secs(10));
    let _ = s.set_write_timeout(t);
    s.write_all(payload)
        .map_err(|e| format!("send to {path} failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_match_python() {
        // python: inbox._frames("t0k", "héllo \"x\"\n")
        let want = "{\"type\": \"auth\", \"token\": \"t0k\"}\n{\"type\": \"user\", \"message\": \
                    {\"role\": \"user\", \"content\": \"h\u{e9}llo \\\"x\\\"\\n\"}}\n";
        assert_eq!(
            String::from_utf8(frames(Some("t0k"), "h\u{e9}llo \"x\"\n")).unwrap(),
            want
        );
        assert!(!String::from_utf8(frames(None, "a"))
            .unwrap()
            .contains("auth"));
    }

    #[test]
    fn refuses_what_python_refuses() {
        assert_eq!(
            send("x", Some("t"), "  ").unwrap_err(),
            "content must be a non-empty string"
        );
        if cfg!(windows) {
            assert_eq!(
                send("x", None, "hi").unwrap_err(),
                "a token is required on native Windows"
            );
            assert_eq!(
                send("x", Some("t"), "hi").unwrap_err(),
                "not a named-pipe path: 'x'"
            );
        }
    }
}
