//! The pull request a session's branch belongs to, and whether CI likes it.
//! A port of `imperatorium/prs.py`: `gh pr view` in the session's folder,
//! never on the request path - a thread refreshes the folders the sidebar
//! is showing and rows read the cache. A folder whose branch has no PR is
//! remembered as such (one call per refresh period, not per poll).

use crate::locks::LockExt;
use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

const FIELDS: &str = "number,state,url,title,isDraft,reviewDecision,statusCheckRollup,headRefName";

/// prs.checks_state: one word for a statusCheckRollup.
pub fn checks_state(rollup: Option<&Value>) -> &'static str {
    let Some(items) = rollup.and_then(Value::as_array).filter(|a| !a.is_empty()) else {
        return "none";
    };
    let s = |c: &Value, k: &str| c.get(k).and_then(Value::as_str).unwrap_or("").to_uppercase();
    let (mut pass, mut fail, mut pending) = (false, false, false);
    for c in items {
        if !c.is_object() {
            continue;
        }
        let context = c.get("__typename").and_then(Value::as_str) == Some("StatusContext")
            || (c.get("state").is_some() && c.get("status").is_none());
        if context {
            match s(c, "state").as_str() {
                "SUCCESS" => pass = true,
                "FAILURE" | "ERROR" => fail = true,
                _ => pending = true,
            }
        } else if s(c, "status") != "COMPLETED" {
            pending = true;
        } else if ["SUCCESS", "NEUTRAL", "SKIPPED"].contains(&s(c, "conclusion").as_str()) {
            pass = true;
        } else {
            fail = true;
        }
    }
    if fail {
        "failing"
    } else if pending {
        "pending"
    } else if pass {
        "passing"
    } else {
        "none"
    }
}

/// prs.summarise: the fields the sidebar badge needs.
pub fn summarise(raw: &Value) -> Value {
    let low = |k: &str| raw.get(k).and_then(Value::as_str).unwrap_or("").to_lowercase();
    let review = low("reviewDecision");
    json!({
        "number": raw.get("number").cloned().unwrap_or(Value::Null),
        "state": low("state"),
        "draft": crate::pyish::truthy(raw.get("isDraft")),
        "url": raw.get("url").cloned().unwrap_or(Value::Null),
        "title": raw.get("title").cloned().unwrap_or(Value::Null),
        "branch": raw.get("headRefName").cloned().unwrap_or(Value::Null),
        "review": if review.is_empty() { Value::Null } else { json!(review) },
        "checks": checks_state(raw.get("statusCheckRollup")),
    })
}

/// prs.lookup: Ok(the PR for cwd's branch, or None); Err only when gh is
/// missing - a failure worth showing, and the end of polling.
fn lookup(cwd: &str) -> Result<Option<Value>, String> {
    let mut c = Command::new("gh");
    c.args(["pr", "view", "--json", FIELDS]).current_dir(cwd);
    let o = match crate::proc::run_timeout(&mut c, Duration::from_secs(20)) {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err("gh is not installed".into()),
        Err(_) => return Ok(None),
    };
    if !o.status.success() {
        return Ok(None);
    }
    Ok(serde_json::from_slice::<Value>(&o.stdout).ok().map(|raw| summarise(&raw)))
}

struct Entry {
    at: f64,
    pr: Option<Value>,
    branch: String,
}

#[derive(Default)]
struct Inner {
    cache: HashMap<String, Entry>,   // normalised cwd ->
    wanted: HashMap<String, f64>,    // cwd -> when the sidebar last asked
    disabled: Option<String>,
}

/// prs.Tracker: {cwd: pr-or-None}, refreshed off the request path.
#[derive(Default)]
pub struct Tracker {
    inner: Mutex<Inner>,
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl Tracker {
    /// What is known for `cwd` now; asks for it to be refreshed.
    pub fn get(&self, cwd: &str) -> Option<Value> {
        if cwd.is_empty() {
            return None;
        }
        let key = crate::paths::abs_normcase(cwd).unwrap_or_else(|| cwd.to_string());
        let mut inner = self.inner.lk();
        inner.wanted.insert(cwd.to_string(), now());
        inner.cache.get(&key).and_then(|e| e.pr.clone())
    }

    /// Tracker._loop: a pass every 5 s, for as long as the process lives.
    pub fn start(self: &std::sync::Arc<Self>) {
        let me = std::sync::Arc::clone(self);
        std::thread::spawn(move || loop {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| me.poll_once()));
            std::thread::sleep(Duration::from_secs_f64(crate::timing::t().pr_pass));
        });
    }

    /// Tracker.poll_once: refresh every wanted folder whose entry is older
    /// than REFRESH or was fetched for another branch.
    pub fn poll_once(&self) {
        let t = now();
        let wanted: Vec<String> = {
            let mut inner = self.inner.lk();
            inner.wanted.retain(|_, asked| t - *asked <= 10.0 * crate::timing::t().pr_refresh);
            inner.wanted.keys().cloned().collect()
        };
        for cwd in wanted {
            if self.inner.lk().disabled.is_some() {
                return;
            }
            let key = crate::paths::abs_normcase(&cwd).unwrap_or_else(|| cwd.clone());
            let Ok(branch) = crate::changes::git(&cwd, &["branch", "--show-current"], true) else { continue };
            let branch = crate::pyish::strip(&branch).to_string();
            {
                let inner = self.inner.lk();
                if let Some(hit) = inner.cache.get(&key) {
                    if hit.branch == branch && t - hit.at < crate::timing::t().pr_refresh {
                        continue;
                    }
                }
            }
            match lookup(&cwd) {
                Err(e) => {
                    self.inner.lk().disabled = Some(e);
                    return;
                }
                Ok(pr) => {
                    self.inner.lk().cache.insert(key, Entry { at: t, pr, branch });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_fold_both_shapes() {
        assert_eq!(checks_state(None), "none");
        assert_eq!(checks_state(Some(&json!([]))), "none");
        assert_eq!(checks_state(Some(&json!([{"status": "COMPLETED", "conclusion": "SUCCESS"},
                                               {"__typename": "StatusContext", "state": "SUCCESS"}]))), "passing");
        assert_eq!(checks_state(Some(&json!([{"status": "IN_PROGRESS"},
                                               {"status": "COMPLETED", "conclusion": "SUCCESS"}]))), "pending");
        assert_eq!(checks_state(Some(&json!([{"status": "IN_PROGRESS"}, {"state": "ERROR"}]))), "failing");
        assert_eq!(checks_state(Some(&json!(["x", 1]))), "none");
    }

    #[test]
    fn summary_fields() {
        let s = summarise(&json!({"number": 7, "state": "OPEN", "isDraft": true, "reviewDecision": "",
                                  "headRefName": "b", "statusCheckRollup": null}));
        assert_eq!(s["state"], "open");
        assert_eq!(s["draft"], true);
        assert_eq!(s["review"], Value::Null);
        assert_eq!(s["checks"], "none");
        assert_eq!(s["url"], Value::Null);
    }
}
