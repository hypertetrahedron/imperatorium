//! Hooks: what a tool hook records, the idle check after a Stop, the mod's beats, and the hook-only listener and tunnels.

use super::*;

impl Shared {
    /// Control.on_hook: one hook event. `host` is set only by a host's
    /// tunnel listener, and a remote session's records are kept under a
    /// host-qualified key, so a host can only ever describe its own.
    pub(super) fn on_hook(&self, payload: &Map<String, Value>, host: Option<&str>) -> Value {
        let Some(store) = &self.store else {
            return json!({"ok": false, "error": "a read-only shadow takes no hooks"});
        };
        let event = payload
            .get("hook_event_name")
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str)
            .unwrap_or_else(|| "unknown".into());
        let sid = payload
            .get("session_id")
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str);
        let qualified = |sid: &str| match host {
            Some(h) => remote_key(h, sid),
            None => sid.to_string(),
        };
        if let Some(sid) = &sid {
            if event == "imperatorium_statusline" || event == "ccontrol_statusline" {
                // Several a minute per session: memory only, never the store.
                if let Some(crossing) = self.usage.ingest(&qualified(sid), payload, Self::now()) {
                    self.budget_crossed(&crossing);
                }
                return json!({"ok": true});
            }
            if crate::live::TOOL_EVENTS.contains(&event.as_str()) {
                self.live.ingest(&qualified(sid), payload, Self::now());
                return json!({"ok": true});
            }
        }
        const INTERESTING: &[&str] = &[
            "SessionStart",
            "SessionEnd",
            "Notification",
            "Stop",
            "StopFailure",
            "SubagentStop",
            "TaskCompleted",
        ];
        let Some(sid) = sid.filter(|_| INTERESTING.contains(&event.as_str())) else {
            return json!({"ok": true, "ignored": event});
        };
        let key = qualified(&sid);
        let mut meta = Map::new();
        meta.insert(
            "cwd".into(),
            payload.get("cwd").cloned().unwrap_or(Value::Null),
        );
        meta.insert("last_event".into(), json!(event));
        meta.insert("last_event_at".into(), json!(Self::now()));
        let token = payload.get("messaging_token").cloned();
        if let Some(sock) = payload.get("messaging_socket").filter(|v| truthy(Some(v))) {
            meta.insert("socket".into(), sock.clone());
        }
        // herdr-core hosting comes with step 3; a SessionStart outside it
        // replaces the old terminal's record, as in Python.
        if host.is_none() && !truthy(payload.get("hc_terminal")) && event == "SessionStart" {
            meta.insert("hc".into(), json!({}));
        }
        let ev = |kind: &str, extra: Value| {
            let mut e = json!({"kind": kind, "session_id": sid})
                .as_object()
                .unwrap()
                .clone();
            if let Value::Object(x) = extra {
                e.extend(x);
            }
            if let Some(h) = host {
                e.insert("host".into(), json!(h));
            }
            store.add_event(e);
        };
        let cwd = payload.get("cwd").cloned().unwrap_or(Value::Null);
        match event.as_str() {
            "SessionStart" => {
                meta.insert(
                    "name".into(),
                    payload.get("session_name").cloned().unwrap_or(Value::Null),
                );
                store.register(&key, token.as_ref(), &meta);
                store.undismiss(&key);
                self.invalidate();
                ev("session_start", json!({"cwd": meta.get("cwd")}));
            }
            "SessionEnd" => {
                let gone = store.session(&key).unwrap_or_default();
                store.forget(&key);
                self.live.forget(&key);
                self.usage.forget(&key);
                if let (Some(link), None) = (&self.modlink, host) {
                    link.forget(&key);
                }
                self.invalidate();
                let cwd = if truthy(Some(&cwd)) {
                    cwd.clone()
                } else {
                    gone.get("cwd").cloned().unwrap_or(Value::Null)
                };
                ev(
                    "session_end",
                    json!({"cwd": cwd, "name": gone.get("name").cloned().unwrap_or(Value::Null)}),
                );
            }
            "Notification" => {
                let kind = payload
                    .get("notification_type")
                    .filter(|v| truthy(Some(v)))
                    .map(crate::pyish::py_str)
                    .unwrap_or_else(|| "notification".into());
                meta.insert("last_notification".into(), json!(kind));
                let caption = match kind.as_str() {
                    "permission_prompt" => Some("needs permission"),
                    "idle_prompt" => Some("waiting for you"),
                    "agent_needs_input" => Some("needs input"),
                    "elicitation_dialog" => Some("needs an answer"),
                    "elicitation_url_dialog" => Some("needs a URL"),
                    _ => None,
                };
                match caption {
                    Some(c) => {
                        store.raise_attention(&key, c, "notification", &meta);
                        if truthy(token.as_ref()) {
                            store.register(&key, token.as_ref(), &Map::new());
                        }
                        self.announce(&key, c);
                    }
                    None => store.register(&key, token.as_ref(), &meta),
                }
                self.invalidate();
                ev(&kind, json!({"cwd": cwd}));
            }
            "StopFailure" => {
                let summary = payload
                    .get("last_assistant_message")
                    .filter(|v| truthy(Some(v)))
                    .cloned();
                let error = payload.get("error").cloned().unwrap_or(Value::Null);
                let detail = payload
                    .get("error_details")
                    .filter(|v| truthy(Some(v)))
                    .cloned()
                    .or(summary);
                let detail_s = detail
                    .as_ref()
                    .map(crate::pyish::py_str)
                    .unwrap_or_default();
                let caption = registry::failure_caption(error.as_str(), &detail_s);
                let failure = if truthy(Some(&error)) {
                    error.clone()
                } else {
                    json!("unknown")
                };
                meta.insert("last_failure".into(), failure);
                meta.insert(
                    "last_failure_detail".into(),
                    json!(crate::pyish::take(&detail_s, 280)),
                );
                match caption {
                    Some(c) => {
                        store.raise_attention(&key, c, "failure", &meta);
                        self.announce(&key, c);
                    }
                    None => store.register(&key, None, &meta),
                }
                if truthy(token.as_ref()) {
                    store.register(&key, token.as_ref(), &Map::new());
                }
                self.invalidate();
                ev(
                    "stopfailure",
                    json!({"cwd": cwd, "error": error,
                                         "summary": crate::pyish::take(&detail_s, 280)}),
                );
            }
            _ => {
                let summary = payload
                    .get("last_assistant_message")
                    .filter(|v| truthy(Some(v)))
                    .map(crate::pyish::py_str)
                    .unwrap_or_default();
                store.register(&key, token.as_ref(), &meta);
                if ["Stop", "SubagentStop", "TaskCompleted"].contains(&event.as_str()) {
                    store.clear_attention(&key);
                }
                self.invalidate();
                ev(
                    &event.to_lowercase(),
                    json!({"cwd": cwd, "summary": crate::pyish::take(&summary, 280)}),
                );
                if event == "Stop" {
                    let stop_at = meta.get("last_event_at").cloned().unwrap_or(Value::Null);
                    if let Some(me) = self.me.get().and_then(std::sync::Weak::upgrade) {
                        let (k, at) = (key.clone(), stop_at);
                        let (h, s) = (host.map(str::to_string), sid.clone());
                        self.timer.after(
                            std::time::Duration::from_secs_f64(crate::timing::t().idle_after),
                            move || {
                                me.idle_check(&k, &at, h.as_deref(), &s);
                            },
                        );
                    }
                    // The turn is over: one queued prompt per turn.
                    if let Some(id) = self.drain(store, &key) {
                        return json!({"ok": true, "delivered": id});
                    }
                }
            }
        }
        json!({"ok": true})
    }

    /// Control._idle_check: the `idle_prompt` Claude Code no longer sends. A
    /// minute after Stop, a session that is idle, has had no hook since and
    /// has nothing running is waiting for you. True when it raised it.
    fn idle_check(&self, key: &str, stop_at: &Value, host: Option<&str>, sid: &str) -> bool {
        let Some(store) = &self.store else {
            return false;
        };
        let rec = store.session(key).unwrap_or_default();
        if rec.get("last_event_at") != Some(stop_at) || truthy(rec.get("attention_since")) {
            return false;
        }
        self.invalidate();
        let Some(row) = self.row(key) else {
            return false;
        };
        if row.get("status").and_then(Value::as_str) != Some("idle") {
            return false;
        }
        // Background work holds idle_prompt back; a pending wakeup is not
        // work, but the session will take its next turn by itself.
        let work = row.get("work").cloned().unwrap_or(json!({}));
        let busy = |k: &str| work.get(k).is_some_and(|v| truthy(Some(v)));
        if registry::still_running(&row, Some(&json!("idle_prompt")))
            || truthy(row.get("shell_processes"))
            || ["agents", "shells", "monitors", "wakeups"]
                .iter()
                .any(|k| busy(k))
        {
            return false;
        }
        let caption = "waiting for you";
        let mut meta = Map::new();
        meta.insert("last_notification".into(), json!("idle_prompt"));
        store.raise_attention(key, caption, "notification", &meta);
        self.announce(key, caption);
        self.invalidate();
        let mut e = json!({"kind": "idle_prompt", "session_id": if sid.is_empty() { key } else { sid },
                       "cwd": rec.get("cwd").cloned().unwrap_or(Value::Null),
                       "summary": "raised by the dispatcher"});
        if let Some(h) = host {
            e["host"] = json!(h);
        }
        store.add_event_json(e);
        true
    }

    /// Control.mod_beat: what the session's mod says it is doing, its usage
    /// and what became of its prompts; answers the prompts to submit now and
    /// the line to show under the prompt.
    pub(super) fn mod_beat(&self, link: &crate::modlink::ModLink, beat: &Map<String, Value>) -> Value {
        let sid = beat
            .get("session_id")
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        if sid.is_empty() {
            return json!({"ok": false, "error": "no session_id"});
        }
        let now = Self::now();
        let was = link.state(&sid, now);
        let give = link.beat(beat, now);
        let acks = beat
            .get("acks")
            .filter(|v| truthy(Some(v)))
            .and_then(Value::as_array);
        if let Some(store) = &self.store {
            for ack in acks.into_iter().flatten() {
                if ack.get("outcome").and_then(Value::as_str) == Some("dropped") {
                    let reason = ack
                        .get("reason")
                        .filter(|v| truthy(Some(v)))
                        .map(crate::pyish::py_str)
                        .unwrap_or_default();
                    let e = json!({"kind": "dispatch_dropped", "session_id": sid,
                                   "cwd": beat.get("cwd").cloned().unwrap_or(Value::Null),
                                   "summary": crate::pyish::take(&reason, 280)});
                    store.add_event_json(e);
                }
            }
        }
        if let Some(u) = beat.get("usage").filter(|u| u.is_object()) {
            let reading = crate::modlink::statusline_payload(u);
            if let Some(crossing) = self.usage.ingest(&sid, reading.as_object().unwrap(), now) {
                self.budget_crossed(&crossing);
            }
        }
        let busy = truthy(beat.get("busy"));
        if was.as_ref().is_none_or(|w| truthy(w.get("busy")) != busy)
            || !give.is_empty()
            || truthy(beat.get("acks"))
        {
            self.invalidate();
        }
        if let (Some(store), Some(stopped)) = (&self.store, beat.get("stopped").and_then(Value::as_array)) {
            for s in stopped {
                let outcome = s.get("outcome").and_then(Value::as_str).unwrap_or("");
                let reason = s.get("reason").and_then(Value::as_str).unwrap_or("");
                store.add_event_json(json!({"kind": "stopped", "session_id": sid,
                    "cwd": beat.get("cwd").cloned().unwrap_or(Value::Null),
                    "summary": crate::pyish::take(&format!("{outcome}{}{reason}", if reason.is_empty() { "" } else { ": " }), 280)}));
            }
        }
        let mut reply = json!({"ok": true, "deliver": give, "status": self.mod_status_line(link, &sid)});
        // Only when one waits: a beat's reply is otherwise as it always was.
        if let Some(stop) = link.take_stop(&sid, now) {
            reply["stop"] = json!(stop);
        }
        reply
    }

    /// Control.mod_status_line: who else is waiting on you, and what is
    /// queued here. None: nothing to say.
    fn mod_status_line(&self, link: &crate::modlink::ModLink, sid: &str) -> Value {
        // Any snapshot from the last 15 s will do: a beat comes every two
        // seconds from every session.
        let rows = self.sessions(15.0);
        let waiting: Vec<&Row> = rows
            .iter()
            .filter(|r| {
                truthy(r.get("attention"))
                    && r.get("sessionId").and_then(Value::as_str) != Some(sid)
            })
            .collect();
        let mut parts = vec![];
        if !waiting.is_empty() {
            let names: Vec<String> = waiting
                .iter()
                .take(3)
                .map(|r| {
                    let name = r
                        .get("name")
                        .filter(|v| truthy(Some(v)))
                        .map(crate::pyish::py_str)
                        .unwrap_or_else(|| {
                            let cwd = r
                                .get("cwd")
                                .filter(|v| truthy(Some(v)))
                                .map(crate::pyish::py_str)
                                .unwrap_or_default();
                            let trimmed = cwd.trim_end_matches(['/', '\\']);
                            let base = trimmed.rsplit(['/', '\\']).next().unwrap_or("");
                            let base = if cfg!(windows) && base.len() == 2 && base.ends_with(':') {
                                ""
                            } else {
                                base
                            };
                            if base.is_empty() {
                                "?".to_string()
                            } else {
                                base.to_string()
                            }
                        });
                    crate::pyish::take(&name, 24)
                })
                .collect();
            let more = if waiting.len() > 3 {
                format!(" +{}", waiting.len() - 3)
            } else {
                String::new()
            };
            parts.push(format!(
                "{} waiting on you: {}{}",
                waiting.len(),
                names.join(", "),
                more
            ));
        }
        let queued = self
            .store
            .as_ref()
            .map(|s| s.queued(sid).len())
            .unwrap_or(0);
        let here = queued + link.pending(sid).len();
        if here > 0 {
            parts.push(format!("{here} queued here"));
        }
        if parts.is_empty() {
            Value::Null
        } else {
            json!(parts.join(" \u{b7} "))
        }
    }

    /// Control.mod_check: the permission policy's verdict on a tool call the
    /// session's mod asks about - the same policy.decide the hook runs, and
    /// only once auto-approval is on (`decision_format: "object"`).
    pub(super) fn mod_check(&self, request: &Map<String, Value>) -> Value {
        let policy = std::fs::read_to_string(paths::policy_path(self.home.as_deref()))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        if policy.get("decision_format").and_then(Value::as_str) != Some("object") {
            return json!({"verdict": "ask", "reason": "auto-approval is off"});
        }
        let get = |k: &str| request.get(k).cloned().unwrap_or(Value::Null);
        let req = json!({"tool_name": get("tool_name"), "tool_input": get("tool_input"),
                         "cwd": get("cwd"), "session_id": get("session_id")});
        let got = crate::policy::decide(&req, &policy);
        let empty = Map::new();
        let input = req["tool_input"].as_object().unwrap_or(&empty);
        let detail = ["command", "file_path", "pattern"]
            .iter()
            .find_map(|k| input.get(*k).filter(|v| truthy(Some(v))))
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let record = json!({"at": Self::now(), "verdict": got.verdict, "reason": got.reason, "tier": got.tier,
                            "tool": req["tool_name"], "via": "mod", "detail": crate::pyish::take(&detail, 200),
                            "cwd": req["cwd"], "session_id": req["session_id"], "agent_id": get("agent_id")});
        // decisions.jsonl beside the state, as the hook writes its own.
        let path = self.state_dir().join("decisions.jsonl");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
        {
            use std::io::Write;
            let _ =
                f.write_all(format!("{}\n", crate::pyish::json_dumps(&record, false)).as_bytes());
        }
        json!({"verdict": got.verdict, "reason": got.reason, "tier": got.tier})
    }
}

/// Control.decisions: the permission hook's recent verdicts, newest first,
/// from the decisions.jsonl beside the state file.
pub(super) fn decisions(dir: &std::path::Path, limit: usize) -> Vec<Value> {
    let Some(text) = tail_lines(&dir.join("decisions.jsonl"), limit) else {
        return vec![];
    };
    // readlines() keeps each line's end; the last `limit` of them.
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..]
        .iter()
        .rev()
        .map(|l| crate::pyish::strip(l))
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

/// The end of a file holding at least its last `limit` lines (whole), read
/// backwards in chunks rather than the whole file; None when it cannot be
/// read or what is kept is not UTF-8, as read_to_string fails.
fn tail_lines(path: &std::path::Path, limit: usize) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut start = len;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = 64 * 1024u64;
    // More than `limit` newlines means the line before the last `limit` has
    // ended in the buffer, so they are all whole.
    while start > 0 && buf.iter().filter(|b| **b == b'\n').count() <= limit {
        let step = chunk.min(start);
        start -= step;
        f.seek(SeekFrom::Start(start)).ok()?;
        let mut piece = vec![0u8; step as usize];
        f.read_exact(&mut piece).ok()?;
        piece.extend_from_slice(&buf);
        buf = piece;
        chunk = chunk.saturating_mul(2);
    }
    if start > 0 {
        // Drop the partial line in front; a newline is never inside a
        // UTF-8 sequence, so what is left starts on a character.
        let cut = buf.iter().position(|b| *b == b'\n').map_or(0, |i| i + 1);
        buf.drain(..cut);
    }
    String::from_utf8(buf).ok()
}

/// dispatcher.start_tunnels: a hook-only listener and a reverse tunnel for
/// each host that asks. Run after the main port is bound, so a second
/// dispatcher that failed to bind never fights the first for the hosts'
/// ports.
pub(super) fn start_tunnels(shared: &Arc<Shared>, fleet: &Arc<crate::remote::Fleet>) {
    for h in &fleet.hosts {
        let Some((local, remote)) = h.tunnel else {
            continue;
        };
        let addr = format!("127.0.0.1:{local}");
        let listener = match tiny_http::Server::http(&addr) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("tunnel {}: cannot listen on {local}: {e}", h.name);
                continue;
            }
        };
        let (me, name) = (Arc::clone(shared), h.name.clone());
        std::thread::Builder::new()
            .name(format!("hooks-{name}"))
            .spawn(move || hook_only(listener, me, name))
            .ok();
        let tunnel = crate::remote::Tunnel::new(&h.name, local, remote);
        tunnel.start();
        shared.tunnels.lk().insert(h.name.clone(), tunnel);
        println!("tunnel {}: host 127.0.0.1:{remote} -> hooks on 127.0.0.1:{local}", h.name);
    }
}

/// HookOnlyHandler: what a host's tunnel lands on - `/hook` and
/// `/permission/park` (its sessions' permission requests, shown in the
/// window under the host's name), and nothing else. Everything else the
/// dispatcher serves would let any process on that host drive the sessions
/// on this machine; a park can only ask, and its answer goes back to the
/// host. The host is known from which listener the connection came in on,
/// never from the payload.
fn hook_only(server: tiny_http::Server, shared: Arc<Shared>, host: String) {
    let inflight = Arc::new(AtomicUsize::new(0));
    // Parks are held until answered: counted apart, so a few waiting
    // requests never crowd out the host's hooks.
    let parked = Arc::new(AtomicUsize::new(0));
    for request in server.incoming_requests() {
        let park = request.url().split('?').next().unwrap_or("").trim_end_matches('/') == "/permission/park";
        let (count, most) = if park { (&parked, MAX_TUNNEL_PARKS) } else { (&inflight, MAX_TUNNEL_HOOKS) };
        let Some(slot) = Slot::take(count, most) else {
            busy(request);
            continue;
        };
        let (shared, host) = (Arc::clone(&shared), host.clone());
        std::thread::spawn(move || {
            let _slot = slot;
            hook_only_one(request, &shared, &host)
        });
    }
}

fn hook_only_one(mut request: tiny_http::Request, shared: &Arc<Shared>, host: &str) {
    {
        let path = request.url().split('?').next().unwrap_or("").trim_end_matches('/').to_string();
        let park = path == "/permission/park";
        let (code, body) = if request.method() != &tiny_http::Method::Post || !(path == "/hook" || park) {
            (404, json!({"error": "no such path"}))
        } else {
            let ctype = request
                .headers()
                .iter()
                .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("Content-Type"))
                .map(|h| h.value.as_str().to_string())
                .unwrap_or_default();
            let mut raw = Vec::new();
            let _ = std::io::Read::read_to_end(
                &mut std::io::Read::take(request.as_reader(), 4 * 1024 * 1024 + 1),
                &mut raw,
            );
            if ctype.split(';').next().unwrap_or("").trim() != "application/json" {
                (415, json!({"error": "expected application/json"}))
            } else if raw.is_empty() || raw.len() > 4 * 1024 * 1024 {
                (413, json!({"error": format!("body must be 1..{} bytes", 4 * 1024 * 1024)}))
            } else {
                match serde_json::from_slice::<Value>(&raw) {
                    Err(_) => (400, json!({"error": "body is not JSON"})),
                    Ok(v) => {
                        let empty = Map::new();
                        let p = v.as_object().unwrap_or(&empty);
                        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            if park {
                                // Which host is the listener's to say; a pid
                                // over there means nothing here.
                                let mut p = p.clone();
                                p.remove("_hook_pid");
                                p.insert("_host".into(), json!(host));
                                shared.park_request(&p)
                            } else {
                                shared.on_hook(p, Some(host))
                            }
                        }))
                        .unwrap_or_else(|e| {
                            eprintln!("imperatorium: hook from {host} panicked: {}", panic_text(&*e));
                            json!({"ok": false, "error": "internal error"})
                        });
                        (200, out)
                    }
                }
            }
        };
        let resp = tiny_http::Response::from_string(body.to_string())
            .with_status_code(code)
            .with_header(json_header());
        let _ = request.respond(resp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_of_a_file_has_its_last_lines() {
        let dir = std::env::temp_dir().join(format!("imp-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("decisions.jsonl");
        // Long enough that the tail is read in more than one chunk.
        let mut text = String::new();
        for i in 0..5000 {
            text.push_str(&format!("{{\"n\": {i}, \"pad\": \"{}\"}}\n", "\u{e9}".repeat(i % 40)));
        }
        for (body, limit) in [
            (text.clone(), 60usize),
            (text.clone() + "{\"n\": \"no newline\"}", 60),
            (text.clone(), 1),
            (text.clone(), 10000),
            ("{\"a\": 1}\n\n{\"b\": 2}\n".to_string(), 60),
            (String::new(), 60),
        ] {
            std::fs::write(&p, &body).unwrap();
            let whole: Vec<&str> = body.split_inclusive('\n').collect();
            let want = whole[whole.len().saturating_sub(limit)..].concat();
            let got = tail_lines(&p, limit).unwrap();
            let got_lines: Vec<&str> = got.split_inclusive('\n').collect();
            assert_eq!(got_lines[got_lines.len().saturating_sub(limit)..].concat(), want);
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
