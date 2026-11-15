//! Context, cost and rate-limit readings from the statusline, and when the
//! limit will be hit: a port of `imperatorium/usage.py`. In memory, except
//! the account's windows, kept in limits.json beside the state so a restart
//! shows the meters at once (written only when this binary has a store).

use crate::locks::LockExt;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use crate::pyish::{py_fsum_builtin, truthy};

const WINDOWS: &[&str] = &["five_hour", "seven_day", "spend_limit"];
const PACE_SPAN: f64 = 45.0 * 60.0;
const MIN_SPAN: f64 = 5.0 * 60.0;
pub const DEFAULT_THRESHOLDS: &[f64] = &[80.0, 95.0];
const PLAN_SPAN: f64 = 30.0 * 60.0;

fn providers(p: &str) -> Option<&'static str> {
    Some(match p {
        "bedrock" => "AWS Bedrock",
        "vertex" => "Google Vertex AI",
        "foundry" => "Microsoft Foundry",
        "api" => "Anthropic API",
        "gateway" => "API gateway",
        _ => return None,
    })
}

/// usage._num: a number (not a bool) as a float, else None.
fn num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
}

fn jnum(v: Option<f64>) -> Value {
    v.map(|f| json!(f)).unwrap_or(Value::Null)
}

fn sub<'a>(v: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    v.filter(|x| truthy(Some(x))).and_then(|x| x.get(k))
}

/// usage.provider_of: 'bedrock', 'vertex', ... or None for the login.
pub fn provider_of(payload: &Map<String, Value>) -> Option<String> {
    let named = [
        payload.get("imperatorium_provider"),
        payload.get("ccontrol_provider"),
    ]
    .into_iter()
    .flatten()
    .find(|v| truthy(Some(v)))
    .and_then(Value::as_str);
    if let Some(n) = named.filter(|n| providers(n).is_some()) {
        return Some(n.into());
    }
    let mid = sub(payload.get("model"), "id")
        .filter(|v| truthy(Some(v)))
        .map(crate::pyish::py_str)
        .unwrap_or_default();
    if mid.starts_with("arn:aws:bedrock")
        || mid.contains(".anthropic.claude")
        || mid.starts_with("anthropic.claude")
    {
        return Some("bedrock".into());
    }
    if mid.starts_with("claude-") && mid.contains('@') {
        return Some("vertex".into());
    }
    None
}

/// `oauthAccount.organizationType` from ~/.claude.json, lower-cased.
fn org_type() -> String {
    std::fs::read_to_string(crate::paths::user_home().join(".claude.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            v.get("oauthAccount")
                .and_then(|o| o.get("organizationType"))
                .cloned()
        })
        .filter(|v| truthy(Some(v)))
        .map(|v| crate::pyish::py_str(&v).to_lowercase())
        .unwrap_or_default()
}

/// usage.account_plan: 'Claude Enterprise' / 'Claude Team', else None.
pub fn account_plan() -> Option<&'static str> {
    let kind = org_type();
    if kind.contains("enterprise") {
        Some("Claude Enterprise")
    } else if kind.contains("team") {
        Some("Claude Team")
    } else {
        None
    }
}

/// usage.subscription: a Pro or Max login (five-hour and weekly windows).
pub fn subscription() -> bool {
    let kind = org_type();
    kind.contains("max") || kind.contains("pro")
}

/// usage.reading: the fields worth keeping from one statusline payload.
pub fn reading(payload: &Map<String, Value>) -> Map<String, Value> {
    let ctx = payload.get("context_window");
    let cost = payload.get("cost");
    let model = payload.get("model");
    let mut limits = Map::new();
    for name in WINDOWS {
        let w = sub(payload.get("rate_limits"), name);
        let (pct, resets) = (num(sub(w, "used_percentage")), num(sub(w, "resets_at")));
        if let Some(p) = pct {
            limits.insert(
                (*name).into(),
                json!({"used": p, "resets_at": jnum(resets)}),
            );
        }
    }
    let model_name = [sub(model, "display_name"), sub(model, "id")]
        .into_iter()
        .flatten()
        .find(|v| truthy(Some(v)))
        .cloned()
        .or_else(|| sub(model, "id").cloned())
        .unwrap_or(Value::Null);
    json!({
        "context_pct": jnum(num(sub(ctx, "used_percentage"))),
        "context_size": jnum(num(sub(ctx, "context_window_size"))),
        "cost_usd": jnum(num(sub(cost, "total_cost_usd"))),
        "duration_ms": jnum(num(sub(cost, "total_duration_ms"))),
        "api_duration_ms": jnum(num(sub(cost, "total_api_duration_ms"))),
        "lines_added": jnum(num(sub(cost, "total_lines_added"))),
        "lines_removed": jnum(num(sub(cost, "total_lines_removed"))),
        "model": model_name,
        "provider": provider_of(payload).map(Value::String).unwrap_or(Value::Null),
        "limits": limits,
    })
    .as_object()
    .unwrap()
    .clone()
}

fn same_window(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => (x - y).abs() <= 120.0,
        (None, None) => true,
        _ => false,
    }
}

/// usage.pace: percent per hour over the recent samples, and when 100 is
/// reached; None with too little history.
pub fn pace(samples: &[(f64, f64)], now: f64, resets_at: Option<f64>) -> Value {
    let recent: Vec<(f64, f64)> = samples
        .iter()
        .copied()
        .filter(|(t, _)| now - t <= PACE_SPAN)
        .collect();
    if recent.len() < 2 || recent[recent.len() - 1].0 - recent[0].0 < MIN_SPAN {
        return Value::Null;
    }
    let n = recent.len() as f64;
    let mt = py_fsum_builtin(recent.iter().map(|(t, _)| *t)) / n;
    let mp = py_fsum_builtin(recent.iter().map(|(_, p)| *p)) / n;
    let var = py_fsum_builtin(recent.iter().map(|(t, _)| (t - mt).powi(2)));
    if var <= 0.0 {
        return Value::Null;
    }
    let slope = py_fsum_builtin(recent.iter().map(|(t, p)| (t - mt) * (p - mp))) / var;
    let per_hour = slope * 3600.0;
    let last = recent[recent.len() - 1].1;
    if slope <= 0.0 {
        return json!({"per_hour": per_hour.max(0.0), "full_at": null, "before_reset": false});
    }
    let full_at = now + (100.0 - last) / slope;
    let before = resets_at.is_some_and(|r| r != 0.0 && full_at < r);
    json!({"per_hour": per_hour, "full_at": full_at, "before_reset": before})
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Map<String, Value>>,
    limits: indexmap::IndexMap<String, Map<String, Value>>,
    history: HashMap<String, Vec<(f64, f64)>>,
    fired: HashSet<(String, String, String)>,
    window: HashMap<String, Option<f64>>,
    saved: Option<Vec<(String, String, String)>>,
}

pub struct Usage {
    inner: Mutex<Inner>,
    thresholds: Mutex<Vec<f64>>,
    /// limits.json, written when `writes` (a store of our own).
    path: Option<PathBuf>,
    writes: bool,
}

fn fkey(v: Option<f64>) -> String {
    v.map(crate::pyish::float_repr)
        .unwrap_or_else(|| "None".into())
}

impl Usage {
    pub fn thresholds(&self) -> Vec<f64> {
        self.thresholds.lk().clone()
    }

    /// A saved budget.thresholds applies at once, sorted, as in Python.
    pub fn set_thresholds(&self, mut t: Vec<f64>) {
        t.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        *self.thresholds.lk() = t;
    }

    pub fn new(thresholds: Vec<f64>, path: Option<PathBuf>, writes: bool, now: f64) -> Self {
        let mut t = thresholds;
        t.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let u = Usage {
            inner: Mutex::new(Inner::default()),
            thresholds: Mutex::new(t),
            path,
            writes,
        };
        u.load(now);
        u
    }

    fn signature(inner: &Inner) -> Vec<(String, String, String)> {
        let mut s: Vec<(String, String, String)> = inner
            .limits
            .iter()
            .map(|(n, w)| {
                (
                    n.clone(),
                    fkey(num(w.get("used"))),
                    fkey(num(w.get("resets_at"))),
                )
            })
            .collect();
        s.sort();
        s
    }

    fn load(&self, now: f64) {
        let Some(path) = &self.path else { return };
        let Some(got) = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            return;
        };
        let mut inner = self.inner.lk();
        for (name, w) in got
            .get("limits")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if !WINDOWS.contains(&name.as_str()) || !w.is_object() {
                continue;
            }
            let (Some(used), resets, at) = (
                num(w.get("used")),
                num(w.get("resets_at")),
                num(w.get("at")),
            ) else {
                continue;
            };
            let at = at.filter(|a| *a != 0.0).unwrap_or(now);
            inner.limits.insert(
                name.clone(),
                json!({"used": used, "resets_at": jnum(resets), "at": at})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            inner.window.insert(name.clone(), resets);
            inner.history.insert(name.clone(), vec![]);
            for t in &self.thresholds() {
                if used >= *t {
                    inner
                        .fired
                        .insert((name.clone(), fkey(resets), crate::pyish::float_repr(*t)));
                }
            }
        }
        inner.saved = Some(Self::signature(&inner));
    }

    fn save(&self, inner: &mut Inner) {
        let (Some(path), true) = (&self.path, self.writes) else {
            return;
        };
        let sig = Self::signature(inner);
        if inner.saved.as_ref() == Some(&sig) {
            return;
        }
        let body = json!({"limits": inner.limits.iter().map(|(k, v)| (k.clone(), Value::Object(v.clone()))).collect::<Map<String, Value>>()});
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if crate::paths::write_atomic(path, body.to_string().as_bytes()).is_ok() {
            inner.saved = Some(sig);
        }
    }

    /// Usage.ingest: one statusline payload; the highest threshold crossed.
    pub fn ingest(&self, key: &str, payload: &Map<String, Value>, now: f64) -> Option<Value> {
        let got = reading(payload);
        let mut crossings: Vec<Value> = Vec::new();
        let mut inner = self.inner.lk();
        let mut rec = got.clone();
        rec.insert("at".into(), json!(now));
        inner.sessions.insert(key.into(), rec);
        for (name, w) in got
            .get("limits")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let used = num(w.get("used")).unwrap_or(0.0);
            let resets = num(w.get("resets_at"));
            let prev = inner.limits.get(name).cloned();
            let mut window = inner.window.get(name).copied().flatten();
            if prev
                .as_ref()
                .is_none_or(|p| !same_window(num(p.get("resets_at")), resets))
            {
                inner.history.insert(name.clone(), vec![]);
                window = resets;
                inner.window.insert(name.clone(), resets);
            }
            let mut entry = w.as_object().cloned().unwrap_or_default();
            entry.insert("at".into(), json!(now));
            inner.limits.insert(name.clone(), entry);
            let hist = inner.history.entry(name.clone()).or_default();
            if hist
                .last()
                .is_none_or(|(t, p)| *p != used || now - t > 60.0)
            {
                hist.push((now, used));
                if hist.len() > 240 {
                    let cut = hist.len() - 240;
                    hist.drain(..cut);
                }
            }
            for t in self.thresholds() {
                let mark = (name.clone(), fkey(window), crate::pyish::float_repr(t));
                if used >= t && !inner.fired.contains(&mark) {
                    inner.fired.insert(mark);
                    crossings.push(json!({"window": name, "threshold": t, "used": used, "resets_at": jnum(resets)}));
                }
            }
        }
        self.save(&mut inner);
        crossings.into_iter().max_by(|a, b| {
            a["threshold"]
                .as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&b["threshold"].as_f64().unwrap_or(0.0))
                .unwrap()
        })
    }

    pub fn forget(&self, key: &str) {
        self.inner.lk().sessions.remove(key);
    }

    pub fn session(&self, key: &str) -> Option<Map<String, Value>> {
        self.inner.lk().sessions.get(key).cloned()
    }

    /// Usage.summary: the account's windows, each with its pace.
    pub fn summary(&self, now: f64) -> Value {
        let inner = self.inner.lk();
        let mut out = Map::new();
        for (name, w) in &inner.limits {
            let resets = num(w.get("resets_at"));
            if let Some(r) = resets.filter(|r| *r != 0.0 && *r < now) {
                out.insert(
                    name.clone(),
                    json!({"used": 0.0, "resets_at": null, "at": w.get("at").cloned().unwrap_or(Value::Null),
                           "reset_at": r, "waiting": true, "pace": null}),
                );
                continue;
            }
            let mut entry = w.clone();
            entry.insert(
                "pace".into(),
                pace(
                    inner.history.get(name).map(Vec::as_slice).unwrap_or(&[]),
                    now,
                    resets,
                ),
            );
            out.insert(name.clone(), Value::Object(entry));
        }
        let seen: Vec<Option<String>> = inner
            .sessions
            .values()
            .filter(|s| now - s.get("at").and_then(Value::as_f64).unwrap_or(0.0) <= PLAN_SPAN)
            .map(|s| {
                s.get("provider")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        let has = !out.is_empty();
        json!({"limits": out, "thresholds": self.thresholds(), "plan": plan(&seen, has)})
    }
}

/// Usage.plan: what to show where the meters go, or None.
fn plan(seen: &[Option<String>], has_limits: bool) -> Value {
    let mut others: Vec<String> = seen
        .iter()
        .flatten()
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    others.sort();
    let mut labels: Vec<String> = others
        .iter()
        .filter_map(|p| providers(p))
        .map(str::to_string)
        .collect();
    if !has_limits && (others.is_empty() || seen.iter().any(Option::is_none)) {
        if let Some(acct) = account_plan() {
            labels.insert(0, acct.into());
        }
    }
    if labels.is_empty() {
        return Value::Null;
    }
    json!({"label": labels.join(" \u{b7} "), "providers": others, "alongside": has_limits})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_keeps_what_matters() {
        let p = json!({"model": {"display_name": "Opus", "id": "claude-opus-5-5"},
                       "context_window": {"used_percentage": 23, "context_window_size": 200000},
                       "rate_limits": {"five_hour": {"used_percentage": 41.5, "resets_at": 1800000000},
                                       "seven_day": {"used_percentage": true}}});
        let r = reading(p.as_object().unwrap());
        assert_eq!(r["context_pct"], 23.0);
        assert_eq!(r["model"], "Opus");
        assert_eq!(r["limits"]["five_hour"]["used"], 41.5);
        assert!(r["limits"].get("seven_day").is_none()); // a bool is not a number
    }

    #[test]
    fn pace_needs_five_minutes_of_readings() {
        assert_eq!(pace(&[(0.0, 10.0), (60.0, 11.0)], 60.0, None), Value::Null);
        // python: usage.pace([(0.0,10.0),(600.0,20.0)], 600.0, 3000.0)
        //   -> {'per_hour': 60.0, 'full_at': 5400.0, 'before_reset': False}
        let p = pace(&[(0.0, 10.0), (600.0, 20.0)], 600.0, Some(3000.0));
        assert_eq!(p["per_hour"], 60.0);
        assert_eq!(p["full_at"], 5400.0);
        assert_eq!(p["before_reset"], false);
        assert_eq!(
            pace(&[(0.0, 10.0), (600.0, 20.0)], 600.0, Some(6000.0))["before_reset"],
            true
        );
    }
}

/// Ported from the Python suite (test_features.py): readings, pace,
/// threshold crossings and windows that reset.
#[cfg(test)]
mod features_tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn five_hour(used: f64, resets: f64) -> Map<String, Value> {
        obj(json!({"rate_limits": {"five_hour": {"used_percentage": used, "resets_at": resets}}}))
    }

    fn fresh() -> Usage {
        Usage::new(DEFAULT_THRESHOLDS.to_vec(), None, false, 0.0)
    }

    #[test]
    fn features_reading_extracts_the_meter_fields_and_tolerates_none() {
        let got = reading(&obj(json!({
            "context_window": {"used_percentage": 12.5, "context_window_size": 200000},
            "cost": {"total_cost_usd": 1.23},
            "model": {"display_name": "Opus 5.5"},
            "rate_limits": {"five_hour": {"used_percentage": 40, "resets_at": 999}, "junk": {}},
        })));
        assert_eq!(got["context_pct"], 12.5);
        assert_eq!(got["cost_usd"], 1.23);
        assert_eq!(got["model"], "Opus 5.5");
        assert_eq!(got["limits"], json!({"five_hour": {"used": 40.0, "resets_at": 999.0}}));
        let empty = reading(&Map::new());
        assert_eq!(empty["context_pct"], Value::Null);
        assert_eq!(empty["limits"], json!({}));
    }

    #[test]
    fn features_pace_flat_falling_and_old_samples() {
        let now = 10_000.0;
        let falling = pace(&[(now - 1200.0, 50.0), (now - 600.0, 40.0), (now, 30.0)], now, None);
        assert_eq!(falling["per_hour"], 0.0);
        assert_eq!(falling["full_at"], Value::Null);
        assert_eq!(falling["before_reset"], false);
        // A sample older than the pace span (45 minutes) does not count.
        let now = 100_000.0;
        let old = pace(&[(now - 100_000.0, 0.0), (now - 600.0, 20.0), (now, 30.0)], now, None);
        assert_ne!(old, Value::Null); // only the last two count
        assert_eq!(old["per_hour"], 60.0);
        // Rising: when it hits 100.
        let now = 10_000.0;
        let rising = pace(&[(now - 1200.0, 10.0), (now - 600.0, 20.0), (now, 30.0)], now, None);
        assert!(rising["per_hour"].as_f64().unwrap() > 0.0);
        assert!(rising["full_at"].as_f64().unwrap() > now);
    }

    #[test]
    fn features_a_threshold_crossing_fires_once_and_the_top_one_wins() {
        let u = Usage::new(vec![80.0, 95.0], None, false, 0.0);
        let first = u.ingest("s1", &five_hour(85.0, 5000.0), 1.0).expect("crossed 80");
        assert_eq!(first["threshold"], 80.0);
        assert!(u.ingest("s1", &five_hour(85.0, 5000.0), 2.0).is_none());
        let jump = Usage::new(vec![80.0, 95.0], None, false, 0.0);
        let top = jump.ingest("s1", &five_hour(97.0, 5000.0), 1.0).unwrap();
        assert_eq!(top["threshold"], 95.0);
    }

    #[test]
    fn features_a_new_reset_time_starts_a_fresh_history() {
        let u = fresh();
        u.ingest("s1", &five_hour(10.0, 100.0), 1.0);
        // A reset time that only jitters is the same window.
        u.ingest("s1", &five_hour(15.0, 150.0), 2.0);
        assert_eq!(u.inner.lk().history["five_hour"], vec![(1.0, 10.0), (2.0, 15.0)]);
        // One that really moved on starts over.
        u.ingest("s1", &five_hour(90.0, 100_000.0), 3.0);
        assert_eq!(u.inner.lk().history["five_hour"], vec![(3.0, 90.0)]);
    }

    #[test]
    fn features_a_window_that_reset_waits_at_zero_for_a_new_reading() {
        let u = fresh();
        u.ingest("s1", &five_hour(10.0, 50.0), 1.0);
        let w = u.summary(1000.0)["limits"]["five_hour"].clone();
        assert_eq!(
            (w["used"].clone(), w["resets_at"].clone(), w["reset_at"].clone(), w["waiting"].clone()),
            (json!(0.0), Value::Null, json!(50.0), json!(true))
        );
        assert_eq!(w["pace"], Value::Null);
        // The new window's first reading replaces it.
        u.ingest("s1", &five_hour(3.0, 18050.0), 1001.0);
        let w = u.summary(1002.0)["limits"]["five_hour"].clone();
        assert_eq!((w["used"].clone(), w["resets_at"].clone()), (json!(3.0), json!(18050.0)));
        assert!(w.get("waiting").is_none());
    }

    #[test]
    fn features_forget_drops_a_sessions_reading() {
        let u = fresh();
        u.ingest("s1", &obj(json!({"context_window": {"used_percentage": 5}})), 1.0);
        assert!(u.session("s1").is_some());
        u.forget("s1");
        assert!(u.session("s1").is_none());
    }

    #[test]
    fn features_provider_comes_from_the_script_or_the_model_id() {
        let of = |v: Value| provider_of(&obj(v));
        assert_eq!(of(json!({"imperatorium_provider": "vertex"})).as_deref(), Some("vertex"));
        for mid in [
            "us.anthropic.claude-opus-5-5-v1:0",
            "anthropic.claude-sonnet-5-5-v1:0",
            "arn:aws:bedrock:us-east-1:123:inference-profile/x",
        ] {
            assert_eq!(of(json!({"model": {"id": mid}})).as_deref(), Some("bedrock"), "{mid}");
        }
        assert_eq!(
            of(json!({"model": {"id": "claude-opus-5-5@20260801"}})).as_deref(),
            Some("vertex")
        );
        assert_eq!(of(json!({"model": {"id": "claude-opus-5-5"}})), None);
        assert_eq!(of(json!({"imperatorium_provider": "nonsense"})), None);
    }

    #[test]
    fn features_a_session_gone_quiet_stops_counting() {
        // (The account's own plan reads the real ~/.claude.json, so only a
        // provider name is used here; the HTTP tests cover the login.)
        let u = fresh();
        u.ingest("s1", &obj(json!({"imperatorium_provider": "bedrock"})), 1.0);
        let plan = u.summary(2.0)["plan"].clone();
        assert_eq!(plan["label"], "AWS Bedrock");
        assert_eq!(plan["providers"], json!(["bedrock"]));
        assert_eq!(plan["alongside"], false);
        let quiet = u.summary(1.0 + PLAN_SPAN + 1.0)["plan"].clone();
        assert_ne!(quiet["label"], "AWS Bedrock");
    }
}
