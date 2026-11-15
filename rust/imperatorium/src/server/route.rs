//! Routing: GET dispatch (handle) and POST dispatch (post, post_api).

use super::*;

impl Shared {
    /// Handler._post_api, for the routes that change only the dispatcher's
    /// own state. The rest (resume, new, close, worktree, settings, remote)
    /// act on sessions, terminals or files and come later in step 2.
    fn post_api(
        &self,
        store: &crate::store::Store,
        path: &str,
        p: &Map<String, Value>,
        from_window: bool,
    ) -> Value {
        match path {
            "/api/prefs" => {
                let allowed: Map<String, Value> = ["pinned", "archived", "group", "filter"]
                    .iter()
                    .filter_map(|k| p.get(*k).map(|v| (k.to_string(), v.clone())))
                    .collect();
                json!({"ok": true, "prefs": store.set_prefs(&allowed)})
            }
            "/api/notify" => {
                let channel = p.get("channel").cloned().unwrap_or(Value::Null);
                let name = channel.as_str().unwrap_or("");
                if name != "ntfy" && name != "speak" {
                    return json!({"ok": false, "error": format!("no such channel: {}",
                        if channel.is_null() { "None".to_string() } else { crate::pyish::py_str(&channel) })});
                }
                let mut saved = self
                    .state()
                    .get("prefs")
                    .and_then(|x| x.get("notify"))
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                saved.insert(name.into(), json!(truthy(p.get("enabled"))));
                let mut change = Map::new();
                change.insert("notify".into(), Value::Object(saved));
                store.set_prefs(&change);
                let mut out = self.api_notify();
                out["ok"] = json!(true);
                out
            }
            "/api/unqueue" => {
                let sid = p
                    .get("session")
                    .map(crate::pyish::py_str)
                    .unwrap_or_default();
                let ok = store.unqueue(&sid, p.get("id").unwrap_or(&Value::Null));
                self.invalidate();
                json!({"ok": ok})
            }
            "/api/dismiss" => self.dismiss_session(store, p.get("session_id")),
            "/api/reviewed" => {
                // Control.mark_reviewed.
                let sid = p.get("session_id").and_then(Value::as_str).unwrap_or("");
                let Some(row) = self.row(sid).filter(|_| !sid.is_empty()) else {
                    return json!({"ok": false, "error": "no such session"});
                };
                if !truthy(row.get("attention")) {
                    return json!({"ok": true, "reviewed": truthy(row.get("reviewed"))});
                }
                if registry::in_progress(&row) {
                    // Work still going cannot have been reviewed; the window
                    // checks too, but its copy of the row can be a second old.
                    return json!({"ok": true, "reviewed": false, "in_progress": true});
                }
                store.mark_reviewed(sid, Self::now());
                self.invalidate();
                json!({"ok": true, "reviewed": true})
            }
            "/api/queue" => {
                let prompt = p.get("prompt").and_then(Value::as_str).unwrap_or("");
                if crate::pyish::strip(prompt).is_empty() {
                    return json!({"ok": false, "error": "no prompt"});
                }
                let sid = p.get("session_id").and_then(Value::as_str).unwrap_or("");
                if self.row(sid).is_none() {
                    return json!({"ok": false, "error": "no such session"});
                }
                self.enqueue(store, sid, prompt, from_window)
            }
            _ => json!({"ok": false, "error": "no such path"}),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn post(
        &self,
        path: &str,
        origin: Option<&str>,
        ctype: Option<&str>,
        body: &[u8],
        port: u16,
        window: Option<&str>,
        mod_token: Option<&str>,
    ) -> (u16, Body) {
        const MAX_BODY: usize = 4 * 1024 * 1024;
        if let Some(o) = origin {
            if o != format!("http://127.0.0.1:{port}") && o != format!("http://localhost:{port}") {
                return (
                    403,
                    Body::Json(json!({"error": "cross-origin requests are refused"})),
                );
            }
        }
        if path == "/api/transcribe" {
            // Raw audio, not JSON (Handler._read_binary).
            if body.is_empty() || body.len() > MAX_BODY * 8 {
                return (413, Body::Json(json!({"error": "body out of range"})));
            }
            return (200, Body::Json(self.transcribe(body)));
        }
        let ctype = ctype.unwrap_or("").split(';').next().unwrap_or("").trim();
        if ctype != "application/json" {
            return (
                415,
                Body::Json(json!({"error": "expected application/json"})),
            );
        }
        if body.is_empty() || body.len() > MAX_BODY {
            return (
                413,
                Body::Json(json!({"error": format!("body must be 1..{MAX_BODY} bytes")})),
            );
        }
        let Ok(payload) = std::str::from_utf8(body)
            .map_err(|_| ())
            .and_then(|t| serde_json::from_str::<Value>(t).map_err(|_| ()))
        else {
            return (400, Body::Json(json!({"error": "body is not JSON"})));
        };
        let empty = Map::new();
        let object = payload.as_object();
        const API: &[&str] = &[
            "/api/queue",
            "/api/unqueue",
            "/api/worktree",
            "/api/resume",
            "/api/new",
            "/api/close",
            "/api/dismiss",
            "/api/reviewed",
            "/api/prefs",
            "/api/notify",
            "/api/settings",
            "/api/remote/refresh",
        ];
        if API.contains(&path) && object.is_none() {
            return (400, Body::Json(json!({"error": "expected an object"})));
        }
        let p = object.unwrap_or(&empty);
        let from_window = origin.is_some()
            && window.is_some_and(|w| w.as_bytes() == self.window_token.as_bytes());
        if path == "/api/mod/beat" || path == "/api/mod/check" {
            if object.is_none() {
                return (400, Body::Json(json!({"error": "expected an object"})));
            }
            let Some(link) = self.modlink.as_ref().filter(|l| l.authentic(mod_token)) else {
                return (
                    403,
                    Body::Json(json!({"ok": false, "error": "not the Imperatorium mod"})),
                );
            };
            let out = if path == "/api/mod/beat" {
                self.mod_beat(link, p)
            } else {
                self.mod_check(p)
            };
            return (200, Body::Json(out));
        }
        match path {
            "/permission/park" => {
                // The hook blocks here until a human answers or the wait
                // runs out; running out is a silent "ask".
                let wait = self.park_wait(p);
                let (verdict, who, answers) = self.pending.park(p, wait);
                let mut reply = json!({"verdict": verdict, "decided_by": who});
                if truthy(Some(&answers)) {
                    reply["answers"] = answers;
                }
                return (200, Body::Json(reply));
            }
            "/api/decide" => {
                let who = p
                    .get("who")
                    .filter(|v| truthy(Some(v)))
                    .cloned()
                    .unwrap_or(json!("window"));
                let ok = self.pending.decide(
                    p.get("id").unwrap_or(&Value::Null),
                    p.get("verdict").unwrap_or(&Value::Null),
                    who,
                    p.get("answers"),
                );
                return (if ok { 200 } else { 409 }, Body::Json(json!({"ok": ok})));
            }
            _ => {}
        }
        match path {
            "/hook" => (200, Body::Json(self.on_hook(p, None))),
            "/api/update/check" => (200, Body::Json(self.updates.check(&self.config()))),
            "/api/update/apply" => {
                if !from_window {
                    return (403, Body::Json(json!({"ok": false, "error": "only the Imperatorium window installs an update"})));
                }
                if self.store.is_none() {
                    return (200, Body::Json(json!({"ok": false, "error": "a read-only shadow changes nothing"})));
                }
                let log = self.state_dir().join("update.log");
                (200, Body::Json(self.updates.apply(&self.config(), &log)))
            }
            "/api/remote/refresh" => {
                let n = self.fleet.as_ref().map(|f| f.refresh()).unwrap_or(0);
                (200, Body::Json(json!({"ok": true, "hosts": n})))
            }
            "/api/dispatch" => {
                let Some(store) = &self.store else {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                };
                if object.is_none() {
                    return (400, Body::Json(json!({"error": "expected an object"})));
                }
                (200, Body::Json(self.dispatch(store, p, from_window)))
            }
            "/api/voice/warm" => (200, Body::Json(self.warm_voice())),
            "/api/kill_process" => {
                // Ends one process a session's shell started: only from the
                // window, and only one that is beneath that session.
                if !from_window {
                    return (403, Body::Json(json!({"ok": false, "error": "only the Imperatorium window"})));
                }
                let sid = p.get("session_id").and_then(Value::as_str).unwrap_or("");
                let want = p.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32;
                let Some(root) = self.row(sid).filter(|r| !truthy(r.get("remote")))
                    .and_then(|r| r.get("pid").and_then(Value::as_u64)) else {
                    return (200, Body::Json(json!({"ok": false, "error": "no such local session"})));
                };
                let under = crate::procs::details(root as u32)
                    .iter()
                    .any(|d| d.get("pid").and_then(Value::as_u64) == Some(want as u64));
                if want == 0 || !under {
                    return (200, Body::Json(json!({"ok": false, "error": "that process is not one this session started"})));
                }
                (200, Body::Json(match crate::procs::kill(want) {
                    Ok(()) => json!({"ok": true}),
                    Err(e) => json!({"ok": false, "error": e}),
                }))
            }
            "/api/dictate" => {
                if !from_window {
                    return (403, Body::Json(json!({"ok": false, "error": "dictation opens the microphone: only the Imperatorium window and its herdr mods"})));
                }
                // The mods' own microphone (voice.Recorder) is herdr-only.
                (200, Body::Json(json!({"ok": false, "error": "dictation from the herdr mods is not ported yet"})))
            }
            "/api/open_window" => {
                let (route, size) = match p.get("page").and_then(Value::as_str) {
                    Some("matrix") => ("/matrix", "1100,720"),
                    Some("window") => ("/", "1280,820"),
                    _ => return (400, Body::Json(json!({"ok": false, "error": "page is one of matrix, window"}))),
                };
                if !from_window {
                    return (403, Body::Json(json!({"ok": false, "error": "only the Imperatorium window and its herdr mods"})));
                }
                let url = format!("http://127.0.0.1:{port}{route}");
                (200, Body::Json(match crate::voice::open_window(&url, size) {
                    Err(e) => json!({"ok": false, "error": e}),
                    Ok(used) => json!({"ok": true, "url": url, "browser": used.map(|b| {
                        b.rsplit(['/', '\\']).next().unwrap_or("").to_string()
                    })}),
                }))
            }
            "/api/focus" => {
                if object.is_none() {
                    return (400, Body::Json(json!({"error": "expected an object"})));
                }
                let Some(store) = &self.store else {
                    return (200, Body::Json(json!({"ok": false, "error": "a read-only shadow changes nothing"})));
                };
                (200, Body::Json(self.focus(store, p.get("project"), p.get("session_id"))))
            }
            "/api/typed" => {
                if object.is_none() {
                    return (400, Body::Json(json!({"error": "expected an object"})));
                }
                if !from_window {
                    return (
                        403,
                        Body::Json(json!({"ok": false,
                        "error": "Send As Typed is only available from the Imperatorium window"})),
                    );
                }
                let Some(store) = &self.store else {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                };
                (
                    200,
                    Body::Json(self.send_typed(store, p.get("session_id"), p.get("prompt"))),
                )
            }
            "/api/resume" | "/api/new" | "/api/close" => {
                let Some(store) = &self.store else {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                };
                let text = |k: &str| {
                    p.get(k)
                        .filter(|v| truthy(Some(v)))
                        .map(crate::pyish::py_str)
                };
                let sid = text("session_id");
                let got = match path {
                    "/api/resume" => self.resume(
                        store,
                        sid.as_deref().unwrap_or(""),
                        text("where").as_deref().unwrap_or("window"),
                        truthy(p.get("fork")),
                        text("prompt").as_deref(),
                        text("cwd"),
                    ),
                    "/api/new" => {
                        self.new_session(store, sid.as_deref(), text("cwd"), truthy(p.get("close")))
                    }
                    _ => self.close_session(store, sid.as_deref()),
                };
                (200, Body::Json(got))
            }
            "/api/worktree" => {
                let Some(store) = &self.store else {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                };
                let project = p
                    .get("project")
                    .filter(|v| !v.is_null())
                    .map(crate::pyish::py_str);
                // Only a directory the catalogue or a live session vouches
                // for, never a path taken on trust from the request.
                let Some(project) = project.filter(|pr| self.known_project(pr)) else {
                    let shown = p
                        .get("project")
                        .map(crate::pyish::py_str)
                        .unwrap_or_else(|| "None".into());
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": format!("not a known project: {shown}")}),
                        ),
                    );
                };
                let path = p.get("path").map(crate::pyish::py_str).unwrap_or_default();
                let action = p
                    .get("action")
                    .map(crate::pyish::py_str)
                    .unwrap_or_else(|| "None".into());
                let got = crate::changes::finish(&project, &path, &action, truthy(p.get("force")));
                if got["ok"] == json!(true) {
                    let summary = format!("{} {}", action, crate::pyish::py_str(&got["branch"]));
                    let e = json!({"kind": format!("worktree_{action}"), "cwd": project, "summary": summary});
                    store.add_event_json(e);
                }
                (200, Body::Json(got))
            }
            "/api/settings" => {
                if self.store.is_none() {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                }
                // Turning Send As Typed on lets a message speak as you, so
                // only the Settings window may do it.
                let ch = p.get("changes");
                if ch
                    .and_then(|c| c.get("chat.send_as_typed"))
                    .is_some_and(|v| truthy(Some(v)))
                    && !from_window
                {
                    return (
                        200,
                        Body::Json(json!({"ok": false,
                        "errors": {"chat.send_as_typed": "can only be turned on from the Settings window"},
                        "error": "some values were not accepted"})),
                    );
                }
                (200, Body::Json(self.save_settings(ch)))
            }
            "/api/prefs" | "/api/notify" | "/api/unqueue" | "/api/dismiss" | "/api/reviewed" | "/api/queue" => {
                let Some(store) = &self.store else {
                    return (
                        200,
                        Body::Json(
                            json!({"ok": false, "error": "a read-only shadow changes nothing"}),
                        ),
                    );
                };
                (200, Body::Json(self.post_api(store, path, p, from_window)))
            }
            _ => (404, Body::Json(json!({"error": "no such path"}))),
        }
    }

    fn page(&self, page: &ui::Page, token: &str, with_look: bool) -> Body {
        let drawn = if with_look {
            Some(look(&self.config()))
        } else {
            None
        };
        Body::Text(
            ui::with_meta(token, &page.linked(), &self.boot, drawn.as_ref()),
            "text/html; charset=utf-8",
        )
    }

    pub fn handle(&self, method: &str, url: &str) -> (u16, Body) {
        let (raw_path, query) = url.split_once('?').unwrap_or((url, ""));
        // As the Python handler does: a trailing slash is the same path.
        let trimmed = raw_path.trim_end_matches('/');
        let path = if trimmed.is_empty() { "/" } else { trimmed };
        let q = parse_query(query);
        // Handler._arg: the first value of a query parameter, else "".
        let arg = |name: &str| q.get(name).cloned().unwrap_or_default();
        if method == "GET" {
            if let Some(name) = path.strip_prefix("/ui/") {
                // The window's styles and scripts. A request carrying the
                // current ?v= can be kept for good; any other still
                // revalidates by ETag.
                return match (ui::asset(name), ui::version(name)) {
                    (Some((ctype, body)), Some(v)) => {
                        let cache = if arg("v") == v {
                            "public, max-age=31536000, immutable"
                        } else {
                            "no-cache"
                        };
                        (200, Body::Static(body, ctype, cache, v))
                    }
                    _ => (404, Body::Json(json!({"error": "no such file"}))),
                };
            }
            if let Some(name) = path.strip_prefix("/fonts/") {
                return match ui::font(name) {
                    Some(bytes) => (200, Body::Bytes(bytes, "font/woff2", "max-age=86400")),
                    None => (404, Body::Json(json!({"error": "no such font"}))),
                };
            }
        }
        match (method, path) {
            ("GET", "/") => (200, self.page(&ui::SHELL, &self.window_token, true)),
            ("GET", "/matrix") => (200, self.page(&ui::MATRIX, "", true)),
            ("GET", "/settings") => (200, self.page(&ui::SETTINGS, &self.window_token, false)),
            ("GET", "/api/history") => (200, Body::Json(self.api_history(&q))),
            // Remote hosts are phase 4: until then this dispatcher polls none.
            ("GET", "/api/hosts") => (200, Body::Json(self.hosts_status())),
            ("GET", "/api/update") => (200, Body::Json(self.updates.status(&self.config()))),
            ("GET", "/api/events") => {
                // store.events(100): the newest hundred, newest first.
                let st = self.state();
                let all = st
                    .get("events")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let recent: Vec<Value> = all.iter().rev().take(100).cloned().collect();
                (200, Body::Json(json!({"events": recent})))
            }
            ("GET", "/api/queue") => {
                // store.queued(): every session that has something queued.
                let st = self.state();
                let q: Map<String, Value> = st
                    .get("queue")
                    .and_then(Value::as_object)
                    .map(|q| {
                        q.iter()
                            .filter(|(_, v)| truthy(Some(v)))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                (200, Body::Json(json!({"queue": q})))
            }
            ("GET", "/api/prefs") => {
                let st = self.state();
                let prefs = st
                    .get("prefs")
                    .filter(|p| p.is_object() || p.is_array())
                    .cloned()
                    .unwrap_or(json!({}));
                (200, Body::Json(json!({"prefs": prefs})))
            }
            ("GET", "/api/usage") => (200, Body::Json(self.api_usage())),
            ("GET", "/api/pending") => {
                (200, Body::Json(json!({"pending": self.pending.waiting()})))
            }
            ("GET", "/api/matrix") => (200, Body::Json(self.api_matrix())),
            ("GET", "/api/decisions") => (
                200,
                Body::Json(json!({"decisions": decisions(&self.state_dir(), 60)})),
            ),
            ("GET", "/api/settings") => (200, Body::Json(self.api_settings())),
            ("GET", "/api/projects") => (200, Body::Json(json!({"projects": self.catalogue()}))),
            ("GET", "/api/changes") => (200, Body::Json(self.api_changes(&arg("session")))),
            ("GET", "/api/activity") => (200, Body::Json(self.api_activity(&arg("session")))),
            ("GET", "/api/costs") => {
                let raw = arg("days");
                let raw = if raw.is_empty() {
                    "30".to_string()
                } else {
                    raw
                };
                // int(): digits with an optional sign and surrounding space.
                let days = crate::pyish::strip(&raw)
                    .parse::<i64>()
                    .map(|d| d.clamp(1, 90))
                    .unwrap_or(30);
                let got = self
                    .cost_history
                    .lk()
                    .summary(days, Self::now(), 8.0);
                (200, Body::Json(got))
            }
            ("GET", "/api/session_status") => {
                (200, Body::Json(self.api_session_status(&arg("session"))))
            }
            ("GET", "/api/awaited") => (200, Body::Json(self.api_awaited(&arg("session")))),
            ("GET", "/api/timeline") => (200, self.api_timeline(&arg("session"), &arg("sig"))),
            ("GET", "/api/subagent") => (200, Body::Json(self.api_subagent(&arg("session"), &arg("agent")))),
            ("GET", "/api/processes") => (200, Body::Json(self.api_processes(&arg("session")))),
            ("GET", "/api/errors") => {
                let raw = arg("days");
                let raw = if raw.is_empty() { "7".to_string() } else { raw };
                let days = crate::pyish::py_float(&json!(raw))
                    .map(|d| d.clamp(0.1, 90.0))
                    .unwrap_or(7.0);
                (200, Body::Json(self.api_errors(days)))
            }
            ("GET", "/api/worktrees") => {
                let project = arg("project");
                let body = if self.known_project(&project) {
                    changes::worktrees(&project)
                } else {
                    json!({"ok": false, "error": format!("not a known project: {project}")})
                };
                (200, Body::Json(body))
            }
            ("GET", "/api/worktree_diff") => {
                let project = arg("project");
                let body = if self.known_project(&project) {
                    changes::worktree_diff(&project, &arg("path"))
                } else {
                    json!({"ok": false, "error": format!("not a known project: {project}")})
                };
                (200, Body::Json(body))
            }
            ("GET", "/api/notify") => (200, Body::Json(self.api_notify())),
            ("GET", "/api/quick") => (
                200,
                Body::Json(json!({"key": settings::value(&self.config(), "quick.key"),
                                  "phrases": settings::value(&self.config(), "quick.phrases"), "ok": true})),
            ),
            ("GET", "/api/health") => (
                200,
                Body::Json(
                    json!({"ok": true, "pid": std::process::id(), "boot": self.boot,
                                  "look": look(&self.config()),
                                  "conversation_parses": self.parses.load(std::sync::atomic::Ordering::Relaxed)}),
                ),
            ),
            ("GET", "/api/conversation") => self.api_conversation(&q),
            ("GET", "/api/sessions") => (200, Body::Raw(self.api_sessions())),
            _ => (404, Body::Json(json!({"error": "no such path"}))),
        }
    }
}
