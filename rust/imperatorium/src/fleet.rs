//! The fleet, for agents and for Ask (2026-11-20; docs/research-super-agent.md).
//!
//! Three things, none of which lets a model act on its own:
//!
//! - **The summary**: every session as a few structured fields (state, who is
//!   waiting and for how long, what it is doing, what it has cost). Free text
//!   that comes from a transcript (`doing`, `title`) is data, never
//!   instructions; whoever shows it to a model says so.
//! - **Watches**: rules the dispatcher checks itself, with no model in the
//!   loop - a session finishes, waits for you longer than N minutes, spends
//!   more than $N. A model may *write* one (it is proposed); only the window
//!   *confirms* it, and only then is it checked.
//! - **Actions**: a fixed menu (queue a prompt, stop, compact, open). A model
//!   may propose one; it runs only when the window confirms it, and a prompt
//!   it carries is delivered framed as someone else's, never as yours.
//!
//! This file is the pure part: shapes, validation, the book they are kept
//! in and the rules' evaluation. `server/fleet.rs` wires it to the
//! dispatcher.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::pyish::truthy;
use crate::registry::{self, Row};

pub const WATCH_KINDS: &[&str] = &["finishes", "waiting", "spend"];
pub const ACTIONS: &[&str] = &["queue_prompt", "stop", "compact", "open"];

/// Proposals, firings and finished actions kept, newest last.
const KEEP_FIRED: usize = 50;
const KEEP_ACTIONS: usize = 50;
/// Watches at once; a model in a loop cannot fill the disk.
const MAX_WATCHES: usize = 100;
const MAX_PROPOSED: usize = 30;
/// A queued prompt an action may carry.
pub const MAX_PROMPT: usize = 4000;

fn s<'a>(row: &'a Row, k: &str) -> Option<&'a str> {
    row.get(k).and_then(Value::as_str).filter(|v| !v.is_empty())
}

fn cut(text: &str, n: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= n {
        return t.to_string();
    }
    let mut out: String = t.chars().take(n.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

/// A session in one word or phrase, the way the sidebar groups it.
pub fn state_word(row: &Row) -> &'static str {
    if truthy(row.get("attention")) {
        return "waiting for you";
    }
    if registry::needs_attention(row) {
        return "blocked";
    }
    if crate::matrix::working(row) {
        return "working";
    }
    if registry::in_progress(row) {
        return "waiting on background work";
    }
    if registry::finished(row) {
        return "finished";
    }
    "idle"
}

/// The key a session is named by everywhere else (the row's `key`).
pub fn key_of(row: &Row) -> String {
    s(row, "key").or_else(|| s(row, "sessionId")).unwrap_or("").to_string()
}

fn name_of(row: &Row) -> String {
    s(row, "display_name")
        .or_else(|| s(row, "name"))
        .map(str::to_string)
        .unwrap_or_else(|| key_of(row).chars().take(8).collect())
}

fn project_of(row: &Row) -> String {
    if let Some(p) = s(row, "project") {
        return p.to_string();
    }
    let cwd = s(row, "cwd").unwrap_or("");
    cwd.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

/// One session, as the summary shows it.
pub fn summary_row(row: &Row, cost: Option<f64>, queued: usize, now: f64) -> Value {
    let waiting = truthy(row.get("attention"));
    let mut out = json!({
        "session": key_of(row),
        "name": name_of(row),
        "project": project_of(row),
        "state": state_word(row),
        "waiting_for_you": waiting,
    });
    let o = out.as_object_mut().unwrap();
    if let Some(h) = s(row, "host").filter(|_| truthy(row.get("remote"))) {
        o.insert("host".into(), json!(h));
    }
    if waiting {
        let since = registry::attention_start(row);
        if since > 0.0 {
            o.insert("waiting_minutes".into(), json!(((now - since) / 60.0).max(0.0).round()));
        }
        if let Some(r) = s(row, "attention_reason").or_else(|| s(row, "attention_kind")) {
            o.insert("waiting_reason".into(), json!(r));
        }
    }
    if let Some(d) = s(row, "doing") {
        o.insert("doing".into(), json!(cut(d, 200)));
    }
    if let Some(t) = s(row, "window_title") {
        o.insert("title".into(), json!(cut(t, 120)));
    }
    if let Some(c) = cost {
        o.insert("cost_usd".into(), json!((c * 100.0).round() / 100.0));
    }
    if queued > 0 {
        o.insert("queued_prompts".into(), json!(queued));
    }
    if let Some(at) = row.get("last_event_at").and_then(Value::as_f64).filter(|t| *t > 0.0) {
        o.insert("last_activity_minutes_ago".into(), json!(((now - at) / 60.0).max(0.0).round()));
    }
    out
}

/// Every session, waiting ones first, then working, then the rest by name.
pub fn summary(rows: &[Row], costs: &HashMap<String, f64>, queued: &HashMap<String, usize>, now: f64) -> Value {
    let mut list: Vec<(u8, String, Value)> = rows
        .iter()
        .filter(|r| !key_of(r).is_empty())
        .map(|r| {
            let k = key_of(r);
            let sid = s(r, "sessionId").unwrap_or(&k).to_string();
            let cost = costs.get(&sid).or_else(|| costs.get(&k)).copied();
            let v = summary_row(r, cost, queued.get(&k).copied().unwrap_or(0), now);
            let rank = match v["state"].as_str() {
                Some("waiting for you") | Some("blocked") => 0,
                Some("working") | Some("waiting on background work") => 1,
                _ => 2,
            };
            (rank, v["name"].as_str().unwrap_or("").to_lowercase(), v)
        })
        .collect();
    list.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let mut counts = Map::new();
    for (_, _, v) in &list {
        let k = v["state"].as_str().unwrap_or("idle").to_string();
        let n = counts.get(&k).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(k, json!(n + 1));
    }
    let total: f64 = list.iter().filter_map(|(_, _, v)| v["cost_usd"].as_f64()).sum();
    json!({
        "sessions": list.into_iter().map(|(_, _, v)| v).collect::<Vec<_>>(),
        "counts": counts,
        "cost_usd": (total * 100.0).round() / 100.0,
    })
}

// -- which sessions a rule or action names --------------------------------------

/// Does `row` match `target`: its key, session id (or a prefix of 6+), or
/// its name, case-insensitively.
pub fn names_session(row: &Row, target: &str) -> bool {
    let t = target.trim().to_lowercase();
    if t.is_empty() {
        return false;
    }
    let key = key_of(row).to_lowercase();
    let sid = s(row, "sessionId").unwrap_or("").to_lowercase();
    key == t
        || sid == t
        || (t.len() >= 6 && (sid.starts_with(&t) || key.starts_with(&t)))
        || name_of(row).to_lowercase() == t
        || s(row, "name").is_some_and(|n| n.to_lowercase() == t)
}

fn in_project(row: &Row, project: &str) -> bool {
    let p = project.trim().to_lowercase();
    !p.is_empty() && project_of(row).to_lowercase() == p
}

/// The rows a rule covers: one session, a project's, or every one.
fn covered<'a>(w: &Map<String, Value>, rows: &'a [Row]) -> Vec<&'a Row> {
    let session = w.get("session").and_then(Value::as_str).unwrap_or("");
    let project = w.get("project").and_then(Value::as_str).unwrap_or("");
    rows.iter()
        .filter(|r| {
            if !session.is_empty() {
                names_session(r, session)
            } else if !project.is_empty() {
                in_project(r, project)
            } else {
                true
            }
        })
        .collect()
}

// -- validation -------------------------------------------------------------------

fn text_field(p: &Map<String, Value>, k: &str, max: usize) -> Option<String> {
    p.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| cut(v, max))
}

fn number(p: &Map<String, Value>, k: &str) -> Option<f64> {
    match p.get(k)? {
        Value::Number(n) => n.as_f64(),
        Value::String(t) => t.trim().trim_start_matches('$').parse().ok(),
        _ => None,
    }
    .filter(|n: &f64| n.is_finite())
}

/// A watch as asked for, checked and reduced to the fields a watch has.
/// `rows` resolves a session's name to its key, so a rule keeps pointing at
/// the same session if it is renamed.
pub fn validate_watch(p: &Map<String, Value>, rows: &[Row]) -> Result<Map<String, Value>, String> {
    let kind = p.get("kind").and_then(Value::as_str).unwrap_or("");
    if !WATCH_KINDS.contains(&kind) {
        return Err(format!("kind is one of {}", WATCH_KINDS.join(", ")));
    }
    let mut w = Map::new();
    w.insert("kind".into(), json!(kind));
    if let Some(target) = text_field(p, "session", 200) {
        let hits: Vec<&Row> = rows.iter().filter(|r| names_session(r, &target)).collect();
        match hits.as_slice() {
            [one] => {
                w.insert("session".into(), json!(key_of(one)));
                w.insert("label".into(), json!(name_of(one)));
            }
            [] => return Err(format!("no running session is called {target}")),
            _ => return Err(format!("{target} names more than one session; use its id")),
        }
    } else if let Some(project) = text_field(p, "project", 200) {
        w.insert("project".into(), json!(project));
        w.insert("label".into(), json!(project));
    } else if kind == "finishes" {
        return Err("a finishes watch names a session or a project".into());
    }
    match kind {
        "waiting" => {
            let m = number(p, "minutes").unwrap_or(10.0);
            if !(0.0..=1440.0).contains(&m) {
                return Err("minutes is between 0 and 1440".into());
            }
            w.insert("minutes".into(), json!(m));
        }
        "spend" => {
            let d = number(p, "dollars").ok_or("a spend watch needs dollars")?;
            if !(0.01..=100000.0).contains(&d) {
                return Err("dollars is between 0.01 and 100000".into());
            }
            w.insert("dollars".into(), json!(d));
        }
        _ => {}
    }
    // One-shot unless asked otherwise: a single session's finish or spend
    // happens once; "anything waiting more than 10 minutes" is standing.
    let default_once = w.contains_key("session") && kind != "waiting";
    let once = p.get("once").map(|v| truthy(Some(v))).unwrap_or(default_once);
    w.insert("once".into(), json!(once));
    if let Some(n) = text_field(p, "note", 200) {
        w.insert("note".into(), json!(n));
    }
    w.insert("text".into(), json!(describe_watch(&w)));
    Ok(w)
}

/// "tell me when gem-trip-4 finishes", for the window and the push.
pub fn describe_watch(w: &Map<String, Value>) -> String {
    let who = match (w.get("session"), w.get("project")) {
        (Some(_), _) => w.get("label").and_then(Value::as_str).unwrap_or("the session").to_string(),
        (None, Some(p)) => format!("any {} session", p.as_str().unwrap_or("")),
        _ => "any session".to_string(),
    };
    let what = match w.get("kind").and_then(Value::as_str) {
        Some("finishes") => "finishes its work".to_string(),
        Some("waiting") => {
            let m = w.get("minutes").and_then(Value::as_f64).unwrap_or(0.0);
            if m <= 0.0 {
                "starts waiting for you".to_string()
            } else {
                format!("waits for you more than {} min", fmt_num(m))
            }
        }
        Some("spend") => format!(
            "has cost more than ${}",
            fmt_num(w.get("dollars").and_then(Value::as_f64).unwrap_or(0.0))
        ),
        _ => "changes".to_string(),
    };
    format!("when {who} {what}")
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n:.2}")
    }
}

/// An action as proposed, checked: one from the menu, a session that is
/// running, and for queue_prompt the prompt.
pub fn validate_action(p: &Map<String, Value>, rows: &[Row]) -> Result<Map<String, Value>, String> {
    let action = p.get("action").and_then(Value::as_str).unwrap_or("");
    if !ACTIONS.contains(&action) {
        return Err(format!("action is one of {}", ACTIONS.join(", ")));
    }
    let target = text_field(p, "session", 200).ok_or("an action names a session")?;
    let hits: Vec<&Row> = rows.iter().filter(|r| names_session(r, &target)).collect();
    let row = match hits.as_slice() {
        [one] => *one,
        [] => return Err(format!("no running session is called {target}")),
        _ => return Err(format!("{target} names more than one session; use its id")),
    };
    let mut a = Map::new();
    a.insert("action".into(), json!(action));
    a.insert("session".into(), json!(key_of(row)));
    a.insert("session_id".into(), json!(s(row, "sessionId").unwrap_or("")));
    a.insert("label".into(), json!(name_of(row)));
    if action == "queue_prompt" {
        let prompt = p.get("prompt").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if prompt.is_empty() {
            return Err("queue_prompt needs a prompt".into());
        }
        if prompt.chars().count() > MAX_PROMPT {
            return Err(format!("a prompt is at most {MAX_PROMPT} characters"));
        }
        if prompt.starts_with('/') {
            // A slash command is typed at the terminal as real input: only
            // the fixed `compact` action does that.
            return Err("a queued prompt cannot be a slash command".into());
        }
        a.insert("prompt".into(), json!(prompt));
    }
    if let Some(r) = text_field(p, "reason", 300) {
        a.insert("reason".into(), json!(r));
    }
    a.insert("text".into(), json!(describe_action(&a)));
    Ok(a)
}

pub fn describe_action(a: &Map<String, Value>) -> String {
    let who = a.get("label").and_then(Value::as_str).unwrap_or("the session");
    match a.get("action").and_then(Value::as_str) {
        Some("queue_prompt") => format!("queue a prompt for {who}"),
        Some("stop") => format!("stop {who}'s turn"),
        Some("compact") => format!("compact {who}"),
        Some("open") => format!("open {who}'s terminal"),
        _ => "do nothing".into(),
    }
}

// -- the book -------------------------------------------------------------------

/// Watches, proposed actions and what has fired, kept in `fleet.json` beside
/// the state. A model can only add `proposed` entries; every change of
/// status past that is the window's.
#[derive(Default)]
pub struct Book {
    path: Option<PathBuf>,
    pub watches: Vec<Map<String, Value>>,
    pub actions: Vec<Map<String, Value>>,
    pub fired: Vec<Map<String, Value>>,
    seq: u64,
}

fn list_of(v: &Value, k: &str) -> Vec<Map<String, Value>> {
    v.get(k)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_object().cloned()).collect())
        .unwrap_or_default()
}

impl Book {
    /// The book at `path` (None: kept in memory only, as a shadow does).
    pub fn open(path: Option<PathBuf>) -> Book {
        let mut b = Book { path, ..Default::default() };
        if let Some(text) = b.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            b.watches = list_of(&v, "watches");
            b.actions = list_of(&v, "actions");
            b.fired = list_of(&v, "fired");
            b.seq = v.get("seq").and_then(Value::as_u64).unwrap_or(0);
        }
        b
    }

    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        let v = json!({"watches": self.watches, "actions": self.actions,
                       "fired": self.fired, "seq": self.seq});
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap_or_default()).is_ok()
            && std::fs::rename(&tmp, path).is_err()
        {
            let _ = std::fs::copy(&tmp, path);
            let _ = std::fs::remove_file(&tmp);
        }
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.seq += 1;
        let salt: u32 = {
            let mut b = [0u8; 4];
            let _ = getrandom::getrandom(&mut b);
            u32::from_le_bytes(b) & 0xffff
        };
        format!("{prefix}{}{salt:04x}", self.seq)
    }

    fn proposed(&self) -> usize {
        self.watches.iter().chain(self.actions.iter()).filter(|x| status(x) == "proposed").count()
    }

    /// A watch, `active` when the window made it, else `proposed`.
    pub fn add_watch(&mut self, mut w: Map<String, Value>, confirmed: bool, source: &str, now: f64) -> Result<Map<String, Value>, String> {
        if self.watches.len() >= MAX_WATCHES {
            return Err(format!("there are already {MAX_WATCHES} watches; remove some first"));
        }
        if !confirmed && self.proposed() >= MAX_PROPOSED {
            return Err("too many proposals are waiting for the window already".into());
        }
        w.insert("id".into(), json!(self.next_id("w")));
        w.insert("status".into(), json!(if confirmed { "active" } else { "proposed" }));
        w.insert("source".into(), json!(source));
        w.insert("created_at".into(), json!(now));
        self.watches.push(w.clone());
        self.save();
        Ok(w)
    }

    pub fn propose_action(&mut self, mut a: Map<String, Value>, source: &str, now: f64) -> Result<Map<String, Value>, String> {
        if self.proposed() >= MAX_PROPOSED {
            return Err("too many proposals are waiting for the window already".into());
        }
        a.insert("id".into(), json!(self.next_id("a")));
        a.insert("status".into(), json!("proposed"));
        a.insert("source".into(), json!(source));
        a.insert("created_at".into(), json!(now));
        self.actions.push(a.clone());
        self.trim_actions();
        self.save();
        Ok(a)
    }

    fn trim_actions(&mut self) {
        // Finished ones go first; a proposal stays until it is answered.
        while self.actions.len() > KEEP_ACTIONS {
            match self.actions.iter().position(|a| status(a) != "proposed") {
                Some(i) => {
                    self.actions.remove(i);
                }
                None => break,
            }
        }
    }

    pub fn watch_mut(&mut self, id: &str) -> Option<&mut Map<String, Value>> {
        self.watches.iter_mut().find(|w| w.get("id").and_then(Value::as_str) == Some(id))
    }

    pub fn action_mut(&mut self, id: &str) -> Option<&mut Map<String, Value>> {
        self.actions.iter_mut().find(|a| a.get("id").and_then(Value::as_str) == Some(id))
    }

    /// The window confirms a proposed watch, or pauses / resumes one.
    pub fn set_watch_status(&mut self, id: &str, to: &str) -> Result<Map<String, Value>, String> {
        let w = self.watch_mut(id).ok_or("no such watch")?;
        let from = status(w).to_string();
        let ok = match to {
            "active" => matches!(from.as_str(), "proposed" | "paused" | "fired"),
            "paused" => from == "active",
            _ => false,
        };
        if !ok {
            return Err(format!("a {from} watch cannot become {to}"));
        }
        w.insert("status".into(), json!(to));
        let out = w.clone();
        self.save();
        Ok(out)
    }

    pub fn remove_watch(&mut self, id: &str) -> bool {
        let n = self.watches.len();
        self.watches.retain(|w| w.get("id").and_then(Value::as_str) != Some(id));
        let gone = self.watches.len() != n;
        if gone {
            self.save();
        }
        gone
    }

    /// An answered action: `done`, `failed` or `declined`, with what came of it.
    pub fn finish_action(&mut self, id: &str, outcome: &str, result: Value, now: f64) {
        if let Some(a) = self.action_mut(id) {
            a.insert("status".into(), json!(outcome));
            a.insert("answered_at".into(), json!(now));
            a.insert("result".into(), result);
        }
        self.trim_actions();
        self.save();
    }

    pub fn record_fired(&mut self, f: &Firing, now: f64) {
        self.fired.push(
            json!({"watch": f.watch, "session": f.session, "text": f.text, "at": now})
                .as_object()
                .cloned()
                .unwrap_or_default(),
        );
        if self.fired.len() > KEEP_FIRED {
            let drop = self.fired.len() - KEEP_FIRED;
            self.fired.drain(..drop);
        }
        if let Some(w) = self.watch_mut(&f.watch) {
            let n = w.get("fired_count").and_then(Value::as_u64).unwrap_or(0) + 1;
            w.insert("fired_count".into(), json!(n));
            w.insert("last_fired_at".into(), json!(now));
            if truthy(w.get("once")) {
                w.insert("status".into(), json!("fired"));
            }
        }
        self.save();
    }

    pub fn view(&self) -> Value {
        json!({"watches": self.watches, "actions": self.actions,
               "fired": self.fired.iter().rev().collect::<Vec<_>>(),
               "proposed": self.proposed()})
    }
}

pub fn status(x: &Map<String, Value>) -> &str {
    x.get("status").and_then(Value::as_str).unwrap_or("")
}

// -- evaluation -----------------------------------------------------------------

/// One rule that just came true for one session.
#[derive(Debug, Clone, PartialEq)]
pub struct Firing {
    pub watch: String,
    pub session: String,
    pub text: String,
}

/// What the evaluation remembers between looks; not kept across restarts
/// (a restart sees every session afresh, so it cannot announce a finish it
/// did not see start).
#[derive(Default)]
pub struct Memory {
    /// (watch, session) -> was it in progress at the last look.
    busy: HashMap<(String, String), bool>,
    /// (watch, session) -> its last Stop hook's time at the last look: a
    /// turn shorter than the look between two is never seen busy, but its
    /// Stop is (2026-11-21: a 2 s "OK" turn did not fire).
    stops: HashMap<(String, String), f64>,
    /// What has fired already: (watch, session, raise) for waiting,
    /// (watch, session) for spend.
    done: HashSet<(String, String, String)>,
}

/// Every active watch against the sessions as they are now.
pub fn evaluate(
    watches: &[Map<String, Value>],
    rows: &[Row],
    costs: &HashMap<String, f64>,
    mem: &mut Memory,
    now: f64,
) -> Vec<Firing> {
    let mut out = vec![];
    for w in watches.iter().filter(|w| status(w) == "active") {
        let id = w.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let mine = covered(w, rows);
        let kind = w.get("kind").and_then(Value::as_str).unwrap_or("");
        let mut fire = |row_key: String, name: String, what: String| {
            out.push(Firing {
                watch: id.clone(),
                session: row_key,
                text: format!("{name} {what}"),
            });
        };
        match kind {
            "finishes" => {
                let seen: HashSet<String> = mine.iter().map(|r| key_of(r)).collect();
                for r in &mine {
                    let k = key_of(r);
                    let now_busy = registry::in_progress(r);
                    let before = mem.busy.insert((id.clone(), k.clone()), now_busy);
                    let stop = r.get("last_stop_at").and_then(Value::as_f64).unwrap_or(0.0);
                    let last_stop = mem.stops.insert((id.clone(), k.clone()), stop);
                    let stopped = last_stop.is_some_and(|s| stop > s);
                    if !now_busy && (before == Some(true) || stopped) {
                        let what = if registry::needs_attention(r) || truthy(r.get("attention")) {
                            "finished and is waiting for you"
                        } else {
                            "finished"
                        };
                        fire(k, name_of(r), what.into());
                    }
                }
                // A session that was working and is gone has ended.
                let gone: Vec<String> = mem
                    .busy
                    .iter()
                    .filter(|((wid, k), b)| *wid == id && **b && !seen.contains(k))
                    .map(|((_, k), _)| k.clone())
                    .collect();
                for k in gone {
                    mem.busy.remove(&(id.clone(), k.clone()));
                    let label = w.get("label").and_then(Value::as_str).filter(|_| w.contains_key("session"));
                    let name = label.map(str::to_string).unwrap_or_else(|| k.chars().take(8).collect());
                    fire(k, name, "ended".into());
                }
            }
            "waiting" => {
                let limit = w.get("minutes").and_then(Value::as_f64).unwrap_or(10.0) * 60.0;
                for r in &mine {
                    if !truthy(r.get("attention")) {
                        continue;
                    }
                    let since = registry::attention_start(r);
                    if since <= 0.0 || now - since < limit {
                        continue;
                    }
                    let mark = (id.clone(), key_of(r), format!("{since}"));
                    if mem.done.insert(mark) {
                        let mins = ((now - since) / 60.0).round();
                        let what = match r.get("attention_reason").and_then(Value::as_str).filter(|x| !x.is_empty()) {
                            Some(why) => format!("has waited {} min for you ({why})", fmt_num(mins)),
                            None => format!("has waited {} min for you", fmt_num(mins)),
                        };
                        fire(key_of(r), name_of(r), what);
                    }
                }
            }
            "spend" => {
                let limit = w.get("dollars").and_then(Value::as_f64).unwrap_or(f64::INFINITY);
                for r in &mine {
                    let k = key_of(r);
                    let sid = s(r, "sessionId").unwrap_or(&k).to_string();
                    let Some(c) = costs.get(&sid).or_else(|| costs.get(&k)).copied() else {
                        continue;
                    };
                    if c >= limit && mem.done.insert((id.clone(), k.clone(), String::new())) {
                        fire(k, name_of(r), format!("has cost ${c:.2} (over ${})", fmt_num(limit)));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Where the book lives: beside the state file.
pub fn book_path(state_dir: &Path) -> PathBuf {
    state_dir.join("fleet.json")
}

/// Proposals in a model's answer: each fenced block whose info string is
/// `imperatorium` holds one JSON object (or an array of them), `{"type":
/// "watch", ...}` or `{"type": "action", ...}`. Returns the answer with the
/// blocks taken out, and the objects.
/// Ask's citations, made to open their session (2026-11-21): the model
/// sometimes cites a session by its name or an id prefix instead of the
/// key it was given. Each `[[session:X]]` that names exactly one session
/// becomes `[[session:KEY]]`; one that names none (or several) becomes
/// plain `X`, so the window never shows a broken link.
pub fn resolve_cites(answer: &str, rows: &[Row]) -> String {
    static CITE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = CITE.get_or_init(|| regex::Regex::new(r"\[\[session:\s*([^\]]+?)\s*\]\]").unwrap());
    re.replace_all(answer, |c: &regex::Captures| {
        let target = &c[1];
        let hits: Vec<&Row> = rows.iter().filter(|r| names_session(r, target)).collect();
        let exact = hits.iter().find(|r| key_of(r) == target);
        match (exact, hits.as_slice()) {
            (Some(r), _) | (None, [r]) => format!("[[session:{}]]", key_of(r)),
            _ => target.to_string(),
        }
    })
    .into_owned()
}

pub fn take_proposals(answer: &str) -> (String, Vec<Map<String, Value>>) {
    let mut text = String::new();
    let mut found = vec![];
    let mut block: Option<String> = None;
    for line in answer.split_inclusive('\n') {
        let trimmed = line.trim();
        match &mut block {
            None if trimmed.starts_with("```") && trimmed[3..].trim().eq_ignore_ascii_case("imperatorium") => {
                block = Some(String::new());
            }
            None => text.push_str(line),
            Some(b) if trimmed.starts_with("```") => {
                match serde_json::from_str::<Value>(b) {
                    Ok(Value::Object(o)) => found.push(o),
                    Ok(Value::Array(a)) => found.extend(a.into_iter().filter_map(|x| x.as_object().cloned())),
                    _ => {}
                }
                block = None;
            }
            Some(b) => b.push_str(line),
        }
    }
    (text.trim().to_string(), found.into_iter().take(10).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(v: Value) -> Row {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn ask_citations_by_name_or_prefix_become_keys() {
        let rs = rows();
        let got = resolve_cites("[[session:gem-trip-4]] and [[session:bbbbbbbb-2]], [[session:bbbbbbbb]] and [[session:nobody]]", &rs);
        assert_eq!(got, "[[session:aaaaaaaa-1]] and [[session:bbbbbbbb-2]], [[session:bbbbbbbb-2]] and nobody");
    }

    fn rows() -> Vec<Row> {
        vec![
            row(json!({"key": "aaaaaaaa-1", "sessionId": "aaaaaaaa-1", "name": "gem-trip-4",
                       "cwd": "C:\\src\\gem-trip", "status": "busy"})),
            row(json!({"key": "bbbbbbbb-2", "sessionId": "bbbbbbbb-2", "name": "laser-ledger-1",
                       "cwd": "/src/laser-ledger", "status": "idle", "attention": true,
                       "attention_since": 1000.0, "attention_reason": "needs permission"})),
            row(json!({"key": "cccccccc-3", "sessionId": "cccccccc-3", "name": "gem-trip-5",
                       "cwd": "C:\\src\\gem-trip", "status": "idle"})),
        ]
    }

    #[test]
    fn summary_puts_waiting_first_and_counts() {
        let costs = HashMap::from([("aaaaaaaa-1".to_string(), 1.234)]);
        let s = summary(&rows(), &costs, &HashMap::new(), 1000.0 + 600.0);
        let list = s["sessions"].as_array().unwrap();
        assert_eq!(list[0]["name"], "laser-ledger-1");
        assert_eq!(list[0]["state"], "waiting for you");
        assert_eq!(list[0]["waiting_minutes"], 10.0);
        assert_eq!(list[0]["waiting_reason"], "needs permission");
        assert_eq!(list[1]["state"], "working");
        assert_eq!(list[1]["cost_usd"], 1.23);
        assert_eq!(list[1]["project"], "gem-trip");
        assert_eq!(s["counts"]["idle"], 1);
    }

    #[test]
    fn a_watch_names_one_running_session_by_key() {
        let p = json!({"kind": "finishes", "session": "GEM-TRIP-4"});
        let w = validate_watch(p.as_object().unwrap(), &rows()).unwrap();
        assert_eq!(w["session"], "aaaaaaaa-1");
        assert_eq!(w["once"], true);
        assert_eq!(w["text"], "when gem-trip-4 finishes its work");
        let p = json!({"kind": "finishes", "session": "nobody"});
        assert!(validate_watch(p.as_object().unwrap(), &rows()).is_err());
        let p = json!({"kind": "finishes"});
        assert!(validate_watch(p.as_object().unwrap(), &rows()).is_err());
        let p = json!({"kind": "explode"});
        assert!(validate_watch(p.as_object().unwrap(), &rows()).is_err());
        let p = json!({"kind": "waiting", "minutes": "5"});
        let w = validate_watch(p.as_object().unwrap(), &rows()).unwrap();
        assert_eq!((w["minutes"].as_f64(), &w["once"]), (Some(5.0), &json!(false)));
        assert_eq!(w["text"], "when any session waits for you more than 5 min");
        let p = json!({"kind": "spend", "project": "gem-trip"});
        assert!(validate_watch(p.as_object().unwrap(), &rows()).is_err(), "dollars missing");
    }

    #[test]
    fn actions_are_from_the_menu_and_never_slash_commands() {
        let ok = json!({"action": "queue_prompt", "session": "laser-ledger-1", "prompt": "commit it"});
        let a = validate_action(ok.as_object().unwrap(), &rows()).unwrap();
        assert_eq!(a["session_id"], "bbbbbbbb-2");
        assert_eq!(a["text"], "queue a prompt for laser-ledger-1");
        for bad in [
            json!({"action": "approve", "session": "laser-ledger-1"}),
            json!({"action": "queue_prompt", "session": "laser-ledger-1"}),
            json!({"action": "queue_prompt", "session": "laser-ledger-1", "prompt": "/config"}),
            json!({"action": "stop"}),
        ] {
            assert!(validate_action(bad.as_object().unwrap(), &rows()).is_err(), "{bad}");
        }
    }

    fn active(w: Value, id: &str) -> Map<String, Value> {
        let mut w = validate_watch(w.as_object().unwrap(), &rows()).unwrap();
        w.insert("id".into(), json!(id));
        w.insert("status".into(), json!("active"));
        w
    }

    #[test]
    fn finishes_fires_on_the_way_out_of_work_and_on_exit() {
        let w = vec![active(json!({"kind": "finishes", "project": "gem-trip"}), "w1")];
        let mut mem = Memory::default();
        let mut r = rows();
        assert!(evaluate(&w, &r, &HashMap::new(), &mut mem, 1.0).is_empty(), "first look is a baseline");
        r[0].insert("status".into(), json!("idle"));
        let got = evaluate(&w, &r, &HashMap::new(), &mut mem, 2.0);
        assert_eq!(got, vec![Firing { watch: "w1".into(), session: "aaaaaaaa-1".into(), text: "gem-trip-4 finished".into() }]);
        assert!(evaluate(&w, &r, &HashMap::new(), &mut mem, 3.0).is_empty(), "once per finish");
        // gem-trip-5 starts and then disappears.
        r[2].insert("status".into(), json!("busy"));
        assert!(evaluate(&w, &r, &HashMap::new(), &mut mem, 4.0).is_empty());
        r.remove(2);
        let got = evaluate(&w, &r, &HashMap::new(), &mut mem, 5.0);
        assert_eq!(got.len(), 1);
        assert!(got[0].text.ends_with("ended"), "{got:?}");
    }

    /// 2026-11-21, live: a 2 s turn between two 5 s looks was never seen
    /// busy, so its finish was missed; its Stop hook's time is seen.
    #[test]
    fn finishes_fires_for_a_turn_too_short_to_be_seen_working() {
        let w = vec![active(json!({"kind": "finishes", "project": "gem-trip"}), "w1")];
        let mut mem = Memory::default();
        let mut r = rows();
        r[0].insert("status".into(), json!("idle"));
        r[0].insert("last_stop_at".into(), json!(100.0));
        assert!(evaluate(&w, &r, &HashMap::new(), &mut mem, 1.0).is_empty(), "an old Stop is the baseline");
        r[0].insert("last_stop_at".into(), json!(103.0));
        let got = evaluate(&w, &r, &HashMap::new(), &mut mem, 2.0);
        assert_eq!(got, vec![Firing { watch: "w1".into(), session: "aaaaaaaa-1".into(), text: "gem-trip-4 finished".into() }]);
        assert!(evaluate(&w, &r, &HashMap::new(), &mut mem, 3.0).is_empty(), "once per Stop");
        // A Stop seen while it is already working again waits for that turn.
        r[0].insert("status".into(), json!("busy"));
        r[0].insert("last_stop_at".into(), json!(110.0));
        assert!(evaluate(&w, &r, &HashMap::new(), &mut mem, 4.0).is_empty());
        r[0].insert("status".into(), json!("idle"));
        assert_eq!(evaluate(&w, &r, &HashMap::new(), &mut mem, 5.0).len(), 1);
    }

    #[test]
    fn waiting_fires_once_per_raise_after_the_threshold() {
        let w = vec![active(json!({"kind": "waiting", "minutes": 5}), "w2")];
        let mut mem = Memory::default();
        let mut r = rows();
        assert!(evaluate(&w, &r, &HashMap::new(), &mut mem, 1000.0 + 299.0).is_empty());
        let got = evaluate(&w, &r, &HashMap::new(), &mut mem, 1000.0 + 301.0);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "laser-ledger-1 has waited 5 min for you (needs permission)");
        assert!(evaluate(&w, &r, &HashMap::new(), &mut mem, 1000.0 + 900.0).is_empty());
        r[1].insert("attention_since".into(), json!(2000.0));
        assert_eq!(evaluate(&w, &r, &HashMap::new(), &mut mem, 2000.0 + 301.0).len(), 1, "a new raise");
    }

    #[test]
    fn spend_fires_once_and_paused_watches_never() {
        let mut w = vec![active(json!({"kind": "spend", "dollars": 2}), "w3")];
        let mut mem = Memory::default();
        let mut costs = HashMap::from([("aaaaaaaa-1".to_string(), 1.5)]);
        assert!(evaluate(&w, &rows(), &costs, &mut mem, 1.0).is_empty());
        costs.insert("aaaaaaaa-1".into(), 2.5);
        let got = evaluate(&w, &rows(), &costs, &mut mem, 2.0);
        assert_eq!(got[0].text, "gem-trip-4 has cost $2.50 (over $2)");
        assert!(evaluate(&w, &rows(), &costs, &mut mem, 3.0).is_empty());
        w[0].insert("status".into(), json!("paused"));
        costs.insert("cccccccc-3".into(), 9.0);
        assert!(evaluate(&w, &rows(), &costs, &mut mem, 4.0).is_empty());
    }

    #[test]
    fn the_book_keeps_proposals_until_the_window_answers() {
        let dir = std::env::temp_dir().join(format!("imp-fleet-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = book_path(&dir);
        let _ = std::fs::remove_file(&path);
        let mut b = Book::open(Some(path.clone()));
        let w = validate_watch(json!({"kind": "waiting"}).as_object().unwrap(), &rows()).unwrap();
        let w = b.add_watch(w, false, "agent", 1.0).unwrap();
        assert_eq!(w["status"], "proposed");
        let id = w["id"].as_str().unwrap().to_string();
        assert!(b.set_watch_status(&id, "paused").is_err(), "a proposal is not paused, it is confirmed");
        assert_eq!(b.set_watch_status(&id, "active").unwrap()["status"], "active");
        let again = Book::open(Some(path.clone()));
        assert_eq!(again.watches.len(), 1, "kept on disk");
        assert_eq!(again.view()["proposed"], 0);
        let f = Firing { watch: id.clone(), session: "s".into(), text: "t".into() };
        b.record_fired(&f, 2.0);
        assert_eq!(b.watches[0]["fired_count"], 1);
        assert_eq!(b.watches[0]["status"], "active", "a standing rule stays");
        assert!(b.remove_watch(&id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn proposals_come_out_of_fenced_blocks() {
        let answer = "Two sessions wait.\n\n```imperatorium\n{\"type\": \"watch\", \"kind\": \"waiting\", \"minutes\": 5}\n```\nMore text.\n```imperatorium\n[{\"type\": \"action\"}, 3]\n```\n```rust\nfn x() {}\n```\n";
        let (text, found) = take_proposals(answer);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0]["kind"], "waiting");
        assert!(text.contains("Two sessions wait.") && text.contains("More text.") && text.contains("fn x()"));
        assert!(!text.contains("imperatorium"));
    }
}
