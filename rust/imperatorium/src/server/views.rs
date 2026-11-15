//! Read-only views of one session or the fleet: activity, status, awaited input, processes, changes, hosts, and the project catalogue.

use super::*;

impl Shared {
    /// Control.hosts_status: GET /api/hosts.
    pub(super) fn hosts_status(&self) -> Value {
        let Some(fleet) = &self.fleet else {
            return json!({"hosts": [], "status": {}});
        };
        let tunnels = self.tunnels.lk();
        let hosts: Vec<Value> = fleet
            .hosts
            .iter()
            .map(|h| {
                let mut e = h.public();
                if let Some(t) = tunnels.get(&h.name) {
                    e.insert("tunnel_status".into(), t.status());
                }
                Value::Object(e)
            })
            .collect();
        json!({"hosts": hosts, "status": fleet.status()})
    }

    /// Control.catalogue, local part: the roots from config.json (else the
    /// folder the checkout sits in) and the folders sessions run in.
    pub(super) fn catalogue(&self) -> Vec<Value> {
        let configured: Vec<String> = self
            .config()
            .get("roots")
            .and_then(Value::as_array)
            .map(|r| {
                r.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let roots = if configured.is_empty() {
            // projects.DEFAULT_ROOTS: the folder the checkout sits in.
            let home = paths::config_home(self.home.as_deref());
            vec![home
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()]
        } else {
            configured
        };
        let rows = self.sessions(2.0);
        let local: Vec<String> = rows
            .iter()
            .filter(|r| !truthy(r.get("remote")))
            .filter_map(|r| {
                r.get("cwd")
                    .and_then(Value::as_str)
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
            })
            .collect();
        let mut found = projects::discover(&roots, &local);
        // A remote project is whatever a remote session has worked in,
        // host-qualified: the same name exists on more than one machine,
        // and firing at the wrong one is the wrong-repository mistake.
        let mut seen = std::collections::HashSet::new();
        for r in rows.iter() {
            let (Some(host), Some(cwd)) = (
                r.get("host").filter(|_| truthy(r.get("remote"))).map(crate::pyish::py_str),
                r.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty()),
            ) else {
                continue;
            };
            if !seen.insert((host.clone(), cwd.to_string())) {
                continue;
            }
            let trimmed = cwd.trim_end_matches('/');
            let leaf = trimmed.rsplit('/').next().filter(|l| !l.is_empty()).unwrap_or(cwd);
            found.push(json!({"name": format!("{host}/{leaf}"), "path": cwd,
                              "source": "remote", "host": host}));
        }
        found
    }

    /// Control.activity: one session's tool calls, from its conversation.
    /// (Python prefers the tool hooks' record when it has one; the shadow
    /// gets no hooks, so it always reads the conversation.)
    pub(super) fn api_activity(&self, session: &str) -> Value {
        let hit = |rows: &[Row]| {
            rows.iter()
                .find(|r| {
                    r.get("sessionId").and_then(Value::as_str) == Some(session)
                        || r.get("key").and_then(Value::as_str) == Some(session)
                })
                .cloned()
        };
        let row = hit(&self.sessions(15.0)).or_else(|| hit(&self.sessions(2.0)));
        let sid = row
            .as_ref()
            .and_then(|r| r.get("sessionId").and_then(Value::as_str))
            .unwrap_or(session)
            .to_string();
        let key = row
            .as_ref()
            .and_then(|r| r.get("key").and_then(Value::as_str))
            .unwrap_or(session)
            .to_string();
        let events = self.live.events(&key);
        let (calls, source) = if !events.is_empty() {
            (events, "hooks")
        } else if row.as_ref().is_some_and(|r| truthy(r.get("remote"))) {
            return json!({"ok": true, "source": "none", "timeline": [],
                          "tally": activity::tally(&[]),
                          "note": "remote sessions report activity only through tool hooks"});
        } else {
            let items = match self.conversation(&sid) {
                Ok(c) => {
                    let start = c.raw.len().saturating_sub(200);
                    conversation::merge_turns(c.raw[start..].to_vec())
                }
                Err(e) => return json!({"ok": false, "error": format!("RuntimeError: {e}")}),
            };
            (activity::timeline(&items), "conversation")
        };
        let start = calls.len().saturating_sub(200);
        json!({"ok": true, "source": source, "timeline": calls[start..].to_vec(),
               "tally": activity::tally(&calls), "current": self.live.current(&key, Self::now())})
    }

    /// Control.session_status: what the transcript and process tree add to
    /// the row - background work, prompt cache, tokens, spend.
    pub(super) fn api_session_status(&self, session: &str) -> Value {
        let hit = |rows: &[Row]| {
            rows.iter()
                .find(|r| {
                    r.get("sessionId").and_then(Value::as_str) == Some(session)
                        || r.get("key").and_then(Value::as_str) == Some(session)
                })
                .cloned()
        };
        let Some(row) = hit(&self.sessions(15.0)).or_else(|| hit(&self.sessions(2.0))) else {
            return json!({"ok": false, "error": "no such session"});
        };
        if truthy(row.get("remote")) {
            return json!({"ok": true, "now": Self::now(),
                          "note": "remote sessions report only what their tile shows"});
        }
        let now = Self::now();
        let sid = row
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut stat = self.stats.read(&sid, now);
        if let Some(Value::Object(st)) = stat.as_mut() {
            st.shift_remove("known_tasks");
        }
        let (per, _) = self
            .cost_history
            .lk()
            .sessions(std::slice::from_ref(&sid), 1.0);
        let spend = per.get(&sid).cloned().unwrap_or(Value::Null);
        let pid = row
            .get("pid")
            .filter(|p| p.is_u64() || p.is_i64())
            .and_then(Value::as_u64);
        let alive = pid.is_some() && truthy(row.get("status"));
        if let (Some(Value::Object(st)), false) = (stat.as_mut(), alive) {
            if !st.is_empty() {
                for name in ["agents", "shells", "monitors", "wakeups"] {
                    st.insert(name.into(), json!([]));
                }
            }
        }
        let mut out = Map::new();
        out.insert("ok".into(), json!(true));
        out.insert("now".into(), json!(now));
        out.insert("stat".into(), stat.unwrap_or_else(|| json!({})));
        out.insert("spend".into(), spend);
        out.insert("alive".into(), json!(alive));
        if let (true, Some(pid)) = (alive, pid) {
            let n = procs::waiting_shells(&[pid as u32], &procs::process_table(), now)
                .get(&(pid as u32))
                .copied()
                .unwrap_or(0);
            out.insert("shell_processes".into(), json!(n));
        }
        Value::Object(out)
    }

    /// Control.awaited: the end of what each shell and monitor the session
    /// is waiting on has printed, and what each subagent has said and done.
    /// Empty once the session's process has gone.
    pub(super) fn api_awaited(&self, session: &str) -> Value {
        let hit = |rows: &[Row]| {
            rows.iter()
                .find(|r| {
                    r.get("sessionId").and_then(Value::as_str) == Some(session)
                        || r.get("key").and_then(Value::as_str) == Some(session)
                })
                .cloned()
        };
        let Some(row) = hit(&self.sessions(15.0)).or_else(|| hit(&self.sessions(2.0))) else {
            return json!({"ok": false, "error": "no such session"});
        };
        let now = Self::now();
        let alive =
            row.get("pid").is_some_and(|p| p.is_u64() || p.is_i64()) && truthy(row.get("status"));
        if truthy(row.get("remote")) && alive {
            // Read on the session's own machine.
            let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
            let sid = row.get("sessionId").map(crate::pyish::py_str).unwrap_or_default();
            return match self.fleet.as_ref().map(|f| f.awaited(&host, &sid)) {
                Some(Ok(tasks)) => json!({"ok": true, "now": now, "tasks": tasks}),
                Some(Err(e)) => json!({"ok": false, "error": format!("reading on {host}: {e}")}),
                None => json!({"ok": true, "now": now, "tasks": []}),
            };
        }
        if truthy(row.get("remote")) || !alive {
            return json!({"ok": true, "now": now, "tasks": []});
        }
        let sid = row
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let stat = self.stats.read(&sid, now).unwrap_or_else(|| json!({}));
        let folder = crate::taskout::tasks_dir(&sid);
        let ran_agents = stat
            .get("finished")
            .and_then(Value::as_array)
            .is_some_and(|f| f.iter().any(|x| x.get("kind").and_then(Value::as_str) == Some("agents")));
        let agents = if truthy(stat.get("agents")) || ran_agents {
            crate::taskout::subagents_dir(sessionstat::transcript_path(&sid).as_deref())
        } else {
            None
        };
        let tasks = crate::taskout::awaited(&stat, folder.as_deref(), agents.as_deref());
        json!({"ok": true, "now": now, "tasks": tasks})
    }

    /// The processes beneath a local session's own, with their CPU and memory.
    pub(super) fn api_processes(&self, session: &str) -> Value {
        let Some(row) = self.row(session) else {
            return json!({"ok": false, "error": "no such session"});
        };
        let Some(pid) = row.get("pid").and_then(Value::as_u64).filter(|_| !truthy(row.get("remote"))) else {
            return json!({"ok": true, "processes": []});
        };
        json!({"ok": true, "pid": pid, "processes": crate::procs::details(pid as u32)})
    }

    /// Control.changes: git status and diff in the session's folder.
    pub(super) fn api_changes(&self, session: &str) -> Value {
        let Some(row) = self.row(session) else {
            return json!({"ok": false, "error": "no such session"});
        };
        if truthy(row.get("remote")) {
            let name = row.get("name").filter(|n| truthy(Some(n))).map(crate::pyish::py_str)
                .unwrap_or_else(|| session.to_string());
            let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
            return json!({"ok": false, "error":
                format!("{name} is on {host} - changes are only read on this machine so far")});
        }
        let cwd = row.get("cwd").and_then(Value::as_str).unwrap_or("");
        changes::changes(cwd)
    }

    /// Control._known_project: a path the catalogue or a live session
    /// vouches for, never one taken on trust from the request.
    pub(super) fn known_project(&self, path: &str) -> bool {
        if path.is_empty() {
            return false;
        }
        let Some(want) = paths::abs_normcase(path) else {
            return false;
        };
        let mut known: Vec<String> = self
            .catalogue()
            .iter()
            .filter(|p| !truthy(p.get("host")))
            .filter_map(|p| p.get("path").and_then(Value::as_str).map(str::to_string))
            .collect();
        known.extend(
            self.sessions(2.0)
                .iter()
                .filter(|r| !truthy(r.get("remote")))
                .filter_map(|r| r.get("cwd").and_then(Value::as_str).map(str::to_string)),
        );
        known
            .iter()
            .any(|k| !k.is_empty() && paths::abs_normcase(k).as_deref() == Some(want.as_str()))
    }
}
