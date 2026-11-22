//! The command line: what `impctl.py` did, as `imperatorium` subcommands
//! (docs/native-port.md). Sessions, the queue, approvals and the event log
//! are clients of the dispatcher's HTTP API (`IMPERATORIUM_URL`, default
//! http://127.0.0.1:8792); `start | stop | restart | service` drive the
//! service manager through `Service`; `test` runs `cargo test` in a checkout.
//!
//! Output text and exit codes follow impctl: 0 done, 1 refused or failed, 2
//! a bad command line. Only the program name in messages differs.

use std::time::Duration;

use serde_json::{json, Value};

use crate::http;
use crate::pyish::{float_repr, json_dumps, json_dumps_indent, py_repr, py_str, take, truthy};
use crate::service::Service;
use crate::service_ctl::{Op, Outcome};

pub const DEFAULT_URL: &str = "http://127.0.0.1:8792";
const PROG: &str = "imperatorium";

/// A command that failed with something to say; printed as `imperatorium: MSG`, exit 1.
#[derive(Debug)]
pub struct Fail(pub String);

type Res<T> = Result<T, Fail>;

fn fail<T>(msg: impl Into<String>) -> Res<T> {
    Err(Fail(msg.into()))
}

/// The dispatcher to talk to: IMPERATORIUM_URL, else the default.
pub fn base() -> String {
    base_of(crate::paths::env_nonempty("IMPERATORIUM_URL").as_deref())
}

fn base_of(env: Option<&str>) -> String {
    env.unwrap_or(DEFAULT_URL).trim_end_matches('/').to_string()
}

/// The port of a base URL, for the window launcher.
fn port_of(base: &str) -> String {
    let rest = base.split("://").last().unwrap_or(base);
    let host = rest.split('/').next().unwrap_or("");
    host.rsplit_once(':').map(|(_, p)| p.to_string()).filter(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty()).unwrap_or_else(|| "8792".into())
}

// -- argument parsing --------------------------------------------------------

/// What one command accepts: options with a value and plain flags, each as
/// (short, long); `""` for no short form.
#[derive(Default)]
pub struct Spec {
    pub valued: &'static [(&'static str, &'static str)],
    pub flags: &'static [(&'static str, &'static str)],
}

#[derive(Default, Debug, Clone)]
pub struct Args {
    pub pos: Vec<String>,
    opts: Vec<(String, String)>,
    flags: Vec<String>,
}

impl Args {
    pub fn flag(&self, long: &str) -> bool {
        self.flags.iter().any(|f| f == long)
    }
    /// The last value given for `--long`.
    pub fn opt(&self, long: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(k, _)| k == long).map(|(_, v)| v.as_str())
    }
    pub fn all(&self, long: &str) -> Vec<&str> {
        self.opts.iter().filter(|(k, _)| k == long).map(|(_, v)| v.as_str()).collect()
    }
}

fn looks_negative_number(a: &str) -> bool {
    a.len() > 1 && a.starts_with('-') && a[1..].parse::<f64>().is_ok()
}

/// argparse's reading of a command line: `--name VALUE`, `--name=VALUE`,
/// `-s VALUE`, `-sVALUE`, flags, `--` ending the options. `--home` is
/// always accepted (the binary reads it before the subcommand sees it).
pub fn parse(argv: &[String], spec: &Spec) -> Result<Args, String> {
    let mut out = Args::default();
    let mut it = argv.iter().peekable();
    let mut only_positional = false;
    while let Some(a) = it.next() {
        if only_positional || !a.starts_with('-') || a == "-" || looks_negative_number(a) {
            out.pos.push(a.clone());
            continue;
        }
        if a == "--" {
            only_positional = true;
            continue;
        }
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let long_of = |list: &[(&str, &str)]| {
            list.iter()
                .find(|(s, l)| name == format!("--{l}") || (!s.is_empty() && name == format!("-{s}")))
                .map(|(_, l)| l.to_string())
        };
        if let Some(l) = long_of(spec.flags) {
            if inline.is_some() {
                return Err(format!("argument --{l}: ignored explicit argument {:?}", inline.unwrap_or_default()));
            }
            out.flags.push(l);
        } else if name == "--home" || long_of(spec.valued).is_some() {
            let l = long_of(spec.valued).unwrap_or_else(|| "home".into());
            let v = match inline {
                Some(v) => v,
                None => it.next().cloned().ok_or_else(|| format!("argument --{l}: expected one argument"))?,
            };
            out.opts.push((l, v));
        } else if let Some(l) = spec.valued.iter().find(|(s, _)| !s.is_empty() && name.starts_with(&format!("-{s}")) && !name.starts_with("--")) {
            // -sVALUE
            out.opts.push((l.1.to_string(), name[1 + l.0.len()..].to_string()));
        } else {
            return Err(format!("unrecognized arguments: {a}"));
        }
    }
    Ok(out)
}

// -- the dispatcher API ----------------------------------------------------------

fn unreachable_msg(base: &str) -> String {
    format!("the dispatcher is not responding at {base} - start it with `{PROG} start`")
}

/// impctl.api: the JSON answer to a GET (or, with a body, a POST). A 4xx/5xx
/// that answers JSON is returned like any other (`{"ok": false, ...}`).
pub fn api(base: &str, path: &str, body: Option<&Value>, timeout: u64) -> Res<Value> {
    let url = format!("{base}{path}");
    let body = body.map(|b| b.to_string());
    match http::call(&url, body.as_deref(), Duration::from_secs(timeout)) {
        Ok((code, text)) => {
            let text = if text.trim().is_empty() { "{}".to_string() } else { text };
            match serde_json::from_str::<Value>(&text) {
                Ok(v) => Ok(v),
                Err(_) if code >= 400 => fail(format!("{path} answered {code}")),
                Err(_) => fail(format!("{path} did not answer JSON")),
            }
        }
        Err(_) => fail(unreachable_msg(base)),
    }
}

/// urllib.parse.quote(value, safe="").
pub fn quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn get<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    v.get(k)
}

/// `v[k] or ""`, as a string.
fn s_or_empty(v: &Value, k: &str) -> String {
    get(v, k).filter(|x| truthy(Some(x))).map(py_str).unwrap_or_default()
}

fn list_of(v: &Value, k: &str) -> Vec<Value> {
    get(v, k).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn sid(row: &Value) -> String {
    ["sessionId", "id"].iter().find_map(|k| get(row, k).filter(|x| truthy(Some(x))).map(py_str)).unwrap_or_default()
}

fn pystr_repr(s: &str) -> String {
    py_repr(&Value::String(s.to_string()))
}

/// A session by id, id prefix, name or window title (unique substring).
pub fn find_session(base: &str, spec: &str) -> Res<Value> {
    let rows = list_of(&api(base, "/api/sessions", None, 15)?, "sessions");
    pick_session(&rows, spec)
}

fn pick_session(rows: &[Value], spec: &str) -> Res<Value> {
    let spec_l = spec.to_lowercase();
    let exact: Vec<&Value> = rows
        .iter()
        .filter(|r| ["sessionId", "key", "name"].iter().any(|k| get(r, k).and_then(Value::as_str) == Some(spec)))
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0].clone());
    }
    let hits: Vec<&Value> = rows
        .iter()
        .filter(|r| {
            s_or_empty(r, "sessionId").starts_with(spec)
                || s_or_empty(r, "name").to_lowercase().contains(&spec_l)
                || s_or_empty(r, "window_title").to_lowercase().contains(&spec_l)
        })
        .collect();
    match hits.len() {
        1 => Ok(hits[0].clone()),
        0 => fail(format!("no live session matches {} - see `{PROG} sessions`", pystr_repr(spec))),
        n => fail(format!(
            "{} matches {n} sessions: {}",
            pystr_repr(spec),
            hits.iter().map(|r| {
                let name = s_or_empty(r, "name");
                if name.is_empty() { s_or_empty(r, "sessionId") } else { name }
            }).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// A live session, or failing that a past one by id or id prefix: what
/// resume and fork need, since a finished session is not on the live list.
pub fn find_any_session(base: &str, spec: &str) -> Res<Value> {
    let miss = match find_session(base, spec) {
        Ok(r) => return Ok(r),
        Err(e) => e,
    };
    let mut found: Vec<Value> = Vec::new();
    for query in [spec, ""] {
        let got = api(base, &format!("/api/history?q={}", quote(query)), None, 60)?;
        for r in list_of(&got, "results") {
            let id = s_or_empty(&r, "session_id");
            if id.starts_with(spec) && !found.iter().any(|f| s_or_empty(f, "session_id") == id) {
                found.push(r);
            }
        }
        if !found.is_empty() {
            break;
        }
    }
    match found.len() {
        1 => Ok(json!({"sessionId": s_or_empty(&found[0], "session_id"), "cwd": get(&found[0], "cwd").cloned().unwrap_or(Value::Null)})),
        0 => fail(format!(
            "{}, nor any past session id (search with `{PROG} history`)",
            miss.0.split(" - ").next().unwrap_or("")
        )),
        _ => fail(format!("{} matches {} past sessions - give more of the id", pystr_repr(spec), found.len())),
    }
}

fn dumps2(v: &Value) -> String {
    json_dumps_indent(v, 2)
}

/// Print a POST answer: raw with --json, else one line. 1 on refusal.
fn result(out: &Value, json_mode: bool, ok_text: impl Fn(&Value) -> String) -> i32 {
    let refused = get(out, "ok") == Some(&Value::Bool(false)) || truthy(get(out, "error"));
    if json_mode {
        println!("{}", dumps2(out));
    } else if refused {
        let why = get(out, "error").filter(|e| truthy(Some(e))).map(py_str).unwrap_or_else(|| "no reason given".into());
        println!("refused: {why}");
    } else {
        println!("{}", ok_text(out));
    }
    i32::from(refused)
}

fn now_secs() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// How long ago `ts` was: `12d`, `3h`, `5m`, `40s`; empty for none.
pub fn ago(ts: Option<&Value>, now: f64) -> String {
    let Some(v) = ts.filter(|v| truthy(Some(v))) else { return String::new() };
    let Some(t) = crate::pyish::py_float(v) else { return String::new() };
    let s = (now - t).max(0.0);
    for (unit, n) in [("d", 86400.0), ("h", 3600.0), ("m", 60.0)] {
        if s >= n {
            return format!("{}{unit}", (s / n).floor() as i64);
        }
    }
    format!("{}s", s.floor() as i64)
}

/// `%-Ns`: padded on the right to N characters.
fn pad(s: &str, n: usize) -> String {
    let len = s.chars().count();
    format!("{s}{}", " ".repeat(n.saturating_sub(len)))
}

// -- commands --------------------------------------------------------------------

const JSON: (&str, &str) = ("", "json");

pub fn sessions_line(r: &Value, now: f64) -> String {
    let mut name = s_or_empty(r, "name");
    if truthy(get(r, "host")) {
        name = format!("{}:{}", s_or_empty(r, "host"), name);
    }
    let status = if truthy(get(r, "background_tasks")) {
        format!("bg:{}", get(r, "background_tasks").and_then(crate::pyish::py_float).map(|f| f as i64).unwrap_or(0))
    } else if truthy(get(r, "status")) {
        s_or_empty(r, "status")
    } else {
        s_or_empty(r, "state")
    };
    let flag = if truthy(get(r, "attention")) { " *" } else { "" };
    format!(
        "{} {} {} {} {}{flag}",
        pad(&take(&sid(r), 8), 8),
        pad(&take(&name, 28), 28),
        pad(&status, 10),
        pad(&ago(get(r, "status_changed_at"), now), 6),
        s_or_empty(r, "cwd")
    )
}

fn cmd_sessions(a: &Args, base: &str) -> Res<i32> {
    let rows = list_of(&api(base, "/api/sessions", None, 15)?, "sessions");
    if a.flag("json") {
        println!("{}", dumps2(&Value::Array(rows)));
        return Ok(0);
    }
    if rows.is_empty() {
        println!("no live sessions");
        return Ok(0);
    }
    println!("{} {} {} {} folder", pad("id", 8), pad("name", 28), pad("status", 10), pad("since", 6));
    let now = now_secs();
    for r in &rows {
        println!("{}", sessions_line(r, now));
    }
    Ok(0)
}

fn cmd_projects(a: &Args, base: &str) -> Res<i32> {
    let rows = list_of(&api(base, "/api/projects", None, 15)?, "projects");
    if a.flag("json") {
        println!("{}", dumps2(&Value::Array(rows)));
        return Ok(0);
    }
    for p in &rows {
        println!("{} {}", pad(&take(&s_or_empty(p, "name"), 30), 30), s_or_empty(p, "path"));
    }
    Ok(0)
}

fn cmd_send(a: &Args, base: &str) -> Res<i32> {
    if a.pos.is_empty() {
        return Err(Fail("\u{1}the following arguments are required: prompt".into()));
    }
    let prompt = a.pos.join(" ");
    let (session, project) = (a.opt("session"), a.opt("project"));
    if session.is_some_and(|s| !s.is_empty()) == project.is_some_and(|s| !s.is_empty()) {
        return fail("name exactly one of --session or --project");
    }
    let mode = a.opt("mode").unwrap_or("auto");
    if !["auto", "inject", "launch", "background"].contains(&mode) {
        return Err(Fail(format!("\u{1}argument --mode: invalid choice: {} (choose from 'auto', 'inject', 'launch', 'background')", pystr_repr(mode))));
    }
    let mut body = json!({"prompt": prompt, "mode": if a.flag("dry-run") { "dry-run" } else { mode }});
    if let Some(s) = session.filter(|s| !s.is_empty()) {
        body["session_id"] = json!(sid(&find_session(base, s)?));
    } else {
        body["project"] = json!(project.unwrap_or(""));
    }
    let out = api(base, "/api/dispatch", Some(&body), 15)?;
    Ok(result(&out, a.flag("json"), |o| {
        let mut target = ["session", "project", "url"]
            .iter()
            .find_map(|k| get(o, k).filter(|x| truthy(Some(x))).cloned())
            .unwrap_or(Value::Null);
        if let Value::Object(_) = target {
            target = json!(["path", "name"].iter().find_map(|k| get(&target, k).filter(|x| truthy(Some(x))).map(py_str)).unwrap_or_default());
        }
        let target = if truthy(Some(&target)) { py_str(&target) } else { String::new() };
        let action = {
            let a = s_or_empty(o, "action");
            if a.is_empty() { "sent".to_string() } else { a }
        };
        if target.is_empty() { action } else { format!("{action} -> {target}") }
    }))
}

fn cmd_queue(a: &Args, base: &str) -> Res<i32> {
    let Some(action) = a.pos.first().map(String::as_str) else {
        return Err(Fail("\u{1}the following arguments are required: action".into()));
    };
    if !["list", "add", "remove"].contains(&action) {
        return Err(Fail(format!("\u{1}argument action: invalid choice: {} (choose from 'list', 'add', 'remove')", pystr_repr(action))));
    }
    let session = a.pos.get(1);
    let text = a.pos.get(2..).unwrap_or(&[]);
    let json_mode = a.flag("json");
    if action == "list" {
        let queued = get(&api(base, "/api/queue", None, 15)?, "queue").cloned().unwrap_or(json!({}));
        let empty = !truthy(Some(&queued));
        if json_mode {
            println!("{}", dumps2(&queued));
        } else if empty {
            println!("nothing queued");
        } else if let Some(map) = queued.as_object() {
            for (key, items) in map {
                println!("{key}");
                for i in items.as_array().cloned().unwrap_or_default() {
                    println!("   {}  {}", s_or_empty(&i, "id"), take(&s_or_empty(&i, "prompt"), 100));
                }
            }
        }
        return Ok(0);
    }
    let Some(session) = session else { return fail(format!("queue {action} needs a session")) };
    let row = find_session(base, session)?;
    if action == "add" {
        if text.is_empty() {
            return fail("queue add needs the prompt text");
        }
        let out = api(base, "/api/queue", Some(&json!({"session_id": sid(&row), "prompt": text.join(" ")})), 15)?;
        return Ok(result(&out, json_mode, |o| {
            if !truthy(get(o, "waiting")) {
                "delivered now - it was idle".to_string()
            } else {
                format!("queued ({} waiting)", py_str(&o["waiting"]))
            }
        }));
    }
    let Some(id) = text.first() else {
        return fail(format!("queue remove needs the item id (see `{PROG} queue list`)"));
    };
    let key = {
        let k = s_or_empty(&row, "key");
        if k.is_empty() { sid(&row) } else { k }
    };
    let out = api(base, "/api/unqueue", Some(&json!({"session": key, "id": id})), 15)?;
    Ok(result(&out, json_mode, |_| "removed".to_string()))
}

fn cmd_focus(a: &Args, base: &str) -> Res<i32> {
    let project = a.opt("project").filter(|p| !p.is_empty());
    let session = a.pos.first();
    if project.is_none() && session.is_none() {
        return fail("name a session or --project");
    }
    let body = match project {
        Some(p) => json!({"project": p}),
        None => json!({"session_id": sid(&find_session(base, session.map(String::as_str).unwrap_or(""))?)}),
    };
    let out = api(base, "/api/focus", Some(&body), 15)?;
    Ok(result(&out, a.flag("json"), |o| {
        let act = s_or_empty(o, "action");
        if act.is_empty() { "focused".into() } else { act }
    }))
}

fn cmd_launch(kind: &str, a: &Args, base: &str) -> Res<i32> {
    let Some(spec) = a.pos.first() else {
        return Err(Fail("\u{1}the following arguments are required: session".into()));
    };
    if kind == "new" {
        let row = find_session(base, spec)?;
        let cwd = get(&row, "cwd").cloned().unwrap_or(Value::Null);
        let out = api(base, "/api/new", Some(&json!({"session_id": sid(&row), "cwd": cwd})), 15)?;
        return Ok(result(&out, a.flag("json"), |_| format!("opened a new session in {}", py_str(&cwd))));
    }
    let row = find_any_session(base, spec)?;
    let wh = if a.flag("bg") { "background" } else { "window" };
    let body = json!({
        "session_id": sid(&row), "where": wh, "fork": kind == "fork",
        "prompt": a.opt("prompt").map(|p| json!(p)).unwrap_or(Value::Null),
        "cwd": get(&row, "cwd").cloned().unwrap_or(Value::Null),
    });
    let out = api(base, "/api/resume", Some(&body), 15)?;
    Ok(result(&out, a.flag("json"), |o| {
        format!("{}: {}", py_str(get(o, "where").unwrap_or(&Value::Null)), py_str(get(o, "command").unwrap_or(&Value::Null)))
    }))
}

fn cmd_history(a: &Args, base: &str) -> Res<i32> {
    if a.pos.is_empty() {
        return Err(Fail("\u{1}the following arguments are required: query".into()));
    }
    let got = api(base, &format!("/api/history?q={}", quote(&a.pos.join(" "))), None, 60)?;
    let ok = get(&got, "ok").is_none_or(|v| truthy(Some(v)));
    if a.flag("json") || !ok {
        println!("{}", dumps2(&got));
        return Ok(i32::from(!ok));
    }
    let results = list_of(&got, "results");
    for r in &results {
        let title = {
            let t = s_or_empty(r, "title");
            if t.is_empty() { s_or_empty(r, "first_prompt") } else { t }
        };
        println!(
            "{} {} {} {}",
            pad(&take(&s_or_empty(r, "session_id"), 8), 8),
            pad(if truthy(get(r, "live")) { "live" } else { "" }, 5),
            pad(&take(&title, 44), 44),
            s_or_empty(r, "cwd")
        );
    }
    println!("{}", if results.is_empty() { "no matches".to_string() } else { format!("(resume one with `{PROG} resume ID`)") });
    Ok(0)
}

fn cmd_pending(a: &Args, base: &str) -> Res<i32> {
    let items = list_of(&api(base, "/api/pending", None, 15)?, "pending");
    if a.flag("json") {
        println!("{}", dumps2(&Value::Array(items)));
        return Ok(0);
    }
    if items.is_empty() {
        println!("no permission requests waiting");
    }
    let now = now_secs();
    for i in &items {
        let text = ["summary", "tool"]
            .iter()
            .find_map(|k| get(i, k).filter(|x| truthy(Some(x))).map(py_str))
            .unwrap_or_else(|| take(&json_dumps(i, true), 100));
        println!("{}  {}  {}", s_or_empty(i, "id"), ago(get(i, "at"), now), text);
    }
    Ok(0)
}

fn cmd_decide(verdict: &str, a: &Args, base: &str) -> Res<i32> {
    let Some(id) = a.pos.first() else {
        return Err(Fail("\u{1}the following arguments are required: id".into()));
    };
    let out = api(base, "/api/decide", Some(&json!({"id": id, "verdict": verdict, "who": "cli"})), 15)?;
    Ok(result(&out, a.flag("json"), |_| format!("{} {id}", if verdict == "allow" { "allowed" } else { "denied" })))
}

fn cmd_notify(a: &Args, base: &str) -> Res<i32> {
    if let Some(channel) = a.pos.first() {
        let state = match a.pos.get(1).map(String::as_str) {
            Some(s @ ("on" | "off")) => s,
            Some(other) => return Err(Fail(format!("\u{1}argument state: invalid choice: {} (choose from 'on', 'off')", pystr_repr(other)))),
            None => return fail(format!("notify {channel} needs on or off")),
        };
        let out = api(base, "/api/notify", Some(&json!({"channel": channel, "enabled": state == "on"})), 15)?;
        return Ok(result(&out, a.flag("json"), |_| format!("{channel} {state}")));
    }
    println!("{}", dumps2(&api(base, "/api/notify", None, 15)?));
    Ok(0)
}

fn cmd_get_json(path: &str, a: &Args, base: &str) -> Res<i32> {
    let _ = a;
    println!("{}", dumps2(&api(base, path, None, 60)?));
    Ok(0)
}

fn cmd_with_session(kind: &str, a: &Args, base: &str) -> Res<i32> {
    let Some(spec) = a.pos.first() else {
        return Err(Fail("\u{1}the following arguments are required: session".into()));
    };
    let row = find_session(base, spec)?;
    cmd_get_json(&format!("/api/{kind}?session={}", quote(&sid(&row))), a, base)
}

/// `--days`: an int-looking default stays `7`; a given value is a float (`3.0`).
fn days_text(given: Option<&str>) -> Result<String, String> {
    match given {
        None => Ok("7".into()),
        Some(g) => g
            .trim()
            .parse::<f64>()
            .map(float_repr)
            .map_err(|_| format!("argument --days: invalid float value: {}", pystr_repr(g))),
    }
}

/// The `%m-%d %H:%M` stamp of an event.
fn event_stamp(e: &Value) -> String {
    let at = ["at", "ts"].iter().find_map(|k| get(e, k).filter(|x| truthy(Some(x))));
    match at.and_then(crate::pyish::py_float) {
        Some(t) => {
            let (_, m, d) = crate::paths::local_date(t);
            format!("{m:02}-{d:02} {}", crate::paths::local_hm(t))
        }
        None => String::new(),
    }
}

pub fn event_line(e: &Value) -> String {
    let text = ["summary", "project", "cwd"].iter().find_map(|k| get(e, k).filter(|x| truthy(Some(x))).map(py_str)).unwrap_or_default();
    format!("{} {} {}", pad(&event_stamp(e), 11), pad(&s_or_empty(e, "kind"), 14), take(&text, 90))
}

/// The last `n` of events served newest first, oldest of them first
/// (`events[::-1][-n:]`); 0 is all.
pub fn pick_events(served: &[Value], n: i64) -> Vec<Value> {
    let mut events: Vec<Value> = served.iter().rev().cloned().collect();
    if n > 0 {
        let keep = events.len().saturating_sub(n as usize);
        events.drain(..keep);
    } else if n < 0 {
        let skip = (-n as usize).min(events.len());
        events.drain(..skip);
    }
    events
}

fn cmd_events(a: &Args, base: &str) -> Res<i32> {
    let n = match a.opt("n") {
        None => 30,
        Some(v) => v.trim().parse::<i64>().map_err(|_| Fail(format!("\u{1}argument -n: invalid int value: {}", pystr_repr(v))))?,
    };
    let events = pick_events(&list_of(&api(base, "/api/events", None, 15)?, "events"), n);
    if a.flag("json") {
        println!("{}", dumps2(&Value::Array(events)));
        return Ok(0);
    }
    for e in &events {
        println!("{}", event_line(e));
    }
    Ok(0)
}

fn cmd_open(base: &str) -> Res<i32> {
    println!("{}", crate::hotkey::raise_or_open(&port_of(base)));
    Ok(0)
}

// -- the service ---------------------------------------------------------------------

const COMPONENTS: [(&str, &str); 3] = [("dispatcher", "Dispatcher"), ("streamdeck", "StreamDeck"), ("hotkey", "Hotkey")];

/// The components a command names, in start order.
fn pick_components(a: &Args) -> Result<Vec<(&'static str, &'static str)>, String> {
    match a.pos.first().map(String::as_str) {
        None | Some("all") => Ok(COMPONENTS.to_vec()),
        Some(c) => COMPONENTS
            .iter()
            .find(|(k, _)| *k == c)
            .map(|x| vec![*x])
            .ok_or_else(|| format!("argument component: invalid choice: {} (choose from 'all', 'dispatcher', 'streamdeck', 'hotkey')", pystr_repr(c))),
    }
}

fn service_for(a: &Args, home: Option<&str>) -> Service {
    let prefix = a.opt("task-prefix").unwrap_or("Imperatorium").to_string();
    // --port and --state only for a second, separate copy (a test's): without
    // them the dispatcher unit runs `serve --live`.
    let mut serve_args = Vec::new();
    for key in ["port", "state"] {
        if let Some(v) = a.opt(key) {
            serve_args.extend([format!("--{key}"), v.to_string()]);
        }
    }
    Service::new(prefix, std::env::current_exe().unwrap_or_default(), crate::paths::config_home(home), serve_args)
}

/// `--task-prefix P` as the install commands take it.
fn prefix_args(a: &Args) -> Vec<String> {
    a.opt("task-prefix").map(|p| vec!["--task-prefix".to_string(), p.to_string()]).unwrap_or_default()
}

fn svc_stop(svc: &Service, names: &[(&str, &str)], only: Option<&str>) {
    for (c, unit) in names.iter().rev() {
        if svc.control(unit, Op::Stop, only) == Outcome::Done {
            println!("stopped {c}");
        }
    }
}

fn svc_start(svc: &Service, names: &[(&str, &str)], only: Option<&str>) {
    for (i, (c, unit)) in names.iter().enumerate() {
        match svc.control(unit, Op::Start, only) {
            Outcome::Skipped => continue,
            Outcome::Missing => {
                let what = if cfg!(windows) { "no task registered" } else { "not installed" };
                println!("{c}: {what} - run `{PROG} service install`");
                continue;
            }
            Outcome::Done => {}
        }
        if cfg!(windows) && *c == "dispatcher" && i + 1 < names.len() {
            std::thread::sleep(Duration::from_secs(2)); // let it bind the port before the others look for it
        }
        println!("started {c}");
    }
}

fn cmd_start(a: &Args, home: Option<&str>) -> Res<i32> {
    let names = pick_components(a).map_err(|e| Fail(format!("\u{1}{e}")))?;
    svc_start(&service_for(a, home), &names, a.opt("task-prefix"));
    Ok(0)
}

fn cmd_stop(a: &Args, home: Option<&str>) -> Res<i32> {
    let names = pick_components(a).map_err(|e| Fail(format!("\u{1}{e}")))?;
    svc_stop(&service_for(a, home), &names, a.opt("task-prefix"));
    Ok(0)
}

fn cmd_restart(a: &Args, base: &str, home: Option<&str>) -> Res<i32> {
    let names = pick_components(a).map_err(|e| Fail(format!("\u{1}{e}")))?;
    let (svc, only) = (service_for(a, home), a.opt("task-prefix"));
    svc_stop(&svc, &names, only);
    std::thread::sleep(Duration::from_secs(1));
    svc_start(&svc, &names, only);
    if names.iter().any(|(c, _)| *c == "dispatcher") {
        for _ in 0..10 {
            std::thread::sleep(Duration::from_secs(1));
            if let Ok(h) = api(base, "/api/health", None, 2) {
                println!("dispatcher back, pid {}", py_str(get(&h, "pid").unwrap_or(&Value::Null)));
                return Ok(0);
            }
        }
        println!("dispatcher has not answered yet - check `{PROG} status`");
        return Ok(1);
    }
    Ok(0)
}

/// Restart the dispatcher's unit (what `remote add` offers after a config change).
pub fn restart_dispatcher(home: Option<&str>) -> i32 {
    let a = Args { pos: vec!["dispatcher".into()], ..Default::default() };
    match cmd_restart(&a, &base(), home) {
        Ok(code) => code,
        Err(Fail(m)) => {
            eprintln!("{PROG}: {}", m.trim_start_matches('\u{1}'));
            1
        }
    }
}

/// Run the dispatcher in this terminal, for debugging.
fn cmd_run(a: &Args, home: Option<&str>) -> Res<i32> {
    let host = a.opt("host").unwrap_or("127.0.0.1").to_string();
    let port: u16 = match a.opt("port") {
        None => 8792,
        Some(p) => p.parse().map_err(|_| Fail(format!("\u{1}argument --port: invalid int value: {}", pystr_repr(p))))?,
    };
    if !a.all("root").is_empty() {
        eprintln!("{PROG}: --root is ignored; the folders of projects come from `roots` in config.json");
    }
    println!(
        "dispatcher in the foreground on {host}:{port} - ctrl+c stops it. Stop the service first (`{PROG} stop dispatcher`) or the port is taken."
    );
    let state = crate::paths::state_dir().join("state.json").to_string_lossy().into_owned();
    Ok(crate::server::serve(&host, port, home, Some(&state), true))
}

/// `service install|uninstall|restart|status`: the start-at-logon units.
fn cmd_service(a: &Args, base: &str, home: Option<&str>) -> Res<i32> {
    let Some(action) = a.pos.first().map(String::as_str) else {
        return Err(Fail("\u{1}the following arguments are required: action".into()));
    };
    let svc = service_for(a, home);
    let mut say = |s: String| println!("{s}");
    match action {
        "install" => {
            let deck = a.flag("streamdeck") && !a.flag("no-streamdeck");
            let hotkey = cfg!(windows) && !a.flag("no-hotkey");
            match svc.install(deck, hotkey, a.flag("dry-run"), &mut say) {
                Ok(()) => Ok(0),
                Err(e) => fail(e),
            }
        }
        "uninstall" => {
            if !a.flag("dry-run") && !confirmed("Remove the start-at-logon service", a.flag("yes")) {
                return Ok(1);
            }
            svc.uninstall(a.flag("dry-run"), &mut say);
            Ok(0)
        }
        "restart" => cmd_restart(&Args { pos: vec![], ..a.clone() }, base, home),
        "status" => Ok(crate::install::status_main(&prefix_args(a), home)),
        other => Err(Fail(format!("\u{1}argument action: invalid choice: {} (choose from 'install', 'uninstall', 'restart', 'status')", pystr_repr(other)))),
    }
}

/// Ask before removing; with no terminal to ask in, only --yes proceeds.
pub fn confirmed(what: &str, yes: bool) -> bool {
    use std::io::{IsTerminal, Write};
    if yes {
        return true;
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("{PROG}: {what} needs --yes when there is no terminal to confirm in");
        return false;
    }
    print!("{what}? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
        println!();
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

// -- test ----------------------------------------------------------------------------

/// The Cargo workspace the tests live in: `rust/` of a checkout, found from
/// the working directory, then from this exe (a cargo target dir sits inside it).
pub fn find_checkout() -> Option<std::path::PathBuf> {
    let mut starts: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(d) = std::env::current_dir() {
        starts.push(d);
    }
    if let Ok(e) = std::env::current_exe() {
        starts.extend(e.parent().map(|p| p.to_path_buf()));
    }
    for start in starts {
        for dir in start.ancestors() {
            for rel in ["rust", "."] {
                let ws = dir.join(rel);
                if ws.join("imperatorium").join("Cargo.toml").is_file() || is_our_crate(&ws.join("Cargo.toml")) {
                    return Some(ws);
                }
            }
        }
    }
    None
}

fn is_our_crate(manifest: &std::path::Path) -> bool {
    std::fs::read_to_string(manifest).is_ok_and(|t| t.contains("name = \"imperatorium\""))
}

fn cargo_program() -> String {
    if let Some(c) = crate::launch::which("cargo") {
        return c;
    }
    let home = crate::paths::user_home().join(".cargo").join("bin").join(if cfg!(windows) { "cargo.exe" } else { "cargo" });
    if home.is_file() { home.to_string_lossy().into_owned() } else { "cargo".into() }
}

/// The lines of `cargo test` output worth showing: each suite's result and any failures.
pub fn test_summary(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|l| l.starts_with("test result:") || l.starts_with("failures:") || (l.starts_with("test ") && l.ends_with("FAILED")))
        .map(str::to_string)
        .collect()
}

fn cmd_test(a: &Args) -> Res<i32> {
    let Some(ws) = find_checkout() else {
        println!("tests need a checkout: run this from a clone of the repository (the folder holding rust/), or `cargo test` in it");
        return Ok(1);
    };
    let mut cmd = std::process::Command::new(cargo_program());
    cmd.arg("test").current_dir(&ws);
    if !a.pos.is_empty() {
        cmd.arg("--").args(&a.pos);
    }
    crate::proc::no_window(&mut cmd);
    let got = match cmd.output() {
        Ok(o) => o,
        Err(e) => return fail(format!("could not run cargo: {e}")),
    };
    let text = String::from_utf8_lossy(&got.stdout).into_owned() + &String::from_utf8_lossy(&got.stderr);
    let lines = test_summary(&text);
    for l in &lines {
        println!("{l}");
    }
    if lines.is_empty() {
        println!("{}", text.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
    }
    let code = got.status.code().unwrap_or(1);
    if code != 0 {
        println!("failed (run `cargo test` in {} for the detail)", ws.display());
    }
    Ok(i32::from(code != 0))
}

// -- the table of commands -------------------------------------------------------------

/// (section, name, arguments, what it does). The order `help` lists them in.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("Hooks and policy (what Claude Code runs)", "hook", "", "Claude Code event hook: reads the event on stdin, reports it to the dispatcher"),
    ("Hooks and policy (what Claude Code runs)", "statusline", "", "Claude Code statusline: prints the usage line, reports usage"),
    ("Hooks and policy (what Claude Code runs)", "permission", "[--home DIR]", "Claude Code permission hook: the policy decides, or the request is parked for you"),
    ("Hooks and policy (what Claude Code runs)", "policy-decide", "", "Conformance: JSON lines {request, policy} in, [verdict, reason, tier] out"),
    ("Hooks and policy (what Claude Code runs)", "install-hooks", "--settings F --native BIN [...]", "Only the settings.json edit (hooks/install.py's flags); `remote add` does a host's"),
    ("Service", "status", "[--task-prefix P]", "Installation, the service's units, and whether the dispatcher answers (exit 1 if it does not)"),
    ("Service", "start", "[all|dispatcher|streamdeck|hotkey] [--task-prefix P]", "Start the service (all, or one component)"),
    ("Service", "stop", "[all|dispatcher|streamdeck|hotkey] [--task-prefix P]", "Stop it; it stays down until `start` (a Windows task is disabled so its heartbeat does not revive it)"),
    ("Service", "restart", "[all|dispatcher|streamdeck|hotkey] [--task-prefix P]", "Stop then start; waits for the dispatcher to answer"),
    ("Service", "run", "[--host H] [--port N]", "Run the dispatcher in this terminal, for debugging (stop the service first)"),
    ("Service", "serve", "[--port N] [--host H] [--home DIR] [--state FILE] [--live]", "The dispatcher and window (what the service runs); without --live a shadow with its own state"),
    ("Service", "service", "install|uninstall|restart|status [--streamdeck] [--no-hotkey] [--dry-run] [-y]", "Register or remove the start-at-logon service"),
    ("Setup", "install", "[--yes] [--dry-run] [--home DIR] [--root DIR]... [--no-service] [...]", "Install on this machine: binary, config, Claude Code hooks, service"),
    ("Setup", "uninstall", "[--yes] [--dry-run] [--purge]", "Remove hooks, mod and service (config and state are kept; --purge removes them too)"),
    ("Setup", "check", "", "Check prerequisites and what is installed; changes nothing"),
    ("Setup", "update", "[--check] [--channel stable|preview] [--version V] [--from URL] [--rollback]", "Self-update from the project's releases (signed manifest, SHA-256 checked); --version installs that release, up or down; --rollback goes back to the binary the last update replaced"),
    ("Setup", "test", "[FILTER ...]", "Run `cargo test` in a checkout (needs one: the folder holding rust/)"),
    ("Setup", "release-package", "--version V --out DIR [--channel stable|preview] [--notes FILE] [--scripts DIR] [--sign-key-env NAME]", "Lay out a release folder: assets by target, SHA256SUMS, manifest.json"),
    ("Setup", "release-keygen", "", "A new release signing key pair, as JSON (secret for the release workflow, public for signing.rs)"),
    ("Other machines", "remote", "add|remove|check [HOST] [--label L] [--no-tunnel] [--purge] ...", "Onboard, remove or check a machine reached over SSH"),
    ("Other machines", "remote-probe", "[HOST ...]", "Poll remote hosts once and print their sessions"),
    ("Window", "open", "", "Raise the Imperatorium window, or open it"),
    ("Window", "hotkey", "[--once] [--hotkey KEYS]", "The global hotkey that raises the window (Windows)"),
    ("Window", "streamdeck", "", "The Stream Deck surface"),
    ("Window", "voice", "...", "The dictation model: fetch and check"),
    ("Sessions", "sessions", "[--json]", "List live sessions (* = wants attention); also: sessions chain|info|titles|items|list|stats ID, sessions batch (a whole corpus in one run)"),
    ("Sessions", "projects", "[--json]", "List the projects the dispatcher knows"),
    ("Sessions", "send", "PROMPT... (-s SESSION | -p PROJECT) [--mode auto|inject|launch|background] [--dry-run]", "Send a prompt to a session, or route it to a project"),
    ("Sessions", "queue", "list | add SESSION TEXT... | remove SESSION ITEM", "Prompts held until a session's turn ends"),
    ("Sessions", "focus", "[SESSION] [-p PROJECT]", "Bring a session's terminal to the front"),
    ("Sessions", "new", "SESSION", "Start a fresh session in a session's project, in a new tab"),
    ("Sessions", "resume", "SESSION [--bg] [--prompt TEXT]", "Resume a session (a finished one; use fork for a live one)"),
    ("Sessions", "fork", "SESSION [--bg] [--prompt TEXT]", "Fork a session: a copy, the original untouched"),
    ("Sessions", "history", "QUERY...", "Search past sessions"),
    ("Sessions", "changes", "SESSION", "Git status and diff for a session's folder"),
    ("Sessions", "activity", "SESSION", "A session's recent tool calls"),
    ("Approvals", "pending", "", "Permission requests waiting for an answer"),
    ("Approvals", "allow", "ID", "Approve a pending request"),
    ("Approvals", "deny", "ID", "Deny a pending request"),
    ("Monitoring", "events", "[-n N]", "The event log, newest last (default 30, 0 = all)"),
    ("Monitoring", "usage", "", "Context and rate-limit usage"),
    ("Monitoring", "errors", "[--days N]", "Tool-call failure tallies"),
    ("Monitoring", "hosts", "", "Remote hosts and their state"),
    ("Monitoring", "notify", "[CHANNEL on|off]", "Show notification channels, or turn one on/off"),
    ("Monitoring", "health", "", "Is the dispatcher up (and its pid)"),
    ("Monitoring", "mcp", "", "The fleet as MCP tools for an agent, over stdio: register with `claude mcp add --scope user imperatorium -- imperatorium mcp`"),
    ("Run by the binary itself, and for conformance", "remote-channel", "", "On a host, over SSH: the dispatcher's read/stats channel (JSON lines)"),
    ("Run by the binary itself, and for conformance", "inbox-send", "SOCKET < frames", "On a host, over SSH: write frames into a session's inbox socket"),
    ("Run by the binary itself, and for conformance", "hook-relay", "...", "On a host: relay a hook's event through the tunnel to the dispatcher"),
    ("Run by the binary itself, and for conformance", "type-into", "PID ...", "Type into a session's console (how a queued prompt is delivered on Windows)"),
    ("Run by the binary itself, and for conformance", "settings-check", "", "Conformance: settings.check over JSON lines"),
    ("Run by the binary itself, and for conformance", "slash-check", "", "Conformance: slash-command parsing over JSON lines"),
    ("Run by the binary itself, and for conformance", "projects-check", "", "Conformance: project discovery over JSON lines"),
    ("Run by the binary itself, and for conformance", "focus-check", "", "Conformance: window matching over JSON lines"),
    ("Help", "version", "", "Print the version"),
    ("Help", "help", "[COMMAND]", "This list, or one command"),
];

pub fn help_text(topic: Option<&str>) -> Result<String, String> {
    let mut out = String::new();
    if let Some(t) = topic {
        let (_, name, args, what) = COMMANDS.iter().find(|c| c.1 == t).ok_or_else(|| format!("no command {} - `{PROG} help` lists them", pystr_repr(t)))?;
        out += &format!("usage: {PROG} {name} {args}\n\n{what}\n");
        if let Some(extra) = detail(name) {
            out += &format!("\n{extra}\n");
        }
        return Ok(out);
    }
    out += &format!("imperatorium {} - one binary for everything Imperatorium does.\n", crate::update::version());
    out += &format!("\nUsage: {PROG} COMMAND [ARGS]     (the HTTP commands also take --json)\n");
    let mut section = "";
    for (s, name, args, what) in COMMANDS {
        if *s != section {
            out += &format!("\n{s}\n");
            section = s;
        }
        out += &format!("  {}\n      {what}\n", format!("{name} {args}").trim_end());
    }
    out += &format!("\nSessions can be named by id, id prefix, or name. `{PROG} help COMMAND` shows one command.\n");
    out += &format!("Set IMPERATORIUM_URL to reach a dispatcher other than {DEFAULT_URL}.\n");
    Ok(out)
}

fn detail(name: &str) -> Option<&'static str> {
    Some(match name {
        "send" => "  -s, --session S   session id, id prefix, or name\n  -p, --project P   project name (launches one if none is running)\n  --mode M          how to deliver it (default: auto)\n  --dry-run         say what would happen, do nothing",
        "queue" => "  list              what is queued, per session\n  add S TEXT...    hold a prompt until S's turn ends (delivered now if it is idle)\n  remove S ITEM    drop an item (see `queue list`)",
        "start" | "stop" | "restart" => "  Units are found by what they run: this binary, with the component's subcommand.\n  --task-prefix P   only the tasks, units or agents named P*",
        "service" => "  install     register and start every unit (the Stream Deck with --streamdeck)\n  uninstall   remove them; asks first unless --yes\n  --task-prefix P, --home DIR, --dry-run; --port N --state FILE for a separate copy (without them: serve --live)",
        "events" => "  -n N   how many (default 30, 0 = all)",
        "errors" => "  --days N   how far back (default 7)",
        "resume" | "fork" => "  --bg          in the background, not a new tab\n  --prompt T    --bg: its first message",
        "remote" => "  add HOST      probe it over SSH, put the binary there, install its reporting hooks, write the config entry\n  remove HOST   its hooks and its config entry (--purge: also its ~/.claude/imperatorium)\n  check [HOST]  every configured host, step by step\n  --label L  --no-tunnel  --install-claude  --no-restart  -y  --home DIR",
        _ => return None,
    })
}

// -- entry points ----------------------------------------------------------------------

const HTTP: Spec = Spec { valued: &[], flags: &[JSON] };

fn spec_of(name: &str) -> Spec {
    match name {
        "send" => Spec { valued: &[("s", "session"), ("p", "project"), ("", "mode")], flags: &[JSON, ("", "dry-run")] },
        "focus" => Spec { valued: &[("p", "project")], flags: &[JSON] },
        "resume" | "fork" => Spec { valued: &[("", "prompt")], flags: &[JSON, ("", "bg")] },
        "events" => Spec { valued: &[("n", "n")], flags: &[JSON] },
        "errors" => Spec { valued: &[("", "days")], flags: &[JSON] },
        "run" => Spec { valued: &[("", "host"), ("", "port"), ("", "root")], flags: &[JSON] },
        "start" | "stop" | "restart" | "status" => Spec { valued: &[("", "task-prefix")], flags: &[JSON] },
        "service" => Spec {
            valued: &[("", "task-prefix"), ("", "port"), ("", "state")],
            flags: &[JSON, ("", "streamdeck"), ("", "no-streamdeck"), ("", "no-hotkey"), ("", "dry-run"), ("y", "yes")],
        },
        _ => HTTP,
    }
}

/// The names `main` hands to [`main`]: the commands this module runs.
pub fn handles(name: &str) -> bool {
    matches!(
        name,
        "start" | "stop" | "restart" | "run" | "service" | "open" | "sessions" | "projects" | "send" | "queue" | "focus"
            | "new" | "resume" | "fork" | "history" | "changes" | "activity" | "pending" | "allow" | "deny" | "events"
            | "usage" | "errors" | "hosts" | "notify" | "health" | "test" | "help" | "--help" | "-h"
    )
}

/// `imperatorium COMMAND ...` for the commands above. Returns the exit code.
pub fn main(name: &str, argv: &[String], home: Option<&str>) -> i32 {
    if name == "help" || name == "--help" || name == "-h" {
        return match help_text(argv.first().map(String::as_str)) {
            Ok(t) => {
                print!("{t}");
                0
            }
            Err(e) => {
                eprintln!("{PROG}: {e}");
                1
            }
        };
    }
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        return main("help", &[name.to_string()], home);
    }
    let a = match parse(argv, &spec_of(name)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("usage: {PROG} {name} ...\n{PROG} {name}: error: {e}");
            return 2;
        }
    };
    let base = base();
    let home = a.opt("home").or(home);
    let got = match name {
        "start" => cmd_start(&a, home),
        "stop" => cmd_stop(&a, home),
        "restart" => cmd_restart(&a, &base, home),
        "run" => cmd_run(&a, home),
        "service" => cmd_service(&a, &base, home),
        "status" => Ok(crate::install::status_main(&prefix_args(&a), home)),
        "open" => cmd_open(&base),
        "sessions" => cmd_sessions(&a, &base),
        "projects" => cmd_projects(&a, &base),
        "send" => cmd_send(&a, &base),
        "queue" => cmd_queue(&a, &base),
        "focus" => cmd_focus(&a, &base),
        "new" | "resume" | "fork" => cmd_launch(name, &a, &base),
        "history" => cmd_history(&a, &base),
        "changes" | "activity" => cmd_with_session(name, &a, &base),
        "pending" => cmd_pending(&a, &base),
        "allow" => cmd_decide("allow", &a, &base),
        "deny" => cmd_decide("deny", &a, &base),
        "events" => cmd_events(&a, &base),
        "usage" => cmd_get_json("/api/usage", &a, &base),
        "errors" => match days_text(a.opt("days")) {
            Ok(d) => cmd_get_json(&format!("/api/errors?days={d}"), &a, &base),
            Err(e) => Err(Fail(format!("\u{1}{e}"))),
        },
        "hosts" => cmd_get_json("/api/hosts", &a, &base),
        "notify" => cmd_notify(&a, &base),
        "health" => cmd_get_json("/api/health", &a, &base),
        "test" => cmd_test(&a),
        _ => Ok(2),
    };
    match got {
        Ok(code) => code,
        // A message that begins with \u{1} is a bad command line, as argparse reports it.
        Err(Fail(m)) if m.starts_with('\u{1}') => {
            eprintln!("usage: {PROG} {name} ...\n{PROG} {name}: error: {}", &m[1..]);
            2
        }
        Err(Fail(m)) => {
            eprintln!("{PROG}: {m}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn endpoint_comes_from_the_environment() {
        assert_eq!(base_of(None), "http://127.0.0.1:8792");
        assert_eq!(base_of(Some("http://h:9/")), "http://h:9");
        assert_eq!(port_of("http://127.0.0.1:8861"), "8861");
        assert_eq!(port_of("http://example.com"), "8792");
    }

    #[test]
    fn arguments_read_as_argparse_does() {
        let a = parse(&v(&["hello", "-s", "ab", "--mode=inject", "world", "--dry-run", "--json"]), &spec_of("send")).unwrap();
        assert_eq!(a.pos, ["hello", "world"]);
        assert_eq!((a.opt("session"), a.opt("mode")), (Some("ab"), Some("inject")));
        assert!(a.flag("dry-run") && a.flag("json") && !a.flag("project"));
        let a = parse(&v(&["-sab", "--", "-p", "x"]), &spec_of("send")).unwrap();
        assert_eq!((a.opt("session"), a.pos.clone()), (Some("ab"), v(&["-p", "x"])));
        // A negative number is a value, not an option.
        assert_eq!(parse(&v(&["-n", "-3"]), &spec_of("events")).unwrap().opt("n"), Some("-3"));
        assert!(parse(&v(&["--nope"]), &spec_of("send")).unwrap_err().contains("--nope"));
        assert!(parse(&v(&["-s"]), &spec_of("send")).unwrap_err().contains("expected one argument"));
        // --home is accepted everywhere.
        assert_eq!(parse(&v(&["--home", "/h"]), &HTTP).unwrap().opt("home"), Some("/h"));
    }

    #[test]
    fn components_default_to_all_in_start_order() {
        let names = |args: &[&str]| pick_components(&parse(&v(args), &spec_of("start")).unwrap()).map(|c| c.iter().map(|x| x.0).collect::<Vec<_>>());
        assert_eq!(names(&[]).unwrap(), ["dispatcher", "streamdeck", "hotkey"]);
        assert_eq!(names(&["all"]).unwrap(), ["dispatcher", "streamdeck", "hotkey"]);
        assert_eq!(names(&["hotkey"]).unwrap(), ["hotkey"]);
        assert!(names(&["nope"]).unwrap_err().contains("invalid choice: 'nope'"));
    }

    #[test]
    fn quoting_is_urllibs() {
        assert_eq!(quote("a b/c?d=é~_.-"), "a%20b%2Fc%3Fd%3D%C3%A9~_.-");
    }

    #[test]
    fn ages() {
        let now = 1_000_000.0;
        let a = |t: f64| ago(Some(&json!(t)), now);
        assert_eq!((a(now - 5.0), a(now - 125.0), a(now - 7300.0), a(now - 200_000.0)), ("5s".into(), "2m".into(), "2h".into(), "2d".into()));
        assert_eq!(a(now + 50.0), "0s");
        assert_eq!((ago(None, now), ago(Some(&json!(0)), now), ago(Some(&json!(null)), now)), (String::new(), String::new(), String::new()));
    }

    #[test]
    fn session_rows_line_up() {
        let r = json!({"sessionId": "abcdef0123456789", "name": "a-very-long-session-name-over-28", "host": "box",
                       "status": "idle", "status_changed_at": 1000.0, "cwd": "/w/p", "attention": true});
        let line = sessions_line(&r, 1090.0);
        assert_eq!(line, "abcdef01 box:a-very-long-session-name idle       1m     /w/p *");
        let bg = json!({"id": "12345678-x", "name": "n", "background_tasks": 3, "state": "busy", "cwd": "/c"});
        assert_eq!(sessions_line(&bg, 0.0), "12345678 n                            bg:3              /c");
        let plain = json!({"sessionId": "s", "state": "busy"});
        assert_eq!(sessions_line(&plain, 0.0), "s                                     busy              ");
    }

    #[test]
    fn a_session_is_found_by_id_name_or_title() {
        let rows = vec![
            json!({"sessionId": "aaa111", "key": "k1", "name": "Alpha", "window_title": "work"}),
            json!({"sessionId": "bbb222", "key": "k2", "name": "alphabet", "window_title": "play"}),
        ];
        let ok = |s: &str| pick_session(&rows, s).map(|r| sid(&r));
        assert_eq!(ok("aaa111").unwrap(), "aaa111");
        assert_eq!(ok("k2").unwrap(), "bbb222");
        assert_eq!(ok("Alpha").unwrap(), "aaa111"); // exact beats the substring hit on both
        assert_eq!(ok("play").unwrap(), "bbb222");
        assert_eq!(ok("bbb").unwrap(), "bbb222");
        assert_eq!(ok("nope").unwrap_err().0, "no live session matches 'nope' - see `imperatorium sessions`");
        assert_eq!(ok("alph").unwrap_err().0, "'alph' matches 2 sessions: Alpha, alphabet");
    }

    #[test]
    fn events_are_the_last_n_oldest_first() {
        let served: Vec<Value> = (1..=5).map(|i| json!({"kind": format!("k{i}"), "summary": format!("s{i}"), "at": null})).collect();
        let kinds = |n: i64| pick_events(&served, n).iter().map(|e| s_or_empty(e, "kind")).collect::<Vec<_>>();
        assert_eq!(kinds(2), ["k2", "k1"]);
        assert_eq!(kinds(0), ["k5", "k4", "k3", "k2", "k1"]);
        assert_eq!(kinds(99).len(), 5);
        let line = event_line(&json!({"kind": "stop", "summary": "x".repeat(120), "at": null}));
        assert_eq!(line, format!("{} {} {}", " ".repeat(11), "stop          ", "x".repeat(90)));
        assert_eq!(event_line(&json!({"project": "proj"})), format!("{} {} proj", " ".repeat(11), " ".repeat(14)));
        assert!(event_stamp(&json!({"at": 1_700_000_000.0})).len() == 11);
    }

    #[test]
    fn days_keep_pythons_spelling() {
        assert_eq!(days_text(None).unwrap(), "7");
        assert_eq!(days_text(Some("3")).unwrap(), "3.0");
        assert_eq!(days_text(Some("0.5")).unwrap(), "0.5");
        assert!(days_text(Some("x")).is_err());
    }

    #[test]
    fn refusals_exit_1() {
        assert_eq!(result(&json!({"ok": false, "error": "no"}), false, |_| String::new()), 1);
        assert_eq!(result(&json!({"error": "no"}), true, |_| String::new()), 1);
        assert_eq!(result(&json!({"ok": true}), false, |_| "fine".into()), 0);
        assert_eq!(result(&json!({}), false, |_| "fine".into()), 0);
    }

    #[test]
    fn every_command_is_in_the_help() {
        let all = help_text(None).unwrap();
        for name in ["status", "start", "stop", "restart", "run", "service", "open", "sessions", "projects", "send", "queue", "focus",
                     "history", "pending", "notify", "events", "test", "remote", "check", "install", "hook", "serve", "update", "release-keygen"] {
            assert!(COMMANDS.iter().any(|c| c.1 == name), "{name}");
            assert!(all.contains(&format!("  {name}")), "{name}");
        }
        assert!(help_text(Some("send")).unwrap().contains("--dry-run"));
        assert!(help_text(Some("nope")).is_err());
    }

    #[test]
    fn every_subcommand_of_main_is_listed() {
        let main = include_str!("main.rs");
        let re = regex::Regex::new(r#"Some\("([a-z][a-z-]*)"\)"#).unwrap();
        let mut seen = 0;
        for c in re.captures_iter(main) {
            let name = &c[1];
            // Not subcommands: `sessions`' own verbs and the `--version` spelling.
            if ["chain", "info", "titles", "items", "list", "stats", "batch"].contains(&name) {
                continue;
            }
            seen += 1;
            assert!(COMMANDS.iter().any(|c| c.1 == name), "`{name}` is a subcommand of main.rs but not in the help");
        }
        assert!(seen > 25, "{seen}");
    }

    #[test]
    fn cargo_test_output_is_summarised() {
        let out = "running 2 tests\ntest a ... ok\ntest b ... FAILED\n\nfailures:\n\ntest result: FAILED. 1 passed; 1 failed;\n";
        assert_eq!(test_summary(out), ["test b ... FAILED", "failures:", "test result: FAILED. 1 passed; 1 failed;"]);
    }
}

/// What test_impctl.py pinned down for the question before a removal.
#[cfg(test)]
mod impctl_cov {
    use super::*;

    #[test]
    fn yes_skips_the_question() {
        assert!(confirmed("x", true));
        // -y and --yes both reach it as the flag.
        let spec = spec_of("service");
        for flag in ["-y", "--yes"] {
            assert!(parse(&[flag.to_string()], &spec).unwrap().flag("yes"), "{flag}");
        }
    }
}
