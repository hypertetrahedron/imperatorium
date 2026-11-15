//! Helpers for the inbox / notify / settings twin replays (included by each
//! with `#[path]`, so tests/common/mod.rs stays untouched): twin.py's
//! `masked`, inbox_twin.py's `scrub`, a recording ntfy server, a Windows
//! named-pipe server, local time of a unix second, Python-style JSON.
#![allow(dead_code)]

use super::common::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// twin.py CLOCKS.
const CLOCKS: &[&str] = &[
    "seen_at",
    "last_event_at",
    "attention_since",
    "at",
    "start",
    "end",
    "since",
    "now",
];

/// twin.py masked(): clock-keyed numbers (a bool is an int in Python too).
pub fn masked(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| {
                    let m = if CLOCKS.contains(&k.as_str()) && (x.is_number() || x.is_boolean()) {
                        json!("<clock>")
                    } else {
                        masked(x)
                    };
                    (k.clone(), m)
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(masked).collect()),
        other => other.clone(),
    }
}

/// inbox_twin.py scrub().
pub fn scrub(v: &Value) -> Value {
    fn inner(v: &Value) -> Value {
        match v {
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, x)| {
                        let m = match x {
                            Value::String(_) if ["id", "delivered", "mod_prompt"].contains(&k.as_str()) => {
                                json!("<id>")
                            }
                            Value::String(s) if k == "error" && s.to_lowercase().contains("pipe") => {
                                json!("<error>")
                            }
                            _ => inner(x),
                        };
                        (k.clone(), m)
                    })
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.iter().map(inner).collect()),
            other => other.clone(),
        }
    }
    inner(&masked(v))
}

/// The inside of a JSON string literal (json.dumps(s)[1:-1]).
pub fn json_inner(s: &str) -> String {
    let t = serde_json::to_string(&Value::String(s.to_string())).unwrap();
    t[1..t.len() - 1].to_string()
}

/// Local HH:MM of a unix time (time.strftime("%H:%M", time.localtime(t))).
pub fn local_hhmm(unix: i64) -> String {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let ticks = (unix + 11_644_473_600) as u64 * 10_000_000;
    let ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    unsafe {
        let mut utc: SYSTEMTIME = std::mem::zeroed();
        let mut local: SYSTEMTIME = std::mem::zeroed();
        assert!(FileTimeToSystemTime(&ft, &mut utc) != 0);
        assert!(SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) != 0);
        format!("{:02}:{:02}", local.wHour, local.wMinute)
    }
}

// ------------------------------------------------------------------ ntfy

/// A fake ntfy server: each POST kept as {topic, body, Title, Priority,
/// Tags, Authorization} (null when the header is absent), as notify_twin's.
pub struct Ntfy {
    pub port: u16,
    pub pushes: Arc<Mutex<Vec<Value>>>,
    server: Arc<tiny_http::Server>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Ntfy {
    pub fn start() -> Ntfy {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let port = server.server_addr().to_ip().unwrap().port();
        let pushes = Arc::new(Mutex::new(Vec::new()));
        let (s, p) = (server.clone(), pushes.clone());
        let thread = std::thread::spawn(move || {
            for mut req in s.incoming_requests() {
                let mut body = Vec::new();
                let _ = req.as_reader().read_to_end(&mut body);
                let header = |name: &str| {
                    req.headers()
                        .iter()
                        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
                        .map(|h| json!(h.value.as_str()))
                        .unwrap_or(Value::Null)
                };
                p.lock().unwrap().push(json!({
                    "topic": req.url().trim_matches('/'),
                    "body": String::from_utf8_lossy(&body),
                    "Title": header("Title"), "Priority": header("Priority"),
                    "Tags": header("Tags"), "Authorization": header("Authorization"),
                }));
                let _ = req.respond(tiny_http::Response::from_data(b"{}".to_vec()));
            }
        });
        Ntfy {
            port,
            pushes,
            server,
            thread: Some(thread),
        }
    }

    pub fn server_url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    /// The pushes to `topic`, without the topic.
    pub fn to(&self, topic: &str) -> Vec<Value> {
        self.pushes
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p["topic"] == topic)
            .map(|p| {
                let mut p = p.clone();
                p.as_object_mut().unwrap().remove("topic");
                p
            })
            .collect()
    }
}

impl Drop for Ntfy {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// notify_twin.py config(): ntfy on at `server`, speech off, the config text
/// as Python's json.dump wrote it (the settings twin compares it byte for
/// byte).
pub fn notify_config_text(server: &str, topic: &str) -> String {
    format!(
        "{{\"notify\": {{\"ntfy\": {{\"enabled\": true, \"server\": \"{server}\", \"topic\": \"{topic}\", \
         \"delay\": 1.5, \"priority\": \"high\", \"token\": \"tk\"}}, \"speak\": {{\"enabled\": false}}}}}}"
    )
}

/// A dispatcher whose --home is a folder of its own holding that config.
pub fn start_notify(
    ntfy: &Ntfy,
    topic: &str,
    scale: Option<f64>,
) -> (Dispatcher, TempDir) {
    let home = TempDir::new("twin-home");
    std::fs::write(
        home.join("config.json"),
        notify_config_text(&ntfy.server_url(), topic),
    )
    .unwrap();
    let env = scale
        .map(|s| vec![("IMPERATORIUM_TEST_TIME_SCALE".to_string(), s.to_string())])
        .unwrap_or_default();
    let d = Dispatcher::start(Options {
        home: Some(home.path.clone()),
        env,
        ..Options::new()
    });
    (d, home)
}

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
            security: *mut c_void,
        ) -> isize;
        pub fn ConnectNamedPipe(h: isize, overlapped: *mut c_void) -> i32;
        pub fn ReadFile(h: isize, buf: *mut u8, n: u32, read: *mut u32, overlapped: *mut c_void) -> i32;
        pub fn CloseHandle(h: isize) -> i32;
    }
}

/// A named pipe accepting any number of writers, keeping what each wrote,
/// one entry per connection (inbox_twin.py Pipe).
pub struct Pipe {
    pub path: String,
    got: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Pipe {
    pub fn new(name: &str) -> Pipe {
        let path = format!("\\\\.\\pipe\\{name}");
        let got: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
        let (p, g) = (path.clone(), got.clone());
        std::thread::spawn(move || {
            let wide: Vec<u16> = p.encode_utf16().chain(Some(0)).collect();
            loop {
                // PIPE_ACCESS_INBOUND; byte pipe, blocking; unlimited instances.
                let h = unsafe {
                    ffi::CreateNamedPipeW(wide.as_ptr(), 1, 0, 255, 65536, 65536, 0, std::ptr::null_mut())
                };
                if h == -1 {
                    return;
                }
                // Fails with ERROR_PIPE_CONNECTED when the writer came first.
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

    /// How many connections have finished since the last take.
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
}

/// Wait until `pipe` has `n` finished writes (up to `limit`), then take them.
pub fn take_n(pipe: &Pipe, n: usize, limit: Duration) -> Vec<String> {
    wait_until(limit, || (pipe.count() >= n).then_some(()));
    pipe.take()
}
