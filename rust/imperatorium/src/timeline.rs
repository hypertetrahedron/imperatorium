//! The session timeline (after claude-lens's): one point per API request of
//! the main conversation, the prompts that opened each turn, and one span per
//! subagent run - read straight from the transcript the session is writing.
//!
//! The rules are claude-lens's (`build_dashboard.cache_scan`,
//! `agent_spans`, `jsonl_ingest`), applied to one file instead of a database:
//!
//! - a request is an assistant entry's `requestId`; its lines repeat it, and
//!   the one with the most output carries the final usage;
//! - context is input + cache read + cache write, as the CLI measures it;
//! - a cache miss is a request that wrote more than half its context to the
//!   cache while the request before it was reading from one, and its cause
//!   is the first that fits: model, effort or speed switched, a compaction
//!   landed between them, or the session sat idle past the cache's hour;
//! - a turn starts at the first request after a prompt somebody typed;
//!   harness turns (task notifications, peers, plugins) fold into it;
//! - the tool calls a point carries are those made since the request before
//!   it, with their failures and the size of what came back;
//! - a subagent runs from the call that launched it to its last request.
//!
//! Subagent requests stay out of the series: they run against their own
//! context, and mixing them in would draw a sawtooth that never happened.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use crate::pyish::{py_str, truthy};

const MISS_SHARE: f64 = 0.5;
/// Minutes the main conversation's cache lives (claude-lens MAIN_TTL_MIN).
const TTL_MIN: f64 = 60.0;
const PROMPT_CHARS: usize = 400;

/// Openers of harness-injected turns (claude-lens INJECTED_PREFIXES).
const INJECTED: &[&str] = &[
    "<task-notification>",
    "<teammate-message",
    "<system-reminder>",
    "<command-name>",
    "<local-command-caveat>",
    "Caveat: The messages below",
    "This session is being continued",
    "Another Claude session sent a message:",
    "[Request interrupted",
];
const WRAPPERS: &[&str] = &["<ide_", "<system-reminder>"];

fn s<'a>(e: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    e.get(k).and_then(Value::as_str)
}

fn at(e: &Map<String, Value>) -> Option<f64> {
    crate::sessionstat::when(e.get("timestamp"))
}

fn origin_kind(e: &Map<String, Value>) -> Option<&str> {
    e.get("origin").and_then(Value::as_object).and_then(|o| s(o, "kind"))
}

fn text_blocks(msg: Option<&Map<String, Value>>) -> Vec<String> {
    match msg.and_then(|m| m.get("content")) {
        Some(Value::String(t)) => vec![t.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_object)
            .filter(|b| s(b, "type") == Some("text"))
            .map(|b| s(b, "text").unwrap_or("").to_string())
            .collect(),
        _ => vec![],
    }
}

fn is_wrapper(t: &str) -> bool {
    let t = t.trim_start();
    WRAPPERS.iter().any(|w| t.starts_with(w))
}

/// What the person typed: the text blocks without the IDE's envelopes.
fn prompt_text(msg: Option<&Map<String, Value>>) -> String {
    let blocks = text_blocks(msg);
    let kept: Vec<&str> = blocks
        .iter()
        .map(String::as_str)
        .filter(|b| !b.trim().is_empty() && !is_wrapper(b))
        .collect();
    let text = if kept.is_empty() {
        blocks.iter().map(|b| b.trim()).filter(|b| !b.is_empty()).collect::<Vec<_>>().join("\n")
    } else {
        kept.join("\n")
    };
    text.chars().take(PROMPT_CHARS).collect()
}

fn has_tool_result(msg: Option<&Map<String, Value>>) -> bool {
    matches!(msg.and_then(|m| m.get("content")), Some(Value::Array(a))
        if a.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")))
}

fn is_injected(msg: Option<&Map<String, Value>>) -> bool {
    let blocks: Vec<String> = text_blocks(msg).into_iter().filter(|b| !b.trim().is_empty()).collect();
    !blocks.is_empty()
        && blocks.iter().all(|b| {
            let t = b.trim_start();
            is_wrapper(t) || INJECTED.iter().any(|p| t.starts_with(p))
        })
}

/// Does this entry open a turn? jsonl_ingest.is_human_prompt, with two
/// differences. A prompt Imperatorium sent through the session's socket (or
/// another session's message) arrives as a "peer" turn, and opens one. And
/// the shape rule holds in every file: this CLI marks a typed prompt with no
/// origin at all, so a file that holds one peer turn - which claude-lens
/// reads as "modern" - would otherwise have no prompts in it.
fn is_human_prompt(e: &Map<String, Value>) -> bool {
    if s(e, "type") != Some("user") || truthy(e.get("isSidechain")) || truthy(e.get("isCompactSummary")) {
        return false;
    }
    if e.get("origin").is_some_and(Value::is_object) {
        return matches!(origin_kind(e), Some("human") | Some("peer"));
    }
    if truthy(e.get("isMeta")) {
        return false;
    }
    let msg = e.get("message").and_then(Value::as_object);
    if has_tool_result(msg) || s(e, "promptSource") == Some("system") || is_injected(msg) {
        return false;
    }
    !prompt_text(msg).trim().is_empty()
}

fn is_compaction(e: &Map<String, Value>, text: Option<&str>) -> bool {
    truthy(e.get("isCompactSummary"))
        || truthy(e.get("compactMetadata"))
        || matches!(s(e, "type"), Some("compact-boundary") | Some("summary"))
        || text.is_some_and(|t| t.trim_start().starts_with("This session is being continued"))
}

fn content_len(c: Option<&Value>) -> i64 {
    match c {
        None | Some(Value::Null) => 0,
        Some(Value::String(t)) => t.chars().count() as i64,
        Some(Value::Array(a)) => a
            .iter()
            .map(|b| match b {
                Value::String(t) => t.chars().count() as i64,
                Value::Object(o) => {
                    if let Some(t) = s(o, "text") {
                        t.chars().count() as i64
                    } else if let Some(d) = o.get("source").and_then(|x| x.get("data")).and_then(Value::as_str) {
                        d.len() as i64
                    } else {
                        b.to_string().len() as i64
                    }
                }
                other => other.to_string().len() as i64,
            })
            .sum(),
        Some(v) => v.to_string().len() as i64,
    }
}

fn int(v: Option<&Value>) -> i64 {
    v.and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64))).unwrap_or(0)
}

#[derive(Clone, Default)]
struct Req {
    t: f64,
    order: usize,
    model: String,
    effort: Option<String>,
    speed: Option<String>,
    input: i64,
    output: i64,
    cr: i64,
    cw: i64,
    c5: i64,
    c1: i64,
    turn: Option<usize>,
}

impl Req {
    fn cost(&self) -> (f64, bool) {
        let (key, known) = crate::costs::resolve(Some(&json!(self.model)));
        if !known {
            return (0.0, false);
        }
        let unsplit = (self.cw - self.c5 - self.c1).max(0);
        (crate::costs::price(&key, &[self.input, self.output, self.cr, self.c5 + unsplit, self.c1]), true)
    }
}

/// Every request a transcript records, keyed by requestId, from its lines.
fn note_request(reqs: &mut HashMap<String, Req>, e: &Map<String, Value>, turn: Option<usize>) {
    let Some(msg) = e.get("message").and_then(Value::as_object) else { return };
    let Some(u) = msg.get("usage").and_then(Value::as_object) else { return };
    let Some(rid) = s(e, "requestId").filter(|r| !r.is_empty()) else { return };
    let out = int(u.get("output_tokens"));
    let n = reqs.len();
    let r = reqs.entry(rid.to_string()).or_insert_with(|| Req { order: n, output: -1, ..Default::default() });
    if r.output >= out {
        return;
    }
    let cc = u.get("cache_creation").and_then(Value::as_object);
    r.t = at(e).unwrap_or(r.t);
    r.model = s(msg, "model").unwrap_or("?").to_string();
    r.effort = s(e, "effort").map(str::to_string);
    r.speed = s(u, "speed").map(|v| if v == "normal" { "standard".to_string() } else { v.to_string() });
    r.input = int(u.get("input_tokens"));
    r.output = out;
    r.cr = int(u.get("cache_read_input_tokens"));
    r.cw = int(u.get("cache_creation_input_tokens"));
    r.c5 = int(cc.and_then(|c| c.get("ephemeral_5m_input_tokens")));
    r.c1 = int(cc.and_then(|c| c.get("ephemeral_1h_input_tokens")));
    if r.turn.is_none() {
        r.turn = turn;
    }
}

struct Call {
    t: f64,
    name: String,
    turn: Option<usize>,
    err: bool,
    bytes: Option<i64>,
    ms: Option<f64>,
    agent: Option<(Option<String>, Option<String>)>,
}

struct Turn {
    t: f64,
    text: String,
    cost: f64,
    priced: bool,
    out: i64,
    last: f64,
}

fn entries(path: &Path) -> Vec<Map<String, Value>> {
    let Ok(raw) = std::fs::read(path) else { return vec![] };
    String::from_utf8_lossy(&raw)
        .lines()
        .filter_map(|l| match serde_json::from_str::<Value>(l.trim()) {
            Ok(Value::Object(o)) => Some(o),
            _ => None,
        })
        .collect()
}

/// The timeline of the session whose transcript is `path`.
pub fn read(path: &Path) -> Value {
    let es = entries(path);

    let mut turns: Vec<Turn> = vec![];
    let mut cur: Option<usize> = None;
    let mut human_seen = false;
    let mut reqs: HashMap<String, Req> = HashMap::new();
    let mut calls: Vec<Call> = vec![];
    let mut call_ix: HashMap<String, usize> = HashMap::new();
    let mut compactions: Vec<f64> = vec![];

    for e in &es {
        let kind = s(e, "type");
        let msg = e.get("message").and_then(Value::as_object);
        let side = truthy(e.get("isSidechain")) || truthy(e.get("agentId"));
        if is_human_prompt(e) {
            let text = if origin_kind(e) == Some("peer") {
                crate::conversation::unwrap_peer(msg.and_then(|m| m.get("content"))).chars().take(PROMPT_CHARS).collect()
            } else {
                prompt_text(msg)
            };
            if is_compaction(e, Some(&text)) {
                compactions.extend(at(e));
            }
            turns.push(Turn { t: at(e).unwrap_or(0.0), text, cost: 0.0, priced: true, out: 0, last: 0.0 });
            cur = Some(turns.len() - 1);
            human_seen = true;
            continue;
        }
        if is_compaction(e, None) {
            compactions.extend(at(e));
        }
        match kind {
            Some("assistant") if !side && cur.is_some() => {
                let t = at(e).unwrap_or(0.0);
                if let Some(Value::Array(blocks)) = msg.and_then(|m| m.get("content")) {
                    for b in blocks.iter().filter_map(Value::as_object) {
                        if s(b, "type") != Some("tool_use") {
                            continue;
                        }
                        let Some(id) = s(b, "id") else { continue };
                        let name = s(b, "name").unwrap_or("?");
                        let input = b.get("input").and_then(Value::as_object);
                        let display = match (name, input.and_then(|i| s(i, "skill"))) {
                            ("Skill", Some(sk)) => format!("Skill:{sk}"),
                            _ => name.to_string(),
                        };
                        let agent = (name == "Agent" || name == "Task").then(|| {
                            (
                                input.and_then(|i| s(i, "subagent_type")).map(str::to_string),
                                input.and_then(|i| s(i, "description")).map(str::to_string),
                            )
                        });
                        if !call_ix.contains_key(id) {
                            call_ix.insert(id.to_string(), calls.len());
                            calls.push(Call { t, name: display, turn: cur, err: false, bytes: None, ms: None, agent });
                        }
                    }
                }
                note_request(&mut reqs, e, cur);
            }
            Some("user") if !side => {
                // A harness turn before anybody typed starts a turn of its own.
                if !human_seen && truthy(e.get("promptId")) && !has_tool_result(msg) {
                    let text = prompt_text(msg);
                    if !text.trim().is_empty() {
                        turns.push(Turn { t: at(e).unwrap_or(0.0), text, cost: 0.0, priced: true, out: 0, last: 0.0 });
                        cur = Some(turns.len() - 1);
                    }
                }
                let result = e.get("toolUseResult").and_then(Value::as_object);
                let extra = result.map_or(0, |r| {
                    (s(r, "stdout").unwrap_or("").chars().count() + s(r, "stderr").unwrap_or("").chars().count()) as i64
                });
                let errored = result.is_some_and(|r| truthy(r.get("error")) || truthy(r.get("isError")));
                if let Some(Value::Array(blocks)) = msg.and_then(|m| m.get("content")) {
                    for b in blocks.iter().filter_map(Value::as_object) {
                        if s(b, "type") != Some("tool_result") {
                            continue;
                        }
                        let Some(&i) = s(b, "tool_use_id").and_then(|id| call_ix.get(id)) else { continue };
                        let c = &mut calls[i];
                        if c.bytes.is_none() {
                            c.bytes = Some(content_len(b.get("content")).max(extra));
                            c.ms = at(e).map(|t| ((t - c.t) * 1000.0).max(0.0));
                        }
                        c.err |= truthy(b.get("is_error")) || errored;
                    }
                }
            }
            _ => {}
        }
    }

    // The requests in the order the CLI made them.
    let mut list: Vec<Req> = reqs.into_values().collect();
    list.sort_by(|a, b| a.t.total_cmp(&b.t).then(a.order.cmp(&b.order)));
    compactions.sort_by(f64::total_cmp);
    let mut order: Vec<usize> = (0..calls.len()).collect();
    order.sort_by(|&a, &b| calls[a].t.total_cmp(&calls[b].t));

    let mut points: Vec<Value> = Vec::with_capacity(list.len());
    let mut prev: Option<&Req> = None;
    let (mut ci, mut ti) = (0usize, 0usize);
    let mut last_turn: Option<usize> = None;
    for r in &list {
        let mut seen: Vec<&str> = vec![];
        while ci < compactions.len() && compactions[ci] <= r.t {
            seen.push("compact");
            ci += 1;
        }
        if let Some(p) = prev {
            // Switches are events of their own, ahead of a compaction.
            let mut sw = vec![];
            if p.model != r.model {
                sw.push("model_switch");
            }
            if matches!((&p.effort, &r.effort), (Some(a), Some(b)) if a != b) {
                sw.push("effort_switch");
            }
            if matches!((&p.speed, &r.speed), (Some(a), Some(b)) if a != b) {
                sw.push("speed_switch");
            }
            sw.extend(seen.iter().copied());
            seen = sw;
        }
        let (mut n, mut nerr) = (0i64, 0i64);
        let mut by: Vec<(String, i64, i64, Option<f64>, Option<i64>)> = vec![];
        while ti < order.len() && calls[order[ti]].t <= r.t {
            let c = &calls[order[ti]];
            n += 1;
            nerr += c.err as i64;
            let g = match by.iter_mut().position(|g| g.0 == c.name) {
                Some(i) => &mut by[i],
                None => {
                    by.push((c.name.clone(), 0, 0, None, None));
                    by.last_mut().unwrap()
                }
            };
            g.1 += 1;
            g.2 += c.err as i64;
            if let Some(ms) = c.ms {
                g.3 = Some(g.3.unwrap_or(0.0) + ms);
            }
            if let Some(b) = c.bytes {
                g.4 = Some(g.4.unwrap_or(0) + b);
            }
            ti += 1;
        }
        by.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let ctx = r.input + r.cr + r.cw;
        let mut miss = 0;
        let mut cause = Value::Null;
        if let Some(p) = prev {
            if r.cw as f64 > MISS_SHARE * (r.cr + r.cw) as f64 && p.cr > 0 {
                miss = 1;
                cause = json!(if p.model != r.model {
                    "model_switch"
                } else if matches!((&p.effort, &r.effort), (Some(a), Some(b)) if a != b) {
                    "effort_switch"
                } else if matches!((&p.speed, &r.speed), (Some(a), Some(b)) if a != b) {
                    "speed_switch"
                } else if seen.contains(&"compact") {
                    "compact"
                } else if (r.t - p.t) / 60.0 > TTL_MIN {
                    "idle_gap"
                } else {
                    "unknown"
                });
            }
        }
        let turn = r.turn.is_some() && r.turn != last_turn;
        if r.turn.is_some() {
            last_turn = r.turn;
        }
        let mut p = Map::new();
        p.insert("t".into(), json!(r.t.floor() as i64));
        p.insert("ctx".into(), json!(ctx));
        p.insert("cr".into(), json!(r.cr));
        p.insert("cw".into(), json!(r.cw));
        p.insert("model".into(), json!(r.model));
        if miss == 1 {
            p.insert("miss".into(), json!(1));
            p.insert("cause".into(), cause);
        }
        if let Some(ev) = seen.first() {
            p.insert("event".into(), json!(ev));
        }
        if turn {
            p.insert("turn".into(), json!(1));
        }
        if n > 0 {
            p.insert("tools".into(), json!(n));
            if nerr > 0 {
                p.insert("terr".into(), json!(nerr));
            }
            p.insert(
                "tl".into(),
                Value::Array(
                    by.iter()
                        .map(|g| json!({"name": g.0, "n": g.1, "err": g.2, "ms": g.3.map(|m| m.round()), "bytes": g.4}))
                        .collect(),
                ),
            );
        }
        points.push(Value::Object(p));
        if let Some(i) = r.turn {
            let (c, priced) = r.cost();
            let tn = &mut turns[i];
            tn.cost += c;
            tn.priced &= priced;
            tn.out += r.output.max(0);
            tn.last = tn.last.max(r.t);
        }
        prev = Some(r);
    }

    let spans = agent_spans(path, &calls, &call_ix, &mut turns);
    let rows: Vec<Value> = turns
        .iter()
        .map(|t| {
            json!({"ts": t.t, "text": t.text, "cost": (t.cost * 1e4).round() / 1e4, "est": true,
                   "out": t.out, "wall_s": (t.last - t.t).max(0.0).round() as i64})
        })
        .collect();
    json!({"ok": true, "points": points, "spans": spans, "rows": rows})
}

/// One span per subagent the session ran (build_dashboard.agent_spans), its
/// requests' cost and output added to the turn that launched it.
fn agent_spans(path: &Path, calls: &[Call], call_ix: &HashMap<String, usize>, turns: &mut [Turn]) -> Vec<Value> {
    let dir = crate::taskout::subagents_dir(Some(path));
    let mut out: Vec<Value> = vec![];
    let mut matched: std::collections::HashSet<usize> = Default::default();
    let mut files: Vec<PathBuf> = dir
        .as_deref()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("agent-") && n.ends_with(".jsonl"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    for f in files {
        let meta: Value = std::fs::read_to_string(f.with_extension("meta.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        let launch = meta.get("toolUseId").and_then(Value::as_str).and_then(|id| call_ix.get(id)).copied();
        if let Some(i) = launch {
            matched.insert(i);
        }
        let es = entries(&f);
        let mut reqs: HashMap<String, Req> = HashMap::new();
        let mut first: Option<f64> = None;
        for e in &es {
            if first.is_none() {
                first = at(e);
            }
            if s(e, "type") == Some("assistant") {
                note_request(&mut reqs, e, None);
            }
        }
        let mut list: Vec<Req> = reqs.into_values().collect();
        list.sort_by(|a, b| a.t.total_cmp(&b.t));
        let Some(t0) = launch.map(|i| calls[i].t).or(first).or(list.first().map(|r| r.t)) else { continue };
        let t1 = list.last().map_or(t0, |r| r.t).max(t0);
        let (mut cost, mut priced, mut outn) = (0.0, true, 0i64);
        for r in &list {
            let (c, p) = r.cost();
            cost += c;
            priced &= p;
            outn += r.output.max(0);
        }
        let call_agent = launch.and_then(|i| calls[i].agent.clone()).unwrap_or((None, None));
        let typ = meta.get("agentType").and_then(Value::as_str).map(str::to_string).or(call_agent.0);
        let desc = meta.get("description").and_then(Value::as_str).map(str::to_string).or(call_agent.1);
        let model = list.first().map(|r| r.model.clone()).or_else(|| meta.get("model").map(py_str));
        if let Some(ti) = launch.and_then(|i| calls[i].turn) {
            let tn = &mut turns[ti];
            tn.cost += cost;
            tn.priced &= priced;
            tn.out += outn;
            tn.last = tn.last.max(t1);
        }
        out.push(json!({"t0": t0.floor() as i64, "t1": t1.floor() as i64, "type": typ, "desc": desc,
                        "model": model, "calls": list.len(), "out": outn,
                        "cost": (cost * 1e4).round() / 1e4, "est": 1}));
    }
    // Launched, but no transcript of its own (yet): still a run.
    for (i, c) in calls.iter().enumerate() {
        if let (Some((typ, desc)), false) = (&c.agent, matched.contains(&i)) {
            out.push(json!({"t0": c.t.floor() as i64, "t1": c.t.floor() as i64, "type": typ, "desc": desc,
                            "model": null, "calls": 0, "out": 0, "cost": 0.0, "est": 0}));
        }
    }
    out.sort_by(|a, b| {
        let k = |v: &Value| (v["t0"].as_i64().unwrap_or(0), v["t1"].as_i64().unwrap_or(0));
        k(a).cmp(&k(b))
    });
    out
}

/// The timeline of session `session_id` on this machine, reread only when its
/// transcript (or a subagent's) has changed since the last ask.
pub fn of_session(session_id: &str, known: &str) -> Value {
    static CACHE: Mutex<Option<(PathBuf, String, Value)>> = Mutex::new(None);
    if !crate::sessions::valid_uuid(session_id) {
        return json!({"ok": false, "error": "not a session id"});
    }
    let Some(path) = crate::sessionstat::transcript_path(session_id) else {
        return json!({"ok": false, "error": "no transcript for this session"});
    };
    let stamp = |p: &Path| {
        std::fs::metadata(p)
            .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
            .unwrap_or_default()
    };
    let mut key = stamp(&path);
    if let Some(d) = crate::taskout::subagents_dir(Some(&path)) {
        if let Ok(rd) = std::fs::read_dir(&d) {
            let mut v: Vec<String> = rd.flatten().map(|e| stamp(&e.path())).collect();
            v.sort();
            key.push_str(&v.join(","));
        }
    }
    if let Ok(g) = CACHE.lock() {
        if let Some((p, k, v)) = g.as_ref() {
            if *p == path && *k == key {
                // The caller has this one already: say so, not all of it again.
                if !known.is_empty() && v.get("sig").and_then(Value::as_str) == Some(known) {
                    return json!({"ok": true, "same": true, "sig": known});
                }
                return v.clone();
            }
        }
    }
    let mut v = read(&path);
    let sig = format!("{:x}", {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        h.finish()
    });
    v.as_object_mut().unwrap().insert("sig".into(), json!(sig));
    if let Ok(mut g) = CACHE.lock() {
        *g = Some((path, key, v.clone()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, lines: &[Value]) -> PathBuf {
        let p = dir.join(name);
        let body: Vec<String> = lines.iter().map(Value::to_string).collect();
        std::fs::write(&p, body.join("\n")).unwrap();
        p
    }

    fn asst(rid: &str, ts: &str, model: &str, cr: i64, cw: i64, out: i64, tools: Value) -> Value {
        json!({"type": "assistant", "uuid": rid, "timestamp": ts, "requestId": rid,
               "message": {"model": model, "content": tools,
                           "usage": {"input_tokens": 10, "output_tokens": out,
                                     "cache_read_input_tokens": cr, "cache_creation_input_tokens": cw}}})
    }

    #[test]
    fn requests_turns_tools_misses_and_subagents() {
        let dir = std::env::temp_dir().join(format!("imp-tl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("s/subagents")).unwrap();
        let main = write(&dir, "s.jsonl", &[
            json!({"type": "user", "uuid": "u1", "promptId": "p1", "timestamp": "2026-11-14T10:00:00Z",
                   "message": {"role": "user", "content": "first prompt"}}),
            asst("r1", "2026-11-14T10:00:05Z", "claude-opus-5-5", 0, 5000, 1, json!([])),
            asst("r1", "2026-11-14T10:00:06Z", "claude-opus-5-5", 0, 5000, 40,
                 json!([{"type": "tool_use", "id": "t1", "name": "Bash", "input": {}},
                        {"type": "tool_use", "id": "t2", "name": "Agent",
                         "input": {"subagent_type": "Explore", "description": "look"}}])),
            json!({"type": "user", "uuid": "u2", "promptId": "p1", "timestamp": "2026-11-14T10:00:09Z",
                   "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1",
                                                             "content": "oops", "is_error": true}]}}),
            asst("r2", "2026-11-14T10:00:10Z", "claude-opus-5-5", 5000, 100, 7, json!([])),
            json!({"type": "user", "uuid": "u3", "promptId": "p2", "timestamp": "2026-11-14T12:00:00Z",
                   "isMeta": true, "origin": {"kind": "peer"},
                   "message": {"role": "user", "content": "second"}}),
            json!({"type": "user", "uuid": "u4", "promptId": "p3", "timestamp": "2026-11-14T12:00:01Z",
                   "origin": {"kind": "task-notification"},
                   "message": {"role": "user", "content": "<task-notification>done</task-notification>"}}),
            asst("r3", "2026-11-14T12:00:02Z", "claude-opus-5-5", 0, 5200, 3, json!([])),
        ]);
        write(&dir.join("s/subagents"), "agent-a1.jsonl", &[
            asst("x1", "2026-11-14T10:00:20Z", "claude-haiku-5-5", 0, 900, 50, json!([])),
            asst("x2", "2026-11-14T10:01:00Z", "claude-haiku-5-5", 900, 10, 20, json!([])),
        ]);
        std::fs::write(dir.join("s/subagents/agent-a1.meta.json"),
                       json!({"agentType": "Explore", "description": "look", "toolUseId": "t2"}).to_string()).unwrap();
        let v = read(&main);
        let pts = v["points"].as_array().unwrap();
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0]["ctx"], 5010);
        assert_eq!(pts[0]["turn"], 1);
        // Both calls went out with the first request.
        assert_eq!(pts[0]["tools"], 2);
        assert_eq!(pts[0]["terr"], 1);
        assert!(pts[1].get("turn").is_none());
        // Two hours idle, then the cache was written over again.
        assert_eq!(pts[2]["miss"], 1);
        assert_eq!(pts[2]["cause"], "idle_gap");
        assert_eq!(pts[2]["turn"], 1);
        let spans = v["spans"].as_array().unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0]["type"], "Explore");
        assert_eq!(spans[0]["calls"], 2);
        assert_eq!(spans[0]["t1"].as_i64().unwrap() - spans[0]["t0"].as_i64().unwrap(), 54);
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["text"], "first prompt");
        // Sent from Imperatorium (a peer turn); the notification folds in.
        assert_eq!(rows[1]["text"], "second");
        // The subagent's output is the turn's that launched it.
        assert_eq!(rows[0]["out"], 40 + 7 + 70);
        assert_eq!(rows[0]["wall_s"], 60);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

