//! List prices and what a cold cache costs: the parts of
//! `imperatorium/costs.py` the session rows use (`resolve`, `rewrite_cost`).
//! The spend views (windows, model mix) come with their endpoints.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use crate::pyish::{py_str, truthy};

/// (model id, input $/Mtok, output $/Mtok), in costs.PRICES' order.
pub const PRICES: &[(&str, f64, f64)] = &[
    ("claude-fable-5-1", 10.0, 50.0),
    ("claude-fable-5", 10.0, 50.0),
    ("claude-mythos-5-1", 10.0, 50.0),
    ("claude-opus-5-5", 4.0, 20.0),
    ("claude-opus-5", 5.0, 25.0),
    ("claude-opus-4-8", 5.0, 25.0),
    ("claude-opus-4-7", 5.0, 25.0),
    ("claude-opus-4-6", 5.0, 25.0),
    ("claude-opus-4-5", 5.0, 25.0),
    ("claude-opus-4-1", 15.0, 75.0),
    ("claude-opus-4", 15.0, 75.0),
    ("claude-sonnet-5-5", 2.0, 10.0),
    ("claude-sonnet-5", 2.0, 10.0),
    ("claude-sonnet-4-6", 3.0, 15.0),
    ("claude-sonnet-4-5", 3.0, 15.0),
    ("claude-sonnet-4", 3.0, 15.0),
    ("claude-haiku-5-5", 0.10, 0.50),
    ("claude-haiku-4-5", 1.0, 5.0),
    ("claude-3-5-haiku", 0.80, 4.0),
];
/// Newest of each family, for an id the table has not heard of.
const FAMILIES: &[(&str, &str)] = &[
    ("fable", "claude-fable-5-1"),
    ("opus", "claude-opus-5-5"),
    ("sonnet", "claude-sonnet-5-5"),
    ("haiku", "claude-haiku-5-5"),
];
const WRITE_5M: f64 = 1.25;
const WRITE_1H: f64 = 2.0;
const UNKNOWN: &str = "(unknown)";

/// (price key, priced) for a model id as a transcript spells it.
pub fn resolve(model: Option<&Value>) -> (String, bool) {
    static BRACKETS: OnceLock<Regex> = OnceLock::new();
    let re = BRACKETS.get_or_init(|| Regex::new(r"\[.*?\]").unwrap());
    let empty = json!("");
    let raw = py_str(if truthy(model) {
        model.unwrap()
    } else {
        &empty
    });
    let name = crate::pyish::strip(&re.replace_all(&raw, "")).to_lowercase();
    let mut best: Option<&str> = None;
    for (known, _, _) in PRICES {
        let hit = name == *known || name.starts_with(&format!("{known}-"));
        if hit && best.is_none_or(|b| known.chars().count() > b.chars().count()) {
            best = Some(known);
        }
    }
    if let Some(b) = best {
        return (b.to_string(), true);
    }
    for (family, known) in FAMILIES {
        if name.contains(family) {
            return (known.to_string(), true);
        }
    }
    (
        if name.is_empty() {
            UNKNOWN.to_string()
        } else {
            name
        },
        false,
    )
}

/// Dollars to write `tokens` of context back into the cache; None for a
/// model with no known price or no tokens.
pub fn rewrite_cost(model: Option<&Value>, tokens: Option<&Value>, ttl: Option<&Value>) -> Value {
    let (key, priced) = resolve(model);
    if !priced || !truthy(tokens) {
        return Value::Null;
    }
    let Some(t) = tokens.and_then(Value::as_f64) else {
        return Value::Null;
    };
    let pin = PRICES
        .iter()
        .find(|p| p.0 == key)
        .map(|p| p.1)
        .unwrap_or(0.0);
    let ttl = ttl.and_then(Value::as_f64).unwrap_or(0.0);
    let write = if ttl >= 3600.0 { WRITE_1H } else { WRITE_5M };
    json!(t * pin * write / 1e6)
}

// -- the spend history (costs.History) ------------------------------------------

const READ_DEFAULT: f64 = 0.1;
const SLACK: f64 = 86400.0;
const HEAD: usize = 256;
const WORKTREE: &str = "/.claude/worktrees/";

fn read_ratio(key: &str) -> f64 {
    match key {
        "claude-opus-5-5" => 0.05,
        "claude-fable-5-1" | "claude-mythos-5-1" => 0.025,
        _ => READ_DEFAULT,
    }
}

/// costs.price: dollars for one token vector at a priced model's list price,
/// the terms added in Python's order so the floats agree.
pub fn price(key: &str, v: &[i64; 5]) -> f64 {
    let (pin, pout) = PRICES
        .iter()
        .find(|p| p.0 == key)
        .map(|p| (p.1, p.2))
        .unwrap_or((0.0, 0.0));
    let read = read_ratio(key);
    (v[0] as f64 * pin
        + v[1] as f64 * pout
        + v[2] as f64 * pin * read
        + v[3] as f64 * pin * WRITE_5M
        + v[4] as f64 * pin * WRITE_1H)
        / 1e6
}

/// costs._num: int() of a number (not a bool), else 0.
fn inum(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n
            .as_i64()
            .unwrap_or_else(|| n.as_f64().map(|f| f.trunc() as i64).unwrap_or(0)),
        _ => 0,
    }
}

/// costs._vector: [input, output, cache read, cache write 5m, 1h].
fn vector(usage: &serde_json::Map<String, Value>) -> [i64; 5] {
    let total = inum(usage.get("cache_creation_input_tokens"));
    let (mut w5, mut w1) = (total, 0);
    if let Some(Value::Object(made)) = usage.get("cache_creation") {
        if made.contains_key("ephemeral_5m_input_tokens")
            || made.contains_key("ephemeral_1h_input_tokens")
        {
            w5 = inum(made.get("ephemeral_5m_input_tokens"));
            w1 = inum(made.get("ephemeral_1h_input_tokens"));
            w5 += (total - w5 - w1).max(0);
        }
    }
    [
        inum(usage.get("input_tokens")),
        inum(usage.get("output_tokens")),
        inum(usage.get("cache_read_input_tokens")),
        w5,
        w1,
    ]
}

/// costs.project_of: the repository a working directory belongs to.
pub fn project_of(cwd: Option<&str>) -> String {
    let mut path = cwd.unwrap_or("").replace('\\', "/");
    let with = format!("{path}/");
    if with.contains(WORKTREE) {
        path = with.split(WORKTREE).next().unwrap_or("").to_string();
    }
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return UNKNOWN.into();
    }
    let leaf = path.rsplit('/').next().unwrap_or("");
    if leaf.is_empty() {
        UNKNOWN.into()
    } else {
        leaf.into()
    }
}

/// (date, project, model) - "?"+name for an unpriced model.
type Key = (String, String, String);

/// One session's running totals: (vector, cost, unpriced tokens, by model).
type Running = (
    [i64; 5],
    f64,
    i64,
    indexmap::IndexMap<String, (i64, i64, f64)>,
);

struct File {
    session: String,
    offset: u64,
    size: u64,
    mtime: f64,
    head: Vec<u8>,
    rest: Vec<u8>,
    cwd: Option<String>,
    lines: u64,
    /// message id -> (bucket key, vector it contributed, epoch seconds)
    seen: indexmap::IndexMap<String, (Key, [i64; 5], f64)>,
    buckets: indexmap::IndexMap<Key, [i64; 5]>,
}

impl File {
    fn new(session: &str) -> Self {
        File {
            session: session.into(),
            offset: 0,
            size: 0,
            mtime: 0.0,
            head: vec![],
            rest: vec![],
            cwd: None,
            lines: 0,
            seen: indexmap::IndexMap::new(),
            buckets: indexmap::IndexMap::new(),
        }
    }

    fn reset(&mut self) {
        let session = std::mem::take(&mut self.session);
        *self = File::new(&session);
    }

    fn add(&mut self, key: &Key, vec: &[i64; 5], sign: i64) {
        let have = self.buckets.entry(key.clone()).or_insert([0; 5]);
        for i in 0..5 {
            have[i] += sign * vec[i];
        }
    }

    fn feed(&mut self, data: &[u8]) {
        let mut all = std::mem::take(&mut self.rest);
        all.extend_from_slice(data);
        let end = all
            .iter()
            .rposition(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        self.rest = all[end..].to_vec();
        for line in all[..end].split(|b| *b == b'\n') {
            let has = |needle: &[u8]| line.windows(needle.len()).any(|w| w == needle);
            if !has(b"\"usage\"") && !has(b"\"cwd\"") {
                continue;
            }
            if let Ok(Value::Object(entry)) = serde_json::from_slice::<Value>(line) {
                self.entry(&entry);
            }
        }
    }

    fn entry(&mut self, entry: &serde_json::Map<String, Value>) {
        self.lines += 1;
        if let Some(Value::String(c)) = entry.get("cwd") {
            self.cwd = Some(c.clone());
        }
        if entry.get("type").and_then(Value::as_str) != Some("assistant") {
            return;
        }
        let Some(Value::Object(msg)) = entry.get("message") else {
            return;
        };
        let Some(Value::Object(usage)) = msg.get("usage") else {
            return;
        };
        let model = match msg.get("model") {
            Some(m) if truthy(Some(m)) && m.as_str() != Some("<synthetic>") => m,
            _ => return,
        };
        let Some(at) = entry
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|s| crate::sessions::iso_seconds_as(&s.replace('Z', "+00:00"), true))
        else {
            return;
        };
        let day = crate::paths::ymd(crate::paths::local_date(at));
        let (name, priced) = resolve(Some(model));
        let key: Key = (
            day,
            project_of(self.cwd.as_deref()),
            if priced { name } else { format!("?{name}") },
        );
        let mid = [msg.get("id"), entry.get("uuid")]
            .into_iter()
            .flatten()
            .find(|v| truthy(Some(v)))
            .map(py_str)
            .unwrap_or_else(|| format!("line{}", self.lines));
        let vec = vector(usage);
        if let Some((old_key, old_vec, _)) = self.seen.get(&mid).cloned() {
            self.add(&old_key, &old_vec, -1);
        }
        self.seen.insert(mid, (key.clone(), vec, at));
        self.add(&key, &vec, 1);
    }
}

fn mtime_of(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as f64 + d.subsec_nanos() as f64 * 1e-9)
        .unwrap_or(0.0)
}

fn add_vec(into: &mut [i64; 5], v: &[i64; 5]) {
    for i in 0..5 {
        into[i] += v[i];
    }
}

/// costs._shape: a token vector as the API speaks it.
fn shape(
    vec: &[i64; 5],
    cost: f64,
    unpriced: i64,
    models: &indexmap::IndexMap<String, (i64, i64, f64)>,
) -> Value {
    let m: serde_json::Map<String, Value> = models
        .iter()
        .map(|(k, (o, t, c))| {
            (
                k.clone(),
                json!({"output": o, "total_tokens": t, "cost_usd": c}),
            )
        })
        .collect();
    json!({
        "tokens": {"input": vec[0], "output": vec[1], "cache_read": vec[2], "cache_write": vec[3] + vec[4]},
        "total_tokens": vec.iter().sum::<i64>(), "cost_usd": cost, "unpriced_tokens": unpriced, "models": m,
    })
}

/// costs._by_model: fold one message into the model-mix split.
fn by_model(models: &mut indexmap::IndexMap<String, (i64, i64, f64)>, model: &str, v: &[i64; 5]) {
    let m = models
        .entry(model.trim_start_matches('?').to_string())
        .or_insert((0, 0, 0.0));
    m.0 += v[1];
    m.1 += v.iter().sum::<i64>();
    if !model.starts_with('?') {
        m.2 += price(model, v);
    }
}

pub struct History {
    root: std::path::PathBuf,
    files: indexmap::IndexMap<String, File>,
    pinned: std::collections::HashSet<String>,
}

impl History {
    pub fn new() -> Self {
        History {
            root: crate::sessions::projects_dir(),
            files: indexmap::IndexMap::new(),
            pinned: Default::default(),
        }
    }

    /// (path, session, mtime, size) of transcripts touched since `since`.
    fn candidates(&self, since: f64) -> Vec<(String, String, f64, u64)> {
        let mut found: Vec<(String, String)> = Vec::new();
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return vec![];
        };
        for folder in rd.flatten().filter(|e| e.path().is_dir()) {
            let Ok(entries) = std::fs::read_dir(folder.path()) else {
                continue;
            };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let p = e.path();
                if p.is_file() && name.ends_with(".jsonl") {
                    found.push((
                        p.to_string_lossy().into_owned(),
                        name[..name.len() - 6].to_string(),
                    ));
                } else if p.is_dir() {
                    if let Ok(subs) = std::fs::read_dir(p.join("subagents")) {
                        for s in subs.flatten() {
                            let sname = s.file_name().to_string_lossy().into_owned();
                            if s.path().is_file() && sname.ends_with(".jsonl") {
                                found.push((s.path().to_string_lossy().into_owned(), name.clone()));
                            }
                        }
                    }
                }
            }
        }
        found
            .into_iter()
            .filter_map(|(path, session)| {
                let meta = std::fs::metadata(&path).ok()?;
                let m = mtime_of(&meta);
                (m >= since).then_some((path, session, m, meta.len()))
            })
            .collect()
    }

    fn refresh(&mut self, path: &str, session: &str, mtime: f64, size: u64) -> bool {
        let known = self.files.contains_key(path);
        let state = self
            .files
            .entry(path.to_string())
            .or_insert_with(|| File::new(session));
        if known && state.size == size && state.mtime == mtime {
            return false;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        let head_now = &bytes[..bytes.len().min(HEAD)];
        if size < state.size
            || (state.offset > 0 && head_now != &state.head[..state.head.len().min(HEAD)])
        {
            state.reset();
        }
        let from = (state.offset as usize).min(bytes.len());
        let data = bytes[from..].to_vec();
        if state.offset == 0 {
            state.head = head_now.to_vec();
        }
        state.offset += data.len() as u64;
        state.size = size;
        state.mtime = mtime;
        state.feed(&data);
        true
    }

    /// Bring every transcript touched since `since` up to date, newest first.
    fn scan(&mut self, since: f64, budget: f64, prune: bool) -> bool {
        let started = std::time::Instant::now();
        let mut complete = true;
        let mut found = self.candidates(since);
        if prune {
            let keep: std::collections::HashSet<String> = found
                .iter()
                .map(|f| f.0.clone())
                .chain(self.pinned.iter().cloned())
                .collect();
            self.files.retain(|p, _| keep.contains(p));
        }
        found.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        for (path, session, mtime, size) in found {
            if let Some(known) = self.files.get(&path) {
                if known.size == size && known.mtime == mtime {
                    continue;
                }
            }
            if started.elapsed().as_secs_f64() >= budget {
                complete = false;
                continue;
            }
            self.refresh(&path, &session, mtime, size);
        }
        complete
    }

    /// Files in the order `_messages` and `_fold` walk them: the session's
    /// own transcript before its subagents'.
    fn ordered(&self) -> Vec<&String> {
        let mut paths: Vec<&String> = self.files.keys().collect();
        paths.sort_by(|a, b| {
            let depth = |p: &str| p.matches(crate::paths::SEP).count();
            (depth(a), a.as_str()).cmp(&(depth(b), b.as_str()))
        });
        paths
    }

    /// (session, model key, vector, epoch) for every message once.
    fn messages(&self) -> Vec<(String, String, [i64; 5], f64)> {
        let mut owners: std::collections::HashMap<String, std::collections::HashSet<String>> =
            Default::default();
        let mut out = Vec::new();
        for path in self.ordered() {
            let state = &self.files[path];
            let seen = owners.entry(state.session.clone()).or_default();
            for (mid, (key, vec, at)) in &state.seen {
                if seen.insert(mid.clone()) {
                    out.push((state.session.clone(), key.2.clone(), *vec, *at));
                }
            }
        }
        out
    }

    /// History.totals: tokens and estimated cost of every message sent in
    /// [since, until), across all transcripts here.
    pub fn totals(&mut self, since: f64, until: Option<f64>, now: f64, budget: f64) -> Value {
        let until = until.unwrap_or(now);
        // Not pruned: a short window must not drop files a longer one wants.
        let complete = self.scan(since - SLACK, budget, false);
        let (mut vec, mut cost, mut unpriced) = ([0i64; 5], 0.0f64, 0i64);
        let mut models: indexmap::IndexMap<String, (i64, i64, f64)> = indexmap::IndexMap::new();
        for (_, model, v, at) in self.messages() {
            if since <= at && at < until {
                add_vec(&mut vec, &v);
                if model.starts_with('?') {
                    unpriced += v.iter().sum::<i64>();
                } else {
                    cost += price(&model, &v);
                }
                by_model(&mut models, &model, &v);
            }
        }
        let mut out = shape(&vec, cost, unpriced, &models);
        out["complete"] = json!(complete);
        out
    }

    /// History.sessions: {id: totals} over each session's whole life.
    pub fn sessions(
        &mut self,
        ids: &[String],
        budget: f64,
    ) -> (serde_json::Map<String, Value>, bool) {
        let started = std::time::Instant::now();
        let mut complete = true;
        let mut pinned = std::collections::HashSet::new();
        for sid in ids.iter().filter(|s| !s.is_empty()) {
            for path in session_files(sid, &self.root) {
                pinned.insert(path.clone());
                let Ok(meta) = std::fs::metadata(&path) else {
                    continue;
                };
                if !self.files.contains_key(&path) && started.elapsed().as_secs_f64() >= budget {
                    complete = false;
                    continue;
                }
                self.refresh(&path, sid, mtime_of(&meta), meta.len());
            }
        }
        self.pinned = pinned;
        let want: std::collections::HashSet<&String> = ids.iter().collect();
        let mut acc: indexmap::IndexMap<String, Running> = indexmap::IndexMap::new();
        for (session, model, v, _) in self.messages() {
            if !want.contains(&session) {
                continue;
            }
            let e = acc
                .entry(session)
                .or_insert(([0; 5], 0.0, 0, indexmap::IndexMap::new()));
            by_model(&mut e.3, &model, &v);
            add_vec(&mut e.0, &v);
            if model.starts_with('?') {
                e.2 += v.iter().sum::<i64>();
            } else {
                e.1 += price(&model, &v);
            }
        }
        let out = acc
            .into_iter()
            .map(|(sid, (vec, cost, unpriced, models))| (sid, shape(&vec, cost, unpriced, &models)))
            .collect();
        (out, complete)
    }

    /// History.summary: cost over the last `days` local days, today included.
    pub fn summary(&mut self, days: i64, now: f64, budget: f64) -> Value {
        let days = days.max(1);
        let today = crate::paths::local_date(now);
        let today_n = crate::paths::days_from_civil(today.0, today.1, today.2);
        let window: Vec<String> = (0..days)
            .rev()
            .map(|i| crate::paths::ymd(crate::paths::civil(today_n - i)))
            .collect();
        let inside: std::collections::HashSet<&String> = window.iter().collect();
        let complete = self.scan(now - days as f64 * 86400.0 - SLACK, budget, true);
        // _fold
        let mut total: indexmap::IndexMap<Key, [i64; 5]> = indexmap::IndexMap::new();
        let mut sessions: indexmap::IndexMap<String, std::collections::HashSet<String>> =
            indexmap::IndexMap::new();
        let mut owners: std::collections::HashMap<String, std::collections::HashSet<String>> =
            Default::default();
        for path in self.ordered() {
            let state = &self.files[path];
            for (key, vec) in &state.buckets {
                if inside.contains(&key.0) {
                    add_vec(total.entry(key.clone()).or_insert([0; 5]), vec);
                    if vec.iter().any(|n| *n != 0) {
                        sessions
                            .entry(key.1.clone())
                            .or_default()
                            .insert(state.session.clone());
                    }
                }
            }
            let seen = owners.entry(state.session.clone()).or_default();
            for (mid, (key, vec, _)) in &state.seen {
                if seen.contains(mid) {
                    if inside.contains(&key.0) {
                        if let Some(have) = total.get_mut(key) {
                            for i in 0..5 {
                                have[i] -= vec[i];
                            }
                        }
                    }
                } else {
                    seen.insert(mid.clone());
                }
            }
        }
        let mut per_day: indexmap::IndexMap<String, (f64, i64)> =
            window.iter().map(|d| (d.clone(), (0.0, 0))).collect();
        let mut per_project: indexmap::IndexMap<String, (f64, i64)> = indexmap::IndexMap::new();
        let mut per_model: indexmap::IndexMap<String, (f64, i64)> = indexmap::IndexMap::new();
        let mut unpriced = 0i64;
        for ((day, project, model), vec) in &total {
            let tokens: i64 = vec.iter().sum();
            if tokens == 0 {
                continue;
            }
            let (cost, model) = if let Some(m) = model.strip_prefix('?') {
                unpriced += tokens;
                (0.0, m.to_string())
            } else {
                (price(model, vec), model.clone())
            };
            if let Some(d) = per_day.get_mut(day) {
                d.0 += cost;
                d.1 += tokens;
            }
            for (table, name) in [(&mut per_project, project.clone()), (&mut per_model, model)] {
                let row = table.entry(name).or_insert((0.0, 0));
                row.0 += cost;
                row.1 += tokens;
            }
        }
        let mut projects: Vec<Value> = per_project
            .iter()
            .map(|(name, (c, t))| json!({"project": name, "cost": c, "tokens": t,
                                          "sessions": sessions.get(name).map(|s| s.len()).unwrap_or(0)}))
            .collect();
        let key = |r: &Value| {
            (
                -r["cost"].as_f64().unwrap_or(0.0),
                -r["tokens"].as_i64().unwrap_or(0),
            )
        };
        projects.sort_by(|a, b| {
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut models: Vec<Value> = per_model
            .iter()
            .map(|(name, (c, t))| json!({"model": name, "cost": c, "tokens": t}))
            .collect();
        models.sort_by(|a, b| {
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let day_rows: Vec<Value> = window
            .iter()
            .map(|d| json!({"date": d, "cost": per_day[d].0, "tokens": per_day[d].1}))
            .collect();
        // sum(): compensated in Python 3.12+, so it is here.
        let total_cost = crate::pyish::py_fsum_builtin(per_day.values().map(|v| v.0));
        let total_tokens: i64 = per_day.values().map(|v| v.1).sum();
        json!({
            "ok": true, "estimated": true, "days": day_rows, "projects": projects, "models": models,
            "total": total_cost, "total_tokens": total_tokens, "unpriced_tokens": unpriced,
            "files": self.files.len(), "complete": complete, "window_days": days,
        })
    }
}

/// costs.session_files: a session's transcript and its subagents'.
fn session_files(sid: &str, root: &std::path::Path) -> Vec<String> {
    if sid.len() != 36 || !sid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return vec![];
    }
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    for folder in rd.flatten().filter(|e| e.path().is_dir()) {
        let main = folder.path().join(format!("{sid}.jsonl"));
        if main.is_file() {
            out.push(main.to_string_lossy().into_owned());
        }
        if let Ok(subs) = std::fs::read_dir(folder.path().join(sid).join("subagents")) {
            for s in subs.flatten() {
                if s.file_name().to_string_lossy().ends_with(".jsonl") {
                    out.push(s.path().to_string_lossy().into_owned());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids() {
        assert_eq!(
            resolve(Some(&json!("claude-opus-5-5[1m]"))),
            ("claude-opus-5-5".into(), true)
        );
        assert_eq!(
            resolve(Some(&json!("claude-opus-4-1-20250805"))),
            ("claude-opus-4-1".into(), true)
        );
        assert_eq!(
            resolve(Some(&json!("us.anthropic.claude-sonnet-x"))),
            ("claude-sonnet-5-5".into(), true)
        );
        assert_eq!(resolve(Some(&json!("gpt"))), ("gpt".into(), false));
        assert_eq!(resolve(None), ("(unknown)".into(), false));
    }

    #[test]
    fn a_cold_cache() {
        let c = rewrite_cost(
            Some(&json!("claude-opus-5-5")),
            Some(&json!(100000)),
            Some(&json!(3600.0)),
        );
        assert!((c.as_f64().unwrap() - 0.8).abs() < 1e-12);
        assert_eq!(
            rewrite_cost(Some(&json!("gpt")), Some(&json!(1)), None),
            Value::Null
        );
    }
}
