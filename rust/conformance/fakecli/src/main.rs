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
