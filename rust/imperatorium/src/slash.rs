//! Slash commands and Send As Typed, from `imperatorium/slash.py`: typing
//! into a local session's console, which is real terminal input with the
//! authority of the person at it. So: only into a session idle at its
//! prompt with nothing parked, Ctrl+C first to clear a draft, one line only,
//! and never a command that opens a terminal menu the window cannot drive.

use serde_json::{json, Map, Value};

use crate::pyish::{strip, truthy};

use std::collections::HashSet;

/// What happens to a slash command sent from the window (`classify`).
/// The audit behind these tables: docs/slash-commands-terminal-gaps.md
/// (Claude Code 2.1.330's command table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum How {
    /// Typed (or run through the mod) as it is.
    Run,
    /// The terminal would show a picker; the window shows its own.
    Picker(&'static str),
    /// The terminal would draw a panel; the window shows its own view.
    View(&'static str),
    /// One of the window's own features does this.
    Window(&'static str),
    /// Needs the terminal: (group, hint). Groups: ui, account, display,
    /// detach, unknown.
    Terminal(&'static str, &'static str),
    /// Ending the session is the window's Close.
    Close,
}

impl How {
    pub fn kind(&self) -> &'static str {
        match self {
            How::Run => "run",
            How::Picker(_) => "picker",
            How::View(_) => "view",
            How::Window(_) => "window",
            How::Terminal(..) => "terminal",
            How::Close => "close",
        }
    }

    /// The picker, view or window action; for the terminal, its group.
    pub fn what(&self) -> &'static str {
        match self {
            How::Picker(w) | How::View(w) | How::Window(w) => w,
            How::Terminal(g, _) => g,
            How::Run | How::Close => "",
        }
    }
}

/// What Claude Code says about its own commands, where it has been asked:
/// `text` - the commands that run without a terminal (the headless
/// `initialize` probe, claude_catalog.rs); `builtin` - the built-in ones the
/// session can run (its mod's `$.command.list()`). Either may be empty.
#[derive(Debug, Default, Clone)]
pub struct Known {
    pub text: HashSet<String>,
    pub builtin: HashSet<String>,
}

/// Alias -> the command it names (2.1.330).
const ALIASES: &[(&str, &str)] = &[
    ("settings", "config"),
    ("allowed-tools", "permissions"),
    ("plugins", "plugin"),
    ("marketplace", "plugin"),
    ("bashes", "tasks"),
    ("continue", "resume"),
    ("checkpoint", "rewind"),
    ("undo", "rewind"),
    ("cost", "usage"),
    ("stats", "usage"),
    ("quit", "exit"),
    ("rc", "remote-control"),
    ("tp", "teleport"),
    ("bg", "background"),
    ("share", "bug"),
    ("app", "desktop"),
    ("ios", "mobile"),
    ("android", "mobile"),
    ("remote", "session"),
    ("breaks", "wellbeing"),
    ("break-reminder", "wellbeing"),
    ("downtime", "wellbeing"),
    ("memory-pause", "pause-memory"),
    ("toggle-memory", "pause-memory"),
    ("update", "restart"),
    ("reset", "clear"),
    ("new", "clear"),
    ("name", "rename"),
    ("checkup", "doctor"),
    ("extra-usage", "usage-credits"),
    ("peers", "list-agents"),
    ("routines", "schedule"),
];

/// The command an alias names, or the name itself.
pub fn canonical(name: &str) -> &str {
    ALIASES
        .iter()
        .find(|(a, _)| *a == name)
        .map(|(_, c)| *c)
        .unwrap_or(name)
}

/// The aliases of `name`.
pub fn aliases_of(name: &str) -> Vec<&'static str> {
    ALIASES
        .iter()
        .filter(|(_, c)| *c == name)
        .map(|(a, _)| *a)
        .collect()
}

/// Whether a rule holds always or only with nothing after the name.
#[derive(Clone, Copy, PartialEq)]
enum When {
    Always,
    Bare,
}

/// The built-ins that do not simply run when typed. Anything not here runs,
/// except a built-in Claude Code itself lists as needing a terminal (Known).
const RULES: &[(&str, When, How)] = &[
    ("exit", When::Always, How::Close),
    // Pickers, shown in the window.
    ("model", When::Bare, How::Picker("model")),
    ("effort", When::Bare, How::Picker("effort")),
    ("fast", When::Bare, How::Picker("fast")),
    ("output-style", When::Bare, How::Picker("output-style")),
    // Panels, shown in the window.
    ("context", When::Always, How::View("context")),
    ("usage", When::Always, How::View("usage")),
    ("status", When::Always, How::View("status")),
    ("version", When::Always, How::View("status")),
    ("help", When::Always, How::View("help")),
    ("skills", When::Always, How::View("skills")),
    ("hooks", When::Always, How::View("hooks")),
    ("permissions", When::Bare, How::View("permissions")),
    ("config", When::Bare, How::View("config")),
    ("release-notes", When::Always, How::View("release-notes")),
    ("diff", When::Always, How::View("diff")),
    ("tasks", When::Always, How::View("tasks")),
    ("workflows", When::Always, How::View("tasks")),
    // The window's own features.
    ("resume", When::Bare, How::Window("history")),
    ("export", When::Bare, How::Window("export")),
    ("branch", When::Bare, How::Window("fork")),
    ("add-dir", When::Bare, How::Window("add-dir")),
    ("copy", When::Always, How::Window("copy")),
    // A terminal menu, panel or wizard, whatever follows.
    ("rewind", When::Always, How::Terminal("ui", "")),
    ("plugin", When::Always, How::Terminal("ui", "")),
    ("memory", When::Always, How::Terminal("ui", "")),
    ("ide", When::Always, How::Terminal("ui", "")),
    ("teleport", When::Always, How::Terminal("ui", "")),
    ("cloud-plugins", When::Always, How::Terminal("ui", "")),
    ("daemon", When::Always, How::Terminal("ui", "")),
    ("remote-control", When::Always, How::Terminal("ui", "")),
    ("desktop", When::Always, How::Terminal("ui", "")),
    ("chrome", When::Always, How::Terminal("ui", "")),
    ("feedback", When::Always, How::Terminal("ui", "")),
    ("bug", When::Always, How::Terminal("ui", "")),
    ("btw", When::Always, How::Terminal("ui", "its answer is drawn over the terminal's prompt, never written to the conversation")),
    ("sandbox", When::Always, How::Terminal("ui", "")),
    ("powerup", When::Always, How::Terminal("ui", "")),
    ("artifacts", When::Always, How::Terminal("ui", "")),
    ("access", When::Always, How::Terminal("ui", "")),
    ("keybindings", When::Always, How::Terminal("ui", "")),
    // The same, unless given what to act on.
    ("mcp", When::Bare, How::Terminal("ui", "/mcp reconnect|enable|disable <server> runs from here")),
    ("import", When::Bare, How::Terminal("ui", "/import codex|gemini|cursor runs from here")),
    ("auto-mode-setup", When::Bare, How::Terminal("ui", "")),
    ("advisor", When::Bare, How::Terminal("ui", "")),
    ("autocompact", When::Bare, How::Terminal("ui", "/autocompact auto|<tokens> runs from here")),
    ("color", When::Bare, How::Terminal("ui", "/color <name> runs from here")),
    ("remote-env", When::Bare, How::Terminal("ui", "")),
    ("use-case", When::Bare, How::Terminal("ui", "")),
    ("cd", When::Bare, How::Terminal("ui", "/cd <path> runs from here")),
    // Account, plan and setup: in a terminal, once.
    ("login", When::Always, How::Terminal("account", "")),
    ("logout", When::Always, How::Terminal("account", "")),
    ("upgrade", When::Always, How::Terminal("account", "")),
    ("usage-credits", When::Always, How::Terminal("account", "")),
    ("rate-limit-options", When::Always, How::Terminal("account", "")),
    ("web-setup", When::Always, How::Terminal("account", "")),
    ("install-github-app", When::Always, How::Terminal("account", "")),
    ("install-slack-app", When::Always, How::Terminal("account", "")),
    ("setup-bedrock", When::Always, How::Terminal("account", "")),
    ("setup-vertex", When::Always, How::Terminal("account", "")),
    ("design-login", When::Always, How::Terminal("account", "")),
    ("privacy-settings", When::Always, How::Terminal("account", "")),
    ("pro-trial-expired", When::Always, How::Terminal("account", "")),
    ("passes", When::Always, How::Terminal("account", "")),
    // The terminal's own look and features.
    ("theme", When::Always, How::Terminal("display", "")),
    ("tui", When::Always, How::Terminal("display", "")),
    ("scroll-speed", When::Always, How::Terminal("display", "")),
    ("focus", When::Always, How::Terminal("display", "")),
    ("brief", When::Always, How::Terminal("display", "")),
    ("terminal-setup", When::Always, How::Terminal("display", "")),
    ("wellbeing", When::Always, How::Terminal("display", "")),
    ("voice", When::Always, How::Terminal("display", "")),
    ("radio", When::Always, How::Terminal("display", "")),
    ("stickers", When::Always, How::Terminal("display", "")),
    ("mobile", When::Always, How::Terminal("display", "")),
    ("session", When::Always, How::Terminal("display", "")),
    // They move the session out from under the window.
    ("background", When::Always, How::Terminal("detach", "")),
    ("stop", When::Always, How::Terminal("detach", "Close ends it from here")),
    ("restart", When::Always, How::Terminal("detach", "")),
];

/// The built-ins Claude Code runs itself (its `local` and `local-jsx`
/// commands, read from the 2.1.330 binary). Any other command — a bundled
/// skill such as /doctor or /simplify, a plugin's skill, an MCP prompt, the
/// `prompt` built-ins /init and /insights — becomes a model turn.
const LOCAL: &[&str] = &[
    "__remote-workflow", "access", "add-dir", "advisor", "agents", "artifacts",
    "auto-mode-setup", "autocompact", "autofix-pr", "background", "branch", "brief",
    "btw", "bug", "cd", "clear", "cloud-plugins", "color", "compact", "config",
    "context", "copy", "daemon", "design-consent", "design-login", "design-revoke",
    "desktop", "diff", "effort", "exit", "export", "extra-usage", "fast", "feedback",
    "focus", "fork", "goal", "heapdump", "help", "hooks", "ide", "import",
    "install", "install-github-app", "install-slack-app", "list-agents", "login",
    "logout", "mcp", "memory", "mobile", "model", "output-style", "passes",
    "pause-memory", "permissions", "plan", "plugin", "powerup", "privacy-settings",
    "pro-trial-expired", "project", "radio", "rate-limit-options", "recap",
    "reload-plugins", "reload-skills", "remote-control", "remote-env", "rename",
    "restart", "resume", "scroll-speed", "session", "setup-bedrock", "setup-vertex",
    "skill-doctor", "skills", "status", "stickers", "stop", "subtask", "tasks",
    "teleport", "terminal-setup", "theme", "tui", "ultraplan", "ultrareview",
    "upgrade", "usage", "usage-credits", "use-case", "version", "voice",
    "web-setup", "wellbeing", "workflow-launch-exec", "workflows",
];

/// slash.starts_turn: whether sending `/name` starts a model turn (it
/// prompts the model) rather than Claude Code answering it itself.
pub fn starts_turn(name: &str) -> bool {
    let name = canonical(name);
    !LOCAL.contains(&name) && !RULES.iter().any(|(n, _, _)| *n == name)
}

/// Every built-in RULES names, for the window's list.
pub fn ruled() -> impl Iterator<Item = &'static str> {
    RULES.iter().map(|(n, _, _)| *n)
}

/// slash.classify: what becomes of `/name rest` sent from the window.
pub fn classify(name: &str, rest: &str, known: Option<&Known>) -> How {
    let name = canonical(name);
    let bare = rest.trim().is_empty();
    if let Some((_, when, how)) = RULES.iter().find(|(n, _, _)| *n == name) {
        if *when == When::Always || bare {
            return how.clone();
        }
        return How::Run;
    }
    // A built-in these tables do not know (a newer Claude Code's): when
    // Claude Code says it has no form without a terminal, it opens one.
    if let Some(k) = known {
        if bare && !k.text.is_empty() && k.builtin.contains(name) && !k.text.contains(name) {
            return How::Terminal("unknown", "");
        }
    }
    How::Run
}

/// Why `/name` cannot go as typed, for one classified otherwise.
pub fn explain(name: &str, how: &How) -> Option<String> {
    let hint = |h: &str| if h.is_empty() { String::new() } else { format!(" ({h})") };
    Some(match how {
        How::Run => return None,
        How::Close => "use Close to end the session - it closes the terminal tab too".into(),
        How::Picker(_) => format!(
            "/{name} on its own opens a picker in the terminal - type it in the Imperatorium chat to pick there, or name it (/{name} <value>)"
        ),
        How::View(_) => format!(
            "/{name} draws a panel in the terminal - type it in the Imperatorium chat to see it there"
        ),
        How::Window("add-dir") => format!("/{name} needs the folder after it (/{name} <path>)"),
        How::Window(w) => {
            let feature = match *w {
                "history" => "History",
                "export" => "View > Export as Markdown",
                "fork" => "New / Fork > Fork",
                "copy" => "the copy button on the reply",
                _ => "a window feature",
            };
            format!("/{name} on its own is {feature} in the Imperatorium window")
        }
        How::Terminal("account", h) => format!(
            "/{name} is account or setup - run it once in a terminal: use Show Window{}",
            hint(h)
        ),
        How::Terminal("display", h) => format!(
            "/{name} changes the terminal itself - it has no effect in the window; use Show Window to run it there{}",
            hint(h)
        ),
        How::Terminal("detach", h) => format!(
            "/{name} moves the session out from under the window - use Show Window to run it there{}",
            hint(h)
        ),
        How::Terminal("unknown", _) => format!(
            "/{name} is not one Claude Code runs without a terminal - use Show Window and run it there"
        ),
        How::Terminal(_, h) => format!(
            "/{name} opens a menu in the terminal, which the window cannot drive - use Show Window and run it there{}",
            hint(h)
        ),
    })
}

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


/// slash.refusal: why `text` cannot be typed at a prompt, or None - by the
/// tables alone (what Claude Code says of itself left out).
pub fn refusal(text: &str) -> Option<String> {
    refusal_in(text, None)
}

/// `refusal`, with what Claude Code says of its commands where known.
pub fn refusal_in(text: &str, known: Option<&Known>) -> Option<String> {
    let Some((name, rest)) = parse(text) else {
        return Some("not a slash command".into());
    };
    let t = strip(text);
    if t.contains('\n') || t.contains('\r') {
        return Some("a slash command is one line".into());
    }
    explain(&name, &classify(&name, &rest, known))
}

/// The command's name and what becomes of it, for a one-line command.
pub fn how_of(text: &str, known: Option<&Known>) -> Option<(String, How)> {
    let (name, rest) = parse(text)?;
    let how = classify(&name, &rest, known);
    Some((name, how))
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
    fn skills_and_prompt_commands_start_a_turn() {
        for n in ["doctor", "simplify", "code-review", "init", "insights", "my-skill"] {
            assert!(starts_turn(n), "{n}");
        }
        for n in ["compact", "clear", "reset", "list-agents", "model", "context", "cost", "recap"] {
            assert!(!starts_turn(n), "{n}");
        }
    }

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
            "/model opus",
            "/effort high",
            "/resume abc123",
            "/code-review high",
            "/config verbose=true",
            "/mcp reconnect github",
            "/doctor",
            "/agents",
        ] {
            assert_eq!(refusal(text), None, "{text}");
        }
    }

    #[test]
    fn a_menu_the_window_cannot_drive_is_refused() {
        for text in ["/plugin", "/mcp", "/rewind", "/rewind 3", "/checkpoint", "/memory"] {
            assert!(refusal(text).unwrap().contains("opens a menu"), "{text}");
        }
    }

    #[test]
    fn a_picker_or_panel_is_the_windows_own() {
        assert_eq!(classify("model", "", None), How::Picker("model"));
        assert_eq!(classify("model", "opus", None), How::Run);
        assert_eq!(classify("cost", "", None), How::View("usage"));
        assert_eq!(classify("context", "all", None), How::View("context"));
        assert_eq!(classify("settings", "", None), How::View("config"));
        assert_eq!(classify("continue", "", None), How::Window("history"));
        assert!(refusal("/model").unwrap().contains("/model <value>"));
        assert!(refusal("/context").unwrap().contains("Imperatorium chat"));
    }

    #[test]
    fn account_and_display_commands_say_so() {
        assert!(refusal("/login").unwrap().contains("account"));
        assert!(refusal("/theme").unwrap().contains("no effect in the window"));
        assert!(refusal("/bg").unwrap().contains("out from under"));
    }

    #[test]
    fn a_builtin_claude_code_lists_as_terminal_only_is_refused_bare() {
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        let k = Known { text: set(&["compact", "model"]), builtin: set(&["compact", "model", "brand-new"]) };
        assert_eq!(classify("brand-new", "", Some(&k)), How::Terminal("unknown", ""));
        assert_eq!(classify("brand-new", "x", Some(&k)), How::Run);
        assert_eq!(classify("brand-new", "", None), How::Run);
        // A plugin's or a skill's (not a built-in) always runs.
        assert_eq!(classify("my-skill", "", Some(&k)), How::Run);
    }

    #[test]
    fn exit_points_at_close() {
        assert!(refusal("/exit").unwrap().contains("Close"));
    }

    #[test]
    fn a_second_line_would_submit_the_first() {
        assert!(refusal("/compact\nthen delete it all").unwrap().contains("one line"));
    }

    // Typing into a session's console is Windows-only (blocked() says so
    // first elsewhere), so the next two only hold there.
    #[cfg(windows)]
    #[test]
    fn an_idle_local_session_takes_it() {
        assert_eq!(blocked(&row(json!({})), &[]), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn elsewhere_typing_is_refused_as_windows_only() {
        assert!(blocked(&row(json!({})), &[]).is_some_and(|w| w.contains("Windows-only")));
    }

    #[test]
    fn never_into_a_turn_or_a_prompt_waiting_on_you() {
        for status in [json!("busy"), json!("shell"), json!("waiting"), Value::Null] {
            assert!(blocked(&row(json!({"status": status})), &[]).is_some(), "{status}");
        }
    }

    #[cfg(windows)]
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
        assert!(typed_refusal("/plugin").unwrap().contains("menu"));
    }
}
