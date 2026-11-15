//! Spend: the budget, the cost ledger, usage windows and the error report.

use super::*;

impl Shared {
    /// Control._budget_crossed: a usage threshold was crossed. Notifications
    /// come with the notifier's port; the event is recorded as Python does.
    pub(super) fn budget_crossed(&self, crossing: &Value) {
        let window = crossing["window"].as_str().unwrap_or("");
        // Pushed and spoken only by a dispatcher with a store of its own:
        // the read-only shadow must never buzz a phone.
        let channels = if self.store.is_some() {
            self.notify_enabled()
        } else {
            vec![]
        };
        let text = self.notifier.budget(&channels, crossing);
        if let Some(store) = &self.store {
            let e = json!({"kind": "budget", "summary": text, "window": window, "used": crossing["used"]});
            store.add_event_json(e);
        }
    }

    /// Control._global_windows: [(name, since, resets_at)] the global
    /// spend is counted over.
    fn global_windows(&self, limits: &Value, now: f64) -> Vec<(String, f64, Value)> {
        let spans = [("five_hour", 5.0 * 3600.0), ("seven_day", 7.0 * 86400.0)];
        if spans.iter().any(|(n, _)| truthy(limits.get(*n))) || crate::usage::subscription() {
            return spans
                .iter()
                .map(|(name, span)| {
                    let w = limits.get(*name).cloned().unwrap_or(json!({}));
                    match w
                        .get("resets_at")
                        .and_then(Value::as_f64)
                        .filter(|r| *r != 0.0)
                    {
                        Some(r) => (name.to_string(), r - span, json!(r)),
                        None if truthy(w.get("waiting")) => (
                            name.to_string(),
                            w["reset_at"].as_f64().unwrap_or(0.0),
                            Value::Null,
                        ),
                        None => (
                            name.to_string(),
                            ((now - span) / 60.0).floor() * 60.0,
                            Value::Null,
                        ),
                    }
                })
                .collect();
        }
        vec![("month".into(), paths::local_month_start(now), Value::Null)]
    }

    /// Control.spend: tokens and estimated cost, global and current.
    pub(super) fn spend(&self, now: f64, budget: f64, limits: &Value) -> Value {
        let mut complete = true;
        let mut windows = Vec::new();
        for (name, since, resets) in self.global_windows(limits, now) {
            let mut got = self
                .cost_history
                .lk()
                .totals(since, None, now, budget);
            complete = complete && got["complete"].as_bool().unwrap_or(true);
            got.as_object_mut().unwrap().shift_remove("complete");
            got["name"] = json!(name);
            got["since"] = json!(since);
            got["resets_at"] = resets;
            windows.push(got);
        }
        let rows: Vec<Row> = self
            .sessions(2.0)
            .iter()
            .filter(|r| matrix_active(r))
            .cloned()
            .collect();
        let local: Vec<String> = rows
            .iter()
            .filter(|r| !truthy(r.get("remote")))
            .filter_map(|r| {
                r.get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        let (per, ok) = self.cost_history.lk().sessions(&local, budget);
        complete = complete && ok;
        let mut tokens: indexmap::IndexMap<String, i64> = indexmap::IndexMap::new();
        let mut models: indexmap::IndexMap<String, (i64, i64, f64)> = indexmap::IndexMap::new();
        let (mut cost, mut each) = (0.0f64, Vec::new());
        for row in &rows {
            let sid = row.get("sessionId").and_then(Value::as_str).unwrap_or("");
            let got = if truthy(row.get("remote")) {
                None
            } else {
                per.get(sid)
            };
            let usd = match got {
                Some(g) => g["cost_usd"].clone(),
                None => self
                    .usage
                    .session(sid)
                    .and_then(|u| u.get("cost_usd").cloned())
                    .unwrap_or(Value::Null),
            };
            let tk = got.map(|g| g["tokens"].clone()).unwrap_or(Value::Null);
            for (k, v) in tk.as_object().into_iter().flatten() {
                *tokens.entry(k.clone()).or_insert(0) += v.as_i64().unwrap_or(0);
            }
            for (name, m) in got
                .and_then(|g| g["models"].as_object())
                .into_iter()
                .flatten()
            {
                let into = models.entry(name.clone()).or_insert((0, 0, 0.0));
                into.0 += m["output"].as_i64().unwrap_or(0);
                into.1 += m["total_tokens"].as_i64().unwrap_or(0);
                into.2 += m["cost_usd"].as_f64().unwrap_or(0.0);
            }
            if let Some(u) = usd.as_f64() {
                cost += u;
            }
            each.push(json!({"key": row.get("key").cloned().unwrap_or(Value::Null), "sessionId": sid,
                             "name": row.get("name").cloned().unwrap_or(Value::Null), "tokens": tk,
                             "total_tokens": got.map(|g| g["total_tokens"].clone()).unwrap_or(json!(0)),
                             "cost_usd": usd}));
        }
        each.sort_by(|a, b| {
            let c = |e: &Value| -(e["cost_usd"].as_f64().filter(|c| *c != 0.0).unwrap_or(0.0));
            c(a).partial_cmp(&c(b)).unwrap_or(std::cmp::Ordering::Equal)
        });
        let total_tokens: i64 = tokens.values().sum();
        let tokens: Map<String, Value> = tokens.into_iter().map(|(k, v)| (k, json!(v))).collect();
        json!({
            "global": {"windows": windows},
            "current": {"sessions": rows.len(), "tokens": tokens, "total_tokens": total_tokens,
                        "cost_usd": cost,
                        "models": models.into_iter().map(|(k, (o, t, c))| (k, json!({"output": o, "total_tokens": t, "cost_usd": c}))).collect::<Map<String, Value>>(),
                        "each": each},
            "estimated": true, "complete": complete, "now": now,
        })
    }

    /// GET /api/usage: Control.usage_summary - the meters and the spend.
    pub(super) fn api_usage(&self) -> Value {
        let now = Self::now();
        let mut out = self.usage.summary(now);
        let limits = out["limits"].clone();
        out["spend"] = self.spend(now, 1.0, &limits);
        out
    }

    /// Control.errors: tool failures across recent sessions.
    pub(super) fn api_errors(&self, days: f64) -> Value {
        let now = Self::now();
        if let Some((at, d, result)) = self.errors_cache.lk().as_ref() {
            if *d == days && now - at < 300.0 {
                return result.clone();
            }
        }
        let rows: Vec<Map<String, Value>> = self
            .history
            .list()
            .iter()
            .filter(|r| {
                now - r.get("modified").and_then(Value::as_f64).unwrap_or(0.0) <= days * 86400.0
            })
            .take(120)
            .cloned()
            .collect();
        let mut per_project: indexmap::IndexMap<String, (i64, i64, i64)> =
            indexmap::IndexMap::new();
        let mut tallies = Vec::new();
        for r in &rows {
            let sid = r
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let size = r.get("size").cloned().unwrap_or(Value::Null);
            let cached = self
                .tallies
                .lk()
                .get(&sid)
                .filter(|c| c.0 == size)
                .map(|c| c.1.clone());
            let t = match cached {
                Some(t) => t,
                None => {
                    let Ok(c) = self.conversation(&sid) else {
                        continue;
                    };
                    let t = activity::tally(&activity::timeline(&c.merged));
                    self.tallies
                        .lk()
                        .insert(sid.clone(), (size, t.clone()));
                    t
                }
            };
            let cwd = r
                .get("cwd")
                .filter(|v| truthy(Some(v)))
                .map(crate::pyish::py_str)
                .unwrap_or_else(|| "?".into());
            let cwd = cwd.replace('\\', "/");
            let name = cwd
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let name = if name.is_empty() {
                "?".to_string()
            } else {
                name
            };
            let p = per_project.entry(name).or_insert((0, 0, 0));
            p.0 += t["calls"].as_i64().unwrap_or(0);
            p.1 += t["errors"].as_i64().unwrap_or(0);
            p.2 += 1;
            tallies.push(t);
        }
        let mut out = activity::merge_tallies(&tallies);
        out.insert("ok".into(), json!(true));
        out.insert("days".into(), json!(days));
        out.insert("sessions".into(), json!(tallies.len()));
        out.insert(
            "by_project".into(),
            Value::Object(
                per_project
                    .into_iter()
                    .map(|(k, (c, e, s))| (k, json!({"calls": c, "errors": e, "sessions": s})))
                    .collect(),
            ),
        );
        out.insert("at".into(), json!(now));
        let result = Value::Object(out);
        *self.errors_cache.lk() = Some((now, days, result.clone()));
        result
    }
}
