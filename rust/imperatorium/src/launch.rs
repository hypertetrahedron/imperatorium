//! Starting sessions, from the launch half of `imperatorium/history.py`:
//! resume or fork in a new Windows Terminal tab or in the background, and a
//! fresh session in a new tab. (A herdr-core terminal in place of the tab
//! comes with herdr-core hosting.)

use std::process::{Command, Stdio};

use serde_json::{json, Map, Value};

/// Set by a running Claude Code session for its children. `wt` hands its
/// caller's environment to the new tab, so a `claude` launched with these
/// would take itself for that session's child and never register.
pub const SESSION_ENV: &[&str] = &[
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

/// history.launch_env: this process's environment without SESSION_ENV.
pub fn launch_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(k, _)| !SESSION_ENV.iter().any(|s| s.eq_ignore_ascii_case(k)))
        .collect()
}

/// shutil.which: the first `name` (with each PATHEXT extension on Windows)
/// on PATH.
pub fn which(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        vec![String::new()]
    };
    let has_ext = cfg!(windows)
        && exts
            .iter()
            .any(|e| name.to_lowercase().ends_with(&e.to_lowercase()));
    for dir in std::env::split_paths(&path) {
        let candidates: Vec<String> = if has_ext || !cfg!(windows) {
            vec![name.to_string()]
        } else {
            exts.iter().map(|e| format!("{name}{e}")).collect()
        };
        for c in candidates {
            let p = dir.join(&c);
            if p.is_file() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// history.resume_command: what a person would type.
pub fn resume_command(session_id: &str, fork: bool) -> Vec<String> {
    let mut cmd = vec![
        "claude".to_string(),
        "--resume".into(),
        session_id.to_string(),
    ];
    if fork {
        cmd.push("--fork-session".into());
    }
    cmd
}

/// history.terminal_command: a new tab in the most recent window, in `cwd`.
pub fn terminal_command(cwd: &str, cmd: &[String]) -> Result<Vec<String>, String> {
    let wt = which("wt")
        .or_else(|| which("wt.exe"))
        .ok_or("Windows Terminal (wt.exe) is not on PATH")?;
    let mut out = vec![
        wt,
        "-w".into(),
        "0".into(),
        "new-tab".into(),
        "-d".into(),
        cwd.to_string(),
    ];
    out.extend(cmd.iter().cloned());
    Ok(out)
}

/// history._popen: start it and let it go.
fn popen(cmd: &[String], cwd: &str) -> Result<(), String> {
    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..])
        .current_dir(cwd)
        .env_clear()
        .envs(launch_env())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::proc::no_window(&mut c);
    c.spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn dir_ok(cwd: Option<&str>) -> Result<&str, String> {
    match cwd {
        Some(c) if !c.is_empty() && std::path::Path::new(c).is_dir() => Ok(c),
        Some(c) => Err(format!("its directory is gone: {c}")),
        None => Err("its directory is gone: None".into()),
    }
}

/// history.resume: resume (or fork) in a new tab or in the background.
pub fn resume(
    session_id: &str,
    cwd: Option<&str>,
    where_: &str,
    fork: bool,
    prompt: Option<&str>,
    options: &Map<String, Value>,
) -> Result<Value, String> {
    if session_id.is_empty() {
        return Err("session id is required".into());
    }
    let cwd = dir_ok(cwd)?;
    let action = if fork { "fork" } else { "resume" };
    match where_ {
        "window" => {
            let cmd = resume_command(session_id, fork);
            popen(&terminal_command(cwd, &cmd)?, cwd)?;
            Ok(json!({"ok": true, "action": action, "where": "window", "command": cmd.join(" ")}))
        }
        "background" => {
            let mut extra = vec![json!("--resume"), json!(session_id)];
            if fork {
                extra.push(json!("--fork-session"));
            }
            let mut opts = options.clone();
            opts.insert("name".into(), json!(""));
            let mut all = opts
                .get("extra")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            all.extend(extra.iter().cloned());
            opts.insert("extra".into(), Value::Array(all));
            let text = prompt
                .filter(|p| !p.is_empty())
                .unwrap_or("Continue where you left off.");
            let started = crate::background::start(text, cwd, &opts)?;
            let mut shown = vec!["claude".to_string(), "--bg".into()];
            shown.extend(extra.iter().map(crate::pyish::py_str));
            Ok(
                json!({"ok": true, "action": action, "where": "background", "id": started["id"],
                      "command": shown.join(" ")}),
            )
        }
        _ => Err("where must be window or background".into()),
    }
}

/// The permission modes `claude --permission-mode` takes; the New Session
/// dialog offers these.
pub const PERMISSION_MODES: &[&str] = &["default", "acceptEdits", "plan", "auto", "dontAsk", "bypassPermissions"];

/// Pure: a fresh session's command line, starting in `permission_mode`
/// (none or "default": Claude Code's own default). Anything else is refused.
pub fn new_command(session_id: &str, permission_mode: Option<&str>) -> Result<Vec<String>, String> {
    let mut cmd: Vec<String> = vec!["claude".into(), "--session-id".into(), session_id.into()];
    match permission_mode.filter(|m| !m.is_empty() && *m != "default") {
        None => {}
        Some(m) if PERMISSION_MODES.contains(&m) => cmd.extend(["--permission-mode".into(), m.to_string()]),
        Some(m) => return Err(format!("unknown permission mode: {m}")),
    }
    Ok(cmd)
}

/// history.new_session: a fresh `claude` in a new tab, given its id up front
/// so the window can select it as soon as it appears.
pub fn new_session(cwd: Option<&str>, session_id: &str, permission_mode: Option<&str>) -> Result<Value, String> {
    let cwd = dir_ok(cwd)?;
    let cmd = new_command(session_id, permission_mode)?;
    popen(&terminal_command(cwd, &cmd)?, cwd)?;
    Ok(
        json!({"ok": true, "action": "new", "where": "window", "command": cmd.join(" "),
              "session_id": session_id}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_session_starts_in_the_mode_asked_for() {
        assert_eq!(new_command("s1", None).unwrap(), ["claude", "--session-id", "s1"]);
        assert_eq!(new_command("s1", Some("default")).unwrap(), ["claude", "--session-id", "s1"]);
        assert_eq!(new_command("s1", Some("plan")).unwrap(),
                   ["claude", "--session-id", "s1", "--permission-mode", "plan"]);
        assert_eq!(new_command("s1", Some("bypassPermissions")).unwrap()[4], "bypassPermissions");
        // Never an argument of its own choosing on claude's command line.
        assert!(new_command("s1", Some("--dangerously-skip-permissions")).is_err());
        assert!(new_command("s1", Some("plan --x")).is_err());
    }
}
