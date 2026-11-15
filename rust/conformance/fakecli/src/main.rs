//! Conformance only. Copied to `claude.exe` and `wt.exe` on a twin's PATH so
//! the dispatchers' launches and closes can be compared without starting
//! anything real. Every call is logged as one JSON line to $FAKE_LOG.
//!
//! - `wt ...`: logged, exits 0.
//! - `claude agents ...`: prints `[]` (the session list asks this).
//! - `claude --bg ...`: logged, prints a background session id.
//! - `claude --fake-console [--stubborn]`: a console session that reads what
//!   is typed into it (raw, as Claude Code does) and leaves on `/exit` -
//!   or, stubborn, never does.
//! - `ssh ...` (copied to `ssh.exe`, named by IMPERATORIUM_SSH): a fake
//!   host, never the network. FAKESSH_MODE=sh runs the remote command with
//!   `sh` in a sandbox HOME ($FAKEHOST_HOME; host `downhost` refuses the
//!   connection); otherwise it answers as a host with one live session and
//!   records each call in $FAKESSH_DIR/log.jsonl (rust/conformance/
//!   fakessh.py, which it replaces).
//! - anything else: logged, exits 0.

use std::io::Write;

const SESSION_ENV: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_PID",
    "AI_AGENT",
];

fn log(entry: serde_json::Value) {
    if let Ok(path) = std::env::var("FAKE_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
        {
            let _ = f.write_all(format!("{entry}\n").as_bytes());
        }
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let prog = std::path::Path::new(&argv[0])
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let args = &argv[1..];
    if prog == "ssh" {
        std::process::exit(ssh::main(args));
    }
    if prog == "claude" && args.first().map(String::as_str) == Some("agents") {
        println!("[]");
        return;
    }
    let present: Vec<&str> = SESSION_ENV
        .iter()
        .filter(|k| std::env::var_os(k).is_some())
        .copied()
        .collect();
    let cwd = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    if prog == "claude" && args.iter().any(|a| a == "--fake-console") {
        console(args.iter().any(|a| a == "--stubborn"));
        return;
    }
    log(serde_json::json!({"prog": prog, "args": args, "cwd": cwd, "session_env": present}));
    if prog == "claude" && args.iter().any(|a| a == "--bg") {
        println!("Background session started: 1f2e3d4c");
    }
}

#[cfg(windows)]
fn console(stubborn: bool) {
    use windows_sys::Win32::System::Console::{
        GetStdHandle, ReadConsoleW, SetConsoleMode, STD_INPUT_HANDLE,
    };
    unsafe {
        let input = GetStdHandle(STD_INPUT_HANDLE);
        // Raw, as Claude Code reads: no line editing, no echo, and Ctrl+C
        // arrives as a character rather than a signal.
        SetConsoleMode(input, 0);
        let mut line: Vec<u16> = Vec::new();
        loop {
            let mut buf = [0u16; 64];
            let mut n = 0u32;
            if ReadConsoleW(
                input,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut n,
                std::ptr::null(),
            ) == 0
            {
                return;
            }
            for &unit in &buf[..n as usize] {
                match unit {
                    0x03 => {
                        log(serde_json::json!({"prog": "console", "key": "ctrl-c"}));
                        line.clear();
                    }
                    0x0D => {
                        let text = String::from_utf16_lossy(&line);
                        log(serde_json::json!({"prog": "console", "line": text}));
                        line.clear();
                        if text == "/exit" && !stubborn {
                            return;
                        }
                    }
                    u => line.push(u),
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn console(_stubborn: bool) {}

mod ssh {
    //! The fake ssh. Options with a value (-o, -R, -L) are skipped; `-N` is
    //! a tunnel, which logs itself and then just stays (until killed).
    use serde_json::{json, Value};
    use std::io::{BufRead, IsTerminal, Read, Write};
    use std::path::{Path, PathBuf};

    pub const SID: &str = "11111111-2222-4333-8444-555555555555";
    pub const SOCKET: &str = "/tmp/claude-fake/inbox.sock";
    pub const CWD: &str = "/home/fake/projects/fakeproj";

    fn dir() -> PathBuf {
        PathBuf::from(std::env::var_os("FAKESSH_DIR").expect("FAKESSH_DIR"))
    }

    fn log(rec: Value) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(dir().join("log.jsonl"))
        {
            let _ = f.write_all(format!("{rec}
").as_bytes());
        }
    }

    /// (host, remote command, tunnel?)
    fn parse(args: &[String]) -> (String, String, bool) {
        let mut rest = Vec::new();
        let mut i = 0;
        while i < args.len() {
            if ["-o", "-R", "-L"].contains(&args[i].as_str()) {
                i += 2;
                continue;
            }
            rest.push(args[i].clone());
            i += 1;
        }
        let tunnel = rest.iter().any(|a| a == "-N");
        rest.retain(|a| a != "-N");
        let host = rest.first().cloned().unwrap_or_default();
        let cmd = rest.get(1..).map(|r| r.join(" ")).unwrap_or_default();
        (host, cmd, tunnel)
    }

    pub fn main(args: &[String]) -> i32 {
        let (host, cmd, tunnel) = parse(args);
        if std::env::var("FAKESSH_MODE").as_deref() == Ok("sh") {
            return sandbox(&host, &cmd);
        }
        if tunnel {
            log(json!({"host": host, "tunnel": true}));
            std::thread::sleep(std::time::Duration::from_secs(3600));
            return 0;
        }
        if cmd.contains("remote-channel") {
            log(json!({"host": host, "cmd": "remote-channel"}));
            let stdin = std::io::stdin();
            let mut out = std::io::stdout();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
                let answer = if req["op"] == "read" {
                    json!({"id": req["id"], "ok": true, "title": "Fake remote work",
                           "items": [{"kind": "user", "text": "hello from the host"},
                                     {"kind": "assistant", "text": "hi"}]})
                } else {
                    let mut stats = serde_json::Map::new();
                    for s in req["sessions"].as_array().into_iter().flatten() {
                        stats.insert(
                            s.as_str().unwrap_or_default().to_string(),
                            json!({"context_tokens": 4242, "model_id": "claude-sonnet-5",
                                   "agents": [], "shells": [], "monitors": [], "wakeups": [],
                                   "cache": {"expires_at": 1.0, "ttl": 300}}),
                        );
                    }
                    json!({"id": req["id"], "ok": true, "stats": stats})
                };
                let _ = writeln!(out, "{answer}");
                let _ = out.flush();
            }
            return 0;
        }
        let mut data = Vec::new();
        if !std::io::stdin().is_terminal() {
            let _ = std::io::stdin().read_to_end(&mut data);
        }
        log(json!({"host": host, "cmd": cmd, "stdin": String::from_utf8_lossy(&data)}));
        if cmd.contains("agents --json") {
            let row = json!({"sessionId": SID, "pid": 4242, "cwd": CWD, "kind": "interactive",
                             "name": "fakeproj-ab", "status": "idle"});
            let rec = json!({"pid": 4242, "sessionId": SID, "cwd": CWD, "kind": "interactive",
                             "name": "fakeproj-ab", "status": "idle",
                             "messagingSocketPath": SOCKET, "statusUpdatedAt": 1794600000000u64});
            print!("{}

--- imperatorium session files ---
{rec}
", json!([row]));
            return 0;
        }
        if cmd.starts_with("uname -m") {
            let sha = std::fs::read_to_string(dir().join("sha256")).unwrap_or_default();
            print!(
                "x86_64
{}  /home/fake/.claude/imperatorium/bin/imperatorium
",
                sha.trim()
            );
            return 0;
        }
        if cmd.contains("inbox-send") {
            return 0;
        }
        if cmd.contains("--bg") {
            println!("Started background session 5ac0ffee in fakeproj");
            return 0;
        }
        eprintln!("fakessh: unexpected command: {cmd}");
        3
    }

    /// `sh` on PATH, as a full path.
    fn find_sh() -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .flat_map(|d| [d.join("sh.exe"), d.join("sh")])
            .find(|p| p.is_file())
    }

    /// The remote command run by `sh` with HOME the sandbox and only the
    /// shell's own folder on PATH (so no `claude` of this machine's).
    fn sandbox(host: &str, cmd: &str) -> i32 {
        if host == "downhost" {
            eprintln!("ssh: connect to host downhost port 22: Connection refused");
            return 255;
        }
        let Some(sh) = find_sh() else {
            eprintln!("fakessh: no sh on PATH");
            return 127;
        };
        let home = std::env::var_os("FAKEHOST_HOME").expect("FAKEHOST_HOME");
        let bin = sh.parent().map(Path::to_path_buf).unwrap_or_default();
        match std::process::Command::new(&sh)
            .args(["-c", cmd])
            .env("HOME", home)
            .env("PATH", bin)
            .status()
        {
            Ok(s) => s.code().unwrap_or(1),
            Err(e) => {
                eprintln!("fakessh: {e}");
                127
            }
        }
    }
}
