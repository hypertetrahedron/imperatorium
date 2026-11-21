//! The session snapshot: building it, caching it, invalidating it, and the event streams that wait on it.

use super::*;

impl Shared {
    /// GET /api/matrix: Control.matrix - each running session's replies and
    /// the step it is on, plus what they have cost between them.
    pub(super) fn api_matrix(&self) -> Value {
        let mut rows: Vec<Row> = self
            .sessions(2.0)
            .iter()
            .filter(|r| matrix_active(r))
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            let k = |r: &Row| {
                (
                    r.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_lowercase(),
                    r.get("key")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                )
            };
            k(a).cmp(&k(b))
        });
        let now = Self::now();
        let account = self.usage.summary(now);
        let limits = account["limits"].clone();
        let spent = self.spend(now, 1.0, &limits);
        let per: HashMap<String, Value> = spent["current"]["each"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| (e["sessionId"].as_str().unwrap_or("").to_string(), e.clone()))
            .collect();
        let mut tiles: Vec<Map<String, Value>> = Vec::new();
        for row in &rows {
            let sid = row
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let mut tile = Map::new();
            for k in [
                "key",
                "sessionId",
                "name",
                "cwd",
                "status",
                "state",
                "attention",
                "attention_kind",
                "attention_reason",
                "waitingFor",
                "last_notification",
                "background_tasks",
                "remote",
                "host",
                "work",
                "hc",
                "reviewed",
                "tasks",
                "waiting_tasks",
            ] {
                tile.insert(k.into(), row.get(k).cloned().unwrap_or(Value::Null));
            }
            let mine = per.get(&sid).cloned().unwrap_or(json!({}));
            tile.insert(
                "cost_usd".into(),
                mine.get("cost_usd").cloned().unwrap_or(Value::Null),
            );
            tile.insert(
                "context_pct".into(),
                row.get("usage")
                    .and_then(|u| u.get("context_pct"))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            let (items, error) = if truthy(row.get("remote")) {
                // The tail only, over SSH on every poll: the replies a tile
                // keeps are recent anyway (MATRIX_REMOTE_ITEMS).
                let got = self.remote_conversation(row, 150);
                (
                    got.get("items").and_then(Value::as_array).cloned().unwrap_or_default(),
                    got.get("error").filter(|e| truthy(Some(e))).map(crate::pyish::py_str),
                )
            } else {
                let got = match self.conversation(&sid) {
                    Ok(c) => (c.merged.clone(), None),
                    Err(e) => (vec![], Some(format!("RuntimeError: {e}"))),
                };
                tile.insert(
                    "tokens".into(),
                    mine.get("tokens").cloned().unwrap_or(Value::Null),
                );
                got
            };
            let live = crate::matrix::working(row) || truthy(row.get("attention"));
            tile.extend(crate::matrix::focus(&items, live));
            if let Some(e) = error {
                tile.insert("error".into(), json!(e));
            }
            tiles.push(tile);
        }
        let mut spend = crate::matrix::spend(&tiles, &limits);
        spend.insert("plan".into(), account["plan"].clone());
        spend.insert("global".into(), spent["global"].clone());
        spend.insert("models".into(), spent["current"]["models"].clone());
        spend.insert(
            "complete".into(),
            spent.get("complete").cloned().unwrap_or(json!(true)),
        );
        let always = truthy(Some(&settings::value(&self.config(), "chat.matrix_status")));
        json!({"tiles": tiles, "now": now, "spend": spend, "status_always": always})
    }

    /// Control.invalidate: the next look rebuilds the snapshot, and the
    /// window's event streams are woken to look.
    pub(super) fn invalidate(&self) {
        *self.snap.lk() = None;
        *self.changes.lk() += 1;
        self.changed.notify_all();
    }

    fn generation(&self) -> u64 {
        *self.changes.lk()
    }

    /// Control.wait_change: until something invalidates after `seen`, or
    /// `timeout` seconds.
    fn wait_change(&self, seen: u64, timeout: f64) {
        let g = self.changes.lk();
        let _ = self
            .changed
            .wait_timeout_while(g, std::time::Duration::from_secs_f64(timeout), |g| *g == seen);
    }

    // A POST, with the dispatcher's checks: no foreign Origin, a JSON body
    // of 1..MAX_BODY bytes.
    // The request's parts, as Handler reads them off the request.

    /// Control.sessions(max_age): the rows, cached briefly.
    pub fn sessions(&self, max_age: f64) -> Arc<Vec<Row>> {
        self.snapshot(max_age).1
    }

    /// (when the snapshot was taken, its rows). As registry.snapshot,
    /// `at` is read before `claude agents` runs.
    pub fn snapshot(&self, max_age: f64) -> (f64, Arc<Vec<Row>>) {
        let s = self.snap_of(max_age);
        (s.at, Arc::clone(&s.rows))
    }

    /// The cached snapshot if it is younger than `max_age` and no
    /// invalidate() came since it was begun; else one built now. Builds are
    /// single-flight: whoever waited on another's build takes its rows.
    fn snap_of(&self, max_age: f64) -> Arc<Snap> {
        let fresh = |max_age: f64| {
            let s = self.snap.lk().clone()?;
            (Self::now() - s.at < max_age && s.generation == self.generation()).then_some(s)
        };
        if let Some(s) = fresh(max_age) {
            return s;
        }
        let _building = self.snap_build.lk();
        if let Some(s) = fresh(max_age) {
            return s;
        }
        let generation = self.generation();
        let now = Self::now();
        let rows = Arc::new(self.build_snapshot());
        let s = Arc::new(Snap {
            at: now,
            generation,
            rows,
            public: std::sync::OnceLock::new(),
        });
        *self.snap.lk() = Some(Arc::clone(&s));
        s
    }

    /// The snapshot's public JSON, serialised once per snapshot.
    fn public_snap(&self, max_age: f64) -> Arc<PublicSnap> {
        let s = self.snap_of(max_age);
        Arc::clone(s.public.get_or_init(|| {
            let mut body = json!({"at": s.at, "sessions": public(&s.rows)});
            let data = body.to_string();
            if let Some(o) = body.as_object_mut() {
                o.shift_remove("at");
            }
            Arc::new(PublicSnap {
                data,
                same: json_dumps_sorted(&body),
            })
        }))
    }

    fn build_snapshot(&self) -> Vec<Row> {
        let st = self.state();
        let creds = registry::credentials(&st);
        let mut rows = registry::merge(
            &registry::agents(std::time::Duration::from_secs(20)),
            &registry::session_files(),
            &creds,
        );
        // Ask's own `claude -p` (server/fleet.rs) is not a session of yours.
        rows.retain(|r| !self.is_ask_row(r));
        procs::annotate(&mut rows);
        let empty = Map::new();
        let known = st
            .get("sessions")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let mut titled = Vec::new();
        for (i, row) in rows.iter_mut().enumerate() {
            let sid = row
                .get("sessionId")
                .map(crate::pyish::py_str)
                .unwrap_or_default();
            let Some(Value::Object(extra)) = known.get(&sid) else {
                continue;
            };
            if extra.is_empty() {
                continue;
            }
            // Only a failure needs the transcript: did it get past it?
            let past = if extra.get("attention_kind").and_then(Value::as_str) == Some("failure") {
                self.stats.read(&sid, Self::now()).as_ref().and_then(moved_past)
            } else {
                None
            };
            if overlay(row, extra, past) {
                if let Some(store) = &self.store {
                    store.clear_attention(&sid);
                }
            }
            titled.push(i);
        }
        let ids: Vec<String> = titled
            .iter()
            .filter_map(|&i| {
                rows[i]
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        if let Value::Object(found) = sessions::titles_json(&ids) {
            for &i in &titled {
                let sid = rows[i]
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                rows[i].insert(
                    "window_title".into(),
                    found.get(&sid).cloned().unwrap_or(json!("")),
                );
            }
        }
        rows.extend(self.remote_with_hooks(known, &creds));
        let queue = st
            .get("queue")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for row in rows.iter_mut() {
            self.decorate(row, &queue);
        }
        let dismissed = st
            .get("dismissed")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        // _undismissed: a dismissed session that has a process again was
        // resumed, so it comes back - and stays back once that process ends.
        let mut resumed = Vec::new();
        rows.retain(|r| {
            let key = r.get("key").and_then(Value::as_str).unwrap_or("");
            if !dismissed.contains_key(key) {
                return true;
            }
            if truthy(r.get("pid")) {
                resumed.push(key.to_string());
                return true;
            }
            false
        });
        if let Some(store) = &self.store {
            for key in &resumed {
                store.undismiss(key);
            }
        }
        let hours = get(&self.config(), "list.hide_finished_hours")
            .filter(|v| v.is_number())
            .and_then(Value::as_f64)
            .filter(|h| *h > 0.0)
            .unwrap_or(0.0);
        if hours > 0.0 {
            let cutoff = Self::now() - hours * 3600.0;
            rows.retain(|r| !(registry::finished(r) && finished_at(r) < cutoff));
        }
        for r in rows.iter_mut() {
            registry::classify(r);
        }
        if let Some(store) = &self.store {
            registry::apply_reviewed(&mut rows, &store.reviewed());
        }
        rows
    }

    /// Control._remote_with_hooks: remote rows, with what that host's hooks
    /// reported folded in - kept under host-qualified keys, so nothing a
    /// host posts can overwrite, forget or re-point a session here.
    fn remote_with_hooks(
        &self,
        known: &Map<String, Value>,
        creds: &HashMap<String, (Value, Value)>,
    ) -> Vec<Row> {
        let Some(fleet) = &self.fleet else {
            return vec![];
        };
        let mut out = Vec::new();
        for mut row in fleet.rows() {
            let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
            let sid = row.get("sessionId").map(crate::pyish::py_str).unwrap_or_default();
            let key = remote_key(&host, &sid);
            let extra = known
                .get(&key)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if !truthy(row.get("socket")) && truthy(extra.get("socket")) {
                row.insert("socket".into(), extra["socket"].clone());
            }
            // registry.attach_credentials, for this one row.
            if let Some((token, recorded)) = creds.get(&key) {
                if truthy(Some(token)) {
                    let live = row.get("socket").cloned().unwrap_or(Value::Null);
                    if truthy(Some(recorded)) && truthy(Some(&live)) && *recorded == live {
                        row.insert("token".into(), token.clone());
                    } else {
                        row.insert("stale_credential".into(), json!(true));
                    }
                }
            }
            let d = truthy(row.get("socket")) && truthy(row.get("token"));
            row.insert("dispatchable".into(), json!(d));
            if !extra.is_empty() && overlay(&mut row, &extra, None) {
                if let Some(store) = &self.store {
                    store.clear_attention(&key);
                }
            }
            out.push(row);
        }
        out
    }

    /// Control._decorate, less what only the Python dispatcher knows.
    fn decorate(&self, row: &mut Row, queue: &Map<String, Value>) {
        let sid = row
            .get("sessionId")
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let remote = truthy(row.get("remote"));
        let key = if remote {
            remote_key(&row.get("host").map(crate::pyish::py_str).unwrap_or_default(), &sid)
        } else {
            sid.clone()
        };
        row.insert("key".into(), json!(key));
        let state = row.get("state").and_then(Value::as_str).map(str::to_string);
        let ended = matches!(
            state.as_deref(),
            Some("done") | Some("failed") | Some("stopped")
        );
        let current = self.live.current(&key, Self::now());
        if let Value::Object(c) = &current {
            let summary = activity::first_line(c.get("summary"));
            let tool = c
                .get("tool")
                .map(crate::pyish::py_str)
                .unwrap_or_else(|| "None".into());
            row.insert("doing".into(), json!(format!("{tool} {summary}")));
            row.insert(
                "doing_since".into(),
                c.get("since").cloned().unwrap_or(Value::Null),
            );
        } else if !ended && !remote {
            let line = self.doing_line(&sid);
            row.insert("doing".into(), json!(line));
        }
        if let Some(got) = self.usage.session(&key) {
            let mut u = Map::new();
            for k in [
                "context_pct",
                "context_size",
                "cost_usd",
                "model",
                "at",
                "duration_ms",
                "api_duration_ms",
                "lines_added",
                "lines_removed",
            ] {
                u.insert(k.into(), got.get(k).cloned().unwrap_or(Value::Null));
            }
            row.insert("usage".into(), Value::Object(u));
        }
        if !truthy(row.get("remote")) {
            if let Some(cwd) = row.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty()) {
                if let Some(pr) = self.prs.get(cwd).filter(|p| truthy(Some(p))) {
                    row.insert("pr".into(), pr);
                }
            }
        }
        if let Some(Value::Array(q)) = queue.get(&key) {
            if !q.is_empty() {
                row.insert("queued".into(), json!(q.len()));
            }
        }
        if !truthy(row.get("remote")) {
            if let Some(link) = &self.modlink {
                if let Some(m) = link.state(&sid, Self::now()) {
                    // The mod reaches the session without its socket, and
                    // knows the moment a turn starts or ends.
                    let pick = |k: &str| m.get(k).cloned().unwrap_or(Value::Null);
                    row.insert("mod".into(), json!({"busy": pick("busy"), "since": pick("since"),
                                                    "version": pick("version"), "mod_version": pick("mod_version")}));
                    row.insert("dispatchable".into(), json!(true));
                    let status = match row.get("status") {
                        None | Some(Value::Null) => Some(""),
                        Some(v) => v.as_str(),
                    };
                    if matches!(status, Some("") | Some("idle") | Some("busy"))
                        && !truthy(row.get("attention"))
                    {
                        let busy = truthy(m.get("busy"));
                        row.insert("status".into(), json!(if busy { "busy" } else { "idle" }));
                    }
                }
                let n = link.pending(&sid).len();
                if n > 0 {
                    row.insert("mod_pending".into(), json!(n));
                }
            }
        }
        let bg_idle = row.get("kind").and_then(Value::as_str) == Some("background")
            && !truthy(row.get("pid"))
            && !truthy(row.get("status"));
        if ended || bg_idle {
            row.insert("resumable".into(), json!(true));
        }
        row.insert(
            "resume_command".into(),
            json!(format!("claude --resume {sid}")),
        );
        // A remote row's numbers were read on its host by the poll that
        // found it (remote.Stats).
        let stat = if remote {
            Some(row.remove("stat").unwrap_or_else(|| json!({})))
        } else {
            None
        };
        let work = self.work(row, stat);
        row.insert("work".into(), work);
    }

    /// Control._doing_line: activity.doing over the last 8 messages,
    /// remembered for DOING_EVERY (10 s).
    fn doing_line(&self, sid: &str) -> String {
        let now = Self::now();
        let hit = self.doing.lk().get(sid).cloned();
        if let Some((at, line)) = &hit {
            if now - at < 10.0 {
                return line.clone();
            }
        }
        let line = match self.conversation(sid) {
            Ok(c) => {
                let start = c.raw.len().saturating_sub(8);
                activity::doing(&conversation::merge_turns(c.raw[start..].to_vec()))
            }
            Err(_) => hit.map(|h| h.1).unwrap_or_default(),
        };
        self.doing
            .lk()
            .insert(sid.to_string(), (now, line.clone()));
        line
    }

    /// Control._work: what the tile shows at a glance.
    fn work(&self, row: &Row, stat: Option<Value>) -> Value {
        let sid = row
            .get("sessionId")
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let stat = stat.unwrap_or_else(|| {
            self.stats
                .read(&sid, Self::now())
                .unwrap_or_else(|| json!({}))
        });
        let alive =
            row.get("pid").is_some_and(|p| p.is_u64() || p.is_i64()) && truthy(row.get("status"));
        let cache = stat.get("cache").cloned().unwrap_or_else(|| json!({}));
        let mut out = Map::new();
        out.insert(
            "cache_expires_at".into(),
            cache.get("expires_at").cloned().unwrap_or(Value::Null),
        );
        out.insert(
            "context_tokens".into(),
            stat.get("context_tokens").cloned().unwrap_or(Value::Null),
        );
        out.insert(
            "cold_tokens".into(),
            get(&self.config(), "chat.cold_cache_tokens")
                .cloned()
                .unwrap_or(json!(0)),
        );
        out.insert(
            "resume_cost_usd".into(),
            costs::rewrite_cost(
                stat.get("model_id"),
                stat.get("context_tokens"),
                cache.get("ttl"),
            ),
        );
        out.insert("alive".into(), json!(alive));
        // The session this one was forked from, so the list can nest it.
        if let Some(p) = stat.get("forked_from").filter(|p| p.is_string()) {
            out.insert("forked_from".into(), p.clone());
        }
        if alive {
            for name in ["agents", "shells", "monitors", "wakeups"] {
                let n = stat
                    .get(name)
                    .and_then(Value::as_array)
                    .map(|a| a.len())
                    .unwrap_or(0);
                out.insert(name.into(), json!(n));
            }
            let soonest = stat
                .get("wakeups")
                .and_then(Value::as_array)
                .map(|w| {
                    w.iter()
                        .filter_map(|x| x.get("fires_at").and_then(Value::as_f64))
                        .filter(|f| *f != 0.0)
                        .fold(None, |m: Option<f64>, f| Some(m.map_or(f, |m| m.min(f))))
                })
                .unwrap_or(None);
            out.insert(
                "wakes_at".into(),
                soonest.map(|f| json!(f)).unwrap_or(Value::Null),
            );
            let n = row
                .get("shell_processes")
                .filter(|v| truthy(Some(v)))
                .cloned()
                .unwrap_or(json!(0));
            out.insert("processes".into(), n);
        }
        Value::Object(out)
    }

    /// GET /api/sessions: the snapshot as it may leave the process.
    pub(super) fn api_sessions(&self) -> String {
        // The Stream Deck polls once a second; a snapshot STREAM_EVERY old
        // is as good for a key colour and saves a fork.
        self.public_snap(stream_every()).data.clone()
    }

    /// GET /api/stream: server-sent events, the session list whenever it
    /// changes - looked at every STREAM_EVERY seconds, never more often
    /// than STREAM_MIN_GAP, a keepalive after STREAM_KEEPALIVE of quiet.
    /// HTTP/1.0 and no length, as the Python handler answers.
    ///
    /// Unlike Python it never ends the stream itself: it runs until a write
    /// fails, which the keepalive makes happen within STREAM_KEEPALIVE of
    /// the page going away. Python closes after 600 s and the page
    /// reconnects, but tiny_http cannot close a keep-alive connection:
    /// dropping the writer leaves the socket open for a next request, so
    /// the browser would wait on a stream that never ends and never says
    /// anything again - the window froze ten minutes after each load
    /// (2026-11-13).
    ///
    /// As in Python, it is also woken early by whatever invalidates the
    /// snapshot (a hook event, a remote poll landing), then looks once
    /// STREAM_MIN_GAP has passed. Every stream sends the same snapshot's
    /// JSON, serialised once.
    ///
    /// `/api/stream?matrix=1` is the Chat Matrix's: not the session list but
    /// `event: matrix`, the whole of /api/matrix, sent once on connecting and
    /// again whenever it changes (see `stream_matrix`). Python answers it
    /// with the session list, which the matrix page does not count as a push
    /// and so keeps polling.
    pub fn stream(&self, out: &mut dyn std::io::Write, matrix: bool) {
        let head = "HTTP/1.0 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\n\
                    Cache-Control: no-store\r\n\r\n";
        if out
            .write_all(head.as_bytes())
            .and_then(|_| out.flush())
            .is_err()
        {
            return;
        }
        if matrix {
            return self.stream_matrix(out);
        }
        let (mut quiet, mut looked) = (Self::now(), 0.0);
        let mut last: Option<Arc<PublicSnap>> = None;
        loop {
            let gap = stream_min_gap() - (Self::now() - looked);
            if gap > 0.0 {
                std::thread::sleep(std::time::Duration::from_secs_f64(gap));
            }
            looked = Self::now();
            let seen = self.generation();
            let snap = self.public_snap(stream_every());
            let moved = last
                .as_ref()
                .is_none_or(|l| !Arc::ptr_eq(l, &snap) && l.same != snap.same);
            let sent = if moved {
                let r = write!(out, "event: sessions\ndata: {}\n\n", snap.data)
                    .and_then(|_| out.flush());
                last = Some(snap);
                quiet = Self::now();
                r
            } else if Self::now() - quiet >= stream_keepalive() {
                // A named event, not a `:` comment (2026-11-21): a page cannot see
                // comments, so it could not tell a quiet stream from a dead one.
                quiet = Self::now();
                out.write_all(b"event: ping\ndata: {}\n\n").and_then(|_| out.flush())
            } else {
                Ok(())
            };
            if sent.is_err() {
                return; // the page went away
            }
            self.wait_change(seen, stream_every());
        }
    }

    /// The matrix's stream: what its page would have polled for every
    /// MATRIX_EVERY seconds, sent only when it differs from the last send
    /// (`now` aside, which the page does not read). The same work as the
    /// poll, minus the requests, the parsing and the redraws when nothing
    /// has moved.
    fn stream_matrix(&self, out: &mut dyn std::io::Write) {
        let (mut last, mut quiet) = (String::new(), Self::now());
        loop {
            let mut body = self.api_matrix();
            if let Some(o) = body.as_object_mut() {
                o.remove("now");
            }
            let same = json_dumps_sorted(&body);
            let sent = if same != last {
                last = same;
                quiet = Self::now();
                write!(out, "event: matrix\ndata: {body}\n\n").and_then(|_| out.flush())
            } else if Self::now() - quiet >= stream_keepalive() {
                quiet = Self::now();
                out.write_all(b": keepalive\n\n").and_then(|_| out.flush())
            } else {
                Ok(())
            };
            if sent.is_err() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs_f64(matrix_every()));
        }
    }

    /// Control._row(id): the row whose sessionId or key is `id`.
    pub(super) fn row(&self, id: &str) -> Option<Row> {
        let hit = |r: &Row| {
            r.get("sessionId").and_then(Value::as_str) == Some(id)
                || r.get("key").and_then(Value::as_str) == Some(id)
        };
        self.sessions(2.0).iter().find(|r| hit(r)).cloned()
    }
}

/// dispatcher.remote_key: where a remote session's hook records live.
pub(super) fn remote_key(host: &str, sid: &str) -> String {
    format!("{host}/{sid}")
}

/// One session snapshot: when it was taken, which invalidate() it
/// followed, its rows, and their public JSON, made at most once however
/// many streams and polls send it.
pub(super) struct Snap {
    at: f64,
    generation: u64,
    rows: Arc<Vec<Row>>,
    public: std::sync::OnceLock<Arc<PublicSnap>>,
}

/// A snapshot as it leaves the process: `data` is `{"at", "sessions"}` as
/// sent, `same` the sessions alone, sorted, to tell whether anything moved.
struct PublicSnap {
    data: String,
    same: String,
}

fn stream_every() -> f64 { crate::timing::t().stream_every }

fn stream_min_gap() -> f64 { crate::timing::t().stream_min_gap }

fn stream_keepalive() -> f64 { crate::timing::t().stream_keepalive }
fn matrix_every() -> f64 { crate::timing::t().matrix_every }

/// matrix.active: whether a session gets a tile - its process is up and it
/// has not ended.
pub(super) fn matrix_active(row: &Row) -> bool {
    truthy(row.get("status"))
        && !matches!(
            row.get("state").and_then(Value::as_str),
            Some("done") | Some("failed") | Some("stopped")
        )
}

/// Row fields that never leave the process (dispatcher.PRIVATE_ROW_FIELDS).
const PRIVATE_ROW_FIELDS: &[&str] = &["token"];

fn public(rows: &[Row]) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            let mut r = r.clone();
            for k in PRIVATE_ROW_FIELDS {
                r.shift_remove(*k);
            }
            Value::Object(r)
        })
        .collect()
}

/// dispatcher.finished_at: a finished job's last hook event, else its start.
fn finished_at(row: &Row) -> f64 {
    let num = |k: &str| {
        row.get(k)
            .filter(|v| v.is_number())
            .and_then(Value::as_f64)
            .filter(|v| *v > 0.0)
    };
    num("last_event_at")
        .or_else(|| num("startedAt").map(|s| s / 1000.0))
        .unwrap_or(0.0)
}

/// When a transcript summary last showed the session going on: the later of
/// its newest prompt and its last real reply.
fn moved_past(stat: &Value) -> Option<f64> {
    ["last_prompt_at", "last_reply_at"]
        .iter()
        .filter_map(|k| stat.get(*k).and_then(Value::as_f64))
        .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
}

/// Control._overlay: fold what hooks recorded into a row. Read-only here:
/// where Python would also clear stale attention in the store, this only
/// leaves it off the row (Python's next snapshot does the same).
/// Control._overlay. True when the recorded attention is settled and the
/// store should clear it (Python clears it right there).
///
/// `past` is when the session's transcript last showed it going on - a new
/// prompt or a real reply (sessionstat's `last_prompt_at`/`last_reply_at`).
fn overlay(row: &mut Row, extra: &Map<String, Value>, past: Option<f64>) -> bool {
    for field in [
        "socket",
        "name",
        "cwd",
        "last_notification",
        "last_event",
        "last_event_at",
        "last_failure",
        "last_failure_detail",
        "window_title",
        "hc",
        // What the last Stop hook said the session still waits on.
        "stop_background_tasks",
        "stop_crons",
    ] {
        if truthy(extra.get(field)) && !truthy(row.get(field)) {
            row.insert(field.into(), extra[field].clone());
        }
    }
    let dispatchable = |row: &Row| truthy(row.get("socket")) && truthy(row.get("token"));
    if registry::finished(row) {
        // A finished background job waits for nobody, whatever its last
        // hook said.
        let d = dispatchable(row);
        row.insert("dispatchable".into(), json!(d));
        return truthy(extra.get("attention_since"));
    }
    let since = extra.get("attention_since").filter(|v| truthy(Some(v)));
    let kind = extra.get("attention_kind").and_then(Value::as_str);
    let failure = kind == Some("failure");
    // A failure is not settled by the session merely working again (the
    // retry after a 529 would wipe it), but it is by a prompt or a real reply
    // after it: the user went on (a /login reruns the failed prompt with no
    // new one; its reply follows), or the API answered. 2026-11-18: "login
    // required" stayed up until the rerun's turn ended.
    let stale = if failure {
        let failed_at = ["last_failure_at", "attention_since"]
            .iter()
            .find_map(|k| extra.get(*k).and_then(Value::as_f64).filter(|v| *v > 0.0));
        matches!((failed_at, past), (Some(f), Some(p)) if p > f)
    } else {
        registry::moved_on(row, since.and_then(Value::as_f64))
    };
    let mut clear = false;
    if since.is_some() && stale {
        clear = true;
    } else if since.is_some()
        && !failure
        && registry::still_running(row, extra.get("last_notification"))
    {
        // hidden while the work goes on
    } else if let Some(since) = since {
        row.insert("attention_since".into(), since.clone());
        row.insert("attention".into(), json!(true));
        row.insert(
            "attention_reason".into(),
            extra
                .get("attention_reason")
                .cloned()
                .unwrap_or(Value::Null),
        );
        row.insert(
            "attention_kind".into(),
            extra.get("attention_kind").cloned().unwrap_or(Value::Null),
        );
    }
    let d = dispatchable(row);
    row.insert("dispatchable".into(), json!(d));
    clear
}

#[cfg(test)]
mod overlay_tests {
    use super::*;

    fn obj(v: Value) -> Row {
        v.as_object().unwrap().clone()
    }

    /// The store's record after a "login required" StopFailure at 1000.
    fn failed() -> Row {
        obj(json!({"attention_since": 1000.0, "attention_kind": "failure",
                   "attention_reason": "login required - run /login",
                   "last_failure": "authentication_failed", "last_failure_at": 1000.0}))
    }

    #[test]
    fn a_failure_stays_while_the_session_only_works_again() {
        // The retry after a 529, or a /login's rerun before its reply.
        let mut row = obj(json!({"status": "busy", "status_at_change": "busy",
                                 "status_changed_at": 1500.0}));
        assert!(!overlay(&mut row, &failed(), None));
        assert_eq!(row["attention"], true);
        assert_eq!(row["attention_kind"], "failure");
        // what the transcript showed before the failure proves nothing
        let mut row = obj(json!({"status": "busy"}));
        assert!(!overlay(&mut row, &failed(), Some(990.0)));
        assert_eq!(row["attention"], true);
    }

    #[test]
    fn a_prompt_or_reply_after_the_failure_clears_it() {
        let mut row = obj(json!({"status": "busy"}));
        assert!(overlay(&mut row, &failed(), Some(1010.0)));
        assert!(!row.contains_key("attention"));
        // A record from before last_failure_at goes by the attention clock.
        let mut old = failed();
        old.shift_remove("last_failure_at");
        let mut row = obj(json!({"status": "idle"}));
        assert!(overlay(&mut row, &old, Some(1010.0)));
        // A permission wait showing since 500, then the failure at 1000: a
        // prompt at 800 was before the failure.
        let mut both = failed();
        both.insert("attention_since".into(), json!(500.0));
        let mut row = obj(json!({"status": "idle"}));
        assert!(!overlay(&mut row, &both, Some(800.0)));
        assert_eq!(row["attention"], true);
    }

    #[test]
    fn moved_past_is_the_later_of_prompt_and_reply() {
        assert_eq!(moved_past(&json!({"last_prompt_at": 5.0, "last_reply_at": 9.0})), Some(9.0));
        assert_eq!(moved_past(&json!({"last_prompt_at": 5.0})), Some(5.0));
        assert_eq!(moved_past(&json!({})), None);
    }
}
