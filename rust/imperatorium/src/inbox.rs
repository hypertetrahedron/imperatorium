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

/// The frames as built elsewhere (a remote dispatcher), written as they
/// are: `imperatorium inbox-send` on a host.
pub fn write_raw(socket: &str, payload: &[u8]) -> Result<(), String> {
    write(socket, payload)
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

/// test_core.py's InboxFrames: what `send` refuses before connecting, and
/// that the pipe is opened for writing only.
#[cfg(test)]
mod core_tests {
    use super::*;

    #[test]
    fn oversized_content_is_refused_before_connecting() {
        // The socket does not exist: were it tried, the error would say so.
        let err = send("no-such-socket", Some("tok"), &"a".repeat(MAX_CONTENT + 1)).unwrap_err();
        assert_eq!(err, format!("content is {} chars; the cap is {MAX_CONTENT}", MAX_CONTENT + 1));
        // Exactly the cap is not refused for its size (it fails later, on the socket).
        let err = send("no-such-socket", Some("tok"), &"a".repeat(MAX_CONTENT)).unwrap_err();
        assert!(!err.contains("the cap is"), "{err}");
    }

    #[test]
    fn empty_content_is_refused_whatever_the_blanks() {
        for bad in ["", " ", "\n\t "] {
            assert_eq!(
                send("x", Some("t"), bad).unwrap_err(),
                "content must be a non-empty string",
                "{bad:?}"
            );
        }
    }

    /// The default pipe DACL grants others read only; asking for read as
    /// well as write is never needed and can be what a policy refuses. A
    /// pipe whose server is write-only-for-clients (PipeDirection.In) is
    /// opened only by a client that asks for nothing but write.
    #[cfg(windows)]
    #[test]
    fn pipe_is_opened_write_only() {
        let name = format!("imp-core-inbox-{}", std::process::id());
        let out = std::env::temp_dir().join(format!("{name}.bin"));
        let _ = std::fs::remove_file(&out);
        let script = format!(
            "$s = New-Object System.IO.Pipes.NamedPipeServerStream('{name}', 'In', 1); \
             $s.WaitForConnection(); $m = New-Object System.IO.MemoryStream; $s.CopyTo($m); \
             [System.IO.File]::WriteAllBytes('{}', $m.ToArray()); $s.Dispose()",
            out.display()
        );
        let mut server = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("start powershell");
        let path = format!(r"\\.\pipe\{name}");
        // Until the pipe exists, opening it fails and nothing is consumed.
        // A cold PowerShell on a fresh CI runner can take most of a minute.
        let end = std::time::Instant::now() + std::time::Duration::from_secs(90);
        let mut last = String::new();
        let mut sent = false;
        while std::time::Instant::now() < end {
            match send(&path, Some("tok"), "caf\u{e9} \u{2014} done") {
                Ok(()) => {
                    sent = true;
                    break;
                }
                Err(e) => last = e,
            }
            // A server that has already gone will never open the pipe: say why.
            if let Ok(Some(status)) = server.try_wait() {
                let mut err = String::new();
                if let Some(mut e) = server.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut e, &mut err);
                }
                panic!("the pipe server exited ({status}) before a client connected: {err}");
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !sent {
            let _ = server.kill();
        }
        assert!(sent, "never delivered: {last}");
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !out.exists() && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let got = std::fs::read(&out).expect("the server saved what it read");
        let _ = server.wait();
        let _ = std::fs::remove_file(&out);
        assert_eq!(got, frames(Some("tok"), "caf\u{e9} \u{2014} done"));
    }

    /// An open that is refused names the usual cause, the session running
    /// elevated. A directory cannot be opened for writing: access denied.
    #[cfg(windows)]
    #[test]
    fn an_access_denied_names_the_elevated_session() {
        let temp = std::env::temp_dir();
        let dir = format!(r"\\?\{}", temp.display()).trim_end_matches('\\').to_string();
        let err = write(&dir, b"x\n").unwrap_err();
        assert!(err.contains("elevated"), "{err}");
    }
}
