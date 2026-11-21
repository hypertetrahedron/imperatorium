//! Sending to sessions: resume, new, close, focus, type, the queue of prompts and the dictation that feeds it.

use super::*;

impl Shared {
    /// Control.resume: resume or fork a local session, in a new tab or in
    /// the background.
    pub(super) fn resume(
        &self,
        store: &crate::store::Store,
        session_id: &str,
        where_: &str,
        fork: bool,
        prompt: Option<&str>,
        cwd: Option<String>,
    ) -> Value {
        let row = self.row(session_id);
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("remote"))) {
            return json!({"ok": false, "error": format!("resuming a session on {} is not supported yet",
                crate::pyish::py_str(r.get("host").unwrap_or(&Value::Null)))});
        }
        let text = |r: &Row, k: &str| {
            r.get(k)
                .filter(|v| truthy(Some(v)))
                .map(crate::pyish::py_str)
        };
        let mut cwd = cwd.or_else(|| row.as_ref().and_then(|r| text(r, "cwd")));
        if cwd.is_none() {
            cwd = self
                .history
                .list()
                .iter()
                .find(|h| h.get("session_id").and_then(Value::as_str) == Some(session_id))
                .and_then(|h| {
                    h.get("cwd")
                        .filter(|v| truthy(Some(v)))
                        .map(crate::pyish::py_str)
                });
        }
        if let Some(r) = &row {
            if (truthy(r.get("pid")) || truthy(r.get("status"))) && !fork && where_ == "window" {
                return json!({"ok": false, "error": "that session is still running - focus it, or fork it to branch off"});
            }
        }
        if where_ == "window" && self.hc_hosting() {
            return json!({"ok": false, "error": "herdr-core hosting is not ported yet"});
        }
        let options = self
            .config()
            .get("background")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        // A fork runs beside the session it came from, so it gets a worktree
        // of its own, made from the branch that session has checked out -
        // else both edit the same files. A resume by id finds the session
        // from any directory (checked 2026-11-21 against 2.1.330).
        let made = match cwd.as_deref() {
            Some(c) if fork && std::path::Path::new(c).is_dir() => {
                let name = format!("fork-{}", session_id.chars().take(8).collect::<String>());
                match crate::changes::make_worktree(c, &name) {
                    Ok(m) => m,
                    Err(e) => return json!({"ok": false, "error": format!("could not make its worktree: {e}")}),
                }
            }
            _ => None,
        };
        let run_in = made
            .as_ref()
            .and_then(|m| m["path"].as_str().map(str::to_string))
            .or_else(|| cwd.clone());
        let mut got =
            match crate::launch::resume(session_id, run_in.as_deref(), where_, fork, prompt, &options)
            {
                Ok(g) => g,
                Err(e) => {
                    if let Some(m) = &made {
                        crate::changes::drop_worktree(m);
                    }
                    return json!({"ok": false, "error": e});
                }
            };
        let mut e = json!({"kind": if fork { "fork" } else { "resume" }, "session_id": session_id,
                           "cwd": cwd, "summary": got["command"]});
        if let Some(m) = &made {
            let base = match m["base"].as_str() {
                Some(b) => b.to_string(),
                None => m["start"].as_str().unwrap_or("").chars().take(12).collect(),
            };
            for (k, v) in [("worktree", m["path"].clone()), ("branch", m["branch"].clone()),
                           ("base", json!(base))] {
                e[k] = v.clone();
                got[k] = v;
            }
        }
        store.add_event_json(e);
        self.invalidate();
        got
    }

    /// Whether window sessions start in a herdr-core terminal (terminal.host).
    fn hc_hosting(&self) -> bool {
        settings::value(&self.config(), "terminal.host").as_str() == Some("hc")
    }

    /// Control.new_session: a fresh session in the same project, in a new
    /// tab; with `close`, the old one ends once the new one is launched.
    pub(super) fn new_session(
        &self,
        store: &crate::store::Store,
        session_id: Option<&str>,
        cwd: Option<String>,
        close: bool,
        permission_mode: Option<&str>,
    ) -> Value {
        let row = session_id.and_then(|s| self.row(s));
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("remote"))) {
            return json!({"ok": false, "error": format!("starting a session on {} is not supported yet",
                crate::pyish::py_str(r.get("host").unwrap_or(&Value::Null)))});
        }
        if close && !row.as_ref().is_some_and(|r| truthy(r.get("pid"))) {
            return json!({"ok": false, "error": NOT_RUNNING});
        }
        let cwd = cwd.or_else(|| {
            row.as_ref().and_then(|r| {
                r.get("cwd")
                    .filter(|v| truthy(Some(v)))
                    .map(crate::pyish::py_str)
            })
        });
        if self.hc_hosting() {
            return json!({"ok": false, "error": "herdr-core hosting is not ported yet"});
        }
        let sid = uuid4();
        let mut got = match crate::launch::new_session(cwd.as_deref(), &sid, permission_mode) {
            Ok(g) => g,
            Err(e) => return json!({"ok": false, "error": e}),
        };
        let e = json!({"kind": "new", "cwd": cwd, "summary": got["command"], "session_id": got["session_id"]});
        store.add_event_json(e);
        if close {
            got["closed"] = self.close_row(store, row.as_ref().unwrap());
        }
        self.invalidate();
        got
    }

    /// Control.close_session: end a local session and, with it, its tab.
    pub(super) fn close_session(&self, store: &crate::store::Store, session_id: Option<&str>) -> Value {
        let row = session_id.and_then(|s| self.row(s));
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("remote"))) {
            return json!({"ok": false, "error": format!("closing a session on {} is not supported yet",
                crate::pyish::py_str(r.get("host").unwrap_or(&Value::Null)))});
        }
        let Some(row) = row.filter(|r| truthy(r.get("pid"))) else {
            return json!({"ok": false, "error": NOT_RUNNING});
        };
        let got = self.close_row(store, &row);
        self.invalidate();
        got
    }

    /// Control._close_row. (A herdr-core terminal is closed through hc,
    /// which comes with herdr-core hosting.)
    fn close_row(&self, store: &crate::store::Store, row: &Row) -> Value {
        let hosted = row
            .get("hc")
            .and_then(|h| h.get("address"))
            .is_some_and(|a| truthy(Some(a)));
        let got = if hosted {
            json!({"ok": false, "error": "closing a herdr-core terminal is not ported yet"})
        } else {
            crate::closing::close(row.get("pid").unwrap_or(&Value::Null))
        };
        let summary = got
            .get("how")
            .filter(|v| truthy(Some(v)))
            .or(got.get("error"))
            .cloned()
            .unwrap_or(Value::Null);
        let e = json!({"kind": "close", "cwd": row.get("cwd").cloned().unwrap_or(Value::Null),
                       "session_id": row.get("sessionId").cloned().unwrap_or(Value::Null), "summary": summary});
        store.add_event_json(e);
        got
    }

    /// POST /permission/park, from this machine's hook or (with `_host` set
    /// by the listener) a remote host's: the hook blocks here until a human
    /// answers, passes it to the terminal, or nobody waits for it any more.
    /// No verdict is a silent "ask": the terminal prompts.
    pub(super) fn park_request(&self, p: &Map<String, Value>) -> Value {
        let wait = self.park_wait(p);
        let (verdict, who, answers) = self.pending.park_while(p, wait, |item| self.park_abandoned(item));
        let mut reply = json!({"verdict": verdict, "decided_by": who});
        if truthy(Some(&answers)) {
            reply["answers"] = answers;
        }
        reply
    }

    /// Whether nobody waits for parked `item` any more: this machine's hook
    /// process has ended (Claude Code ends it when the person interrupts),
    /// or the session ended - or, for a remote one, whose hook cannot be
    /// looked at from here, its turn stopped.
    pub(super) fn park_abandoned(&self, item: &Map<String, Value>) -> bool {
        let host = item.get("host").and_then(Value::as_str);
        if host.is_none() {
            if let Some(pid) = item.get("hook_pid").and_then(Value::as_u64) {
                if !crate::proc::pid_alive(pid as u32) {
                    return true;
                }
            }
        }
        let Some(sid) = item.get("session_id").and_then(Value::as_str) else {
            return false;
        };
        let key = match host {
            Some(h) => super::snapshot::remote_key(h, sid),
            None => sid.to_string(),
        };
        let at = item.get("at").and_then(Value::as_f64).unwrap_or(0.0);
        self.row(&key).is_some_and(|r| {
            let after = r.get("last_event_at").and_then(Value::as_f64).is_some_and(|t| t > at);
            let ended = match r.get("last_event").and_then(Value::as_str) {
                Some("SessionEnd") => true,
                Some("Stop" | "StopFailure") => host.is_some(),
                _ => false,
            };
            after && ended
        })
    }

    /// Control.park_wait: how long a parked request is held. A background
    /// session has no terminal, so it gets `wait_background`; a question
    /// takes longer to answer than an Allow.
    pub(super) fn park_wait(&self, p: &Map<String, Value>) -> f64 {
        let num = |k: &str| {
            p.get(k)
                .filter(|v| truthy(Some(v)))
                .and_then(crate::pyish::py_float)
                .unwrap_or(0.0)
        };
        let mut wait = num("wait");
        if p.get("tool_name").and_then(Value::as_str) == Some(crate::pending::QUESTION_TOOL)
            && p.get("wait_question").is_some_and(|v| !v.is_null())
        {
            wait = wait.max(
                p.get("wait_question")
                    .and_then(crate::pyish::py_float)
                    .unwrap_or(0.0),
            );
        }
        let Some(bg) = p.get("wait_background").filter(|v| !v.is_null()) else {
            return wait;
        };
        let sid = p.get("session_id").and_then(Value::as_str);
        let background = self
            .sessions(2.0)
            .iter()
            .find(|r| r.get("sessionId").and_then(Value::as_str) == sid && sid.is_some())
            .is_some_and(|r| r.get("kind").and_then(Value::as_str) == Some("background"));
        if background {
            return crate::pyish::py_float(bg).unwrap_or(0.0);
        }
        wait
    }

    /// Control.enqueue: hold `prompt` until the session's turn ends; one
    /// that is idle and reachable gets it at once.
    pub(super) fn enqueue(
        &self,
        store: &crate::store::Store,
        session_id: &str,
        prompt: &str,
        as_user: bool,
    ) -> Value {
        let Some(row) = self.row(session_id) else {
            return json!({"ok": false, "error": "no such session"});
        };
        if crate::slash::is_command(prompt) {
            // Refuse now what could never be typed, rather than leave it at
            // the head of the queue failing on every sweep.
            if let Some(why) =
                crate::slash::refusal(prompt).or_else(|| crate::slash::unreachable(&row))
            {
                return json!({"ok": false, "error": why});
            }
        }
        let key = row
            .get("key")
            .and_then(Value::as_str)
            .filter(|k| !k.is_empty())
            .unwrap_or(session_id)
            .to_string();
        let item = store.enqueue(&key, prompt, as_user);
        self.invalidate();
        if row.get("status").and_then(Value::as_str) == Some("idle")
            && !truthy(row.get("attention"))
            && truthy(row.get("dispatchable"))
        {
            self.drain(store, &key);
        }
        json!({"ok": true, "queued": item, "session": session_id,
               "waiting": store.queued(&key).len()})
    }

    /// Control.drain: deliver the oldest queued prompt for `key`; its id.
    pub(super) fn drain(&self, store: &crate::store::Store, key: &str) -> Option<Value> {
        if store.queued(key).is_empty() {
            return None;
        }
        self.invalidate();
        let row = self
            .sessions(2.0)
            .iter()
            .find(|r| r.get("key").and_then(Value::as_str) == Some(key))
            .cloned()?;
        if !truthy(row.get("dispatchable")) {
            return None; // the sweep retries once it registers
        }
        let item = store.take_queued(key)?;
        let prompt = item.get("prompt").and_then(Value::as_str).unwrap_or("");
        let error = if crate::slash::is_command(prompt) {
            let got = self.run_command(store, &row, prompt);
            if got["ok"] == json!(true) {
                None
            } else {
                Some(crate::pyish::py_str(&got["error"]))
            }
        } else {
            self.deliver(store, &row, prompt, truthy(item.get("as_user")))
        };
        if let Some(e) = error {
            store.requeue_front(key, &item, Some(&e));
            return None;
        }
        self.invalidate();
        item.get("id").cloned()
    }

    /// Control.sweep_queue: deliver to queued sessions that are idle and not
    /// waiting on you - a prompt queued before the session had a token, or a
    /// missed Stop.
    pub(super) fn sweep_queue(&self) -> Vec<Value> {
        let Some(store) = &self.store else {
            return vec![];
        };
        let keys = store.queued_keys();
        if keys.is_empty() {
            return vec![];
        }
        self.invalidate();
        let rows: HashMap<String, Row> = self
            .sessions(2.0)
            .iter()
            .filter_map(|r| Some((r.get("key")?.as_str()?.to_string(), r.clone())))
            .collect();
        let mut out = vec![];
        for key in keys {
            let Some(row) = rows.get(&key) else { continue };
            if row.get("status").and_then(Value::as_str) == Some("idle")
                && !truthy(row.get("attention"))
                && truthy(row.get("dispatchable"))
            {
                if let Some(id) = self.drain(store, &key) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// Control._deliver: put the prompt in the session's inbox. None, or the
    /// error. (The mod's path, for sessions whose mod is live, comes with the
    /// mod link; remote hosts with phase 4.)
    fn deliver(
        &self,
        store: &crate::store::Store,
        row: &Row,
        prompt: &str,
        as_user: bool,
    ) -> Option<String> {
        let sid = row.get("sessionId").and_then(Value::as_str).unwrap_or("");
        if let Some(link) = self
            .modlink
            .as_ref()
            .filter(|l| !truthy(row.get("remote")) && l.live(sid, Self::now()))
        {
            // The mod submits it once the session is idle: as the person's
            // own words only when they sent it from the window.
            let text = if as_user {
                prompt.to_string()
            } else {
                crate::conversation::with_origin(prompt)
            };
            let id = link.offer(sid, &text, as_user, Self::now());
            let e = json!({"kind": "dispatch_mod", "session_id": sid,
                           "cwd": row.get("cwd").cloned().unwrap_or(Value::Null), "as_user": as_user,
                           "mod_prompt": id, "summary": crate::pyish::take(prompt, 280)});
            store.add_event_json(e);
            self.invalidate();
            return None;
        }
        let text = crate::conversation::with_origin(prompt);
        let socket = row.get("socket").and_then(Value::as_str).unwrap_or("");
        let token = row.get("token").and_then(Value::as_str);
        let host = truthy(row.get("remote"))
            .then(|| row.get("host").map(crate::pyish::py_str).unwrap_or_default());
        let sent = match &host {
            Some(h) => match self.fleet.as_ref().filter(|f| f.host(h).is_some()) {
                None => return Some(format!("host {h} is not configured")),
                Some(f) => f.inject(h, socket, token, &text),
            },
            None => crate::inbox::send(socket, token, &text),
        };
        if let Err(e) = sent {
            return Some(e);
        }
        let sid = row.get("sessionId").cloned().unwrap_or(Value::Null);
        let summary = crate::pyish::take(&crate::conversation::strip_origin(&text), 280);
        let mut e = json!({"kind": "dispatch_inject", "session_id": sid,
                       "cwd": row.get("cwd").cloned().unwrap_or(Value::Null), "summary": summary});
        if let Some(h) = host.filter(|h| !h.is_empty()) {
            e["host"] = json!(h);
        }
        store.add_event_json(e);
        None
    }

    /// Control.dispatch: route an utterance. mode is auto | inject | launch |
    /// background | dry-run. `session_id` or `path` skip the name matching:
    /// the window has already chosen. Only from the window does a prompt
    /// speak as the person (as_user).
    pub(super) fn dispatch(
        &self,
        store: &crate::store::Store,
        p: &Map<String, Value>,
        as_user: bool,
    ) -> Value {
        let text = |k: &str| {
            p.get(k)
                .filter(|v| truthy(Some(v)))
                .map(crate::pyish::py_str)
        };
        let mode = match p.get("mode") {
            None => "auto".to_string(),
            Some(v) => crate::pyish::py_str(v),
        };
        let session_id = text("session_id");
        let mut prompt = text("prompt");
        let mut project = text("project");
        if let (Some(sid), Some(pr)) = (&session_id, &prompt) {
            if mode == "auto" || mode == "inject" {
                let Some(row) = self
                    .sessions(2.0)
                    .iter()
                    .find(|r| r.get("sessionId").and_then(Value::as_str) == Some(sid))
                    .cloned()
                else {
                    return json!({"ok": false, "error": "no such session"});
                };
                if crate::slash::is_command(pr) {
                    return self.run_command(store, &row, pr);
                }
                if !truthy(row.get("dispatchable")) {
                    // Claude Code relaunching a session (its daemon does, on an
                    // update) gives it a new inbox without a hook to say so.
                    if truthy(row.get("stale_credential")) {
                        return json!({"ok": false, "error":
                            "that session is not reachable (Claude Code restarted it, so its credential is out of date - it registers again on its next hook event)"});
                    }
                    return json!({"ok": false, "error":
                        "that session is not reachable (no credential yet - it registers on its next hook event)"});
                }
                if let Some(e) = self.deliver(store, &row, pr, as_user) {
                    return json!({"ok": false, "error": e});
                }
                return json!({"ok": true, "action": "inject", "session": sid,
                              "session_name": row.get("name").cloned().unwrap_or(Value::Null),
                              "host": row.get("host").cloned().unwrap_or(Value::Null)});
            }
        }
        if let (Some(t), None) = (text("text"), &prompt) {
            let (spoken, said) = crate::projects::parse_utterance(&t);
            prompt = Some(said).filter(|s| !s.is_empty());
            project = project.or(spoken);
        }
        let Some(prompt) = prompt else {
            return json!({"ok": false, "error": "no prompt"});
        };
        let res = if let Some(path) = text("path") {
            match self.exact_project(&path, text("host").as_deref()) {
                Some(r) => r,
                None => return json!({"ok": false, "error": format!("no project at {path}")}),
            }
        } else if let Some(name) = &project {
            crate::projects::resolve(name, &self.catalogue())
        } else {
            return json!({"ok": false, "error": "no project named"});
        };
        let heard = project
            .clone()
            .map(Value::from)
            .unwrap_or_else(|| res["match"]["name"].clone());
        let mut out = json!({"heard_project": heard, "prompt": prompt, "confidence": res["confidence"],
                             "score": res["score"], "candidates": res["candidates"]});
        let shown = crate::pyish::py_repr(&project.clone().map(Value::from).unwrap_or(Value::Null));
        let set = |out: &mut Value, more: Value| {
            if let Value::Object(more) = more {
                for (k, v) in more {
                    out[&k] = v;
                }
            }
        };
        if res["confidence"] == "none" {
            set(
                &mut out,
                json!({"ok": false, "error": format!("no project matched {shown}")}),
            );
            return out;
        }
        // A merely probable match is never acted on silently: a misheard
        // name costs a keystroke, not a prompt at the wrong repository.
        if res["confidence"] != "certain" && !truthy(p.get("confirm")) {
            set(
                &mut out,
                json!({"ok": false, "needs_confirmation": true,
                                 "error": format!("ambiguous project {shown}")}),
            );
            return out;
        }
        let m = res["match"].clone();
        out["project"] = m.clone();
        if mode == "dry-run" {
            set(&mut out, json!({"ok": true, "action": "dry-run"}));
            return out;
        }
        let path = m["path"].as_str().unwrap_or("").to_string();
        let remote = truthy(m.get("host"));
        if crate::slash::is_command(&prompt) {
            // Never a launch: a new session started on "/compact" would be a
            // stranger doing nothing useful.
            let target = if remote {
                None
            } else {
                self.session_for(&path)
            };
            let Some(target) = target else {
                set(
                    &mut out,
                    json!({"ok": false, "error":
                    "no running session on this machine for that project to run the command in"}),
                );
                return out;
            };
            let got = self.run_command(store, &target, &prompt);
            set(&mut out, got);
            return out;
        }
        let background_options = || -> Map<String, Value> {
            self.config()
                .get("background")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|(k, _)| {
                    ["model", "permission_mode", "agent", "name", "extra"].contains(&k.as_str())
                })
                .collect()
        };
        if remote {
            let name = crate::pyish::py_str(&m["host"]);
            let Some(host) = self
                .fleet
                .as_ref()
                .and_then(|f| f.host(&name).cloned())
            else {
                set(
                    &mut out,
                    json!({"ok": false, "error": format!("host {name} is not configured")}),
                );
                return out;
            };
            // A live session there whose hooks have reported in is reached
            // as a local one is. Otherwise - and always for launch or
            // background, with no window here to open - the work becomes a
            // background session over there.
            if mode == "auto" || mode == "inject" {
                let target = self.session_for_on(&path, Some(&name));
                let error = match &target {
                    Some(t) => self.deliver(store, t, &prompt, as_user),
                    None => Some("no dispatchable session for that project".to_string()),
                };
                match (error, target) {
                    (None, Some(t)) => {
                        set(
                            &mut out,
                            json!({"ok": true, "action": "inject", "host": name,
                                   "session": t.get("sessionId").cloned().unwrap_or(Value::Null),
                                   "session_name": t.get("name").cloned().unwrap_or(Value::Null)}),
                        );
                        return out;
                    }
                    (Some(e), _) if mode == "inject" => {
                        set(&mut out, json!({"ok": false, "error": e}));
                        return out;
                    }
                    _ => {}
                }
            }
            match crate::remote::start_background(&host, &path, &prompt, &background_options()) {
                Err(e) => set(&mut out, json!({"ok": false, "error": e})),
                Ok(started) => {
                    let e = json!({"kind": "dispatch_remote", "cwd": path,
                                   "summary": crate::pyish::take(&prompt, 280),
                                   "host": name, "bg_id": started["id"]});
                    store.add_event_json(e);
                    set(
                        &mut out,
                        json!({"ok": true, "action": "remote", "host": name, "bg_id": started["id"]}),
                    );
                }
            }
            return out;
        }
        if mode == "background" {
            // In a worktree of its own, made here from the branch the chosen
            // session has checked out (else the project's): left to Claude
            // Code it would branch from the remote's default branch, which on
            // a long-lived feature branch is the wrong code.
            let options = background_options();
            let mut source = path.clone();
            if let Some(row) = session_id.as_deref().and_then(|sid| self.row(sid)) {
                let cwd = row.get("cwd").and_then(Value::as_str).unwrap_or("");
                if !truthy(row.get("remote")) && !cwd.is_empty() && std::path::Path::new(cwd).is_dir() {
                    source = cwd.to_string();
                }
            }
            let name = crate::background::short_name(&prompt)
                .or_else(|| options.get("name").filter(|v| truthy(Some(v))).map(crate::pyish::py_str))
                .unwrap_or_else(|| "bg".into());
            let made = match crate::changes::make_worktree(&source, &name) {
                Ok(m) => m,
                Err(e) => {
                    set(&mut out, json!({"ok": false, "error": format!("could not make its worktree: {e}")}));
                    return out;
                }
            };
            let run_in = made
                .as_ref()
                .and_then(|m| m["path"].as_str())
                .unwrap_or(&path)
                .to_string();
            match crate::background::start(&prompt, &run_in, &options) {
                Err(e) => {
                    if let Some(m) = &made {
                        crate::changes::drop_worktree(m);
                    }
                    set(&mut out, json!({"ok": false, "error": e}));
                }
                Ok(started) => {
                    let mut e = json!({"kind": "dispatch_background", "cwd": path,
                                   "summary": crate::pyish::take(&prompt, 280), "bg_id": started["id"]});
                    let mut done = json!({"ok": true, "action": "background", "bg_id": started["id"]});
                    if let Some(m) = &made {
                        let base = match m["base"].as_str() {
                            Some(b) => b.to_string(),
                            None => m["start"].as_str().unwrap_or("").chars().take(12).collect(),
                        };
                        let extra = [("worktree", m["path"].clone()), ("branch", m["branch"].clone()),
                                     ("base", json!(base))];
                        for (k, v) in extra {
                            e[k] = v.clone();
                            done[k] = v;
                        }
                    }
                    store.add_event_json(e);
                    self.invalidate();
                    set(&mut out, done);
                }
            }
            return out;
        }
        if mode == "auto" || mode == "inject" {
            match self.session_for(&path) {
                Some(target) => match self.deliver(store, &target, &prompt, as_user) {
                    None => {
                        set(
                            &mut out,
                            json!({"ok": true, "action": "inject",
                            "session": target.get("sessionId").cloned().unwrap_or(Value::Null),
                            "session_name": target.get("name").cloned().unwrap_or(Value::Null)}),
                        );
                        return out;
                    }
                    Some(e) if mode == "inject" => {
                        set(&mut out, json!({"ok": false, "error": e}));
                        return out;
                    }
                    Some(_) => {} // auto: a window instead
                },
                None if mode == "inject" => {
                    set(
                        &mut out,
                        json!({"ok": false, "error": "no dispatchable session for that project"}),
                    );
                    return out;
                }
                None => {}
            }
        }
        match crate::deeplink::launch(&path, &prompt) {
            Err(e) => set(&mut out, json!({"ok": false, "error": e})),
            Ok(url) => {
                let e = json!({"kind": "dispatch_launch", "cwd": path, "summary": crate::pyish::take(&prompt, 280)});
                store.add_event_json(e);
                set(
                    &mut out,
                    json!({"ok": true, "action": "launch", "url": url}),
                );
            }
        }
        out
    }

    /// Control._exact_project: the catalogue entry at `path`, as a certain
    /// match. (The catalogue holds this machine's projects only until remote
    /// hosts are ported.)
    fn exact_project(&self, path: &str, host: Option<&str>) -> Option<Value> {
        let host = host.filter(|h| !h.is_empty());
        let want = match host {
            // A remote path is the host's, compared as written.
            Some(_) => path.to_string(),
            None => paths::abs_normcase(path)?,
        };
        let proj = self.catalogue().into_iter().find(|p| {
            let theirs = p.get("host").filter(|h| truthy(Some(h))).map(crate::pyish::py_str);
            theirs.as_deref() == host
                && match host {
                    Some(_) => p["path"].as_str() == Some(want.as_str()),
                    None => {
                        p["path"].as_str().and_then(paths::abs_normcase).as_deref()
                            == Some(want.as_str())
                    }
                }
        })?;
        Some(json!({"match": proj, "score": 1.0, "confidence": "certain", "candidates": [proj]}))
    }

    /// Control._session_for: the best dispatchable session in `path` on this
    /// machine - an idle one if there is one, since a busy one would queue it
    /// behind its own work.
    fn session_for(&self, path: &str) -> Option<Row> {
        self.session_for_on(path, None)
    }

    /// The same on `host` (None: here). A remote path is the host's POSIX
    /// path, compared as written less a trailing slash: normcase/abspath
    /// here would make it a Windows path on the current drive.
    fn session_for_on(&self, path: &str, host: Option<&str>) -> Option<Row> {
        let want = match host {
            Some(_) => path.trim_end_matches('/').to_string(),
            None => paths::abs_normcase(path)?,
        };
        let same = |cwd: &str| match host {
            Some(_) => cwd.trim_end_matches('/') == want,
            None => paths::abs_normcase(cwd).as_deref() == Some(want.as_str()),
        };
        let mut best: Option<Row> = None;
        for row in self.sessions(2.0).iter() {
            let Some(cwd) = row
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())
            else {
                continue;
            };
            if !truthy(row.get("dispatchable")) {
                continue;
            }
            let row_host = row.get("host").filter(|h| truthy(Some(h))).map(crate::pyish::py_str);
            if row_host.as_deref() != host || truthy(row.get("remote")) != host.is_some() {
                continue;
            }
            if !same(cwd) {
                continue;
            }
            let idle = |r: &Row| r.get("status").and_then(Value::as_str) == Some("idle");
            if best.is_none() || (idle(row) && !idle(best.as_ref().unwrap())) {
                best = Some(row.clone());
            }
        }
        best
    }

    /// Control.transcribe: browser-recorded audio to text in the dictation
    /// child, where the model stays loaded.
    pub(super) fn transcribe(&self, audio: &[u8]) -> Value {
        let Some(v) = &self.voice else {
            return json!({"ok": false, "error": "dictation is not available"});
        };
        if let Some(why) = v.missing() {
            return json!({"ok": false, "error": why});
        }
        match v.transcribe_bytes(audio, None) {
            Err(e) => json!({"ok": false, "error": format!("VoiceError: {e}")}),
            Ok(text) => {
                let mut out = v.where_().and_then(|w| w.as_object().cloned()).unwrap_or_default();
                out.insert("ok".into(), json!(true));
                out.insert("text".into(), json!(text));
                Value::Object(out)
            }
        }
    }

    /// Control.warm_voice: the mic was pressed - build the model while the
    /// person speaks, and say where the last load landed.
    pub(super) fn warm_voice(&self) -> Value {
        let Some(v) = &self.voice else {
            return json!({"ok": false, "error": "dictation is not available"});
        };
        if let Some(why) = v.missing() {
            return json!({"ok": false, "error": why});
        }
        v.warm();
        let mut out = v.where_().and_then(|w| w.as_object().cloned()).unwrap_or_default();
        out.insert("ok".into(), json!(true));
        out.insert("running".into(), json!(v.running()));
        // And how long the window may record: it stops itself there.
        out.insert("max_seconds".into(), v.settings.get("max_seconds").cloned().unwrap_or(Value::Null));
        Value::Object(out)
    }

    /// Control.focus_row: raise the window this session is in. One
    /// WindowsTerminal.exe hosts every session, so the process tree finds
    /// the terminal and the session's title picks its window. Three routes,
    /// best first, and the answer says which was used.
    fn focus_row(&self, store: &crate::store::Store, row: &Row) -> Option<Value> {
        let sid = row.get("sessionId").and_then(Value::as_str).unwrap_or("").to_string();
        let pid_v = row.get("pid").cloned().unwrap_or(Value::Null);
        let pid = crate::pyish::py_number(row.get("pid")).map(|f| f as u32).unwrap_or(0);
        let info = crate::sessions::info_json(&sid);
        let want = [info.get("custom_title"), info.get("summary")]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .find(|t| !t.is_empty())
            .unwrap_or("")
            .to_string();
        let host = crate::winfocus::terminal_for(pid);
        let windows = host.map(|h| crate::winfocus::titled_windows(Some(h))).unwrap_or_default();
        let hwnd = if want.is_empty() { None } else { crate::winfocus::match_title(&want, &windows) };
        if let Some(hwnd) = hwnd.filter(|h| crate::winfocus::focus_hwnd(*h)) {
            let meta = json!({"window_hwnd": hwnd, "window_title": want});
            store.register(&sid, None, meta.as_object().unwrap());
            return Some(json!({"ok": true, "action": "focus", "session": sid, "pid": pid_v,
                               "how": "title", "title": want}));
        }
        // Its tab is not the one in front, so no title matches; a tab stays
        // in the window it opened in, so the window it was last seen in is
        // where it still is, just behind another tab.
        let remembered = store
            .session(&sid)
            .and_then(|r| r.get("window_hwnd").and_then(Value::as_i64))
            .filter(|h| *h != 0);
        if let Some(h) = remembered {
            if host.is_some() && crate::winfocus::owner_pid(h as isize) == host && crate::winfocus::focus_hwnd(h as isize) {
                return Some(json!({"ok": true, "action": "focus", "session": sid, "pid": pid_v,
                                   "how": "remembered", "title": want,
                                   "note": format!("its tab is not in front - look for {}",
                                                   crate::pyish::py_repr(&json!(want)))}));
            }
        }
        if crate::winfocus::focus_pid(pid) {
            return Some(json!({"ok": true, "action": "focus", "session": sid, "pid": pid_v,
                               "how": "terminal", "title": want,
                               "note": "raised its terminal; could not tell its window apart"}));
        }
        None
    }

    /// Control.focus: put a project's session in front, or open one when
    /// none is running, so a button press always does something.
    pub(super) fn focus(&self, store: &crate::store::Store, project: Option<&Value>, session_id: Option<&Value>) -> Value {
        let rows = self.sessions(2.0);
        let mut row: Option<Row> = None;
        let mut path: Option<String> = None;
        let gone = |name: &Value, host: &Value| {
            json!({"ok": false, "error": format!("{} is on {} - there is no window here to raise",
                crate::pyish::py_str(name), crate::pyish::py_str(host))})
        };
        if truthy(session_id) {
            let sid = session_id.unwrap();
            row = rows.iter().find(|r| r.get("sessionId") == Some(sid)).cloned();
            if let Some(r) = &row {
                if truthy(r.get("remote")) {
                    // Its pid is a process on another machine.
                    let name = crate::pyish::or(r.get("name"), Some(sid)).unwrap().clone();
                    return gone(&name, r.get("host").unwrap_or(&Value::Null));
                }
            }
            path = row.as_ref().and_then(|r| r.get("cwd").and_then(Value::as_str).map(str::to_string));
        }
        if row.is_none() {
            if !truthy(project) {
                return json!({"ok": false, "error": "no project or session named"});
            }
            let project = project.unwrap();
            let res = crate::projects::resolve(&crate::pyish::py_str(project), &self.catalogue());
            if res["confidence"] == "none" {
                return json!({"ok": false, "error": format!("no project matched {}", crate::pyish::py_repr(project))});
            }
            if truthy(res["match"].get("host")) {
                return gone(&res["match"]["name"], &res["match"]["host"]);
            }
            let p = res["match"]["path"].as_str().unwrap_or("").to_string();
            let want = paths::abs_normcase(&p);
            let mut live: Vec<Row> = rows
                .iter()
                .filter(|r| {
                    let cwd = r.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty());
                    cwd.is_some_and(|c| paths::abs_normcase(c) == want) && truthy(r.get("pid"))
                })
                .cloned()
                .collect();
            // Whatever is asking for a human wins; otherwise the busy one.
            live.sort_by_key(|r| {
                (!truthy(r.get("attention")), r.get("status").and_then(Value::as_str) != Some("busy"))
            });
            row = live.into_iter().next();
            path = Some(p);
        }
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("pid"))) {
            if let Some(got) = self.focus_row(store, r) {
                return got;
            }
            // Running, so opening a window would start a second session on
            // the same project - worse than saying it could not be raised.
            return json!({"ok": false, "error": "could not raise its window",
                          "session": r.get("sessionId").cloned().unwrap_or(Value::Null),
                          "pid": r.get("pid").cloned().unwrap_or(Value::Null)});
        }
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            return json!({"ok": false, "error": "nothing to focus and nowhere to open"});
        };
        match crate::deeplink::launch(&path, "") {
            Err(e) => json!({"ok": false, "error": e}),
            Ok(url) => json!({"ok": true, "action": "launch", "url": url}),
        }
    }

    /// Control.run_command: type slash command `text` at the row's prompt.
    fn run_command(&self, store: &crate::store::Store, row: &Row, text: &str) -> Value {
        if let Some(why) = crate::slash::refusal(text) {
            return json!({"ok": false, "error": why});
        }
        // Status moves under us: decide on a fresh look.
        self.invalidate();
        let row = self.fresh_row(row);
        if let Some(why) = crate::slash::blocked(&row, &self.pending.waiting()) {
            return json!({"ok": false, "error": why});
        }
        if !self.type_into(&row, text) {
            return json!({"ok": false, "error": "could not type into its terminal"});
        }
        let sid = row.get("sessionId").cloned().unwrap_or(Value::Null);
        let t = crate::pyish::strip(text);
        let e = json!({"kind": "command", "session_id": sid,
                       "cwd": row.get("cwd").cloned().unwrap_or(Value::Null),
                       "summary": crate::pyish::take(t, 280)});
        store.add_event_json(e);
        self.invalidate();
        json!({"ok": true, "action": "command", "command": t, "session": sid,
               "session_name": row.get("name").cloned().unwrap_or(Value::Null)})
    }

    /// The row again, from a fresh look; the old one if it has gone.
    fn fresh_row(&self, row: &Row) -> Row {
        let id = row
            .get("key")
            .and_then(Value::as_str)
            .filter(|k| !k.is_empty())
            .or_else(|| row.get("sessionId").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();
        self.row(&id).unwrap_or_else(|| row.clone())
    }

    /// Control._type_into: one line and Enter at the row's prompt. (A
    /// herdr-core terminal is typed into through hc, which comes with
    /// herdr-core hosting.)
    fn type_into(&self, row: &Row, text: &str) -> bool {
        let hosted = row
            .get("hc")
            .and_then(|h| h.get("address"))
            .is_some_and(|a| truthy(Some(a)));
        if hosted && !truthy(row.get("remote")) {
            return false;
        }
        crate::slash::type_command(row.get("pid").unwrap_or(&Value::Null), text)
    }

    /// Control.send_typed: Send As Typed - the text typed at the session's
    /// prompt as one line, with the authority of the person at the
    /// terminal. Off until Settings turns it on; only from the window.
    pub(super) fn send_typed(
        &self,
        store: &crate::store::Store,
        session_id: Option<&Value>,
        text: Option<&Value>,
    ) -> Value {
        if !truthy(Some(&settings::value(&self.config(), "chat.send_as_typed"))) {
            return json!({"ok": false, "error": "Send As Typed is off - turn it on in Settings, under Chat"});
        }
        let id = session_id
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str);
        let Some(row) = id.and_then(|i| self.row(&i)) else {
            return json!({"ok": false, "error": "no such session"});
        };
        let raw = text.and_then(Value::as_str).unwrap_or("");
        let (clean, changed) = crate::slash::clean_typed(raw);
        if let Some(why) = crate::slash::typed_refusal(&clean) {
            return json!({"ok": false, "error": why});
        }
        if crate::slash::is_command(&clean) {
            let mut got = self.run_command(store, &row, &clean);
            got["cleaned"] = json!(changed);
            return got;
        }
        let sid = row
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let name = row.get("name").cloned().unwrap_or(Value::Null);
        let mod_live = self
            .modlink
            .as_ref()
            .is_some_and(|l| l.live(&sid, Self::now()));
        if !truthy(row.get("remote")) && mod_live {
            self.deliver(store, &row, &clean, true);
            return json!({"ok": true, "action": "typed", "via": "mod", "cleaned": changed,
                          "session": sid, "session_name": name});
        }
        self.invalidate();
        let row = self.fresh_row(&row);
        if let Some(why) = crate::slash::blocked(&row, &self.pending.waiting()) {
            return json!({"ok": false, "error": why});
        }
        if !self.type_into(&row, &clean) {
            return json!({"ok": false, "error": "could not type into its terminal"});
        }
        let e = json!({"kind": "typed", "session_id": sid,
                       "cwd": row.get("cwd").cloned().unwrap_or(Value::Null),
                       "summary": crate::pyish::take(&clean, 280)});
        store.add_event_json(e);
        self.invalidate();
        json!({"ok": true, "action": "typed", "cleaned": changed, "session": sid, "session_name": name})
    }

    /// Stop: end the session's running turn, as Esc at its terminal does.
    /// Through its mod when it has one that can (a clean `$.turn.abort`);
    /// otherwise Esc is pressed in its console, never twice within a few
    /// seconds (Esc Esc there opens the rewind menu). Only while it works:
    /// at an idle prompt Esc has nothing to stop. Local sessions only.
    pub(super) fn stop_turn(&self, store: &crate::store::Store, session_id: Option<&Value>) -> Value {
        let id = session_id.filter(|v| truthy(Some(v))).map(crate::pyish::py_str);
        let Some(row) = id.and_then(|i| self.row(&i)) else {
            return json!({"ok": false, "error": "no such session"});
        };
        if truthy(row.get("remote")) {
            return json!({"ok": false, "error": format!("stopping a session on {} is not supported yet",
                crate::pyish::py_str(row.get("host").unwrap_or(&Value::Null)))});
        }
        let sid = row.get("sessionId").and_then(Value::as_str).unwrap_or("").to_string();
        let now = Self::now();
        let link = self.modlink.as_ref();
        let state = link.and_then(|l| l.state(&sid, now));
        let working = matches!(row.get("status").and_then(Value::as_str), Some("busy") | Some("shell"))
            || row.get("state").and_then(Value::as_str) == Some("working")
            || state.as_ref().is_some_and(|s| truthy(s.get("busy")));
        if !working {
            return json!({"ok": false, "error": "it is not working on anything"});
        }
        let event = |via: &str| json!({"kind": "stop", "session_id": sid,
            "cwd": row.get("cwd").cloned().unwrap_or(Value::Null), "summary": format!("asked to stop (via {via})")});
        if let (Some(l), Some(s)) = (link, state.as_ref()) {
            if crate::modlink::can_stop(s.get("mod_version")) {
                let stop = l.request_stop(&sid, now);
                store.add_event_json(event("mod"));
                return json!({"ok": true, "via": "mod", "stop_id": stop});
            }
        }
        let Some(pid) = row.get("pid").and_then(Value::as_u64) else {
            return json!({"ok": false, "error": "it has no terminal here to press Esc in"});
        };
        if row.get("hc").and_then(|h| h.get("address")).is_some_and(|a| truthy(Some(a))) {
            return json!({"ok": false, "error": "stopping a herdr-core session is not supported yet"});
        }
        match crate::closing::press_escape(pid as u32) {
            Ok(()) => {
                store.add_event_json(event("console"));
                self.invalidate();
                json!({"ok": true, "via": "console"})
            }
            Err(e) => json!({"ok": false, "error": e}),
        }
    }

    /// What came of the session's last stop through its mod, once it says.
    pub(super) fn stop_result(&self, session_id: &str) -> Value {
        match self.modlink.as_ref().and_then(|l| l.stop_result(session_id)) {
            Some(r) => json!({"ok": true, "result": r}),
            None => json!({"ok": true, "result": null}),
        }
    }

    /// Control.dismiss_session: take a session with no process off the list.
    pub(super) fn dismiss_session(&self, store: &crate::store::Store, session_id: Option<&Value>) -> Value {
        let id = session_id
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let Some(row) = (if id.is_empty() { None } else { self.row(&id) }) else {
            return json!({"ok": false, "error": "no such session"});
        };
        if truthy(row.get("remote")) {
            return json!({"ok": false, "error": format!("dismissing a session on {} is not supported yet",
                crate::pyish::py_str(row.get("host").unwrap_or(&Value::Null)))});
        }
        if truthy(row.get("pid")) {
            return json!({"ok": false, "error": "that session is still running - close it instead"});
        }
        let key = row
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let sid = row
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        store.dismiss(&key);
        store.forget(&sid);
        let e = json!({"kind": "dismiss", "cwd": row.get("cwd").cloned().unwrap_or(Value::Null),
                       "session_id": sid, "name": row.get("name").cloned().unwrap_or(Value::Null)});
        store.add_event_json(e);
        self.invalidate();
        json!({"ok": true, "how": "dismissed"})
    }
}

/// dispatcher.NOT_RUNNING.
const NOT_RUNNING: &str = "that session is not running here, so there is nothing to close";

/// str(uuid.uuid4()).
fn uuid4() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("random bytes");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}
