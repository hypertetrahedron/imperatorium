//! Conversations: the parsed-transcript cache and the views of one conversation (full, history, search, timeline, subagents).

use super::*;

impl Shared {
    /// convcache._stamp: stat the remembered path; look again (and
    /// remember) only when there is none or it has gone.
    fn stamp(&self, sid: &str) -> Option<(PathBuf, u64, u128)> {
        let known = self.paths.lk().get(sid).cloned();
        if let Some(path) = known {
            if let Ok(meta) = std::fs::metadata(&path) {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                return Some((path, meta.len(), mtime));
            }
        }
        let found = transcript_stamp(sid)?;
        self.paths
            .lk()
            .insert(sid.to_string(), found.0.clone());
        Some(found)
    }

    /// convcache: the per-message items and the merged conversation,
    /// parsed again only when the transcript's size or mtime moves.
    pub(super) fn conversation(&self, sid: &str) -> Result<Arc<Cached>, String> {
        let stamp = self.stamp(sid);
        if let Some(hit) = self.cached(sid, stamp.as_ref()) {
            return Ok(hit);
        }
        // One parse per session at a time: the window, the matrix and a
        // stream polling a just-changed transcript together wait for the
        // first and take what it parsed (convcache._filling).
        let fill = Arc::clone(self.filling.lk().entry(sid.to_string()).or_default());
        let _filling = fill.lk();
        let stamp = self.stamp(sid);
        if let Some(hit) = self.cached(sid, stamp.as_ref()) {
            return Ok(hit);
        }
        let result = self.parse_and_keep(sid, stamp);
        // The last one out takes the lock away (the map holds one, each
        // waiter one, this guard's Arc one).
        {
            let mut filling = self.filling.lk();
            if filling.get(sid).is_some_and(|f| Arc::strong_count(f) <= 2) {
                filling.remove(sid);
            }
        }
        result
    }

    /// The cached conversation, when it is of this transcript as it is now.
    fn cached(&self, sid: &str, stamp: Option<&(PathBuf, u64, u128)>) -> Option<Arc<Cached>> {
        let st = stamp?;
        let mut convs = self.convs.lk();
        let i = convs.get_index_of(sid)?;
        if &convs[i].stamp != st {
            return None;
        }
        // Used now: to the back of the eviction order.
        let last = convs.len() - 1;
        convs.move_index(i, last);
        Some(Arc::clone(&convs[last]))
    }

    /// The budget, in bytes of merged-conversation JSON (chat.conversation_cache_mb).
    fn conv_budget(&self) -> usize {
        let mb = get(&self.config(), "chat.conversation_cache_mb")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(CONVS_BUDGET_MB as f64);
        (mb * 1024.0 * 1024.0) as usize
    }

    fn parse_and_keep(&self, sid: &str, stamp: Option<(PathBuf, u64, u128)>) -> Result<Arc<Cached>, String> {
        self.parses.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let raw = Self::read_items(sid)?;
        let merged = conversation::merge_turns_ref(&raw);
        let sig = signature(&merged);
        let merged_json = serde_json::to_string(&merged).map_err(|e| e.to_string())?;
        let entry = Arc::new(Cached {
            stamp: stamp.clone().unwrap_or_default(),
            raw,
            merged,
            merged_json,
            sig,
        });
        if stamp.is_some() {
            let budget = self.conv_budget();
            let mut convs = self.convs.lk();
            convs.shift_remove(sid);
            convs.insert(sid.to_string(), Arc::clone(&entry));
            evict(&mut convs, CONVS_KEPT, budget);
        }
        Ok(entry)
    }

    /// A session's per-message items, straight from its transcript.
    fn read_items(sid: &str) -> Result<Vec<Value>, String> {
        let chain = match sessions::chain_json(sid)? {
            Value::Array(a) => a,
            _ => vec![],
        };
        Ok(conversation::to_items(&conversation::messages(&chain), 0, false))
    }

    /// The merged conversation's plain text, for a History search: from the
    /// cache when it holds this transcript as it is, else read - and not
    /// remembered, so a search over hundreds of sessions does not push out
    /// the ones the window is showing. None when it cannot be read.
    pub(super) fn search_text(&self, sid: &str) -> Option<String> {
        let stamp = self.stamp(sid);
        if let Some(st) = &stamp {
            let hit = self.convs.lk().get(sid).filter(|c| &c.stamp == st).cloned();
            if let Some(c) = hit {
                return Some(history::plain_text(&c.merged));
            }
        }
        let raw = Self::read_items(sid).ok()?;
        Some(history::plain_text(&conversation::merge_turns(raw)))
    }

    pub(super) fn api_conversation(&self, q: &HashMap<String, String>) -> (u16, Body) {
        let session = q.get("session").cloned().unwrap_or_default();
        if session.is_empty() {
            return (400, Body::Json(json!({"error": "session is required"})));
        }
        let int = |k: &str| {
            q.get(k)
                .and_then(|v| v.trim().parse::<i64>().ok())
                .unwrap_or(0)
                .max(0)
        };
        let limit = int("limit") as usize;
        let blocks = int("blocks");
        // _row(session, max_age=LOOKUP_AGE): a 15 s old snapshot will do,
        // and a fresh one (at most 2 s old) is asked only when it is missing.
        let hit = |rows: &[Row]| {
            rows.iter()
                .find(|r| {
                    r.get("sessionId").and_then(Value::as_str) == Some(session.as_str())
                        || r.get("id").and_then(Value::as_str) == Some(session.as_str())
                })
                .cloned()
        };
        let row = hit(&self.sessions(15.0)).or_else(|| hit(&self.sessions(2.0)));
        let sid = row
            .as_ref()
            .and_then(|r| r.get("sessionId").and_then(Value::as_str))
            .unwrap_or(&session)
            .to_string();
        let field = |k: &str| {
            row.as_ref()
                .and_then(|r| r.get(k))
                .cloned()
                .unwrap_or(Value::Null)
        };
        if let Some(r) = row.as_ref().filter(|r| truthy(r.get("remote"))) {
            // Read on the session's own machine (Handler: a remote body is
            // hashed here, and `blocks` cut from it as from a local one).
            let mut body = self.remote_conversation(r, limit);
            let items = body.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
            let sig = signature(&items);
            if q.get("known").map(String::as_str) == Some(sig.as_str()) {
                return (
                    200,
                    Body::Json(json!({"session": body.get("session").cloned().unwrap_or(Value::Null),
                                      "unchanged": true, "sig": sig})),
                );
            }
            if blocks > 0 {
                let (tail, start, first) = conversation::tail_blocks(&items, blocks);
                body.insert("total".into(), json!(items.len()));
                body.insert("items".into(), Value::Array(tail));
                body.insert("start".into(), json!(start));
                body.insert("first_block".into(), json!(first));
            }
            body.insert("sig".into(), json!(sig));
            return (200, Body::Json(Value::Object(body)));
        }
        let cached = self.conversation(&sid);
        // The poll's usual case - the whole conversation, changed - is
        // answered from the items as serialised once at fill time, spliced
        // into the body, rather than copied and serialised on every poll.
        if let Ok(c) = &cached {
            let known = q.get("known").map(String::as_str) == Some(c.sig.as_str());
            if limit == 0 && blocks == 0 && !known {
                let text = format!(
                    "{{\"session\":{},\"items\":{},\"sig\":{},\"name\":{},\"cwd\":{}}}",
                    json!(session),
                    c.merged_json,
                    json!(c.sig),
                    field("name"),
                    field("cwd")
                );
                return (200, Body::Raw(text));
            }
        }
        let got = cached.map(|c| {
            if limit > 0 {
                let start = c.raw.len().saturating_sub(limit);
                let items = conversation::merge_turns_ref(&c.raw[start..]);
                let sig = signature(&items);
                (items, sig)
            } else {
                (c.merged.clone(), c.sig.clone())
            }
        });
        // Built in the Python handler's key order: c.conversation's body,
        // then `total`/`start`/`first_block` for a tail, and `sig` set last
        // (which leaves it in place where it was already there).
        let mut body = Map::new();
        body.insert("session".into(), json!(session));
        let (items, sig) = match got {
            Ok((items, sig)) => {
                body.insert("items".into(), Value::Null); // placed now, filled below
                body.insert("sig".into(), json!(sig));
                body.insert("name".into(), field("name"));
                body.insert("cwd".into(), field("cwd"));
                (items, sig)
            }
            Err(e) => {
                body.insert("items".into(), json!([]));
                body.insert("error".into(), json!(format!("RuntimeError: {e}")));
                (vec![], signature(&[]))
            }
        };
        if q.get("known").map(String::as_str) == Some(sig.as_str()) {
            return (
                200,
                Body::Json(json!({"session": session, "unchanged": true, "sig": sig})),
            );
        }
        if blocks > 0 {
            let (tail, start, first) = conversation::tail_blocks(&items, blocks);
            body.insert("items".into(), Value::Array(tail));
            body.insert("total".into(), json!(items.len()));
            body.insert("start".into(), json!(start));
            body.insert("first_block".into(), json!(first));
        } else {
            body.insert("items".into(), Value::Array(items));
        }
        body.insert("sig".into(), json!(sig));
        (200, Body::Json(Value::Object(body)))
    }

    /// Control._remote_conversation: the same items, read on the session's
    /// own machine by the host's channel.
    pub(super) fn remote_conversation(&self, row: &Row, limit: usize) -> Map<String, Value> {
        let f = |k: &str| row.get(k).cloned().unwrap_or(Value::Null);
        let mut out = json!({"session": f("sessionId"), "items": [], "remote": true,
                             "host": f("host"), "name": f("name"), "cwd": f("cwd")})
        .as_object()
        .unwrap()
        .clone();
        let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
        let Some(fleet) = self.fleet.as_ref().filter(|f| f.host(&host).is_some()) else {
            out.insert("error".into(), json!(format!("host {host} is not configured")));
            return out;
        };
        let sid = row.get("sessionId").map(crate::pyish::py_str).unwrap_or_default();
        let cwd = row.get("cwd").and_then(Value::as_str);
        match fleet.read(&host, &sid, limit, cwd) {
            Ok((items, title)) => {
                out.insert("items".into(), Value::Array(items));
                out.insert("title".into(), json!(title));
            }
            Err(e) => {
                out.insert("error".into(), json!(format!("reading on {host}: {e}")));
            }
        }
        out
    }

    // -- the session snapshot -------------------------------------------------

    /// Control.search: History, with which of its sessions are live.
    pub(super) fn api_history(&self, q: &HashMap<String, String>) -> Value {
        let query = q.get("q").cloned().unwrap_or_default();
        let read = |sid: &str| self.search_text(sid);
        // History's filters (2026-11-23): folder=<a project folder, its
        // worktrees included>, days=<active within>, sort=modified|created|
        // oldest|size|title. Narrowed before the limit, so a folder's
        // sessions are all there however many newer ones are elsewhere.
        let arg = |k: &str| q.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
        let filter = crate::history::Filter {
            folder: arg("folder").map(str::to_string),
            since: arg("days")
                .and_then(|d| d.parse::<f64>().ok())
                .filter(|d| *d > 0.0)
                .map(|d| Self::now() - d * 86400.0),
            sort: crate::history::Sort::parse(arg("sort").unwrap_or("")),
        };
        let mut got = self.history.search_with(&query, 60, &filter, &read);
        let live: std::collections::HashSet<String> = self
            .sessions(2.0)
            .iter()
            .filter(|r| truthy(r.get("pid")) || truthy(r.get("status")))
            .filter_map(|r| {
                r.get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        if let Some(Value::Array(results)) = got.get_mut("results") {
            for r in results.iter_mut() {
                let sid = r
                    .get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                r["live"] = json!(live.contains(&sid));
            }
        }
        got["ok"] = json!(true);
        got
    }

    /// A subagent of `session` as a chat (taskout.subagent_view), read on the
    /// session's own machine.
    pub(super) fn api_subagent(&self, session: &str, agent: &str) -> Value {
        let Some(row) = self.row(session) else {
            return json!({"ok": false, "error": "no such session"});
        };
        let sid = row.get("sessionId").map(crate::pyish::py_str).unwrap_or_default();
        if truthy(row.get("remote")) {
            let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
            return match self.fleet.as_ref().map(|f| f.subagent(&host, &sid, agent)) {
                Some(Ok(v)) => v,
                Some(Err(e)) => json!({"ok": false, "error": format!("reading on {host}: {e}")}),
                None => json!({"ok": false, "error": format!("host {host} is not configured")}),
            };
        }
        crate::taskout::subagent_view(&sid, agent, Self::now())
    }

    /// The session timeline (timeline.of_session), read on the session's own
    /// machine.
    pub(super) fn api_timeline(&self, session: &str, known: &str) -> Body {
        let Some(row) = self.row(session) else {
            return Body::Json(json!({"ok": false, "error": "no such session"}));
        };
        let sid = row.get("sessionId").map(crate::pyish::py_str).unwrap_or_default();
        if truthy(row.get("remote")) {
            let host = row.get("host").map(crate::pyish::py_str).unwrap_or_default();
            return Body::Json(match self.fleet.as_ref().map(|f| f.timeline(&host, &sid, known)) {
                Some(Ok(v)) => v,
                Some(Err(e)) => json!({"ok": false, "error": format!("reading on {host}: {e}")}),
                None => json!({"ok": false, "error": format!("host {host} is not configured")}),
            });
        }
        // Serialised from the timeline's cache as it stands, not a copy of it.
        let v = crate::timeline::of_session_shared(&sid, known);
        Body::Raw(serde_json::to_string(&*v).unwrap_or_else(|_| "{}".into()))
    }
}

pub(super) struct Cached {
    stamp: (PathBuf, u64, u128),
    pub(super) raw: Vec<Value>,
    pub(super) merged: Vec<Value>,
    /// `merged` serialised once, for the full-conversation answer.
    merged_json: String,
    sig: String,
}

/// convcache.signature: sha1 of json.dumps(items, sort_keys=True), 16 hex.
pub fn signature(items: &[Value]) -> String {
    let text = json_dumps_sorted(&Value::Array(items.to_vec()));
    let digest = Sha1::digest(text.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// sessionstat.transcript_path (the newest projects/*/<id>.jsonl, a miss
/// remembered for a moment so a poll for a session with no transcript yet
/// does not list every folder each time), with its size and mtime.
fn transcript_stamp(sid: &str) -> Option<(PathBuf, u64, u128)> {
    let p = sessionstat::transcript_path(sid)?;
    let meta = std::fs::metadata(&p).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some((p, meta.len(), mtime))
}

/// Parsed conversations kept, at most (convcache.KEEP).
const CONVS_KEPT: usize = 32;
/// chat.conversation_cache_mb's default (convcache.BUDGET: 32 MiB).
const CONVS_BUDGET_MB: u64 = 32;

/// Drop the least recently used until at most `keep` remain and their
/// weight (merged JSON bytes) is within `budget` - but never the newest,
/// however big it is.
fn evict(convs: &mut indexmap::IndexMap<String, Arc<Cached>>, keep: usize, budget: usize) {
    let mut total: usize = convs.values().map(|c| c.merged_json.len()).sum();
    while convs.len() > 1 && (convs.len() > keep || total > budget) {
        if let Some((_, gone)) = convs.shift_remove_index(0) {
            total -= gone.merged_json.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(weights: &[usize], keep: usize, budget: usize) -> Vec<String> {
        let mut convs = indexmap::IndexMap::new();
        for (i, w) in weights.iter().enumerate() {
            let c = Cached {
                stamp: Default::default(),
                raw: vec![],
                merged: vec![],
                merged_json: "x".repeat(*w),
                sig: String::new(),
            };
            convs.insert(format!("s{i}"), Arc::new(c));
        }
        evict(&mut convs, keep, budget);
        convs.keys().cloned().collect()
    }

    #[test]
    fn a_weight_budget_keeps_at_least_the_newest() {
        assert_eq!(kept(&[10, 5000], 32, 100), ["s1"]);
        assert_eq!(kept(&[5000], 32, 0), ["s0"]);
    }

    #[test]
    fn the_least_recently_used_go_first_until_within_budget() {
        assert_eq!(kept(&[40, 40, 40, 40], 32, 100), ["s2", "s3"]);
        assert_eq!(kept(&[40, 40, 40, 40], 32, 160), ["s0", "s1", "s2", "s3"]);
    }

    #[test]
    fn the_count_still_bounds_it() {
        assert_eq!(kept(&[1, 1, 1, 1], 2, usize::MAX), ["s2", "s3"]);
    }

    #[test]
    fn signature_matches_python() {
        // python -c "import hashlib,json; print(hashlib.sha1(json.dumps([{'b':1,'a':'é'}],
        //   sort_keys=True).encode()).hexdigest()[:16])"
        assert_eq!(
            signature(&[json!({"b": 1, "a": "\u{e9}"})]),
            "09e09a7b616b14d8"
        );
    }
}
