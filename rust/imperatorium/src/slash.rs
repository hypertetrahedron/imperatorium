//! Slash commands and Send As Typed, from `imperatorium/slash.py`: typing
//! into a local session's console, which is real terminal input with the
//! authority of the person at it. So: only into a session idle at its
//! prompt with nothing parked, Ctrl+C first to clear a draft, one line only,
//! and never a command that opens a terminal menu the window cannot drive.

use serde_json::{json, Map, Value};

use crate::pyish::{strip, truthy};

/// Open a menu or panel in the terminal whatever follows them.
const MENU: &[&str] = &[
    "agents",
    "allowed-tools",
    "bashes",
    "config",
    "doctor",
    "help",
    "hooks",
    "ide",
    "login",
    "logout",
    "mcp",
    "memory",
    "permissions",
    "plugin",
    "plugins",
    "privacy-settings",
    "settings",
    "status",
    "tasks",
    "terminal-setup",
    "theme",
    "upgrade",
];
/// Open one only when given nothing to act on.
const MENU_BARE: &[&str] = &[
    "add-dir",
    "checkpoint",
    "continue",
    "export",
    "model",
    "output-style",
    "resume",
    "rewind",
];
/// Ending the session is the window's Close, which closes its tab too.
const EXIT: &[&str] = &["exit", "quit"];

/// Long enough for any prompt worth typing; the typer passes it on a
/// child's command line, which Windows caps at 32,767 characters.
pub const TYPED_MAX: usize = 8000;
/// Line and paragraph breaks become spaces: Enter would submit.
const BREAKS: &[char] = &[
    '\r', '\n', '\t', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}',
];
/// At the start of the prompt these switch the input mode.
const MODE_KEYS: &[char] = &['!', '#'];
/// unicodedata.category(ch) == "Cf", Unicode 15.0 (Python 3.12).
const FORMAT: &[(u32, u32)] = &[
    (0xAD, 0xAD),
    (0x600, 0x605),
    (0x61C, 0x61C),
    (0x6DD, 0x6DD),
    (0x70F, 0x70F),
    (0x890, 0x891),
    (0x8E2, 0x8E2),
    (0x180E, 0x180E),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0001, 0xE0001),
    (0xE0020, 0xE007F),
];

/// slash.parse: (name lowercased, rest) for a slash command, else None.
pub fn parse(text: &str) -> Option<(String, String)> {
    let text = strip(text);
    let mut chars = text.chars();
    if chars.next() != Some('/') {
        return None;
    }
    match chars.next() {
        None | Some('/') | Some(' ') => return None,
        _ => {}
    }
    let body = &text[1..];
    let (name, rest) = body.split_once(' ').unwrap_or((body, ""));
    Some((name.to_lowercase(), strip(rest).to_string()))
}

pub fn is_command(text: &str) -> bool {
    parse(text).is_some()
}

/// slash.refusal: why `text` cannot be typed at a prompt, or None.
pub fn refusal(text: &str) -> Option<String> {
    let Some((name, rest)) = parse(text) else {
        return Some("not a slash command".into());
    };
    let t = strip(text);
    if t.contains('\n') || t.contains('\r') {
        return Some("a slash command is one line".into());
    }
    if EXIT.contains(&name.as_str()) {
        return Some("use Close to end the session - it closes the terminal tab too".into());
    }
    let bare = MENU_BARE.contains(&name.as_str());
    if MENU.contains(&name.as_str()) || (bare && rest.is_empty()) {
        let hint = if bare {
            format!(" - give it an argument (e.g. /{name} <name>)")
        } else {
            String::new()
        };
        return Some(format!(
            "/{name} opens a menu in the terminal, which the window cannot drive; use Focus and run it there{hint}"
        ));
    }
    None
}

/// slash.unreachable: why `row` can never be typed into from here.
pub fn unreachable(row: &Map<String, Value>) -> Option<String> {
    if truthy(row.get("remote")) {
        return Some(format!(
            "slash commands can only be typed into a session on this machine, not one on {}",
            crate::pyish::py_str(row.get("host").unwrap_or(&Value::Null))
        ));
    }
    if !cfg!(windows) {
        return Some("typing a slash command into a session is Windows-only for now".into());
    }
    None
}

/// slash.blocked: why `row` cannot be typed into right now.
pub fn blocked(row: &Map<String, Value>, parked: &[Value]) -> Option<String> {
    if let Some(why) = unreachable(row) {
        return Some(why);
    }
    if !row.get("pid").is_some_and(|p| p.is_i64() || p.is_u64()) {
        return Some("that session is not running".into());
    }
    let sid = row.get("sessionId");
    if parked.iter().any(|p| p.get("session_id") == sid) {
        return Some("it is waiting on a permission decision - answer that first".into());
    }
    match row.get("status").and_then(Value::as_str) {
        Some("waiting") => Some("it is waiting on you in the terminal - answer that first".into()),
        Some("idle") => None,
        _ => Some("it is working - send the command when its turn ends, or queue it".into()),
    }
}

/// slash.type_command: clear the prompt, type `text` and Enter.
pub fn type_command(pid: &Value, text: &str) -> bool {
    let Some(pid) = pid.as_u64() else {
        return false;
    };
    crate::closing::type_keys(
        pid as u32,
        &[crate::closing::CTRL_C, strip(text), crate::closing::ENTER],
    )
}

fn is_format(c: char) -> bool {
    let cp = c as u32;
    FORMAT.iter().any(|(a, b)| (*a..=*b).contains(&cp))
}

/// slash.clean_typed: (one safe line, whether it changed). Breaks become
/// spaces, other control and format characters go (bar the emoji joiner),
/// spaces collapse, and a leading `!` or `#` is dropped.
pub fn clean_typed(text: &str) -> (String, bool) {
    let mut out = String::new();
    for ch in text.chars() {
        if BREAKS.contains(&ch) {
            out.push(' ');
        } else if (ch.is_control() || is_format(ch)) && ch != '\u{200d}' {
            continue;
        } else {
            out.push(ch);
        }
    }
    let mut clean = crate::pyish::split_ws(&out).join(" ");
    if !is_command(&clean) {
        clean = strip(clean.trim_start_matches(MODE_KEYS)).to_string();
    }
    let changed = clean != strip(text);
    (clean, changed)
}

/// slash.typed_refusal: why cleaned text cannot be sent as typed.
pub fn typed_refusal(clean: &str) -> Option<String> {
    if clean.is_empty() {
        return Some("nothing left to type".into());
    }
    let n = clean.chars().count();
    if n > TYPED_MAX {
        return Some(format!(
            "too long to type ({n} characters, at most {TYPED_MAX}) - use Send"
        ));
    }
    if is_command(clean) {
        return refusal(clean);
    }
    None
}

/// `imperatorium slash-check`: JSON lines of {text} in, the pure answers
/// out - for the conformance differential.
pub fn check_lines() -> i32 {
    use std::io::BufRead;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let text = serde_json::from_str::<Value>(&line)
            .ok()
            .and_then(|v| v["text"].as_str().map(str::to_string))
            .unwrap_or_default();
        let (clean, changed) = clean_typed(&text);
        let out = json!({"parse": parse(&text).map(|(n, r)| json!([n, r])), "refusal": refusal(&text),
                         "clean": [clean, changed], "typed_refusal": typed_refusal(&clean)});
        println!("{}", crate::pyish::json_dumps(&out, true));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_matches_python() {
        assert_eq!(
            parse("  /Model opus "),
            Some(("model".into(), "opus".into()))
        );
        assert_eq!(parse("/"), None);
        assert_eq!(parse("// x"), None);
        assert_eq!(parse("/ x"), None);
        assert_eq!(parse("hello /x"), None);
    }
}

/// test_slash.py, ported with the same inputs and expected values.
#[cfg(test)]
mod py_ported {
    use super::*;

    const SID: &str = "11111111-1111-1111-1111-111111111111";

    fn row(patch: Value) -> Map<String, Value> {
        let mut r = json!({"sessionId": SID, "pid": 4242, "status": "idle"})
            .as_object()
            .unwrap()
            .clone();
        r.extend(patch.as_object().unwrap().clone());
        r
    }

    fn pair(a: &str, b: &str) -> Option<(String, String)> {
        Some((a.to_string(), b.to_string()))
    }

    #[test]
    fn a_command_and_its_arguments() {
        assert_eq!(parse("/model opus"), pair("model", "opus"));
        assert_eq!(
            parse("  /Compact  keep the API notes "),
            pair("compact", "keep the API notes")
        );
    }

    #[test]
    fn text_that_only_mentions_a_command_is_a_prompt() {
        for text in ["run /compact please", "", "/", "/ compact", "//comment"] {
            assert_eq!(parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn ordinary_commands_are_typed() {
        for text in [
            "/compact",
            "/clear",
            "/context",
            "/cost",
            "/model opus",
            "/resume abc123",
            "/code-review high",
        ] {
            assert_eq!(refusal(text), None, "{text}");
        }
    }

    #[test]
    fn a_menu_the_window_cannot_drive_is_refused() {
        for text in ["/config", "/permissions", "/mcp", "/model", "/rewind"] {
            assert!(refusal(text).unwrap().contains("opens a menu"), "{text}");
        }
    }

    #[test]
    fn a_bare_menu_command_is_told_to_take_an_argument() {
        assert!(refusal("/model").unwrap().contains("/model <name>"));
    }

    #[test]
    fn exit_points_at_close() {
        assert!(refusal("/exit").unwrap().contains("Close"));
    }

    #[test]
    fn a_second_line_would_submit_the_first() {
        assert!(refusal("/compact\nthen delete it all").unwrap().contains("one line"));
    }

    #[test]
    fn an_idle_local_session_takes_it() {
        assert_eq!(blocked(&row(json!({})), &[]), None);
    }

    #[test]
    fn never_into_a_turn_or_a_prompt_waiting_on_you() {
        for status in [json!("busy"), json!("shell"), json!("waiting"), Value::Null] {
            assert!(blocked(&row(json!({"status": status})), &[]).is_some(), "{status}");
        }
    }

    #[test]
    fn never_while_a_permission_decision_is_parked_for_it() {
        // Enter would answer the dialog the hook is holding open.
        let why = blocked(&row(json!({})), &[json!({"session_id": SID})]);
        assert!(why.unwrap().contains("permission"));
        assert_eq!(blocked(&row(json!({})), &[json!({"session_id": "other"})]), None);
    }

    #[test]
    fn a_remote_session_has_no_console_here() {
        let why = blocked(&row(json!({"remote": true, "host": "pi"})), &[]);
        assert!(why.unwrap().contains("pi"));
    }

    #[test]
    fn a_session_without_a_process() {
        assert!(blocked(&row(json!({"pid": null})), &[]).is_some());
    }

    /// The Windows check is `cfg!(windows)`, fixed when the binary is built,
    /// so the other branch is only reachable on a non-Windows build.
    #[cfg(not(windows))]
    #[test]
    fn windows_only() {
        assert!(blocked(&row(json!({})), &[]).unwrap().contains("Windows"));
    }

    /// The typer here is the binary's own `type-into` child, with no seam
    /// to substitute (Python took a `typer=` argument), so what can be
    /// checked is the guard: a pid that is not a process id types nothing.
    #[test]
    fn typing_needs_a_process_id() {
        assert!(!type_command(&Value::Null, "/compact"));
        assert!(!type_command(&json!("4242"), "/compact"));
    }

    #[test]
    fn lines_become_one() {
        assert_eq!(
            clean_typed("spend up to\r\n$40\n\n\tthen stop"),
            ("spend up to $40 then stop".to_string(), true)
        );
    }

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(clean_typed("  i approve  "), ("i approve".to_string(), false));
    }

    #[test]
    fn control_and_invisible_characters_go() {
        let (clean, changed) = clean_typed("ok\x1b[31m\u{200b} go\x03");
        assert_eq!(clean, "ok[31m go");
        assert!(changed);
    }

    #[test]
    fn emoji_stay() {
        assert_eq!(clean_typed("ship it \u{1f680}").0, "ship it \u{1f680}");
    }

    #[test]
    fn a_leading_shell_or_memory_key_is_dropped() {
        assert_eq!(clean_typed("!rm -rf build").0, "rm -rf build");
        assert_eq!(clean_typed("# Heading\ntext").0, "Heading text");
    }

    #[test]
    fn a_command_is_kept_as_a_command() {
        assert_eq!(clean_typed("/compact\nkeep notes").0, "/compact keep notes");
    }

    #[test]
    fn typed_refusal_empty_and_too_long() {
        assert!(typed_refusal("").unwrap().contains("nothing"));
        assert!(typed_refusal(&"x".repeat(TYPED_MAX + 1)).unwrap().contains("too long"));
        assert_eq!(typed_refusal(&"x".repeat(TYPED_MAX)), None);
    }

    #[test]
    fn a_command_gets_the_command_rules() {
        assert!(typed_refusal("/config").unwrap().contains("menu"));
    }
}
