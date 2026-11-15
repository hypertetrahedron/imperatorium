//! Hand work to a background session, from `imperatorium/background.py`:
//! `claude --bg "<prompt>"` starts a session with no terminal, prints a
//! short id and returns. A dispatch makes its worktree first
//! (changes::make_worktree, from the chosen session's branch).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{Map, Value};

pub const DEFAULT_TIMEOUT: f64 = 90.0;

/// background.short_name: the first four words, letters and digits only.
pub fn short_name(prompt: &str) -> Option<String> {
    static WORD: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let words: Vec<&str> = WORD
        .get_or_init(|| Regex::new("[A-Za-z0-9]+").expect("the word pattern"))
        .find_iter(prompt)
        .map(|m| m.as_str())
        .collect();
    let name: String = words
        .iter()
        .take(4)
        .cloned()
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase();
    let name: String = name.chars().take(28).collect();
    let name = name.trim_matches('-').to_string();
    (!name.is_empty()).then_some(name)
}

fn opt(options: &Map<String, Value>, k: &str) -> Option<String> {
    options
        .get(k)
        .filter(|v| crate::pyish::truthy(Some(v)))
        .map(crate::pyish::py_str)
}

/// background.build_command. `name`: absent derives one from the prompt;
/// "" keeps the session's own, which resuming wants.
pub fn build_command(prompt: &str, options: &Map<String, Value>) -> Vec<String> {
    let mut cmd = vec!["claude".to_string(), "--bg".to_string()];
    let name = match options.get("name") {
        None | Some(Value::Null) => short_name(prompt),
        Some(v) => Some(crate::pyish::py_str(v)).filter(|s| !s.is_empty()),
    };
    if let Some(n) = name {
        cmd.extend(["--name".into(), n]);
    }
    for (key, flag) in [
        ("model", "--model"),
        ("permission_mode", "--permission-mode"),
        ("agent", "--agent"),
    ] {
        if let Some(v) = opt(options, key) {
            cmd.extend([flag.to_string(), v]);
        }
    }
    for x in options
        .get("extra")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        cmd.push(crate::pyish::py_str(x));
    }
    cmd.push(prompt.to_string());
    cmd
}

/// background.parse_id: the last short hex id printed.
pub fn parse_id(stdout: &str) -> Option<String> {
    static ID: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    ID.get_or_init(|| Regex::new(r"\b([0-9a-f]{6,12})\b").expect("the id pattern"))
        .captures_iter(stdout)
        .last()
        .map(|c| c[1].to_string())
}

/// background.start: {id, command, output}, or why not.
pub fn start(prompt: &str, cwd: &str, options: &Map<String, Value>) -> Result<Value, String> {
    if crate::pyish::strip(prompt).is_empty() {
        return Err("prompt is empty".into());
    }
    if cwd.is_empty() || !std::path::Path::new(cwd).is_dir() {
        return Err(format!("not a directory: {cwd}"));
    }
    let cmd = build_command(prompt, options);
    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..])
        .current_dir(cwd)
        .env_clear()
        .envs(crate::launch::launch_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::proc::no_window(&mut c);
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("claude is not on PATH".into())
        }
        Err(e) => return Err(e.to_string()),
    };
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let o = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = std::io::Read::read_to_end(&mut out, &mut b);
        b
    });
    let e = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = std::io::Read::read_to_end(&mut err, &mut b);
        b
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed().as_secs_f64() < DEFAULT_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20))
            }
            // Not killed: the session it starts must outlive it (keep=True).
            _ => {
                return Err(format!(
                    "claude --bg did not return within {}s",
                    DEFAULT_TIMEOUT
                ))
            }
        }
    };
    let stdout = String::from_utf8_lossy(&o.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&e.join().unwrap_or_default()).into_owned();
    let output = crate::pyish::strip(&format!("{stdout}{stderr}")).to_string();
    if !status.success() {
        let shown = crate::pyish::take(&output, 400);
        return Err(if shown.is_empty() {
            format!("claude --bg exited {}", status.code().unwrap_or(1))
        } else {
            shown
        });
    }
    let Some(id) = parse_id(&stdout) else {
        // It may well have started; say so rather than tempt a retry.
        return Err(format!(
            "started, but no session id was printed: {}",
            crate::pyish::take(&output, 200)
        ));
    };
    Ok(serde_json::json!({"id": id, "command": cmd, "output": crate::pyish::take(&output, 400)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_python() {
        // python: background.short_name("Fix the flaky login test, please!") etc.
        assert_eq!(
            short_name("Fix the flaky login test, please!").as_deref(),
            Some("fix-the-flaky-login")
        );
        assert_eq!(short_name("!!!"), None);
        assert_eq!(
            short_name("supercalifragilistic expialidocious x").as_deref(),
            Some("supercalifragilistic-expiali")
        );
        let o = json!({"model": "opus", "permission_mode": "", "name": "", "extra": ["--resume", "s1"]});
        assert_eq!(
            build_command("go on", o.as_object().unwrap()),
            ["claude", "--bg", "--model", "opus", "--resume", "s1", "go on"]
        );
        assert_eq!(
            parse_id("Started abc123 then 9f8e7d6c."),
            Some("9f8e7d6c".into())
        );
        assert_eq!(parse_id("nothing here"), None);
    }
}

/// test_core.py's BackgroundCommand.
#[cfg(test)]
mod core_tests {
    use super::*;
    use serde_json::json;

    fn opts(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn prompt_is_the_last_argument() {
        // Options before the prompt, so a prompt starting with a dash is not
        // read as a flag.
        let cmd = build_command("--not-a-flag", &opts(json!({"model": "sonnet"})));
        assert_eq!(cmd.last().unwrap(), "--not-a-flag");
        assert_eq!(&cmd[..2], ["claude", "--bg"]);
    }

    #[test]
    fn options_are_omitted_when_unset() {
        // A name is always supplied, derived from the prompt; nothing else is.
        assert_eq!(build_command("x", &Map::new()), ["claude", "--bg", "--name", "x", "x"]);
        let cmd = build_command("x", &Map::new());
        for flag in ["--model", "--permission-mode", "--agent"] {
            assert!(!cmd.iter().any(|a| a == flag), "{flag}");
        }
    }

    #[test]
    fn every_option_is_passed_through() {
        let cmd = build_command(
            "x",
            &opts(json!({"model": "opus", "permission_mode": "plan", "agent": "Plan",
                         "name": "n", "extra": ["--effort", "high"]})),
        );
        for flag in ["--model", "--permission-mode", "--agent", "--name", "--effort"] {
            assert!(cmd.iter().any(|a| a == flag), "{flag} in {cmd:?}");
        }
        assert_eq!(
            cmd,
            ["claude", "--bg", "--name", "n", "--model", "opus", "--permission-mode", "plan",
             "--agent", "Plan", "--effort", "high", "x"]
        );
    }

    #[test]
    fn a_short_name_is_derived_from_the_prompt() {
        // Without one, Claude Code names the session after the whole prompt,
        // which is unreadable on the board and useless on a Stream Deck key.
        assert_eq!(
            short_name("implement the new feature as recommended").as_deref(),
            Some("implement-the-new-feature")
        );
        assert_eq!(
            short_name("Reply with the single word: ok.").as_deref(),
            Some("reply-with-the-single")
        );
    }

    #[test]
    fn a_derived_name_is_a_usable_handle() {
        let name = short_name("fix the *Kohler* shower!! integration").unwrap();
        assert!(!name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{name}");
        assert!(name.chars().count() <= 28);
    }

    #[test]
    fn an_unnameable_prompt_yields_no_name() {
        for prompt in ["   ", "!!!", ""] {
            assert_eq!(short_name(prompt), None, "{prompt:?}");
        }
    }

    #[test]
    fn an_explicit_name_beats_the_derived_one() {
        let cmd = build_command("do the thing", &opts(json!({"name": "mine"})));
        assert!(cmd.iter().any(|a| a == "mine"));
        assert!(!cmd.iter().any(|a| a == "do-the-thing"));
    }

    #[test]
    fn the_id_is_taken_from_the_end_of_the_output() {
        // Preamble can contain hex-looking noise; the id is what it ends with.
        assert_eq!(parse_id("started 8b879562").as_deref(), Some("8b879562"));
        assert_eq!(parse_id("deadbeef then cafe1234").as_deref(), Some("cafe1234"));
    }

    #[test]
    fn no_id_is_reported_as_none() {
        for text in ["", "no identifier printed"] {
            assert_eq!(parse_id(text), None, "{text:?}");
        }
    }

    #[test]
    fn an_empty_prompt_is_refused_before_launching() {
        let here = std::env::current_dir().unwrap().to_string_lossy().into_owned();
        for bad in ["", "   "] {
            assert_eq!(start(bad, &here, &Map::new()).unwrap_err(), "prompt is empty", "{bad:?}");
        }
    }

    #[test]
    fn a_missing_directory_is_refused_before_launching() {
        let gone = std::env::current_dir().unwrap().join("no-such-dir");
        let err = start("do it", &gone.to_string_lossy(), &Map::new()).unwrap_err();
        assert!(err.starts_with("not a directory: "), "{err}");
        assert!(start("do it", "", &Map::new()).is_err());
    }

    #[test]
    fn unknown_config_keys_do_not_reach_the_command() {
        // config.json is hand-edited and carries "_comment" keys.
        let cmd = build_command(
            "do it",
            &opts(json!({"_comment": "ignore me", "permission_mode": "plan"})),
        );
        assert!(cmd.iter().any(|a| a == "--permission-mode"));
        assert!(!cmd.join(" ").contains("_comment") && !cmd.join(" ").contains("ignore me"));
    }
}
