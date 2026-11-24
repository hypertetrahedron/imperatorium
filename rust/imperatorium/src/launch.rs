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

/// Windows Terminal's `wt.exe`: on PATH, else its App Execution Alias in
/// `%LOCALAPPDATA%\Microsoft\WindowsApps` (a zero-byte stub there). Managed
/// machines often leave that folder off PATH, which made "wt.exe is not on
/// PATH" with Windows Terminal installed (corporate reports, 2026-11-21).
pub fn find_wt() -> Option<String> {
    which("wt").or_else(|| which("wt.exe")).or_else(|| {
        if !cfg!(windows) {
            return None;
        }
        let local = std::env::var_os("LOCALAPPDATA")?;
        let alias = std::path::Path::new(&local).join("Microsoft").join("WindowsApps").join("wt.exe");
        // The alias is a reparse point: it exists, but it is no plain file.
        std::fs::symlink_metadata(&alias).ok().map(|_| alias.to_string_lossy().into_owned())
    })
}

/// Why a session could not be opened without Windows Terminal, said so the
/// person can do something about it.
const NO_WT: &str = "Windows Terminal (wt.exe) was not found - install it, or turn on its alias \
    (Settings > Apps > Advanced app settings > App execution aliases)";

/// history.terminal_command: a new tab in the most recent window, in `cwd`.
pub fn terminal_command(cwd: &str, cmd: &[String]) -> Result<Vec<String>, String> {
    let wt = find_wt().ok_or(NO_WT)?;
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

/// `cmd` in a console window of its own (Windows' plain console), for a
/// machine with no Windows Terminal. Started and let go, like a tab.
fn popen_console(cmd: &[String], cwd: &str) -> Result<(), String> {
    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..]).current_dir(cwd).env_clear().envs(launch_env());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
    }
    c.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Opens `cmd` in `cwd` for the person to see: a Windows Terminal tab, else
/// (Windows only) a console window of its own. Which it was: "tab" or
/// "console". Neither on this machine: the reason.
pub fn open_terminal(cwd: &str, cmd: &[String]) -> Result<&'static str, String> {
    match terminal_command(cwd, cmd) {
        Ok(tab) => popen(&tab, cwd).map(|_| "tab"),
        Err(no_wt) if cfg!(windows) => popen_console(cmd, cwd)
            .map(|_| "console")
            .map_err(|e| format!("{no_wt}; starting {} in a console window failed too: {e}", cmd[0])),
        Err(no_wt) => Err(no_wt),
    }
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
            let terminal = open_terminal(cwd, &cmd)?;
            Ok(json!({"ok": true, "action": action, "where": "window", "terminal": terminal,
                      "command": cmd.join(" ")}))
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
            // A resume carries on; a fork with nothing to do yet waits idle
            // (2026-11-23), as a new background session does.
            let text = match prompt.filter(|p| !p.trim().is_empty()) {
                Some(p) => p,
                None if fork => "",
                None => "Continue where you left off.",
            };
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

/// How a new session starts: its permission mode, model and effort (none:
/// Claude Code's own default).
#[derive(Default, Clone, Copy)]
pub struct Start<'a> {
    pub permission_mode: Option<&'a str>,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
}

/// The effort levels `claude --effort` takes.
pub const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Pure: `--model` and `--effort` added to a command line. A model is an
/// alias or an id (`opus`, `claude-opus-5-5`, `opus[1m]`): anything else is
/// refused, so nothing but a name reaches the command line.
pub fn add_model(cmd: &mut Vec<String>, model: Option<&str>, effort: Option<&str>) -> Result<(), String> {
    if let Some(m) = model.filter(|m| !m.is_empty() && *m != "default") {
        let ok = m.len() <= 80
            && !m.starts_with('-')
            && m.chars().all(|c| c.is_ascii_alphanumeric() || "-._[]:@/".contains(c));
        if !ok {
            return Err(format!("not a model name: {m}"));
        }
        cmd.extend(["--model".into(), m.to_string()]);
    }
    if let Some(e) = effort.filter(|e| !e.is_empty()) {
        if !EFFORTS.contains(&e) {
            return Err(format!("unknown effort level: {e}"));
        }
        cmd.extend(["--effort".into(), e.to_string()]);
    }
    Ok(())
}

/// history.new_session: a fresh `claude` in a new tab, given its id up front
/// so the window can select it as soon as it appears; on `model` at
/// `effort` when given.
pub fn new_session(
    cwd: Option<&str>,
    session_id: &str,
    permission_mode: Option<&str>,
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<Value, String> {
    let cwd = dir_ok(cwd)?;
    let mut cmd = new_command(session_id, permission_mode)?;
    add_model(&mut cmd, model, effort)?;
    let terminal = open_terminal(cwd, &cmd)?;
    Ok(
        json!({"ok": true, "action": "new", "where": "window", "terminal": terminal,
              "command": cmd.join(" "), "session_id": session_id}),
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

    #[test]
    fn a_new_session_starts_on_the_model_and_effort_asked_for() {
        let mut c = new_command("s1", None).unwrap();
        add_model(&mut c, Some("opus[1m]"), Some("high")).unwrap();
        assert_eq!(c, ["claude", "--session-id", "s1", "--model", "opus[1m]", "--effort", "high"]);
        let mut c = new_command("s1", None).unwrap();
        add_model(&mut c, Some("default"), Some("")).unwrap();
        assert_eq!(c, ["claude", "--session-id", "s1"]);
        for bad in ["--dangerously-skip-permissions", "opus --x", "a;b"] {
            assert!(add_model(&mut vec![], Some(bad), None).is_err(), "{bad}");
        }
        assert!(add_model(&mut vec![], None, Some("ultra")).is_err());
    }
}
