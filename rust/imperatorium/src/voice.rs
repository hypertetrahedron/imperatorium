//! Dictation for the prompt box: the dispatcher side of `imperatorium/
//! voice.py` (`Worker`). The model itself stays in Python for now (phase 5
//! ports it): it runs in a child, `python -m imperatorium.voice --worker`,
//! from the checkout's venv, over the same JSON-lines protocol -
//!   {"op": "load"}                                -> {"ok", "device", "model"}
//!   {"op": "transcribe", "hint", "bytes": n} + n bytes -> {"ok", "text", ...}
//! The child exits when its stdin closes, and is ended after `idle_unload`
//! seconds unused, which gives back the ~3 GB the loaded model holds.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Map, Value};

const REPLY_TIMEOUT: u64 = 180; // a cold load plus a long utterance on CPU
const REAP_EVERY: u64 = 30;

/// voice.settings_from: a `voice` block over the defaults, unknown keys
/// dropped; a model other than the default doubles as the CPU model.
pub fn settings_from(block: Option<&Value>) -> Map<String, Value> {
    let defaults = json!({
        "model": "large-v3-turbo", "cpu_model": "small", "device": "cuda", "compute_type": "float16",
        "language": "en", "beam_size": 1, "input_device": null, "max_seconds": 120, "preload": true,
        "enabled": true, "idle_unload": 1800,
    });
    let mut out = defaults.as_object().unwrap().clone();
    let empty = Map::new();
    let block = block.and_then(Value::as_object).unwrap_or(&empty);
    for (k, v) in block {
        if out.contains_key(k) {
            out.insert(k.clone(), v.clone());
        }
    }
    let model = block.get("model");
    let named = match model {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.is_empty() && s != "large-v3-turbo",
        Some(_) => true,
    };
    if named && !crate::pyish::truthy(block.get("cpu_model")) {
        out.insert("cpu_model".into(), model.unwrap().clone());
    }
    out
}

/// The venv interpreter the model runs under (python.exe, not pythonw: the
/// child's stdio must be the pipes it is given).
fn interpreter(home: &Path) -> PathBuf {
    if cfg!(windows) {
        home.join(".venv").join("Scripts").join("python.exe")
    } else {
        home.join(".venv").join("bin").join("python")
    }
}

/// voice.available, as far as the binary can tell without importing:
/// the checkout has a venv with the worker module in it.
pub fn available(home: &Path) -> bool {
    interpreter(home).is_file() && home.join("imperatorium").join("voice.py").is_file()
}

/// End a child and everything under it: a venv's python.exe on Windows is
/// a launcher whose child is the real interpreter holding the model.
fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .status();
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

struct Proc {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

struct State {
    proc: Option<Proc>,
    used: f64,
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// voice.Worker: the model in a child process that can be ended to free it.
pub struct Worker {
    pub settings: Map<String, Value>,
    home: PathBuf,
    state: Mutex<State>, // one request at a time; start and stop too
    where_: Mutex<Option<Value>>,
    reaper: AtomicBool,
}

impl Worker {
    pub fn new(settings: Map<String, Value>, home: &Path) -> Arc<Worker> {
        Arc::new(Worker {
            settings,
            home: home.to_path_buf(),
            state: Mutex::new(State { proc: None, used: 0.0 }),
            where_: Mutex::new(None),
            reaper: AtomicBool::new(false),
        })
    }

    /// {device, model} the child last said it runs on, None before that.
    pub fn where_(&self) -> Option<Value> {
        self.where_.lock().unwrap().clone()
    }

    fn alive(p: &mut Proc) -> bool {
        matches!(p.child.try_wait(), Ok(None))
    }

    pub fn running(&self) -> bool {
        match self.state.try_lock() {
            Ok(mut s) => s.proc.as_mut().is_some_and(Self::alive),
            Err(_) => true, // busy with a request, so it is up
        }
    }

    fn stop_locked(s: &mut State) {
        let Some(mut p) = s.proc.take() else { return };
        drop(p.stdin.take()); // the child's cue to exit
        for _ in 0..50 {
            if !Self::alive(&mut p) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        kill_tree(p.child.id());
        let _ = p.child.wait();
    }

    fn ensure(self: &Arc<Self>, s: &mut State) -> Result<(), String> {
        if s.proc.as_mut().is_some_and(Self::alive) {
            return Ok(());
        }
        Self::stop_locked(s);
        let mut cmd = Command::new(interpreter(&self.home));
        cmd.args(["-m", "imperatorium.voice", "--worker", &crate::pyish::json_dumps(&Value::Object(self.settings.clone()), true)])
            .current_dir(&self.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| format!("the dictation process failed: {e}"))?;
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        s.proc = Some(Proc { child, stdin, stdout });
        s.used = now();
        let idle = crate::pyish::py_number(self.settings.get("idle_unload")).unwrap_or(0.0);
        if idle > 0.0 && !self.reaper.swap(true, Ordering::SeqCst) {
            let me = Arc::downgrade(self);
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(REAP_EVERY));
                match me.upgrade() {
                    Some(w) => w.reap(),
                    None => return,
                }
            });
        }
        Ok(())
    }

    fn call(self: &Arc<Self>, s: &mut State, header: Value, payload: &[u8]) -> Result<Value, String> {
        self.ensure(s)?;
        let p = s.proc.as_mut().unwrap();
        let pid = p.child.id();
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        std::thread::spawn(move || {
            for _ in 0..REPLY_TIMEOUT * 10 {
                std::thread::sleep(Duration::from_millis(100));
                if flag.load(Ordering::SeqCst) {
                    return;
                }
            }
            kill_tree(pid);
        });
        let mut line = String::new();
        let sent = (|| -> std::io::Result<usize> {
            let stdin = p.stdin.as_mut().ok_or_else(|| std::io::Error::other("closed"))?;
            let mut msg = crate::pyish::json_dumps(&header, true).into_bytes();
            msg.push(b'\n');
            msg.extend_from_slice(payload);
            stdin.write_all(&msg)?;
            stdin.flush()?;
            p.stdout.read_line(&mut line)
        })();
        done.store(true, Ordering::SeqCst);
        if let Err(e) = sent {
            Self::stop_locked(s);
            return Err(format!("the dictation process failed: {e}"));
        }
        s.used = now();
        if line.is_empty() {
            Self::stop_locked(s);
            return Err("the dictation process exited".into());
        }
        let Ok(reply) = serde_json::from_str::<Value>(&line) else {
            Self::stop_locked(s);
            return Err("the dictation process answered nonsense".into());
        };
        if !crate::pyish::truthy(reply.get("ok")) {
            let e = reply.get("error").filter(|e| crate::pyish::truthy(Some(e)));
            return Err(e.map(crate::pyish::py_str).unwrap_or_else(|| "transcription failed".into()));
        }
        if crate::pyish::truthy(reply.get("device")) {
            *self.where_.lock().unwrap() = Some(json!({"device": reply["device"],
                "model": reply.get("model").cloned().unwrap_or(Value::Null)}));
        }
        Ok(reply)
    }

    /// Start the child if needed and wait until its model is built.
    pub fn load(self: &Arc<Self>) -> Result<Value, String> {
        let mut s = self.state.lock().unwrap();
        self.call(&mut s, json!({"op": "load"}), b"")
    }

    /// `load` in the background: a preload, or a mic just pressed.
    pub fn warm(self: &Arc<Self>) {
        let me = Arc::clone(self);
        std::thread::spawn(move || {
            let _ = me.load();
        });
    }

    /// Encoded audio (anything PyAV reads) to text.
    pub fn transcribe_bytes(self: &Arc<Self>, data: &[u8], hint: Option<&str>) -> Result<String, String> {
        if data.is_empty() {
            return Ok(String::new());
        }
        let mut s = self.state.lock().unwrap();
        let reply = self.call(&mut s, json!({"op": "transcribe", "hint": hint, "bytes": data.len()}), data)?;
        Ok(reply.get("text").and_then(Value::as_str).unwrap_or("").to_string())
    }

    /// Worker.reap: end the child once idle past `idle_unload`, unless a
    /// request holds it right now.
    pub fn reap(&self) {
        let idle = crate::pyish::py_number(self.settings.get("idle_unload")).unwrap_or(0.0);
        if idle <= 0.0 {
            return;
        }
        let Ok(mut s) = self.state.try_lock() else { return };
        if s.proc.as_mut().is_some_and(Self::alive) && now() - s.used >= idle {
            Self::stop_locked(&mut s);
        }
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        Self::stop_locked(&mut self.state.lock().unwrap());
    }
}

/// window.open_window: the page in a chromeless browser window (Edge's or
/// Chrome's --app), else the default browser. The browser used, or None.
pub fn open_window(url: &str, size: &str) -> Result<Option<String>, String> {
    let candidates = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];
    let found = candidates.iter().map(PathBuf::from).find(|p| cfg!(windows) && p.is_file());
    if let Some(browser) = found {
        let mut cmd = Command::new(&browser);
        cmd.arg(format!("--app={url}")).arg(format!("--window-size={size}"));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0000_0008); // DETACHED_PROCESS
        }
        cmd.spawn().map_err(|e| format!("OSError: {e}"))?;
        return Ok(Some(browser.to_string_lossy().into_owned()));
    }
    let opener: &[&str] = if cfg!(windows) {
        &["rundll32", "url.dll,FileProtocolHandler"]
    } else if cfg!(target_os = "macos") {
        &["open"]
    } else {
        &["xdg-open"]
    };
    Command::new(opener[0]).args(&opener[1..]).arg(url).spawn().map_err(|e| format!("OSError: {e}"))?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_over_defaults() {
        let s = settings_from(Some(&json!({"model": "large-v3-turbo", "bogus": 1, "preload": false})));
        assert_eq!(s["cpu_model"], "small");
        assert_eq!(s["preload"], false);
        assert!(!s.contains_key("bogus"));
        let s = settings_from(Some(&json!({"model": "medium"})));
        assert_eq!(s["cpu_model"], "medium");
        let s = settings_from(None);
        assert_eq!(s["idle_unload"], 1800);
    }
}
