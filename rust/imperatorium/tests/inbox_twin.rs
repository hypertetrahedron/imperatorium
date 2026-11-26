//! Replays inbox_twin (answers frozen at `last-python`, see golden/README.md):
//! prompt delivery with no real session anywhere.
//! The made-up session records (golden/inbox_twin/records.json) point at
//! named pipes this test serves; each queued prompt, hook, parked permission
//! request, mod beat and check is sent in the recorded order and what the
//! binary answers, delivers to the pipes and keeps in state.json and
//! decisions.jsonl is compared with Python's.
//!
//! Time: the binary runs at IMPERATORIUM_TEST_TIME_SCALE = SCALE (the 15 s
//! queue sweep is then SCALE * 15 s). The parked requests' `wait` values that
//! only hold a request open are sent as 10 / SCALE (so they still hold 10 s);
//! the ones whose expiry is the point (0.3, 0.2, 1) are sent as recorded and
//! scaled by the binary; this test's pauses past them are scaled with a
//! floor. Everything else polls with a deadline.
#![cfg(windows)]

mod common;
#[path = "common/twins_a.rs"]
mod twins_a;
use common::*;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::time::Duration;
use twins_a::*;

const SCALE: f64 = 0.05;
/// A `wait` that holds a request open for 10 s whatever the scale.
const HOLD: f64 = 10.0 / SCALE;

const IDLE: &str = "aaaaaaaa-1111-4111-8111-111111111111";
const BUSY: &str = "bbbbbbbb-2222-4222-8222-222222222222";
const WAITS: &str = "dddddddd-4444-4444-8444-444444444444";
const LATE: &str = "eeeeeeee-5555-4555-8555-555555555555";
const BG: &str = "ffffffff-6666-4666-8666-666666666666";

struct Ctx<'a> {
    d: &'a Dispatcher,
    pipes: BTreeMap<&'static str, Pipe>,
    token: String,
    places: Places,
    tag: String,
}

impl Ctx<'_> {
    /// Placeholders put back: <tmp> (and the rest of Places), then <tag>.
    fn fill(&self, v: &Value) -> Value {
        fn tag(v: &Value, t: &str) -> Value {
            match v {
                Value::String(s) => Value::String(s.replace("<tag>", t)),
                Value::Array(a) => Value::Array(a.iter().map(|x| tag(x, t)).collect()),
                Value::Object(o) => {
                    Value::Object(o.iter().map(|(k, x)| (k.replace("<tag>", t), tag(x, t))).collect())
                }
                other => other.clone(),
            }
        }
        tag(&self.places.put(v), &self.tag)
    }

    /// What reached each pipe since the last call, once the `expect`ed
    /// writes are in and a moment has passed for a stray one.
    fn deliveries(&self, expect: &Map<String, Value>) -> Value {
        for (sid, want) in expect {
            let n = want.as_array().map(Vec::len).unwrap_or(0);
            wait_until(Duration::from_secs(10), || {
                (self.pipes[sid.as_str()].count() >= n).then_some(())
            });
        }
        std::thread::sleep(Duration::from_millis(60));
        let mut out = Map::new();
        for (sid, p) in &self.pipes {
            let got = p.take();
            if !got.is_empty() {
                out.insert(sid.to_string(), json!(got));
            }
        }
        Value::Object(out)
    }

    fn window(&self, path: &str, body: &Value) -> Value {
        let (c, a) = self.d.post_window(path, body, &self.token);
        json!([c, a])
    }

    fn plain(&self, path: &str, body: &Value) -> Value {
        let (c, a) = self.d.post(path, body);
        json!([c, a])
    }

    fn sessions(&self) -> Vec<Value> {
        self.d.get("/api/sessions").1["sessions"].as_array().cloned().unwrap_or_default()
    }

    fn row(&self, sid: &str) -> Value {
        self.sessions()
            .into_iter()
            .find(|r| r["sessionId"] == sid)
            .unwrap_or(Value::Null)
    }
}

fn pick(row: &Value, keys: &[&str]) -> Value {
    Value::Object(keys.iter().map(|k| (k.to_string(), row.get(*k).cloned().unwrap_or(Value::Null))).collect())
}

/// inbox_twin.py parks(): what each step answered.
fn parks(c: &Ctx) -> Value {
    let d = c.d;
    let mut out: Vec<Value> = Vec::new();
    let waiting = |n: usize| -> Vec<Value> {
        let pending = || d.get("/api/pending").1["pending"].as_array().cloned().unwrap_or_default();
        wait_until(Duration::from_secs(10), || {
            let p = pending();
            (p.len() >= n).then_some(p)
        })
        .unwrap_or_else(pending)
    };
    let decide = |out: &mut Vec<Value>, body: Value| {
        let (code, ans) = d.post("/api/decide", &body);
        out.push(json!(["decide", body["verdict"], [code, ans]]));
    };
    std::thread::scope(|sc| {
        let park = |body: Value| sc.spawn(move || d.post("/permission/park", &body));
        let join = |t: std::thread::ScopedJoinHandle<(u16, Value)>| {
            let (c, a) = t.join().unwrap();
            json!([c, a])
        };

        let bash = json!({"session_id": IDLE, "cwd": "C:\\w", "tool_name": "Bash", "_reason": "not on the list",
            "tool_input": {"command": "rm -rf build", "description": "clean"}, "wait": HOLD});
        let t = park(bash);
        let item = waiting(1)[0].clone();
        out.push(json!(["pending", item]));
        let id = item["id"].clone();
        decide(&mut out, json!({"id": id, "verdict": "maybe"}));
        decide(&mut out, json!({"id": "nope", "verdict": "allow"}));
        decide(&mut out, json!({"id": id, "verdict": "allow", "who": "deck"}));
        decide(&mut out, json!({"id": id, "verdict": "deny"}));
        out.push(json!(["park", join(t)]));

        let question = json!({"session_id": IDLE, "tool_name": "AskUserQuestion", "wait": 1, "wait_question": HOLD,
            "tool_input": {"questions": [
                {"question": "Which database?", "header": "DB", "multiSelect": false,
                 "options": [{"label": "Postgres", "description": "relational"}, {"label": ""}, "x"]},
                {"question": "Also?", "multiSelect": true, "options": [{"label": "Cache"}]}]}});
        let t = park(question);
        let item = waiting(1)[0].clone();
        out.push(json!(["pending", item]));
        let id = item["id"].clone();
        decide(&mut out, json!({"id": id, "verdict": "allow"}));
        decide(&mut out, json!({"id": id, "verdict": "allow", "answers": {"Which database?": "Postgres"}}));
        // Past `wait` (1 s, here SCALE s): wait_question holds it.
        std::thread::sleep(Duration::from_millis(300));
        decide(&mut out, json!({"id": id, "verdict": "allow",
            "answers": {"Which database?": " Postgres ", "Also?": "Cache", "extra": "x"}}));
        out.push(json!(["park", join(t)]));

        // Nobody answers: a silent ask, and it leaves the list.
        let (code, ans) = d.post("/permission/park", &json!({"session_id": IDLE, "tool_name": "Edit",
            "tool_input": {"file_path": "a.py"}, "wait": 0.3}));
        out.push(json!(["expired", [code, ans]]));
        out.push(json!(["after", d.get("/api/pending").1]));
        // Pass: released without a decision.
        let t = park(json!({"session_id": IDLE, "tool_name": "WebFetch", "tool_input": {"url": "https://x"}, "wait": HOLD}));
        let item = waiting(1)[0].clone();
        decide(&mut out, json!({"id": item["id"], "verdict": "pass"}));
        let r = join(t);
        out.push(json!(["pass", r, d.get("/api/pending").1]));
        // A background session has no terminal: wait_background, not wait.
        let t = park(json!({"session_id": BG, "tool_name": "Bash", "tool_input": {"command": "make"},
            "wait": 0.2, "wait_background": HOLD}));
        let item = waiting(1)[0].clone();
        std::thread::sleep(Duration::from_millis(250));
        decide(&mut out, json!({"id": item["id"], "verdict": "deny"}));
        out.push(json!(["background", join(t)]));
    });
    Value::Array(out)
}

/// inbox_twin.py mods(): the mod in the idle session.
fn mods(c: &Ctx) -> (Value, usize) {
    let d = c.d;
    let token_file = d.state.parent().unwrap().join("mod-token");
    let token = wait_until(Duration::from_secs(10), || {
        std::fs::read_to_string(&token_file).ok().filter(|t| !t.trim().is_empty())
    })
    .expect("mod-token")
    .trim()
    .to_string();
    let work = d.root.join("work").to_string_lossy().into_owned();
    let mut out: Vec<Value> = Vec::new();
    let post_mod = |path: &str, body: Option<&Value>, tok: Option<&str>, raw: Option<&[u8]>| -> Value {
        let bytes = raw.map(|r| r.to_vec()).unwrap_or_else(|| serde_json::to_vec(body.unwrap()).unwrap());
        let headers: Vec<(&str, &str)> = tok.map(|t| vec![("X-Imperatorium-Mod", t)]).unwrap_or_default();
        let (code, ans) = d.request(path, Some(bytes), Some("application/json"), &headers);
        json!([code, ans])
    };
    let usage = json!({"context": {"percent": 31.5, "window": 200000}, "cost": {"usd": 2.5},
        "model": "Opus 5.5", "rateLimits": [
            {"kind": "five_hour", "percentUsed": 85, "resetsAt": "2100-01-01T00:00:00.000Z"},
            {"kind": "seven_day", "percentUsed": 20, "resetsAt": "2100-01-03T12:00:00+02:00"},
            {"kind": "odd", "percentUsed": "x"}]});
    let beat = json!({"session_id": IDLE, "cwd": work, "busy": false, "version": "2.1.300",
        "mod_version": "0.3.0", "interactive": true, "surfaces": ["terminal"], "usage": usage});
    let b = |out: &mut Vec<Value>, extra: Option<Value>, tok: Option<&str>| -> Value {
        let mut body = beat.clone();
        if let Some(Value::Object(e)) = extra {
            body.as_object_mut().unwrap().extend(e);
        }
        let got = post_mod("/api/mod/beat", Some(&body), tok, None);
        out.push(json!(["beat", got]));
        got[1].clone()
    };
    b(&mut out, None, None);
    b(&mut out, None, Some(&"x".repeat(43)));
    let raw = post_mod("/api/mod/beat", None, Some(&token), Some(b"[1]"));
    out.push(json!(["raw", raw]));
    let nosid = post_mod("/api/mod/beat", Some(&json!({"busy": true})), Some(&token), None);
    out.push(json!(["no sid", nosid]));
    b(&mut out, None, Some(&token));
    let keys = ["mod", "mod_pending", "status", "dispatchable"];
    out.push(json!(["row", pick(&c.row(IDLE), &keys)]));
    // The mod is live: prompts go to it, not the pipe.
    out.push(json!(["queue", c.window("/api/queue", &json!({"session_id": IDLE, "prompt": "typed as me"}))]));
    out.push(json!(["queue", c.plain("/api/queue", &json!({"session_id": IDLE, "prompt": "framed one"}))]));
    std::thread::sleep(Duration::from_millis(150));
    out.push(json!(["pipe", c.pipes[IDLE].take()]));
    out.push(json!(["row", pick(&c.row(IDLE), &["mod", "mod_pending", "status"])]));
    let given = b(&mut out, None, Some(&token))["deliver"].as_array().cloned().unwrap_or_default();
    b(&mut out, None, Some(&token)); // claimed: not handed out again
    if given.len() == 2 {
        b(&mut out, Some(json!({"acks": [
            {"id": given[0]["id"], "outcome": "accepted"},
            {"id": given[1]["id"], "outcome": "dropped", "reason": "engine said no"},
            {"id": "nope", "outcome": "submitted"}, "junk"]})), Some(&token));
        b(&mut out, Some(json!({"busy": true, "acks": [{"id": given[0]["id"], "outcome": "submitted"}]})), Some(&token));
    }
    let read = format!("{work}\\a.py");
    for (tool, inp) in [
        ("Bash", json!({"command": "git status"})),
        ("Bash", json!({"command": "rm -rf /"})),
        ("Read", json!({"file_path": read})),
        ("Bash", json!("not a dict")),
    ] {
        let got = post_mod("/api/mod/check", Some(&json!({"tool_name": tool, "tool_input": inp,
            "cwd": work, "session_id": IDLE, "agent_id": null})), Some(&token), None);
        out.push(json!(["check", got]));
    }
    let u = d.get("/api/usage").1;
    out.push(json!(["usage", pick(&u, &["limits", "plan"])]));
    (Value::Array(out), given.len())
}

#[test]
fn prompts_deliver_park_and_beat_as_python_did() {
    let started = std::time::Instant::now();
    let dir = golden_dir("inbox_twin");
    let steps = read_jsonl(&dir.join("steps.jsonl"));
    let records = read_json(&dir.join("records.json"));
    let tag = format!("{:x}{:x}", std::process::id(), std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos());
    let pipes: BTreeMap<&'static str, Pipe> = [(IDLE, "idle"), (BUSY, "busy"), (WAITS, "waits"), (LATE, "late")]
        .into_iter()
        .map(|(sid, n)| (sid, Pipe::new(&format!("imp-twin-{tag}-{n}"))))
        .collect();
    let d = Dispatcher::start(Options {
        // The recording's policy (the live one is this machine's own).
        policy: Some(json!({"enabled": true, "decision_format": "object", "defaults": {}})),
        env: vec![("IMPERATORIUM_TEST_TIME_SCALE".into(), SCALE.to_string())],
        ..Options::new()
    });
    let c = Ctx {
        d: &d,
        pipes,
        token: d.window_token(),
        places: Places::new(&[("tmp", &d.root.path)]),
        tag,
    };
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    for (name, rec) in records.as_object().unwrap() {
        let mut rec = c.fill(rec);
        rec["pid"] = json!(std::process::id());
        rec["startedAt"] = json!(now_ms);
        d.session_record(name.trim_end_matches(".json"), rec);
    }

    let mut f = Failures::default();
    for s in &steps {
        let kind = s["step"].as_str().unwrap();
        match kind {
            "post" => {
                let path = s["path"].as_str().unwrap();
                let body = c.fill(&s["body"]);
                let (code, ans) = if s["window"] == true {
                    d.post_window(path, &body, &c.token)
                } else {
                    d.post(path, &body)
                };
                let got = deliver_map(&c, s);
                f.check(
                    || format!("POST {path} {}", s["body"]),
                    first_difference(&s["want"], &json!([code, scrub(&ans)]), "$", false),
                );
                f.check(
                    || format!("delivered for {path} {}", s["body"]),
                    first_difference(&c.fill(&s["delivered"]), &got, "$", false),
                );
            }
            "sessions" => {
                let got: Map<String, Value> = c
                    .sessions()
                    .iter()
                    .map(|r| {
                        (
                            r["sessionId"].as_str().unwrap().to_string(),
                            json!([r.get("status").unwrap_or(&Value::Null),
                                   r.get("dispatchable").unwrap_or(&Value::Null),
                                   r.get("attention").unwrap_or(&Value::Null)]),
                        )
                    })
                    .collect();
                f.check(
                    || "sessions (status, dispatchable, attention)".into(),
                    first_difference(&s["want"], &Value::Object(got), "$", false),
                );
            }
            "late hook" => {
                let (code, ans) = d.post("/hook", &c.fill(&s["body"]));
                f.check(
                    || "POST /hook (the late session's SessionStart)".into(),
                    first_difference(&s["want"], &json!([code, ans]), "$", false),
                );
            }
            "sweep" => {
                // The queue sweep (15 s * SCALE) hears the late pipe now.
                let sid = s["session"].as_str().unwrap();
                let got = take_n(&c.pipes[sid], 1, Duration::from_secs(15));
                std::thread::sleep(Duration::from_millis(100));
                let extra = c.pipes[sid].take();
                f.check(
                    || "sweep delivery to the late session".into(),
                    first_difference(&s["want"], &json!(got.iter().take(1).collect::<Vec<_>>()), "$", false),
                );
                f.check(
                    || "sweep delivers once".into(),
                    (!extra.is_empty()).then(|| format!("{} more writes: {extra:?}", extra.len())),
                );
            }
            "parks" => {
                let got = scrub(&parks(&c));
                f.check(|| "parks".into(), first_difference(&c.fill(&s["want"]), &got, "$", false));
            }
            "mod" => {
                let (got, given) = mods(&c);
                f.check(
                    || "mod: prompts handed to the mod".into(),
                    (given as u64 != s["given"].as_u64().unwrap())
                        .then(|| format!("{given} vs {}", s["given"])),
                );
                f.check(|| "mod".into(), first_difference(&c.fill(&s["want"]), &scrub(&got), "$", false));
            }
            "decisions.jsonl" => {
                let file = d.state.parent().unwrap().join("decisions.jsonl");
                let read = || -> Value {
                    let text = std::fs::read_to_string(&file).unwrap_or_default();
                    scrub(&Value::Array(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()))
                };
                let want = c.fill(&s["want"]);
                let got = poll_equal(&want, read);
                f.check(|| "decisions.jsonl".into(), first_difference(&want, &got, "$", false));
            }
            "state.json" => {
                let keys: Vec<&str> = s["keys"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
                let read = || {
                    let st = d.state_json();
                    scrub(&Value::Object(
                        keys.iter().map(|k| (k.to_string(), st.get(*k).cloned().unwrap_or(Value::Null))).collect(),
                    ))
                };
                let want = c.fill(&s["want"]);
                let got = poll_equal(&want, read);
                f.check(|| "state.json (queue, events, sessions)".into(), first_difference(&want, &got, "$", false));
                // The window's prompt stays marked as the person's own.
                f.check(
                    || "as_user kept in the waiting session's queue".into(),
                    (!got["queue"][WAITS].as_array().map(|q| q.iter().any(|i| i["as_user"] == true)).unwrap_or(false))
                        .then(|| "no as_user prompt".to_string()),
                );
            }
            "get" => {
                let path = s["path"].as_str().unwrap();
                let got = scrub(&d.get(path).1);
                f.check(|| format!("GET {path}"), first_difference(&c.fill(&s["want"]), &got, "$", false));
            }
            other => panic!("unknown step {other}"),
        }
    }
    eprintln!("inbox twin: replay took {:?}", started.elapsed());
    f.finish("inbox twin");
}

/// What the pipes received for a post step, once the recorded writes are in.
fn deliver_map(c: &Ctx, step: &Value) -> Value {
    c.deliveries(c.fill(&step["delivered"]).as_object().unwrap())
}

/// Re-read until `read()` equals `want` (a file written a moment after the
/// answer), for up to 10 s; the last reading otherwise.
fn poll_equal(want: &Value, read: impl Fn() -> Value) -> Value {
    let mut last = read();
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while first_difference(want, &last, "$", false).is_some() && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
        last = read();
    }
    last
}
