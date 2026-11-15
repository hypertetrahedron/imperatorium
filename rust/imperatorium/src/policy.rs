//! Decide which permission prompts are worth a human's attention.
//!
//! A port of `imperatorium/policy.py`, which stays the reference until the
//! conformance suite (`rust/conformance/`) passes against this binary. Keep
//! the two in step: a pattern changed in one and not the other is a policy
//! hole, and the differential test is there to catch exactly that.
//!
//! Three rules govern everything here (see policy.py for the long form):
//!   1. Default to `ask`. Nothing is allowed unless a rule says so.
//!   2. This removes prompts, never answers. `deny` is reserved for an agent
//!      changing the machinery that decides what it may do.
//!   3. Fail safe. Any error, any malformed request, any doubt: `ask`.
//!
//! Where Python would raise, this returns `Err(name of the exception)` and
//! `decide` turns it into `ask`, with the same reason text Python gives.
//!
//! Deliberate differences from Python, each stricter, never looser:
//! - a leading `NAME=value` is stripped only for ASCII names (bash accepts no
//!   others), where Python's `\w` would also strip Unicode letters;
//! - a `safe_bash` pattern Python's `re` accepts but the `regex` crate does
//!   not (lookaround, backreferences) is an error, so `ask`.
//!
//! And one that changes only which `ask` reason is given: `\b` follows
//! Unicode's word characters (letters, digits, marks, connectors) where
//! Python's follows `str.isalnum()`, so `½git push` and `\u{301}git push`
//! are asked about for a different reason. The differential test
//! (`rust/conformance/differential.py`) fails on any difference these do
//! not explain, and on any looser verdict at all.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value};

use crate::paths;
use crate::pyish::{or, py_regex, py_str, split_ws, strip, take, truthy};

pub const ALLOW: &str = "allow";
pub const DENY: &str = "deny";
pub const ASK: &str = "ask";

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub verdict: &'static str,
    pub reason: String,
    pub tier: &'static str,
}

fn v(verdict: &'static str, reason: impl Into<String>, tier: &'static str) -> Verdict {
    Verdict {
        verdict,
        reason: reason.into(),
        tier,
    }
}

type PyResult<T> = Result<T, &'static str>;

const READ_ONLY_TOOLS: &[&str] = &[
    "Read",
    "Glob",
    "Grep",
    "NotebookRead",
    "TodoWrite",
    "ListAgents",
];
const PATH_REQUIRED: &[&str] = &["Read", "NotebookRead"];
const EDIT_TOOLS: &[&str] = &["Edit", "Write", "NotebookEdit", "MultiEdit"];

const EDGE: &str = r#"(^|[\s\\/=:"'])"#;
const END: &str = r#"($|\s|["'])"#;

fn sensitive_src() -> Vec<String> {
    vec![
        format!(r"{EDGE}\.env(\.|{END})"),
        format!(r"{EDGE}\.git[\\/]config{END}"),
        format!(r"{EDGE}\.ssh[\\/]"),
        format!(r"{EDGE}\.aws[\\/]"),
        format!(r"{EDGE}\.npmrc{END}"),
        format!(r"{EDGE}\.netrc{END}"),
        format!(r"{EDGE}id_(rsa|ed25519|ecdsa)"),
        format!(r"\.pem{END}"),
        format!(r"\.pfx{END}"),
        format!(r"\.p12{END}"),
        format!(r"{EDGE}credentials(\.|{END})"),
        format!(r"{EDGE}secrets?(\.|[\\/]|{END})"),
    ]
}

fn self_protect_src() -> Vec<String> {
    vec![
        format!(r"{EDGE}\.claude[\\/]settings.*\.json{END}"),
        format!(r"{EDGE}policy\.json{END}"),
        format!(r"{EDGE}hooks[\\/]cc_.*\.py{END}"),
        format!(r"{EDGE}imperatorium[\\/]policy\.py{END}"),
        format!(r"{EDGE}ccontrol[\\/]policy\.py{END}"),
        // The native port: this file, and the installed binary that runs it.
        format!(r"{EDGE}imperatorium[\\/]src[\\/]policy\.rs{END}"),
        format!(r"{EDGE}Programs[\\/]imperatorium[\\/]"),
        format!(r"{EDGE}\.local[\\/]bin[\\/]imperatorium"),
        r"--dangerously-skip-permissions".into(),
        r"\bcrossSessionInbound\b".into(),
    ]
}

const NEVER_AUTO: &[(&str, &str)] = &[
    (
        r"\brm\s+(-[a-zA-Z]*\s+)*-[a-zA-Z]*[rf]",
        "recursive or forced delete",
    ),
    (r"\bgit\s+push\b", "pushes to a remote"),
    (r"\bgit\s+reset\s+--hard\b", "discards work irreversibly"),
    (r"\bgit\s+clean\b", "deletes untracked files"),
    (r"\bgit\s+checkout\s+--\s", "discards local changes"),
    (r"--force\b|--hard\b|\s-f\b", "forcing"),
    (r"\bsudo\b|\brunas\b", "elevates"),
    (
        r"\b(curl|wget|iwr|Invoke-WebRequest)\b.*\|\s*(sh|bash|pwsh|powershell)",
        "pipes a download into a shell",
    ),
    (
        r"\b(shutdown|reboot|mkfs|diskpart|format)\b",
        "touches the machine",
    ),
    (
        r"\b(npm|pnpm|yarn|pip|uv|cargo|gem)\s+(i|install|add|publish)\b",
        "installs or publishes packages",
    ),
    (
        r"\bschtasks\b|\bRegister-ScheduledTask\b|\bsc\s+(create|config)\b",
        "changes scheduled tasks or services",
    ),
    (
        r"\breg\s+(add|delete)\b|\bSet-ItemProperty\b.*HK(LM|CU)",
        "writes the registry",
    ),
    (
        r"\bchmod\s+[0-7]*7[0-7]{2}\b",
        "makes something world-writable",
    ),
    (r">\s*/dev/sd|\bdd\s+if=", "writes a raw device"),
    (r"\bkill\b|\bStop-Process\b|\btaskkill\b", "kills processes"),
    (
        r"\bclaude\b.*--dangerously-skip-permissions",
        "disables permission checks",
    ),
];

const SAFE_BASH: &[&str] = &[
    r"git\s+(status|diff|log|show|branch|remote|stash\s+list|rev-parse|describe)\b",
    r"git\s+(config\s+--get|ls-files)\b",
    r"(ls|dir|pwd|cd|echo|whoami|hostname|date|uname)\b",
    r"(cat|head|tail|wc|file|stat|find|which|where|type)\b",
    r"(grep|rg|ag|fgrep|egrep)\b",
    r"(python|python3|py)\s+-m\s+(pytest|unittest)\b",
    r"(pytest|tox|nox)\b",
    r"(npm|pnpm|yarn)\s+(test|run\s+(test|lint|typecheck|build))\b",
    // Not `fmt`: formatting rewrites source files, which is an edit.
    r"(cargo|go)\s+(test|build|vet)\b",
    r"dotnet\s+(test|build)\b",
    r"(node|python|python3|py)\s+--version\b",
    r"claude\s+agents\s+--json\b",
];

const UNSAFE_OPTIONS: &[(&str, &str, &str)] = &[
    (
        r"find\b",
        r"\s-(delete|exec|execdir|ok|okdir|fprint0?|fprintf|fls)\b",
        "find that deletes, runs a command, or writes a file",
    ),
    (
        r"git\s+(diff|log|show)\b",
        r"\s--output\b",
        "writes its output to a file",
    ),
    (r"(rg)\b", r"\s--pre\b", "runs a preprocessor program"),
    (r"date\b", r"\s(-s|--set)\b", "sets the clock"),
];

const BRANCH_LISTING: &[&str] = &[
    "-a",
    "--all",
    "-r",
    "--remotes",
    "-v",
    "-vv",
    "--verbose",
    "-l",
    "--list",
    "--show-current",
    "--contains",
    "--no-contains",
    "--merged",
    "--no-merged",
    "--points-at",
    "--sort",
    "--format",
    "--column",
    "--no-column",
    "--color",
    "--no-color",
    "-i",
    "--ignore-case",
    "--abbrev",
    "--no-abbrev",
];
const BRANCH_TAKES_VALUE: &[&str] = &[
    "-l",
    "--list",
    "--contains",
    "--no-contains",
    "--merged",
    "--no-merged",
    "--points-at",
    "--sort",
    "--format",
];

/// `re.search(p, s, re.I)`.
fn search(pattern: &str) -> PyResult<Regex> {
    Regex::new(&format!("(?i){}", py_regex(pattern))).map_err(|_| "error")
}

/// `re.match(p, s, re.I)`: anchored at the start only.
fn matcher(pattern: &str) -> PyResult<Regex> {
    Regex::new(&format!("(?i)^(?:{})", py_regex(pattern))).map_err(|_| "error")
}

struct Compiled {
    sensitive: Vec<Regex>,
    self_protect: Vec<Regex>,
    never_auto: Vec<(Regex, &'static str)>,
    safe_bash: Vec<Regex>,
    unsafe_options: Vec<(Regex, Regex, &'static str)>,
    remote_listing: Regex,
    git_remote: Regex,
    git_branch: Regex,
    split: Regex,
    subshell: Regex,
    redirect: Regex,
    env_prefix: Regex,
}

fn compiled() -> &'static Compiled {
    static C: OnceLock<Compiled> = OnceLock::new();
    C.get_or_init(|| {
        let all =
            |src: Vec<String>| -> Vec<Regex> { src.iter().map(|p| search(p).unwrap()).collect() };
        Compiled {
            sensitive: all(sensitive_src()),
            self_protect: all(self_protect_src()),
            never_auto: NEVER_AUTO
                .iter()
                .map(|(p, w)| (search(p).unwrap(), *w))
                .collect(),
            safe_bash: SAFE_BASH.iter().map(|p| matcher(p).unwrap()).collect(),
            unsafe_options: UNSAFE_OPTIONS
                .iter()
                .map(|(verb, p, w)| (matcher(verb).unwrap(), search(p).unwrap(), *w))
                .collect(),
            remote_listing: Regex::new(&format!(
                "(?i){}",
                py_regex(r"^git\s+remote(\s+(-v|--verbose))?(\s+(show|get-url)(\s+\S+)*)?\s*$")
            ))
            .unwrap(),
            git_remote: matcher(r"git\s+remote\b").unwrap(),
            git_branch: matcher(r"git\s+branch\b").unwrap(),
            split: Regex::new(r"&&|\|\||\||;|\n").unwrap(),
            subshell: Regex::new(r"\$\(|`|<\(").unwrap(),
            redirect: Regex::new(&py_regex(r"(^|\s)>{1,2}(\s|$)|(^|\s)\d?>&")).unwrap(),
            // ASCII names only: see the module note.
            env_prefix: Regex::new(&py_regex(r"^([A-Za-z0-9_]+=\S*\s+)+")).unwrap(),
        }
    })
}

fn any_match(patterns: &[Regex], text: &str) -> bool {
    patterns.iter().any(|r| r.is_match(text))
}

/// True for a path no rule may auto-approve (asked about, not refused).
pub fn is_sensitive(path: Option<&Value>) -> bool {
    match path {
        Some(p) if truthy(Some(p)) => {
            any_match(&compiled().sensitive, &py_str(p).replace('\\', "/"))
        }
        _ => false,
    }
}

/// True for this system's own controls, which are refused outright.
pub fn is_self_protected(text: Option<&Value>) -> bool {
    match text {
        Some(t) if truthy(Some(t)) => {
            any_match(&compiled().self_protect, &py_str(t).replace('\\', "/"))
        }
        _ => false,
    }
}

fn unsafe_option(segment: &str) -> Option<&'static str> {
    let c = compiled();
    for (verb, pattern, why) in &c.unsafe_options {
        if verb.is_match(segment) && pattern.is_match(segment) {
            return Some(why);
        }
    }
    if c.git_remote.is_match(segment) && !c.remote_listing.is_match(segment) {
        return Some("changes a git remote");
    }
    if c.git_branch.is_match(segment) {
        let tokens = split_ws(segment);
        let mut expect_value = false;
        for token in tokens.iter().skip(2) {
            if expect_value {
                expect_value = false;
                continue;
            }
            let name = token.split('=').next().unwrap_or("");
            if BRANCH_LISTING.contains(&name) {
                expect_value = BRANCH_TAKES_VALUE.contains(&name) && !token.contains('=');
                continue;
            }
            return Some("creates, deletes or renames a branch");
        }
    }
    None
}

fn segments(command: &str) -> Vec<&str> {
    compiled()
        .split
        .split(command)
        .map(strip)
        .filter(|s| !s.is_empty())
        .collect()
}

/// `_inside(path, root)`: True when path resolves inside root.
fn inside(path: Option<&Value>, root: Option<&Value>) -> bool {
    let (Some(p), Some(r)) = (path, root) else {
        return false;
    };
    if !truthy(Some(p)) || !truthy(Some(r)) {
        return false;
    }
    let (Some(pa), Some(ra)) = (
        paths::abs_normcase(&py_str(p)),
        paths::abs_normcase(&py_str(r)),
    ) else {
        return false;
    };
    pa == ra || pa.starts_with(&format!("{ra}{}", paths::SEP))
}

/// The `tuple(policy.get("safe_bash") or ())` extras: iterated as Python
/// would (a string gives its characters, an object its keys).
fn extra_patterns(policy: &Map<String, Value>) -> PyResult<Vec<Value>> {
    match policy.get("safe_bash") {
        v if !truthy(v) => Ok(vec![]),
        Some(Value::Array(a)) => Ok(a.clone()),
        Some(Value::String(s)) => Ok(s.chars().map(|c| Value::String(c.to_string())).collect()),
        Some(Value::Object(o)) => Ok(o.keys().map(|k| Value::String(k.clone())).collect()),
        _ => Err("TypeError"),
    }
}

fn begins_safely(segment: &str, extra: &[Value]) -> PyResult<bool> {
    if compiled().safe_bash.iter().any(|r| r.is_match(segment)) {
        return Ok(true);
    }
    for pattern in extra {
        let Value::String(p) = pattern else {
            return Err("TypeError");
        };
        if matcher(p)?.is_match(segment) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn bash_verdict(command: &str, policy: &Map<String, Value>) -> PyResult<Verdict> {
    let c = compiled();
    if strip(command).is_empty() {
        return Ok(v(ASK, "empty command", "unknown"));
    }
    if c.subshell.is_match(command) {
        return Ok(v(ASK, "uses command substitution", "opaque"));
    }
    if c.redirect.is_match(command) {
        return Ok(v(ASK, "redirects output", "mutating"));
    }
    for (pattern, why) in &c.never_auto {
        if pattern.is_match(command) {
            return Ok(v(ASK, *why, "consequential"));
        }
    }
    let segs = segments(command);
    if segs.is_empty() {
        return Ok(v(ASK, "nothing to check", "unknown"));
    }
    let extra = extra_patterns(policy)?;
    for segment in segs {
        let stripped_owned = c.env_prefix.replace(segment, "");
        let stripped = strip(&stripped_owned);
        if !begins_safely(stripped, &extra)? {
            return Ok(v(
                ASK,
                format!("not a known read-only command: {}", take(stripped, 60)),
                "unknown",
            ));
        }
        if let Some(why) = unsafe_option(stripped) {
            return Ok(v(ASK, why, "consequential"));
        }
    }
    Ok(v(ALLOW, "read-only shell", "read"))
}

/// Iterate a value as Python's `for x in value` would, or raise.
fn py_iter(value: &Value) -> PyResult<Vec<Value>> {
    match value {
        Value::Array(a) => Ok(a.clone()),
        Value::String(s) => Ok(s.chars().map(|c| Value::String(c.to_string())).collect()),
        Value::Object(o) => Ok(o.keys().map(|k| Value::String(k.clone())).collect()),
        _ => Err("TypeError"),
    }
}

/// `_paths_in`: every path the call names, in Python's order. Python is a
/// generator, so a malformed `file_paths`/`edits` raises only once the
/// consumer reaches it; the error is returned alongside what came before.
fn paths_in(tool_input: &Map<String, Value>) -> (Vec<Value>, Option<&'static str>) {
    let mut out = Vec::new();
    for key in ["file_path", "path", "notebook_path", "filePath"] {
        if let Some(value) = tool_input.get(key).filter(|v| truthy(Some(v))) {
            out.push(value.clone());
        }
    }
    if let Some(fp) = tool_input.get("file_paths").filter(|v| truthy(Some(v))) {
        match py_iter(fp) {
            Ok(items) => out.extend(items),
            Err(e) => return (out, Some(e)),
        }
    }
    if let Some(edits) = tool_input.get("edits").filter(|v| truthy(Some(v))) {
        match py_iter(edits) {
            Ok(items) => {
                for edit in items {
                    if let Value::Object(e) = &edit {
                        if let Some(fp) = e.get("file_path").filter(|v| truthy(Some(v))) {
                            out.push(fp.clone());
                        }
                    }
                }
            }
            Err(e) => return (out, Some(e)),
        }
    }
    (out, None)
}

fn all_paths(tool_input: &Map<String, Value>) -> PyResult<Vec<Value>> {
    match paths_in(tool_input) {
        (p, None) => Ok(p),
        (_, Some(e)) => Err(e),
    }
}

/// Return the verdict for one PermissionRequest. Never fails: an error is
/// an `ask`, as in Python.
pub fn decide(request: &Value, policy: &Value) -> Verdict {
    let empty = Value::Object(Map::new());
    // `policy = policy or {}`
    let policy = if truthy(Some(policy)) { policy } else { &empty };
    match decide_inner(request, policy) {
        Ok(verdict) => verdict,
        Err(name) => v(ASK, format!("policy error ({name})"), "error"),
    }
}

fn decide_inner(request: &Value, policy: &Value) -> PyResult<Verdict> {
    let Value::Object(policy) = policy else {
        return Err("AttributeError");
    };
    if !truthy(policy.get("enabled")) {
        return Ok(v(ASK, "policy disabled", "off"));
    }
    let Value::Object(request) = request else {
        return Err("AttributeError");
    };

    let empty_str = Value::String(String::new());
    let tool_v = or(request.get("tool_name"), Some(&empty_str)).unwrap();
    let tool_input = match request.get("tool_input") {
        Some(Value::Object(o)) => o.clone(),
        _ => Map::new(),
    };
    let cwd = or(request.get("cwd"), Some(&empty_str)).unwrap();

    let project = project_policy(policy, cwd)?;
    if truthy(project.get("never_auto_approve")) {
        return Ok(v(ASK, "project opts out", "off"));
    }

    let (paths, deferred) = paths_in(&tool_input);
    for path in &paths {
        if is_self_protected(Some(path)) {
            return Ok(v(DENY, "would change this system's own controls", "self"));
        }
        if is_sensitive(Some(path)) {
            return Ok(v(
                ASK,
                format!("names a secret: {}", paths::basename(&py_str(path))),
                "secret",
            ));
        }
    }
    if let Some(e) = deferred {
        return Err(e);
    }

    // A list or dict tool passes `tool in ("Bash", ...)` (a tuple compares
    // by ==) but raises at `tool in READ_ONLY_TOOLS` (a set: unhashable).
    let tool: Option<&str> = tool_v.as_str();

    if matches!(tool, Some("Bash") | Some("PowerShell")) {
        let command_v = or(tool_input.get("command"), Some(&empty_str)).unwrap();
        if is_self_protected(Some(command_v)) {
            return Ok(v(DENY, "would change this system's own controls", "self"));
        }
        let Value::String(command) = command_v else {
            return Err("AttributeError");
        };
        if any_match(&compiled().sensitive, &command.replace('\\', "/")) {
            return Ok(v(ASK, "command names a secret", "secret"));
        }
        let mut merged = policy.clone();
        for (k, val) in &project {
            merged.insert(k.clone(), val.clone());
        }
        return bash_verdict(command, &merged);
    }

    if matches!(tool_v, Value::Array(_) | Value::Object(_)) {
        return Err("TypeError"); // unhashable in `tool in READ_ONLY_TOOLS`
    }

    if let Some(t) = tool.filter(|t| READ_ONLY_TOOLS.contains(t)) {
        let paths = all_paths(&tool_input)?;
        if PATH_REQUIRED.contains(&t) && paths.is_empty() {
            return Ok(v(ASK, format!("{t} without a path"), "unknown"));
        }
        for path in &paths {
            if !inside(Some(path), Some(cwd)) && !truthy(project.get("allow_reads_outside_cwd")) {
                return Ok(v(ASK, "reads outside the project", "read"));
            }
        }
        return Ok(v(ALLOW, "read-only tool", "read"));
    }

    if tool.is_some_and(|t| EDIT_TOOLS.contains(&t)) {
        if !truthy(project.get("auto_edit")) {
            return Ok(v(ASK, "edits need auto_edit for this project", "write"));
        }
        let paths = all_paths(&tool_input)?;
        if paths.is_empty() {
            return Ok(v(ASK, "edit with no path", "write"));
        }
        for path in &paths {
            if !inside(Some(path), Some(cwd)) {
                return Ok(v(ASK, "edits outside the project", "write"));
            }
        }
        return Ok(v(ALLOW, "edit inside an opted-in project", "write"));
    }

    let named = if truthy(Some(tool_v)) {
        py_str(tool_v)
    } else {
        "an unnamed tool".into()
    };
    Ok(v(ASK, format!("no rule for {named}"), "unknown"))
}

/// `dict(value)` for a non-dict: a sequence of 2-item pairs, else the
/// exception Python raises.
fn py_dict(value: &Value) -> PyResult<Map<String, Value>> {
    let mut out = Map::new();
    for item in py_iter(value)? {
        let pair = match &item {
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::Array(a) => a.clone(),
            Value::Object(o) => o.keys().map(|k| Value::String(k.clone())).collect(),
            _ => return Err("TypeError"),
        };
        if pair.len() != 2 {
            return Err("ValueError");
        }
        match &pair[0] {
            // A non-string key cannot reach `.get("auto_edit")`-style lookups;
            // Python keeps it, and it never matches, so it can be dropped.
            Value::String(k) => {
                out.insert(k.clone(), pair[1].clone());
            }
            Value::Array(_) | Value::Object(_) => return Err("TypeError"),
            _ => {}
        }
    }
    Ok(out)
}

/// The `projects` entry whose path contains cwd, merged over defaults.
fn project_policy(policy: &Map<String, Value>, cwd: &Value) -> PyResult<Map<String, Value>> {
    let mut merged = match policy.get("defaults") {
        d if !truthy(d) => Map::new(),
        Some(Value::Object(o)) => o.clone(),
        Some(other) => py_dict(other)?,
        None => Map::new(),
    };
    let projects = match policy.get("projects") {
        p if !truthy(p) => return Ok(merged),
        Some(Value::Object(o)) => o,
        _ => return Err("AttributeError"),
    };
    let mut best: Option<&Value> = None;
    let mut best_len: i64 = -1;
    for (path, settings) in projects {
        let path_v = Value::String(path.clone());
        let len = path.chars().count() as i64;
        if inside(Some(cwd), Some(&path_v)) && len > best_len {
            best = Some(settings);
            best_len = len;
        }
    }
    if let Some(Value::Object(b)) = best {
        for (k, val) in b {
            merged.insert(k.clone(), val.clone());
        }
    }
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bash(cmd: &str) -> Value {
        json!({"tool_name": "Bash", "tool_input": {"command": cmd}, "cwd": "/w/p"})
    }
    const ON: fn() -> Value = || json!({"enabled": true, "defaults": {}});

    #[test]
    fn disabled_asks() {
        assert_eq!(
            decide(&bash("git status"), &json!({"enabled": false})).verdict,
            ASK
        );
        assert_eq!(decide(&bash("git status"), &Value::Null).verdict, ASK);
    }

    #[test]
    fn safe_and_unsafe_shell() {
        assert_eq!(decide(&bash("git status"), &ON()).verdict, ALLOW);
        assert_eq!(decide(&bash("git status; rm -rf /"), &ON()).verdict, ASK);
        assert_eq!(decide(&bash("find . -delete"), &ON()).verdict, ASK);
        assert_eq!(decide(&bash("git branch -D x"), &ON()).verdict, ASK);
        assert_eq!(
            decide(&bash("git branch --list 'x*'"), &ON()).verdict,
            ALLOW
        );
        assert_eq!(decide(&bash("FOO=1 pytest -q"), &ON()).verdict, ALLOW);
        assert_eq!(decide(&bash("echo $(rm x)"), &ON()).verdict, ASK);
        assert_eq!(decide(&bash("cat .env"), &ON()).verdict, ASK);
        assert_eq!(decide(&bash("git\u{1c}push"), &ON()).verdict, ASK);
    }

    #[test]
    fn self_protection_denies() {
        let r = decide(&bash("notepad policy.json"), &ON());
        assert_eq!((r.verdict, r.tier), (DENY, "self"));
        let r = decide(
            &json!({"tool_name": "Edit", "tool_input": {"file_path": "C:/x/imperatorium/rust/imperatorium/src/policy.rs"}}),
            &ON(),
        );
        assert_eq!(r.verdict, DENY);
    }

    #[test]
    fn errors_ask() {
        let r = decide(
            &json!({"tool_name": "Bash", "tool_input": {"command": ["ls"]}}),
            &ON(),
        );
        assert_eq!(
            (r.verdict, r.reason.as_str()),
            (ASK, "policy error (AttributeError)")
        );
        let r = decide(&json!({"tool_name": ["Read"], "tool_input": {}}), &ON());
        assert_eq!(r.reason, "policy error (TypeError)");
    }
}

/// test_policy.py's direct calls (its `decide` calls are replayed from the
/// Python goldens by tests/policy_tests.rs).
#[cfg(test)]
mod py_ported {
    use super::*;
    use serde_json::json;

    #[test]
    fn is_sensitive_handles_both_separators_and_junk() {
        assert!(is_sensitive(Some(&json!("a/.env"))));
        assert!(is_sensitive(Some(&json!("a\\.env"))));
        assert!(!is_sensitive(Some(&json!(""))));
        assert!(!is_sensitive(None));
        assert!(!is_sensitive(Some(&Value::Null)));
        assert!(!is_sensitive(Some(&json!("environment.md"))));
    }
}

/// test_rename.py: the policy code is protected under either name.
#[cfg(test)]
mod core_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_policy_code_is_protected_under_either_name() {
        let on = json!({"enabled": true, "defaults": {}});
        for path in ["imperatorium/policy.py", r"imperatorium\policy.py", "ccontrol/policy.py"] {
            let req = json!({"tool_name": "Bash", "tool_input": {"command": format!("edit {path}")},
                             "cwd": "/w/p"});
            let r = decide(&req, &on);
            assert_eq!((r.verdict, r.tier), (DENY, "self"), "{path}");
        }
    }
}
