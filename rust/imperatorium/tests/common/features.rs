//! Helpers for the test_features.py ports (tests/features_*.rs): a
//! dispatcher with a Claude config folder of its own (transcripts and
//! session records written before it starts), a named pipe standing in for
//! a session's inbox, and small readers for the API's answers.
#![allow(dead_code)]

use super::{wait_until, Dispatcher, Options, TempDir};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ----------------------------------------------------------------- pipes

mod ffi {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateNamedPipeW(
            name: *const u16,
            open_mode: u32,
            pipe_mode: u32,
            max: u32,
            out_buf: u32,
            in_buf: u32,
            timeout: u32,
            security: *const c_void,
        ) -> isize;
        pub fn ConnectNamedPipe(h: isize, overlapped: *mut c_void) -> i32;
        pub fn ReadFile(h: isize, buf: *mut u8, n: u32, read: *mut u32, overlapped: *mut c_void) -> i32;
        pub fn CloseHandle(h: isize) -> i32;
    }
}

/// A named pipe accepting any number of writers and keeping what each
/// wrote, one entry per connection: a session's inbox that only listens.
pub struct Pipe {
    pub path: String,
    got: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Pipe {
    pub fn new(tag: &str) -> Pipe {
        let path = format!("\\\\.\\pipe\\imp-feat-{}-{tag}", std::process::id());
        let got: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
        let (p, g) = (path.clone(), got.clone());
        std::thread::spawn(move || {
            let wide: Vec<u16> = p.encode_utf16().chain(Some(0)).collect();
            loop {
                // PIPE_ACCESS_INBOUND; byte pipe, blocking; unlimited instances.
                let h = unsafe {
                    ffi::CreateNamedPipeW(wide.as_ptr(), 1, 0, 255, 65536, 65536, 0, std::ptr::null())
                };
                if h == -1 {
                    return;
                }
                unsafe { ffi::ConnectNamedPipe(h, std::ptr::null_mut()) };
                let g = g.clone();
                std::thread::spawn(move || {
                    let mut data = Vec::new();
                    let mut buf = vec![0u8; 65536];
                    loop {
                        let mut n = 0u32;
                        let ok = unsafe {
                            ffi::ReadFile(h, buf.as_mut_ptr(), buf.len() as u32, &mut n, std::ptr::null_mut())
                        };
                        if ok == 0 || n == 0 {
                            break;
                        }
                        data.extend_from_slice(&buf[..n as usize]);
                    }
                    unsafe { ffi::CloseHandle(h) };
                    g.lock().unwrap().push(data);
                });
            }
        });
        Pipe { path, got }
    }

    pub fn count(&self) -> usize {
        self.got.lock().unwrap().len()
    }

    /// What arrived since the last take.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.got.lock().unwrap())
            .into_iter()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .collect()
    }

    /// Wait until `n` writes have finished (up to `limit`), then take them.
    pub fn take_n(&self, n: usize, limit: Duration) -> Vec<String> {
        wait_until(limit, || (self.count() >= n).then_some(()));
        self.take()
    }
}

// ----------------------------------------------------------- transcripts

/// One transcript line in the shape Claude Code writes, for a conversation
/// of the given steps.
pub enum Step<'a> {
    /// The person typing.
    User(&'a str),
    /// Claude saying something.
    Text(&'a str),
    /// A tool call and its result: (tool id, tool, input, is_error, result text).
    Tool(&'a str, &'a str, Value, bool, &'a str),
    /// A `ScheduleWakeup` (what /loop does) that fires at this Unix time.
    Wakeup(&'a str, f64),
}

pub fn transcript_lines(sid: &str, cwd: &str, steps: &[Step]) -> String {
    let mut out: Vec<Value> = Vec::new();
    let mut parent = Value::Null;
    let mut push = |out: &mut Vec<Value>, kind: &str, message: Value, extra: Option<Value>| {
        let n = out.len() + 1;
        let uuid = format!("{n:08x}-0000-4000-8000-{n:012x}");
        let mut line = json!({"parentUuid": parent.clone(), "isSidechain": false,
                              "userType": "external", "cwd": cwd, "sessionId": sid,
                              "version": "2.1.0", "gitBranch": "main", "type": kind,
                              "message": message, "uuid": uuid,
                              "timestamp": format!("2026-01-01T00:00:{:02}.000Z", n % 60)});
        if let Some(x) = extra {
            line["toolUseResult"] = x;
        }
        out.push(line);
        parent = json!(uuid);
    };
    let assistant = |content: Value| {
        json!({"role": "assistant", "model": "claude-sonnet-5-5", "stop_reason": "end_turn",
               "content": content})
    };
    for s in steps {
        match s {
            Step::User(t) => push(&mut out, "user", json!({"role": "user", "content": t}), None),
            Step::Text(t) => push(&mut out, "assistant", assistant(json!([{"type": "text", "text": t}])), None),
            Step::Tool(id, name, input, is_error, result) => {
                push(
                    &mut out,
                    "assistant",
                    assistant(json!([{"type": "tool_use", "id": id, "name": name, "input": input}])),
                    None,
                );
                let mut block = json!({"type": "tool_result", "tool_use_id": id, "content": result});
                if *is_error {
                    block["is_error"] = json!(true);
                }
                push(&mut out, "user", json!({"role": "user", "content": [block]}), None);
            }
            Step::Wakeup(id, fires) => {
                push(
                    &mut out,
                    "assistant",
                    assistant(json!([{"type": "tool_use", "id": id, "name": "ScheduleWakeup",
                                      "input": {"delaySeconds": 1200, "reason": "watching CI",
                                                "prompt": "/babysit"}}])),
                    None,
                );
                let block = json!({"type": "tool_result", "tool_use_id": id, "content": "scheduled"});
                push(
                    &mut out,
                    "user",
                    json!({"role": "user", "content": [block]}),
                    Some(json!({"scheduledFor": fires * 1000.0, "clampedDelaySeconds": 1200,
                                "wasClamped": false})),
                );
            }
        }
    }
    out.iter().map(Value::to_string).collect::<Vec<_>>().join("
") + "
"
}

// ------------------------------------------------------------- the rig

/// A dispatcher whose Claude config folder (`cfg`) is the test's own, with
/// project folders (`proj_a`, `proj_b`) that exist, and the fake `wt`/`claude`
/// on its PATH when asked.
pub struct Fx {
    pub d: Dispatcher,
    pub cfg: TempDir,
    pub proj_a: PathBuf,
    pub proj_b: PathBuf,
}

/// A transcript to put under the config folder before the dispatcher starts.
pub struct Seed<'a> {
    pub sid: &'a str,
    pub cwd: &'a Path,
    pub steps: Vec<Step<'a>>,
    pub custom_title: Option<&'a str>,
    /// How many seconds ago the file was last written.
    pub age: u64,
}

impl Fx {
    pub fn start(label: &str, fake_cli: Vec<&'static str>, env: Vec<(&str, &str)>, seeds: &[Seed]) -> Fx {
        let cfg = TempDir::new(&format!("featcfg-{label}"));
        let proj_a = cfg.join("work").join("proj-a");
        let proj_b = cfg.join("work").join("proj-b");
        for p in [&proj_a, &proj_b, &cfg.join("sessions")] {
            std::fs::create_dir_all(p).unwrap();
        }
        for s in seeds {
            let folder = cfg
                .join("projects")
                .join(s.cwd.to_string_lossy().replace([':', '\\', '/', '.'], "-"));
            std::fs::create_dir_all(&folder).unwrap();
            let mut text = String::new();
            if let Some(t) = s.custom_title {
                text.push_str(&json!({"type": "custom-title", "customTitle": t, "sessionId": s.sid}).to_string());
                text.push('\n');
            }
            text.push_str(&transcript_lines(s.sid, &s.cwd.to_string_lossy(), &s.steps));
            let file = folder.join(format!("{}.jsonl", s.sid));
            std::fs::write(&file, text).unwrap();
            let when = std::time::SystemTime::now() - Duration::from_secs(s.age);
            std::fs::File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(when)
                .unwrap();
        }
        let mut all: Vec<(String, String)> = vec![(
            "CLAUDE_CONFIG_DIR".into(),
            cfg.path.to_string_lossy().into_owned(),
        )];
        all.extend(env.into_iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let d = Dispatcher::start(Options {
            fake_cli,
            env: all,
            ..Options::new()
        });
        Fx { d, cfg, proj_a, proj_b }
    }

    /// A session record as Claude Code writes it; `pid` is this test
    /// process's, so the session counts as running. NEVER ask the dispatcher
    /// to close one of these.
    pub fn record(&self, sid: &str, fields: Value) {
        let mut rec = json!({"pid": std::process::id(), "sessionId": sid,
                             "cwd": self.proj_a.to_string_lossy(), "name": sid,
                             "kind": "interactive", "status": "idle",
                             "startedAt": std::time::SystemTime::now()
                                 .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64});
        for (k, v) in fields.as_object().into_iter().flatten() {
            rec[k] = v.clone();
        }
        // `"pid": null` is a record whose process is gone.
        if rec["pid"].is_null() {
            rec.as_object_mut().unwrap().remove("pid");
        }
        std::fs::write(
            self.cfg.join("sessions").join(format!("{sid}.json")),
            serde_json::to_vec(&rec).unwrap(),
        )
        .unwrap();
    }

    pub fn hook(&self, body: Value) -> Value {
        let (code, answer) = self.d.post("/hook", &body);
        assert_eq!(code, 200, "{body}");
        answer
    }

    /// A SessionStart (which also makes the dispatcher look again).
    pub fn session_start(&self, sid: &str, socket: Option<&str>, token: Option<&str>) {
        let mut body = json!({"hook_event_name": "SessionStart", "session_id": sid,
                              "cwd": self.proj_a.to_string_lossy(), "source": "startup"});
        if let Some(s) = socket {
            body["messaging_socket"] = json!(s);
        }
        if let Some(t) = token {
            body["messaging_token"] = json!(t);
        }
        self.hook(body);
    }

    pub fn stop(&self, sid: &str) -> Value {
        self.hook(json!({"hook_event_name": "Stop", "session_id": sid,
                         "cwd": self.proj_a.to_string_lossy(), "last_assistant_message": "ok"}))
    }

    pub fn post(&self, path: &str, body: Value) -> Value {
        let (code, answer) = self.d.post(path, &body);
        assert_eq!(code, 200, "{path} {body}: {answer}");
        answer
    }

    pub fn rows(&self) -> Vec<Value> {
        let (code, body) = self.d.get("/api/sessions");
        assert_eq!(code, 200);
        body["sessions"].as_array().cloned().unwrap_or_default()
    }

    pub fn row(&self, sid: &str) -> Option<Value> {
        self.rows().into_iter().find(|r| r["sessionId"] == sid)
    }

    pub fn events(&self) -> Vec<Value> {
        self.d.get("/api/events").1["events"].as_array().cloned().unwrap_or_default()
    }

    pub fn queue(&self) -> Value {
        self.d.get("/api/queue").1["queue"].clone()
    }

    /// Wait for the fake `prog` to have logged `n` calls (a launch is
    /// detached, so its log line can come a moment after the answer).
    pub fn launched_n(&self, prog: &str, n: usize) -> Vec<Value> {
        wait_until(Duration::from_secs(10), || {
            let got = self.launched(prog);
            (got.len() >= n).then_some(got)
        })
        .unwrap_or_else(|| self.launched(prog))
    }

    /// Calls the fake wt/claude logged, `prog` only.
    pub fn launched(&self, prog: &str) -> Vec<Value> {
        self.d.fake_log().into_iter().filter(|e| e["prog"] == prog).collect()
    }
}
