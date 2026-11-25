//! The Claude Code hooks: ports of `hooks/cc_hook.py`, `cc_statusline.py`
//! and `cc_permission.py`, behaviour for behaviour.
//!
//! None of them may ever break a session: every failure is swallowed, the
//! exit code is always 0, and stdout carries only what Claude Code reads.
//!
//! One difference, a fix: Python reads hook stdin in the locale's encoding
//! (cp1252 on Windows), which garbles any non-ASCII text Claude Code sends
//! as UTF-8. These read UTF-8.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::http::{env_timeout, post_json};
use crate::paths::{self, env_nonempty};
use crate::policy::{self, ASK};
use crate::pyish::{
    json_dumps, loads_or_empty, or, py_float, py_str, take, truthy,
};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8792/hook";
const DEFAULT_PARK: &str = "http://127.0.0.1:8792/permission/park";

fn read_stdin() -> Option<String> {
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn env_value(name: &str) -> Value {
    match std::env::var(name) {
        Ok(v) => Value::String(v),
        Err(_) => Value::Null,
    }
}

/// IMPERATORIUM_DEBUG_LOG: a hook that fails invisibly is unfalsifiable.
fn note(line: &str) {
    let Some(path) = env_nonempty("IMPERATORIUM_DEBUG_LOG") else {
        return;
    };
    let stamp = paths::local_stamp();
    if let Ok(mut fh) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(fh, "{stamp} {line}");
    }
}

// -- cc_hook.py ---------------------------------------------------------------

const TOOL_EVENTS: &[&str] = &["PreToolUse", "PostToolUse", "PostToolUseFailure"];
const SUMMARY_KEYS: &[&str] = &[
    "command",
    "file_path",
    "path",
    "pattern",
    "url",
    "prompt",
    "query",
    "description",
];

/// Only what the activity view shows, never a file's contents.
pub fn trim_tool_payload(payload: &Map<String, Value>) -> Map<String, Value> {
    let mut keep = Map::new();
    for k in [
        "hook_event_name",
        "session_id",
        "cwd",
        "tool_name",
        "tool_use_id",
        "agent_id",
        "error",
    ] {
        keep.insert(k.into(), payload.get(k).cloned().unwrap_or(Value::Null));
    }
    if let Some(Value::Object(tool_input)) = payload.get("tool_input") {
        let mut summary = Map::new();
        for k in SUMMARY_KEYS {
            if let Some(val) = tool_input.get(*k).filter(|v| truthy(Some(v))) {
                summary.insert((*k).into(), Value::String(take(&py_str(val), 300)));
            }
        }
        keep.insert("tool_input".into(), Value::Object(summary));
    }
    if payload.get("hook_event_name").and_then(Value::as_str) == Some("PostToolUseFailure") {
        let response = payload.get("tool_response");
        let trimmed = match response {
            r if !truthy(r) => Value::Null,
            Some(Value::String(s)) => Value::String(take(s, 500)),
            Some(other) => Value::String(take(&json_dumps(other, true), 500)),
            None => Value::Null,
        };
        keep.insert("tool_response".into(), trimmed);
    }
    keep
}

pub fn hook_main() -> i32 {
    let Some(raw) = read_stdin() else { return 0 };
    let Some(Value::Object(mut payload)) = loads_or_empty(&raw) else {
        return 0;
    };

    let event = payload
        .get("hook_event_name")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if event.as_deref().is_some_and(|e| TOOL_EVENTS.contains(&e)) {
        payload = trim_tool_payload(&payload);
    }
    for (key, var) in [
        ("messaging_socket", "CLAUDE_CODE_MESSAGING_SOCKET"),
        ("messaging_token", "CLAUDE_CODE_MESSAGING_TOKEN"),
        ("bridge_session_id", "CLAUDE_CODE_BRIDGE_SESSION_ID"),
        ("claude_pid", "CLAUDE_PID"),
    ] {
        payload.insert(key.into(), env_value(var));
    }

    let endpoint =
        std::env::var("IMPERATORIUM_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.into());
    let body = json_dumps(&Value::Object(payload.clone()), true);
    if let Err(e) = post_json(&endpoint, &body, env_timeout("IMPERATORIUM_TIMEOUT", 3.0)) {
        let name = payload
            .get("hook_event_name")
            .map(py_str)
            .unwrap_or_else(|| "None".into());
        note(&format!("{name} error: {e}"));
    }
    0
}

// -- cc_statusline.py ---------------------------------------------------------

const STATUSLINE_EVENT: &str = "imperatorium_statusline";

/// Where this session's requests go, or None for the claude.ai login.
pub fn provider(get: impl Fn(&str) -> Option<String>) -> Option<&'static str> {
    for (name, value) in [
        ("CLAUDE_CODE_USE_BEDROCK", "bedrock"),
        ("CLAUDE_CODE_USE_VERTEX", "vertex"),
        ("CLAUDE_CODE_USE_FOUNDRY", "foundry"),
    ] {
        let flag = get(name).unwrap_or_default();
        if ["1", "true", "yes", "on"].contains(&crate::pyish::strip(&flag).to_lowercase().as_str())
        {
            return Some(value);
        }
    }
    if get("ANTHROPIC_BASE_URL").is_some_and(|v| !v.is_empty()) {
        return Some("gateway");
    }
    if get("ANTHROPIC_API_KEY").is_some_and(|v| !v.is_empty())
        || get("ANTHROPIC_AUTH_TOKEN").is_some_and(|v| !v.is_empty())
    {
        return Some("api");
    }
    None
}

pub fn statusline_main() -> i32 {
    let raw = read_stdin().unwrap_or_default();
    let mut payload = match loads_or_empty(&raw) {
        Some(Value::Object(o)) => o,
        _ => Map::new(),
    };
    if let Some(found) = provider(|n| std::env::var(n).ok()) {
        payload.insert("imperatorium_provider".into(), Value::String(found.into()));
    }
    if truthy(payload.get("session_id")) {
        let mut body = payload.clone();
        body.insert(
            "hook_event_name".into(),
            Value::String(STATUSLINE_EVENT.into()),
        );
        let endpoint =
            std::env::var("IMPERATORIUM_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.into());
        let _ = post_json(
            &endpoint,
            &json_dumps(&Value::Object(body), true),
            env_timeout("IMPERATORIUM_STATUSLINE_TIMEOUT", 0.5),
        );
    }
    // Nothing is printed: the terminal's status line stays empty, and the
    // window shows what it would have said (2026-11-24, the user's call).
    0
}

// -- cc_permission.py ---------------------------------------------------------

const AUDIT_MAX_BYTES: u64 = 2 * 1024 * 1024;
const QUESTION_TOOL: &str = "AskUserQuestion";
const QUESTION_WAIT: f64 = 120.0;

fn load_policy(home: Option<&str>) -> Value {
    // No policy, or a broken one, means no auto-approval.
    fs::read_to_string(paths::policy_path(home))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn audit(record: Value) {
    let dir = paths::state_dir();
    let path = dir.join("decisions.jsonl");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if fs::metadata(&path)
        .map(|m| m.len() > AUDIT_MAX_BYTES)
        .unwrap_or(false)
    {
        let mut rotated = path.clone().into_os_string();
        rotated.push(".1");
        let _ = fs::rename(&path, Path::new(&rotated));
    }
    if let Ok(mut fh) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(fh, "{}", json_dumps(&record, false));
    }
}

/// Offer the decision to the window; its reply, or an empty object.
fn park(
    policy: &Value,
    request: &Map<String, Value>,
    reason: &str,
    detail: &str,
) -> Map<String, Value> {
    let empty = Map::new();
    let settings = match policy.get("park") {
        Some(Value::Object(o)) => o,
        p if !truthy(p) => &empty,
        _ => return Map::new(), // Python: AttributeError, the hook dies silently
    };
    if !truthy(settings.get("enabled")) {
        return Map::new();
    }
    // Held until answered unless the policy sets a limit (2026-11-15: the
    // window's card stays; "pass" sends it to the terminal).
    // Only a missing (or null) wait is no limit; an explicit 0 still means
    // "do not hold it".
    let forever = json!(crate::pending::FOREVER);
    let set = settings.get("wait_seconds").filter(|v| !v.is_null()).unwrap_or(&forever);
    let Some(wait) = py_float(or(Some(set), Some(&json!(0))).unwrap()) else {
        return Map::new();
    };
    let wait_v = json!(wait);
    let Some(wait_bg) =
        py_float(or(settings.get("background_wait_seconds"), Some(&wait_v)).unwrap())
    else {
        return Map::new();
    };
    let longest = wait.max(wait_bg);
    if longest <= 0.0 {
        return Map::new();
    }
    let qdefault = json!(QUESTION_WAIT);
    let Some(wait_q) =
        py_float(or(settings.get("question_wait_seconds"), Some(&qdefault)).unwrap())
    else {
        return Map::new();
    };
    let wait_q = wait_q.min(longest);

    let mut payload = request.clone();
    payload.insert("_reason".into(), json!(reason));
    payload.insert("_detail".into(), json!(detail));
    payload.insert("wait".into(), json!(wait));
    payload.insert("wait_background".into(), json!(wait_bg));
    payload.insert("wait_question".into(), json!(wait_q));
    // So the dispatcher can drop the card once this process is gone (Claude
    // Code ends it when the person interrupts).
    payload.insert("_hook_pid".into(), json!(std::process::id()));
    let endpoint =
        std::env::var("IMPERATORIUM_PARK_ENDPOINT").unwrap_or_else(|_| DEFAULT_PARK.into());
    let timeout = Duration::from_secs_f64((longest + 10.0).min(crate::pending::FOREVER + 60.0));
    match post_json(
        &endpoint,
        &json_dumps(&Value::Object(payload), true),
        timeout,
    ) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(answer)) => answer,
            Ok(_) => Map::new(),
            Err(e) => {
                note(&format!(
                    "park failed, falling through to the terminal: {e}"
                ));
                Map::new()
            }
        },
        Err(e) => {
            note(&format!(
                "park failed, falling through to the terminal: {e}"
            ));
            Map::new()
        }
    }
}

/// The JSON printed for an allow or deny. See cc_permission.reply.
pub fn reply(
    verdict: &str,
    reason: &str,
    fmt: Option<&str>,
    updated_input: Option<Value>,
) -> Value {
    if fmt == Some("object") {
        let mut decision = Map::new();
        decision.insert("behavior".into(), json!(verdict));
        if verdict == "allow" {
            if let Some(u) = updated_input {
                decision.insert("updatedInput".into(), u);
            }
        }
        if verdict == "deny" && !reason.is_empty() {
            decision.insert("message".into(), json!(reason));
        }
        return json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": decision}});
    }
    json!({"hookSpecificOutput": {
        "hookEventName": "PermissionRequest",
        "decision": verdict,
        "permissionDecision": verdict,
        "reason": reason,
        "permissionDecisionReason": reason,
    }})
}

/// The policy a remote host's hook runs (`permission --park-only`): no
/// rules of its own - auto-approval is this machine's - so every request
/// the session would put to the person is offered to the window here.
fn park_only_policy() -> Value {
    json!({"enabled": false, "decision_format": "object", "park": {"enabled": true}})
}

pub fn permission_main(home: Option<&str>, park_only: bool) -> i32 {
    let Some(raw) = read_stdin() else { return 0 };
    let Some(Value::Object(request)) = loads_or_empty(&raw) else {
        return 0;
    };
    let policy = if park_only { park_only_policy() } else { load_policy(home) };
    let request_v = Value::Object(request.clone());
    let decided = policy::decide(&request_v, &policy);
    let reason = if park_only { String::new() } else { decided.reason.clone() };
    let tier = decided.tier;
    let mut verdict: &str = if park_only { ASK } else { decided.verdict };

    let tool_input = request.get("tool_input");
    let mut detail = String::new();
    if let Some(Value::Object(ti)) = tool_input {
        let empty = json!("");
        let first = or(
            or(ti.get("command"), ti.get("file_path")),
            or(ti.get("pattern"), Some(&empty)),
        )
        .unwrap();
        detail = take(&py_str(first), 200);
    }
    let field = |k: &str| request.get(k).cloned().unwrap_or(Value::Null);

    audit(json!({
        "at": now(), "verdict": verdict, "reason": reason, "tier": tier,
        "tool": field("tool_name"), "detail": detail, "cwd": field("cwd"),
        "session_id": field("session_id"),
    }));

    let fmt = policy
        .get("decision_format")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let question = request.get("tool_name").and_then(Value::as_str) == Some(QUESTION_TOOL);
    let mut updated: Option<Value> = None;
    if verdict == ASK && question && fmt.as_deref() != Some("object") {
        return 0;
    }
    if verdict == ASK {
        let answer = park(&policy, &request, &reason, &detail);
        let parked = answer
            .get("verdict")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if question && parked.as_deref() == Some("allow") {
            match (answer.get("answers"), tool_input) {
                (Some(Value::Object(answers)), Some(Value::Object(ti))) if !answers.is_empty() => {
                    let mut u = ti.clone();
                    u.insert("answers".into(), Value::Object(answers.clone()));
                    updated = Some(Value::Object(u));
                }
                _ => return 0,
            }
        }
        match parked.as_deref() {
            Some(p @ ("allow" | "deny")) => {
                audit(json!({
                    "at": now(), "verdict": p, "reason": "answered in the window", "tier": tier,
                    "tool": field("tool_name"), "detail": detail, "cwd": field("cwd"),
                    "session_id": field("session_id"),
                }));
                verdict = if p == "allow" { "allow" } else { "deny" };
            }
            // "ask" is expressed by saying nothing.
            _ => return 0,
        }
    }
    let out = reply(verdict, &reason, fmt.as_deref(), updated);
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(json_dumps(&out, true).as_bytes());
    let _ = stdout.flush();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn tool_payloads_lose_file_contents() {
        let out = trim_tool_payload(&obj(json!({
            "hook_event_name": "PreToolUse", "session_id": "s", "tool_name": "Write",
            "tool_use_id": "t", "tool_input": {"file_path": "a.py", "content": "x".repeat(100_000)}})));
        assert_eq!(out["tool_input"], json!({"file_path": "a.py"}));
        assert!(json_dumps(&Value::Object(out), true).len() < 1000);
    }

    #[test]
    fn a_failure_keeps_a_short_reason() {
        let out = trim_tool_payload(&obj(json!({
            "hook_event_name": "PostToolUseFailure", "session_id": "s", "tool_name": "Bash",
            "tool_input": {"command": "make"}, "error": "exit 2", "tool_response": "y".repeat(5000)})));
        assert_eq!(out["error"], json!("exit 2"));
        assert_eq!(out["tool_response"].as_str().unwrap().len(), 500);
    }

    #[test]
    fn replies() {
        assert_eq!(
            reply("allow", "safe read", Some("object"), None),
            json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}})
        );
        assert_eq!(
            reply("deny", "touches policy.json", Some("object"), None)["hookSpecificOutput"]
                ["decision"],
            json!({"behavior": "deny", "message": "touches policy.json"})
        );
        assert_eq!(
            reply("allow", "x", None, None)["hookSpecificOutput"]["decision"],
            json!("allow")
        );
        let u = json!({"answers": {"q": "a"}});
        assert!(reply("deny", "no", Some("object"), Some(u))
            .to_string()
            .find("updatedInput")
            .is_none());
    }

    #[test]
    fn providers() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |n: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == n)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            provider(env(&[("CLAUDE_CODE_USE_BEDROCK", "1")])),
            Some("bedrock")
        );
        assert_eq!(provider(env(&[("CLAUDE_CODE_USE_BEDROCK", "0")])), None);
        assert_eq!(provider(env(&[("ANTHROPIC_API_KEY", "k")])), Some("api"));
        assert_eq!(
            provider(env(&[("ANTHROPIC_BASE_URL", "https://gw")])),
            Some("gateway")
        );
        assert_eq!(provider(env(&[])), None);
    }
}

/// Ported from the Python suite (test_features.py `StatuslineProvider`,
/// `StatuslineLine`).
#[cfg(test)]
mod features_tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |n: &str| pairs.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string())
    }

    #[test]
    fn features_cloud_flags_win_in_claude_codes_order() {
        assert_eq!(provider(env(&[("CLAUDE_CODE_USE_BEDROCK", "1")])), Some("bedrock"));
        assert_eq!(
            provider(env(&[("CLAUDE_CODE_USE_VERTEX", "true"), ("ANTHROPIC_API_KEY", "k")])),
            Some("vertex")
        );
        assert_eq!(provider(env(&[("CLAUDE_CODE_USE_FOUNDRY", "1")])), Some("foundry"));
        assert_eq!(
            provider(env(&[("CLAUDE_CODE_USE_FOUNDRY", "1"), ("CLAUDE_CODE_USE_BEDROCK", "1")])),
            Some("bedrock")
        );
    }
}
