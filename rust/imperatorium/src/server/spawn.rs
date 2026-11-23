//! POST /api/spawn (2026-11-22): every way the window starts another
//! session - the Send button's four "new session" options, the New session
//! dialog, Resume and Resume in bg - goes through here, so the folder rules
//! in `crate::spawn` apply to all of them. With `plan` it only says what it
//! would do, for the window's confirmation dialog; nothing is started.

use super::*;
use super::dispatch::uuid4;
use crate::spawn::{self as rules, Facts, Kind};

/// A live session on this machine: running, not merely listed.
fn live(r: &Row) -> bool {
    !truthy(r.get("remote")) && !truthy(r.get("resumable")) && (truthy(r.get("pid")) || truthy(r.get("status")))
}

fn same(a: &str, b: &str) -> bool {
    match (paths::abs_normcase(a), paths::abs_normcase(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

impl Shared {
    /// The folder a session ran in: its row, else the history.
    fn cwd_of(&self, sid: &str) -> Option<String> {
        let from_row = self
            .row(sid)
            .and_then(|r| r.get("cwd").filter(|v| truthy(Some(v))).map(crate::pyish::py_str));
        from_row.or_else(|| {
            self.history
                .list()
                .iter()
                .find(|h| h.get("session_id").and_then(Value::as_str) == Some(sid))
                .and_then(|h| h.get("cwd").filter(|v| truthy(Some(v))).map(crate::pyish::py_str))
        })
    }

    /// The facts the rules need, and the folder they are about.
    fn spawn_facts(&self, kind: Kind, parent: Option<&str>, cwd: Option<String>) -> Result<Facts, String> {
        let row = parent.and_then(|s| self.row(s));
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("remote"))) {
            return Err(format!(
                "starting a session from one on {} is not supported yet",
                crate::pyish::py_str(r.get("host").unwrap_or(&Value::Null))
            ));
        }
        let start = match kind {
            Kind::New => cwd.or_else(|| parent.and_then(|p| self.cwd_of(p))),
            Kind::Fork | Kind::Resume => parent.and_then(|p| self.cwd_of(p)).or(cwd),
        };
        let Some(start) = start.filter(|c| std::path::Path::new(c).is_dir()) else {
            return Err("its folder is gone, or none was given".into());
        };
        // A fresh session belongs in the project's main checkout, even when
        // it is started from a session working in a worktree.
        let source = match kind {
            Kind::New => rules::main_checkout(&start).unwrap_or(start),
            _ => start,
        };
        let rows = self.sessions(2.0);
        let occupied = rows.iter().any(|r| {
            live(r)
                && !(kind == Kind::Resume && r.get("sessionId").and_then(Value::as_str) == parent)
                && r.get("cwd").and_then(Value::as_str).is_some_and(|c| same(c, &source))
        });
        let parent_live = kind == Kind::Fork && row.as_ref().is_some_and(live);
        if kind == Kind::Resume && row.as_ref().is_some_and(live) {
            return Err("that session is still running - focus it, or start a new one with its context".into());
        }
        let repo = crate::changes::is_repo(&source);
        let dirty = if repo { rules::dirty(&source) } else { vec![] };
        Ok(Facts { source, repo, occupied, parent_live, dirty })
    }

    /// POST /api/spawn.
    pub(super) fn spawn(&self, store: &crate::store::Store, p: &Map<String, Value>) -> Value {
        let text = |k: &str| p.get(k).filter(|v| truthy(Some(v))).map(crate::pyish::py_str);
        let Some(kind) = Kind::parse(text("kind").as_deref().unwrap_or("new")) else {
            return json!({"ok": false, "error": "kind must be new, fork or resume"});
        };
        let where_ = text("where").unwrap_or_else(|| "window".into());
        if where_ != "window" && where_ != "background" {
            return json!({"ok": false, "error": "where must be window or background"});
        }
        let parent = text("session_id");
        if kind != Kind::New && parent.is_none() {
            return json!({"ok": false, "error": "which session? (session_id)"});
        }
        let facts = match self.spawn_facts(kind, parent.as_deref(), text("cwd")) {
            Ok(f) => f,
            Err(e) => return json!({"ok": false, "error": e}),
        };
        let plan = rules::decide(kind, &facts);
        let prompt = text("prompt").unwrap_or_default();
        let parent_row = parent.as_deref().and_then(|s| self.row(s));
        // Mid-turn, a fork is cut at the end of the last finished turn.
        let cut_needed = kind == Kind::Fork
            && parent_row.as_ref().is_some_and(|r| r.get("status").and_then(Value::as_str) == Some("busy"));
        let mut shown = plan.to_json(&facts);
        shown["kind"] = json!(kind.name());
        shown["where"] = json!(where_);
        shown["mid_turn"] = json!(cut_needed);
        if truthy(p.get("plan")) {
            shown["ok"] = json!(true);
            return shown;
        }
        if where_ == "window" && self.hc_hosting() {
            return json!({"ok": false, "error": "herdr-core hosting is not ported yet"});
        }
        if where_ == "background" && kind != Kind::Resume && prompt.trim().is_empty() {
            return json!({"ok": false, "error": "type what the background session should do first"});
        }
        let cut = if cut_needed {
            let sid = parent.as_deref().unwrap_or("");
            let entries = crate::sessionstat::transcript_path(sid)
                .and_then(|t| std::fs::read_to_string(t).ok())
                .map(|t| crate::transcript::objects(&t, crate::transcript::Split::Newline))
                .unwrap_or_default();
            match rules::last_finished_turn(&entries) {
                Some(c) => Some(c),
                None => {
                    return json!({"ok": false, "error":
                        "it has not finished a turn yet, so there is no context to take - use New Session"})
                }
            }
        } else {
            None
        };
        if cut.is_some() && prompt.trim().is_empty() {
            return json!({"ok": false, "error":
                "it is mid-turn: type the first message - the fork runs it before it opens"});
        }
        // "worktree": false is the person choosing to share the folder.
        let use_tree = plan.placement == "worktree" && p.get("worktree") != Some(&json!(false));
        let made = if use_tree {
            let name = rules::worktree_name(kind, &prompt, parent.as_deref());
            match crate::changes::make_worktree(&plan.folder, &name) {
                Ok(Some(m)) => Some(m),
                Ok(None) => None,
                Err(e) => return json!({"ok": false, "error": format!("could not make its worktree: {e}")}),
            }
        } else {
            None
        };
        let copied = match (&made, kind, truthy(p.get("copy")) && !facts.dirty.is_empty()) {
            (Some(m), Kind::Fork, true) => match rules::copy_changes(&plan.folder, m["path"].as_str().unwrap_or("")) {
                Ok(n) => Some(n),
                Err(e) => {
                    crate::changes::drop_worktree(m);
                    return json!({"ok": false, "error": e});
                }
            },
            _ => None,
        };
        let run_in = made
            .as_ref()
            .and_then(|m| m["path"].as_str().map(str::to_string))
            .unwrap_or_else(|| plan.folder.clone());
        let placement = if made.is_some() { "worktree" } else { "here" };
        // Sharing a folder with a running session: by choice, or not git.
        let shared = made.is_none() && (facts.occupied || facts.parent_live);
        let note = rules::note(kind, placement, &plan.folder, made.as_ref(), copied, shared);
        let first = rules::with_note(&note, &prompt);
        let got = self.spawn_launch(store, kind, &where_, parent.as_deref(), &run_in, &prompt, &first, cut, p);
        let mut got = match got {
            Ok(g) => g,
            Err(e) => {
                if let Some(m) = &made {
                    crate::changes::drop_worktree(m);
                }
                return json!({"ok": false, "error": e});
            }
        };
        got["ok"] = json!(true);
        got["kind"] = json!(kind.name());
        got["where"] = json!(where_);
        got["placement"] = json!(placement);
        got["folder"] = json!(run_in);
        if let Some(n) = copied {
            got["copied"] = json!(n);
        }
        let mut e = json!({"kind": format!("spawn_{}", kind.name()), "cwd": plan.folder, "where": where_,
                           "session_id": got.get("session_id").cloned().unwrap_or(Value::Null),
                           "parent": parent, "summary": crate::pyish::take(&prompt, 280)});
        if let Some(m) = &made {
            let base = match m["base"].as_str() {
                Some(b) => b.to_string(),
                None => m["start"].as_str().unwrap_or("").chars().take(12).collect(),
            };
            for (k, v) in [("worktree", m["path"].clone()), ("branch", m["branch"].clone()), ("base", json!(base))] {
                e[k] = v.clone();
                got[k] = v;
            }
        }
        store.add_event_json(e);
        self.invalidate();
        got
    }

    /// Start it. `prompt` is what the person typed, `first` the same with
    /// the note in front (what the session is sent).
    #[allow(clippy::too_many_arguments)]
    fn spawn_launch(
        &self,
        store: &crate::store::Store,
        kind: Kind,
        where_: &str,
        parent: Option<&str>,
        run_in: &str,
        prompt: &str,
        first: &str,
        cut: Option<String>,
        p: &Map<String, Value>,
    ) -> Result<Value, String> {
        let text = |k: &str| p.get(k).filter(|v| truthy(Some(v))).map(crate::pyish::py_str);
        let mut options: Map<String, Value> = self
            .config()
            .get("background")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|(k, _)| ["model", "permission_mode", "agent", "extra"].contains(&k.as_str()))
            .collect();
        // The New session dialog's choices win over the background defaults.
        for (k, field) in [("permission_mode", "permission_mode"), ("model", "model")] {
            if let Some(v) = text(field).filter(|v| v != "default") {
                options.insert(k.into(), json!(v));
            }
        }
        let queue_first = |sid: &str| {
            if !first.trim().is_empty() {
                store.enqueue(sid, first, true);
            }
        };
        match (kind, where_) {
            (Kind::New, "window") => {
                let sid = uuid4();
                let got = crate::launch::new_session(
                    Some(run_in),
                    &sid,
                    text("permission_mode").as_deref(),
                    text("model").as_deref(),
                    text("effort").as_deref(),
                )?;
                queue_first(&sid);
                Ok(got)
            }
            (Kind::New, _) => {
                if let Some(n) = crate::background::short_name(prompt) {
                    options.insert("name".into(), json!(n));
                }
                let started = crate::background::start(first, run_in, &options)?;
                Ok(json!({"bg_id": started["id"]}))
            }
            (Kind::Fork, w) if cut.is_some() => {
                let sid = uuid4();
                self.print_fork(parent.unwrap_or(""), cut.as_deref().unwrap_or(""), &sid, run_in, first, &options,
                                w == "window")?;
                Ok(json!({"session_id": sid, "first_turn": "headless"}))
            }
            (Kind::Fork, "window") => {
                let sid = uuid4();
                let mut cmd = crate::launch::resume_command(parent.unwrap_or(""), true);
                cmd.extend(["--session-id".into(), sid.clone()]);
                let terminal = crate::launch::open_terminal(run_in, &cmd)?;
                queue_first(&sid);
                Ok(json!({"session_id": sid, "terminal": terminal, "command": cmd.join(" ")}))
            }
            (Kind::Fork, _) => {
                let got = crate::launch::resume(parent.unwrap_or(""), Some(run_in), "background", true, Some(first), &options)?;
                Ok(json!({"bg_id": got["id"], "command": got["command"]}))
            }
            (Kind::Resume, "window") => {
                let sid = parent.unwrap_or("");
                let got = crate::launch::resume(sid, Some(run_in), "window", false, None, &options)?;
                queue_first(sid);
                Ok(json!({"session_id": sid, "terminal": got["terminal"], "command": got["command"]}))
            }
            (Kind::Resume, _) => {
                let sid = parent.unwrap_or("");
                let got = crate::launch::resume(sid, Some(run_in), "background", false, Some(first), &options)?;
                Ok(json!({"session_id": sid, "bg_id": got["id"], "command": got["command"]}))
            }
        }
    }

    /// A fork of a session that is mid-turn: `claude -p` runs its first turn
    /// on the conversation cut at `cut` (only print mode honours
    /// --resume-session-at; docs/print-mode-spike.md). Its permission
    /// requests park in the window like a background session's. Then, for
    /// a window fork, a tab opens on it; a background one is left finished,
    /// to carry on with Resume in bg like any background session.
    #[allow(clippy::too_many_arguments)]
    fn print_fork(
        &self,
        parent: &str,
        cut: &str,
        sid: &str,
        run_in: &str,
        first: &str,
        options: &Map<String, Value>,
        then_window: bool,
    ) -> Result<(), String> {
        let mut cmd: Vec<String> = vec![
            "claude".into(), "-p".into(), "--resume".into(), parent.into(), "--resume-session-at".into(), cut.into(),
            "--fork-session".into(), "--session-id".into(), sid.into(), "--output-format".into(), "json".into(),
        ];
        for (key, flag) in [("model", "--model"), ("permission_mode", "--permission-mode"), ("agent", "--agent")] {
            if let Some(v) = options.get(key).filter(|v| truthy(Some(v))) {
                cmd.extend([flag.to_string(), crate::pyish::py_str(v)]);
            }
        }
        cmd.push(first.to_string());
        let dir = self.state_dir().join("forks");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let out = std::fs::File::create(dir.join(format!("{sid}.json"))).map_err(|e| e.to_string())?;
        let err = std::fs::File::create(dir.join(format!("{sid}.err"))).map_err(|e| e.to_string())?;
        let mut c = std::process::Command::new(&cmd[0]);
        c.args(&cmd[1..])
            .current_dir(run_in)
            .env_clear()
            .envs(crate::launch::launch_env())
            .stdin(std::process::Stdio::null())
            .stdout(out)
            .stderr(err);
        crate::proc::no_window(&mut c);
        let mut child = match c.spawn() {
            Ok(ch) => ch,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err("claude is not on PATH".into()),
            Err(e) => return Err(e.to_string()),
        };
        let me = self.me.get().and_then(std::sync::Weak::upgrade);
        let (sid, run_in) = (sid.to_string(), run_in.to_string());
        std::thread::spawn(move || {
            let status = child.wait();
            let Some(me) = me else { return };
            let ok = status.as_ref().is_ok_and(|s| s.success());
            let mut e = json!({"kind": "fork_first_turn", "session_id": sid, "cwd": run_in, "ok": ok});
            if ok && then_window {
                let cmd = crate::launch::resume_command(&sid, false);
                match crate::launch::open_terminal(&run_in, &cmd) {
                    Ok(t) => e["terminal"] = json!(t),
                    Err(err) => e["error"] = json!(err),
                }
            }
            if let Some(store) = &me.store {
                store.add_event_json(e);
            }
            me.invalidate();
        });
        Ok(())
    }
}
