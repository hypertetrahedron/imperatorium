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
            root: crate::paths::projects_dir(),
            files: indexmap::IndexMap::new(),
            pinned: Default::default(),
        }
    }

    /// (path, session, mtime, size) of transcripts touched since `since`.
    fn candidates(&self, since: f64) -> Vec<(String, String, f64, u64)> {
        let found = crate::transcript::list_files(&self.root);
        found
            .into_iter()
            .filter_map(|(path, session)| {
                let meta = std::fs::metadata(&path).ok()?;
                let path = path.to_string_lossy().into_owned();
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
        // Only what was appended is read: the head (to tell a file that was
        // replaced) and the bytes past the offset - the same bytes, and the
        // same verdicts, as reading the whole file and slicing it.
        let Ok(mut f) = std::fs::File::open(path) else {
            return false;
        };
        let Ok(head_now) = crate::transcript::read_head(&mut f, HEAD) else {
            return false;
        };
        let reset = size < state.size
            || (state.offset > 0 && head_now[..] != state.head[..state.head.len().min(HEAD)]);
        let from = if reset { 0 } else { state.offset };
        // Past the end (the file shrank since it was looked at) reads nothing.
        let Ok(data) = crate::transcript::read_from(&mut f, from) else {
            return false;
        };
        if reset {
            state.reset();
        }
        if state.offset == 0 {
            state.head = head_now;
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

/// test_costs.py, ported: the spend history over transcripts in a temp folder.
#[cfg(test)]
mod history_tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    const M: i64 = 1_000_000;
    const A: &str = "aaaaaaaa-0000-0000-0000-000000000001";
    const B: &str = "bbbbbbbb-0000-0000-0000-000000000002";

    /// Local noon, 2026-10-15.
    fn now() -> f64 {
        crate::paths::local_to_epoch(2026, 10, 15, 12, 0, 0)
    }

    fn today_n() -> i64 {
        crate::paths::days_from_civil(2026, 10, 15)
    }

    /// A UTC ISO timestamp for local `hour` on the day `days_ago` before now.
    fn stamp(days_ago: i64, hour: u32) -> String {
        let (y, m, d) = crate::paths::civil(today_n() - days_ago);
        let t = crate::paths::local_to_epoch(y, m, d, hour, 0, 0) as i64;
        let (y, m, d) = crate::paths::civil(t.div_euclid(86400));
        let s = t.rem_euclid(86400);
        format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s % 3600 / 60, s % 60)
    }

    fn day(days_ago: i64) -> String {
        crate::paths::ymd(crate::paths::civil(today_n() - days_ago))
    }

    /// An assistant entry; `usage` fields beyond input/output go in `extra`.
    fn entry_with(mid: &str, model: &str, cwd: &str, when: &str, usage: Value) -> Value {
        json!({"type": "assistant", "timestamp": when, "cwd": cwd,
               "message": {"id": mid, "model": model, "usage": usage}})
    }

    fn entry(mid: &str, input: i64, output: i64) -> Value {
        entry_with(mid, "claude-sonnet-4-5", r"C:\work\app", &stamp(0, 12),
                   json!({"input_tokens": input, "output_tokens": output}))
    }

    fn out_at(mid: &str, when: &str, output: i64) -> Value {
        entry_with(mid, "claude-sonnet-4-5", r"C:\work\app", when,
                   json!({"input_tokens": 0, "output_tokens": output}))
    }

    fn set_mtime(path: &std::path::Path, t: f64) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(t)).unwrap();
    }

    struct Rig {
        root: PathBuf,
        h: History,
    }

    impl Rig {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let root = std::env::temp_dir().join(format!(
                "imp-costs-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Rig { h: Self::history(&root), root }
        }

        fn history(root: &std::path::Path) -> History {
            History { root: root.to_path_buf(), files: indexmap::IndexMap::new(), pinned: Default::default() }
        }

        fn put(&self, session: &str, entries: &[Value], folder: &str, sub: Option<&str>, append: bool) -> PathBuf {
            let mut base = self.root.join(folder);
            let name = match sub {
                Some(s) => {
                    base = base.join(session).join("subagents");
                    format!("{s}.jsonl")
                }
                None => format!("{session}.jsonl"),
            };
            std::fs::create_dir_all(&base).unwrap();
            let path = base.join(name);
            let mut text = String::new();
            for e in entries {
                text += &serde_json::to_string(e).unwrap();
                text.push('\n');
            }
            if append {
                use std::io::Write;
                std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(text.as_bytes()).unwrap();
            } else {
                std::fs::write(&path, text).unwrap();
            }
            set_mtime(&path, now());
            path
        }

        fn write(&self, session: &str, entries: &[Value]) -> PathBuf {
            self.put(session, entries, "proj", None, false)
        }

        fn summary(&mut self, days: i64) -> Value {
            self.h.summary(days, now(), 60.0)
        }

        fn cost(&mut self) -> f64 {
            self.summary(30)["total"].as_f64().unwrap()
        }

        fn totals(&mut self, since: f64) -> Value {
            self.h.totals(since, None, now(), 60.0)
        }

        fn key(&self, path: &std::path::Path) -> String {
            path.to_string_lossy().into_owned()
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    // -- Pricing ------------------------------------------------------------

    #[test]
    fn input_output() {
        let mut r = Rig::new();
        r.write("s1", &[entry("m1", M, M)]);
        near(r.cost(), 3.0 + 15.0);
    }

    #[test]
    fn streamed_entries_count_once_and_last_wins() {
        let mut r = Rig::new();
        r.write("s1", &[entry("m1", 0, 10), entry("m1", 0, 500), entry("m1", 0, M)]);
        let out = r.summary(30);
        assert_eq!(out["total_tokens"], M);
        near(out["total"].as_f64().unwrap(), 15.0);
    }

    #[test]
    fn cache_write_5m_vs_1h() {
        let mut r = Rig::new();
        let usage = json!({"input_tokens": 0, "output_tokens": 0, "cache_creation_input_tokens": 2 * M,
            "cache_creation": {"ephemeral_5m_input_tokens": M, "ephemeral_1h_input_tokens": M}});
        r.write("s1", &[entry_with("m1", "claude-sonnet-4-5", r"C:\work\app", &stamp(0, 12), usage)]);
        near(r.cost(), 3.0 * 1.25 + 3.0 * 2.0);
    }

    #[test]
    fn cache_write_without_split_is_5m() {
        let mut r = Rig::new();
        let usage = json!({"input_tokens": 0, "output_tokens": 0, "cache_creation_input_tokens": M});
        r.write("s1", &[entry_with("m1", "claude-sonnet-4-5", r"C:\work\app", &stamp(0, 12), usage)]);
        near(r.cost(), 3.0 * 1.25);
    }

    #[test]
    fn cache_read_multipliers() {
        for (model, expect) in [("claude-opus-5-5", 4.0 * 0.05), ("claude-fable-5-1", 10.0 * 0.025),
                                ("claude-sonnet-5-5", 2.0 * 0.1), ("claude-opus-5", 5.0 * 0.1)] {
            let mut r = Rig::new();
            let usage = json!({"input_tokens": 0, "output_tokens": 0, "cache_read_input_tokens": M});
            r.write("s1", &[entry_with("m1", model, r"C:\work\app", &stamp(0, 12), usage)]);
            near(r.cost(), expect);
        }
    }

    #[test]
    fn dated_and_bracketed_model_ids() {
        let mut r = Rig::new();
        let at = |mid: &str, model: &str| {
            entry_with(mid, model, r"C:\work\app", &stamp(0, 12), json!({"input_tokens": M, "output_tokens": 0}))
        };
        r.write("s1", &[at("a", "claude-sonnet-4-5-20250929"), at("b", "claude-opus-5-5[1m]"),
                        at("c", "claude-opus-4-1-20250805")]);
        let out = r.summary(30);
        let cost = |name: &str| {
            out["models"].as_array().unwrap().iter()
                .find(|m| m["model"] == name).unwrap_or_else(|| panic!("no {name}"))["cost"].as_f64().unwrap()
        };
        near(cost("claude-sonnet-4-5"), 3.0);
        near(cost("claude-opus-5-5"), 4.0);
        near(cost("claude-opus-4-1"), 15.0);
        assert_eq!(out["unpriced_tokens"], 0);
    }

    #[test]
    fn family_fallback() {
        let mut r = Rig::new();
        r.write("s1", &[entry_with("m1", "claude-opus-9-9", r"C:\work\app", &stamp(0, 12),
                                   json!({"input_tokens": M, "output_tokens": 0}))]);
        let out = r.summary(30);
        near(out["total"].as_f64().unwrap(), 4.0);
        assert_eq!(out["unpriced_tokens"], 0);
    }

    #[test]
    fn unknown_model_is_unpriced() {
        let mut r = Rig::new();
        r.write("s1", &[entry_with("m1", "gpt-mystery", r"C:\work\app", &stamp(0, 12),
                                   json!({"input_tokens": 300, "output_tokens": 200}))]);
        let out = r.summary(30);
        assert_eq!(out["unpriced_tokens"], 500);
        assert_eq!(out["total"].as_f64().unwrap(), 0.0);
        assert_eq!(out["total_tokens"], 500);
    }

    #[test]
    fn rewriting_a_cold_cache_prices() {
        let f = |m: Option<&str>, t: Option<i64>, ttl: i64| {
            rewrite_cost(m.map(|m| json!(m)).as_ref(), t.map(|t| json!(t)).as_ref(), Some(&json!(ttl)))
        };
        near(f(Some("claude-opus-5-5[1m]"), Some(M), 3600).as_f64().unwrap(), 8.0);
        near(f(Some("claude-opus-5-5"), Some(M), 300).as_f64().unwrap(), 5.0);
        assert_eq!(f(Some("gpt-mystery"), Some(M), 300), Value::Null);
        assert_eq!(f(Some("claude-opus-5-5"), None, 300), Value::Null);
    }

    #[test]
    fn synthetic_and_non_assistant_skipped() {
        let mut r = Rig::new();
        r.write("s1", &[
            entry_with("m1", "<synthetic>", r"C:\work\app", &stamp(0, 12), json!({"input_tokens": M, "output_tokens": 0})),
            json!({"type": "user", "timestamp": stamp(0, 12), "message": {"content": "hi"}}),
        ]);
        assert_eq!(r.summary(30)["total_tokens"], 0);
    }

    // -- Bucketing ----------------------------------------------------------

    #[test]
    fn local_date_and_zero_fill() {
        let mut r = Rig::new();
        r.write("s1", &[out_at("a", &stamp(0, 12), 100), out_at("b", &stamp(2, 12), 200)]);
        let out = r.summary(5);
        let days: Vec<&str> = out["days"].as_array().unwrap().iter().map(|d| d["date"].as_str().unwrap()).collect();
        assert_eq!(days, [4, 3, 2, 1, 0].map(day).iter().map(String::as_str).collect::<Vec<_>>());
        let tokens: Vec<i64> = out["days"].as_array().unwrap().iter().map(|d| d["tokens"].as_i64().unwrap()).collect();
        assert_eq!(tokens, [0, 0, 200, 0, 100]);
        assert_eq!(out["window_days"], 5);
    }

    #[test]
    fn late_evening_stays_on_its_local_day() {
        let mut r = Rig::new();
        r.write("s1", &[out_at("a", &stamp(1, 23), 7)]);
        let out = r.summary(3);
        let row = out["days"].as_array().unwrap().iter().find(|d| d["date"] == day(1).as_str()).unwrap().clone();
        assert_eq!(row["tokens"], 7);
    }

    #[test]
    fn usage_outside_window_not_counted() {
        let mut r = Rig::new();
        r.write("s1", &[out_at("a", &stamp(10, 12), 100), out_at("b", &stamp(0, 12), 5)]);
        let out = r.summary(3);
        assert_eq!(out["total_tokens"], 5);
        assert_eq!(out["days"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn old_file_ignored() {
        let mut r = Rig::new();
        let path = r.write("s1", &[out_at("a", &stamp(0, 12), 100)]);
        set_mtime(&path, now() - 10.0 * 86400.0);
        let out = r.summary(3);
        assert_eq!(out["files"], 0);
        assert_eq!(out["total_tokens"], 0);
    }

    #[test]
    fn old_file_dropped_from_memory() {
        let mut r = Rig::new();
        let path = r.write("s1", &[entry("m1", 0, 100)]);
        assert_eq!(r.summary(3)["files"], 1);
        set_mtime(&path, now() - 10.0 * 86400.0);
        assert_eq!(r.summary(3)["files"], 0);
    }

    // -- Projects -----------------------------------------------------------

    #[test]
    fn worktree_cwd_maps_to_repo() {
        let mut r = Rig::new();
        let at = |mid: &str, cwd: &str| entry_with(mid, "claude-sonnet-4-5", cwd, &stamp(0, 12),
                                                   json!({"input_tokens": 0, "output_tokens": 1}));
        r.write("s1", &[at("a", r"C:\src\repo\.claude\worktrees\feat-x"),
                        at("b", "/home/u/other/.claude/worktrees/y/sub"), at("c", r"C:\src\plain")]);
        let mut names: Vec<String> = r.summary(30)["projects"].as_array().unwrap().iter()
            .map(|p| p["project"].as_str().unwrap().to_string()).collect();
        names.sort();
        assert_eq!(names, ["other", "plain", "repo"]);
    }

    #[test]
    fn sessions_counted_and_subagent_joins_parent() {
        let mut r = Rig::new();
        r.write("s1", &[entry("a", 0, M)]);
        r.put("s1", &[entry("b", 0, M)], "proj", Some("agent-1"), false);
        r.write("s2", &[entry("c", 0, M)]);
        let out = r.summary(30);
        let row = &out["projects"][0];
        assert_eq!(row["project"], "app");
        assert_eq!(row["sessions"], 2);
        assert_eq!(out["files"], 3);
        assert_eq!(out["total_tokens"], 3 * M);
    }

    #[test]
    fn same_message_in_session_and_subagent_counts_once() {
        let mut r = Rig::new();
        r.write("s1", &[entry("dup", 0, 100)]);
        r.put("s1", &[entry("dup", 0, 100)], "proj", Some("agent-1"), false);
        assert_eq!(r.summary(30)["total_tokens"], 100);
    }

    #[test]
    fn projects_sorted_by_cost() {
        let mut r = Rig::new();
        let at = |mid: &str, cwd: &str, out: i64| entry_with(mid, "claude-sonnet-4-5", cwd, &stamp(0, 12),
                                                             json!({"input_tokens": 0, "output_tokens": out}));
        r.write("s1", &[at("a", r"C:\cheap", 1), at("b", r"C:\dear", M)]);
        let names: Vec<String> = r.summary(30)["projects"].as_array().unwrap().iter()
            .map(|p| p["project"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, ["dear", "cheap"]);
    }

    // -- Incremental --------------------------------------------------------

    #[test]
    fn appended_lines_only_are_parsed() {
        let mut r = Rig::new();
        let path = r.write("s1", &[entry("a", 0, 100)]);
        assert_eq!(r.summary(30)["total_tokens"], 100);
        let before = r.h.files[&r.key(&path)].lines;
        r.put("s1", &[entry("b", 0, 50)], "proj", None, true);
        assert_eq!(r.summary(30)["total_tokens"], 150);
        assert_eq!(r.h.files[&r.key(&path)].lines - before, 1);
    }

    #[test]
    fn unchanged_file_not_reread() {
        let mut r = Rig::new();
        let path = r.write("s1", &[entry("a", 0, 100)]);
        r.summary(30);
        let lines = r.h.files[&r.key(&path)].lines;
        r.summary(30);
        assert_eq!(r.h.files[&r.key(&path)].lines, lines);
    }

    #[test]
    fn later_entry_replaces_earlier_across_reads() {
        let mut r = Rig::new();
        r.write("s1", &[entry("a", 0, 10)]);
        assert_eq!(r.summary(30)["total_tokens"], 10);
        r.put("s1", &[entry("a", 0, 90)], "proj", None, true);
        assert_eq!(r.summary(30)["total_tokens"], 90);
    }

    #[test]
    fn partial_line_waits_for_its_newline() {
        use std::io::Write;
        let mut r = Rig::new();
        let path = r.write("s1", &[entry("a", 0, 10)]);
        r.summary(30);
        let text = serde_json::to_string(&entry("b", 0, 20)).unwrap();
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&text.as_bytes()[..30]).unwrap();
        drop(f);
        set_mtime(&path, now() + 1.0);
        assert_eq!(r.summary(30)["total_tokens"], 10);
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(format!("{}\n", &text[30..]).as_bytes()).unwrap();
        drop(f);
        set_mtime(&path, now() + 2.0);
        assert_eq!(r.summary(30)["total_tokens"], 30);
    }

    #[test]
    fn shrunk_file_is_reread() {
        let mut r = Rig::new();
        r.write("s1", &[entry("a", 0, 100), entry("b", 0, 100)]);
        assert_eq!(r.summary(30)["total_tokens"], 200);
        r.write("s1", &[entry("z", 0, 7)]);
        assert_eq!(r.summary(30)["total_tokens"], 7);
    }

    #[test]
    fn replaced_larger_file_is_reread() {
        let mut r = Rig::new();
        r.write("s1", &[entry("a", 0, 1)]);
        assert_eq!(r.summary(30)["total_tokens"], 1);
        let longer = |mid: &str, cwd: &str| entry_with(mid, "claude-sonnet-4-5", cwd, &stamp(0, 12),
                                                       json!({"input_tokens": 0, "output_tokens": 5}));
        r.write("s1", &[longer("x", r"C:\somewhere\longer\path"), longer("y", r"C:\work\app")]);
        assert_eq!(r.summary(30)["total_tokens"], 10);
    }

    #[test]
    fn budget_zero_is_incomplete() {
        let mut r = Rig::new();
        r.write("s1", &[entry("a", 0, 100)]);
        let out = r.h.summary(30, now(), 0.0);
        assert_eq!(out["complete"], false);
        assert_eq!(out["total_tokens"], 0);
        let again = r.summary(30);
        assert_eq!(again["complete"], true);
        assert_eq!(again["total_tokens"], 100);
    }

    #[test]
    fn cached_files_count_when_budget_runs_out() {
        let mut r = Rig::new();
        r.write("s1", &[entry("a", 0, 100)]);
        r.summary(30);
        r.write("s2", &[entry("b", 0, 40)]);
        let out = r.h.summary(30, now(), 0.0);
        assert_eq!(out["complete"], false);
        assert_eq!(out["total_tokens"], 100);
    }

    #[test]
    fn missing_root_is_empty() {
        let r = Rig::new();
        let mut h = Rig::history(&r.root.join("nope"));
        let out = h.summary(30, now(), 60.0);
        assert_eq!(out["ok"], true);
        assert_eq!(out["total"].as_f64().unwrap(), 0.0);
        serde_json::to_string(&out).unwrap();
    }

    // -- Windows: totals and sessions ---------------------------------------

    #[test]
    fn a_window_counts_only_messages_sent_inside_it() {
        let mut r = Rig::new();
        r.write(A, &[out_at("old", &stamp(0, 5), M), out_at("new", &stamp(0, 11), 2 * M)]);
        r.put(B, &[out_at("b1", &stamp(3, 12), 4 * M)], "other", None, false);
        let five = r.totals(now() - 5.0 * 3600.0);
        assert_eq!(five["tokens"]["output"], 2 * M);
        near(five["cost_usd"].as_f64().unwrap(), 30.0);
        assert_eq!(five["complete"], true);
        let week = r.totals(now() - 7.0 * 86400.0);
        assert_eq!(week["total_tokens"], 7 * M);
    }

    #[test]
    fn streamed_entries_count_once_in_a_window() {
        let mut r = Rig::new();
        r.write(A, &[out_at("m", &stamp(0, 11), 10), out_at("m", &stamp(0, 11), M)]);
        assert_eq!(r.totals(now() - 7200.0)["total_tokens"], M);
    }

    #[test]
    fn a_session_is_its_whole_life_with_its_subagents() {
        let mut r = Rig::new();
        let haiku = entry_with("s1", "claude-haiku-4-5", r"C:\work\app", &stamp(0, 12),
                               json!({"input_tokens": 0, "output_tokens": 2 * M}));
        let a1 = entry_with("a1", "claude-sonnet-4-5", r"C:\work\app", &stamp(60, 12),
                            json!({"input_tokens": M, "output_tokens": 0}));
        let old = r.write(A, &[a1, out_at("a2", &stamp(0, 12), M)]);
        r.put(A, &[haiku], "proj", Some("agent-x"), false);
        set_mtime(&old, now() - 50.0 * 86400.0); // untouched for weeks
        let ids = vec![A.to_string(), B.to_string()];
        let (got, complete) = r.h.sessions(&ids, 60.0);
        assert!(complete);
        assert_eq!(got.keys().collect::<Vec<_>>(), [A]); // B has no transcript
        assert_eq!(got[A]["tokens"], json!({"input": M, "output": 3 * M, "cache_read": 0, "cache_write": 0}));
        near(got[A]["cost_usd"].as_f64().unwrap(), 3.0 + 15.0 + 10.0);
        // Pinned: a 30-day cost history does not drop the open session's file.
        r.summary(30);
        assert!(r.h.files.contains_key(&r.key(&old)));
        assert_eq!(r.h.sessions(&[A.to_string()], 60.0).0[A]["total_tokens"], 4 * M);
    }

    #[test]
    fn each_answer_splits_by_model() {
        let mut r = Rig::new();
        let at = |mid: &str, model: &str, input: i64, out: i64| entry_with(
            mid, model, r"C:\work\app", &stamp(0, 11), json!({"input_tokens": input, "output_tokens": out}));
        r.write(A, &[at("a1", "claude-sonnet-4-5", 0, M), at("a2", "claude-haiku-4-5", 5, 2 * M),
                     at("a3", "mystery-9", 0, 7)]);
        let five = r.totals(now() - 5.0 * 3600.0);
        let models = five["models"].as_object().unwrap();
        assert_eq!(models["claude-sonnet-4-5"]["output"], M);
        near(models["claude-sonnet-4-5"]["cost_usd"].as_f64().unwrap(), 15.0);
        assert_eq!(models["claude-haiku-4-5"]["total_tokens"], 2 * M + 5);
        // No price known: counted under its own name, at no cost.
        let unknown: Vec<&String> = models.keys().filter(|k| k.contains("mystery")).collect();
        assert_eq!(unknown.len(), 1);
        assert!(!unknown[0].starts_with('?'));
        assert_eq!(models[unknown[0]], json!({"output": 7, "total_tokens": 7, "cost_usd": 0.0}));
        let (got, _) = r.h.sessions(&[A.to_string()], 60.0);
        assert_eq!(got[A]["models"]["claude-haiku-4-5"]["output"], 2 * M);
    }

    #[test]
    fn answers_follow_the_files() {
        let mut r = Rig::new();
        let path = r.write(A, &[out_at("a1", &stamp(0, 11), M)]);
        assert_eq!(r.totals(now() - 7200.0)["total_tokens"], M);
        r.put(A, &[out_at("a2", &stamp(0, 11), M)], "proj", None, true);
        set_mtime(&path, now() + 1.0);
        assert_eq!(r.totals(now() - 7200.0)["total_tokens"], 2 * M);
    }

    #[test]
    fn not_a_session_id_finds_nothing() {
        let r = Rig::new();
        assert!(session_files("../etc", &r.root).is_empty());
    }
}
