//! Dictation for the prompt box: the dispatcher side of the voice worker.
//! The model runs in a child, `imperatorium-voice --worker` (whisper.cpp),
//! which sits beside this exe, with the settings' model in the models
//! folder; `imperatorium voice fetch` downloads both. It speaks a
//! JSON-lines protocol -
//!   {"op": "load"}                                -> {"ok", "device", "model"}
//!   {"op": "transcribe", "hint", "bytes": n} + n bytes -> {"ok", "text", ...}
//! The child exits when its stdin closes, and is ended after `idle_unload`
//! seconds unused, which gives back the ~3 GB the loaded model holds.

use crate::locks::LockExt;
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

/// Where imperatorium-voice keeps its ggml models (as it computes it).
pub fn models_dir() -> PathBuf {
    if let Some(d) = crate::paths::env_nonempty("IMPERATORIUM_MODELS") {
        return PathBuf::from(d);
    }
    #[cfg(windows)]
    let base = crate::paths::env_nonempty("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
    #[cfg(not(windows))]
    let base = crate::paths::env_nonempty("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
    });
    base.join("imperatorium").join("models")
}

const FETCH_HINT: &str = "run `imperatorium voice fetch` to download it";

/// The worker exe and model this configuration needs (the exe beside this
/// one, or IMPERATORIUM_VOICE_BIN; the model the settings name), or what
/// is missing and how to get it.
pub fn worker_check(settings: &Map<String, Value>) -> Result<PathBuf, String> {
    let exe = match crate::paths::env_nonempty("IMPERATORIUM_VOICE_BIN") {
        Some(p) => PathBuf::from(p),
        None => {
            let name = if cfg!(windows) { "imperatorium-voice.exe" } else { "imperatorium-voice" };
            let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(name)));
            beside.ok_or_else(|| "dictation: cannot locate the voice worker".to_string())?
        }
    };
    if !exe.is_file() {
        return Err(format!("dictation needs the voice worker ({}), which is missing: {FETCH_HINT}", exe.display()));
    }
    let model = settings.get("model").and_then(Value::as_str).unwrap_or("large-v3-turbo");
    let file = if model.contains(['/', '\\']) {
        PathBuf::from(model)
    } else {
        models_dir().join(format!("ggml-{model}.bin"))
    };
    if !file.is_file() {
        return Err(format!("dictation needs the model '{model}' ({}), which is missing: {FETCH_HINT}", file.display()));
    }
    Ok(exe)
}

/// End a child and everything under it: a venv's python.exe on Windows is
/// a launcher whose child is the real interpreter holding the model.
fn kill_tree(pid: u32) {
    crate::proc::kill_pid_tree(pid);
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

    /// What dictation lacks to run, if anything (see `worker_check`).
    pub fn missing(&self) -> Option<String> {
        worker_check(&self.settings).err()
    }

    /// {device, model} the child last said it runs on, None before that.
    pub fn where_(&self) -> Option<Value> {
        self.where_.lk().clone()
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
        let settings = crate::pyish::json_dumps(&Value::Object(self.settings.clone()), true);
        let mut cmd = Command::new(worker_check(&self.settings)?);
        cmd.args(["--worker", &settings]);
        cmd.current_dir(&self.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        crate::proc::no_window(&mut cmd);
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
            *self.where_.lk() = Some(json!({"device": reply["device"],
                "model": reply.get("model").cloned().unwrap_or(Value::Null)}));
        }
        Ok(reply)
    }

    /// Start the child if needed and wait until its model is built.
    pub fn load(self: &Arc<Self>) -> Result<Value, String> {
        let mut s = self.state.lk();
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
        let mut s = self.state.lk();
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
        Self::stop_locked(&mut self.state.lk());
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

/// test_voice.py, ported: the settings, and the protocol between the
/// dispatcher and the dictation child (a stand-in child, compiled once from
/// the source below, which speaks it the way imperatorium-voice does).
#[cfg(test)]
mod worker_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::OnceLock;

    // -- Settings -----------------------------------------------------------

    fn s(v: Value) -> Map<String, Value> {
        settings_from(Some(&v))
    }

    #[test]
    fn defaults_apply_when_there_is_no_config() {
        let d = settings_from(None);
        assert_eq!(d["model"], "large-v3-turbo");
        assert_eq!(d["language"], "en");
        // An empty block, a non-object block and no block are all the defaults.
        assert_eq!(s(json!({})), d);
        assert_eq!(settings_from(Some(&json!("junk"))), d);
    }

    #[test]
    fn config_overrides_only_known_keys() {
        // config.json is hand-edited and carries "_comment"; letting that
        // through would reach the worker as a setting.
        let got = s(json!({"model": "small", "_comment": "note to self", "nonsense": true}));
        assert_eq!(got["model"], "small");
        assert!(!got.contains_key("_comment"));
        assert!(!got.contains_key("nonsense"));
    }

    #[test]
    fn disabling_voice_is_expressed_in_settings() {
        assert_eq!(s(json!({"enabled": false}))["enabled"], false);
        assert_eq!(settings_from(None)["enabled"], true);
    }

    #[test]
    fn the_cpu_gets_a_smaller_model_by_default() {
        assert_eq!(settings_from(None)["cpu_model"], "small");
        assert_eq!(s(json!({}))["cpu_model"], "small");
    }

    #[test]
    fn a_model_named_in_config_is_kept_on_the_cpu() {
        assert_eq!(s(json!({"model": "medium"}))["cpu_model"], "medium");
        // The example config names the default; that is not a choice for the CPU.
        assert_eq!(s(json!({"model": "large-v3-turbo"}))["cpu_model"], "small");
        assert_eq!(s(json!({"model": "medium", "cpu_model": "base"}))["cpu_model"], "base");
    }

    #[test]
    fn an_input_device_index_survives() {
        assert_eq!(s(json!({"input_device": 16}))["input_device"], 16);
    }

    // -- The child process --------------------------------------------------

    /// A std-only program with the worker's protocol. It logs `start` (with
    /// its arguments) and `load` to the file named by settings "log", and
    /// answers a transcribe with "<n> bytes, hint=<hint>", or with the
    /// error in settings "fail".
    const FAKE: &str = r#"
use std::io::{BufRead, Read, Write};
fn field(s: &str, key: &str) -> Option<String> {
    let at = s.find(&format!("\"{key}\":"))? + key.len() + 3;
    let rest = s[at..].trim_start();
    if rest.starts_with("null") { return None; }
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => out.push(chars.next()?),
            c => out.push(c),
        }
    }
    Some(out)
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let settings = args.get(2).cloned().unwrap_or_default();
    let log = field(&settings, "log");
    let fail = field(&settings, "fail");
    let note = |what: &str| {
        if let Some(l) = &log {
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(l).unwrap();
            writeln!(f, "{what}").unwrap();
        }
    };
    note(&format!("start\t{}\t{}", args.get(1).cloned().unwrap_or_default(), settings));
    let mut inp = std::io::BufReader::new(std::io::stdin());
    let mut out = std::io::stdout();
    let mut line = String::new();
    loop {
        line.clear();
        if inp.read_line(&mut line).unwrap_or(0) == 0 { return; }
        let reply = if line.contains("\"load\"") {
            note("load");
            "{\"ok\": true, \"device\": \"cpu\", \"model\": null}".to_string()
        } else {
            let at = line.find("\"bytes\":").unwrap() + 8;
            let n: usize = line[at..].trim_start().chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap();
            let mut data = vec![0u8; n];
            inp.read_exact(&mut data).unwrap();
            note(&format!("transcribe {n}"));
            match &fail {
                Some(e) => format!("{{\"ok\": false, \"error\": \"{e}\"}}"),
                None => format!("{{\"ok\": true, \"text\": \"{n} bytes, hint={}\", \"device\": \"cpu\", \"model\": null}}",
                    field(&line, "hint").unwrap_or_else(|| "None".into())),
            }
        };
        writeln!(out, "{reply}").unwrap();
        out.flush().unwrap();
    }
}
"#;

    /// The compiled stand-in, and IMPERATORIUM_VOICE_BIN pointing at it
    /// (once, for every test here: they all want the same one).
    fn fake_exe() -> &'static PathBuf {
        static EXE: OnceLock<PathBuf> = OnceLock::new();
        EXE.get_or_init(|| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            FAKE.hash(&mut h);
            let dir = std::env::temp_dir().join("imp-fakevoice");
            std::fs::create_dir_all(&dir).unwrap();
            let ext = if cfg!(windows) { ".exe" } else { "" };
            let exe = dir.join(format!("fakevoice-{:x}{ext}", h.finish()));
            if !exe.is_file() {
                let src = dir.join(format!("fakevoice-{}.rs", std::process::id()));
                std::fs::write(&src, FAKE).unwrap();
                let tmp = dir.join(format!("fakevoice-{}{ext}", std::process::id()));
                let rustc = std::env::var_os("RUSTC").map(PathBuf::from).or_else(|| {
                    option_env!("CARGO").map(|c| Path::new(c).with_file_name(format!("rustc{}", if cfg!(windows) { ".exe" } else { "" })))
                }).unwrap_or_else(|| "rustc".into());
                let made = Command::new(rustc).args(["--edition", "2021", "-O", "-o"]).arg(&tmp).arg(&src).output().unwrap();
                assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
                let _ = std::fs::rename(&tmp, &exe);
                let _ = std::fs::remove_file(&tmp);
                let _ = std::fs::remove_file(&src);
            }
            std::env::set_var("IMPERATORIUM_VOICE_BIN", &exe);
            exe
        })
    }

    struct Rig {
        dir: PathBuf,
        log: PathBuf,
        w: Arc<Worker>,
    }

    impl Rig {
        fn new(extra: Value) -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            fake_exe();
            let dir = std::env::temp_dir().join(format!("imp-voice-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let model = dir.join("model.bin"); // a path as the model: no models folder needed
            std::fs::write(&model, b"x").unwrap();
            let log = dir.join("log.txt");
            let mut settings = json!({"model": model.to_string_lossy(), "idle_unload": 0,
                                      "log": log.to_string_lossy()}).as_object().unwrap().clone();
            settings.extend(extra.as_object().unwrap().clone());
            Rig { w: Worker::new(settings, &dir), dir, log }
        }

        fn lines(&self) -> Vec<String> {
            std::fs::read_to_string(&self.log).unwrap_or_default().lines().map(str::to_string).collect()
        }

        fn count(&self, prefix: &str) -> usize {
            self.lines().iter().filter(|l| l.starts_with(prefix)).count()
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            self.w.stop();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn audio_goes_through_and_text_comes_back() {
        let r = Rig::new(json!({}));
        assert_eq!(r.w.transcribe_bytes(b"abcde", Some("laser")).unwrap(), "5 bytes, hint=laser");
        assert_eq!(r.w.transcribe_bytes(b"xy", None).unwrap(), "2 bytes, hint=None");
        assert_eq!(r.count("start"), 1);
        assert!(r.w.running());
        r.w.stop(); // stdin closed: it exits
        assert!(!r.w.running());
    }

    #[test]
    fn the_worker_learns_where_the_model_runs() {
        let r = Rig::new(json!({}));
        assert_eq!(r.w.where_(), None);
        r.w.transcribe_bytes(b"abc", None).unwrap();
        assert_eq!(r.w.where_(), Some(json!({"device": "cpu", "model": null})));
    }

    #[test]
    fn nothing_recorded_never_starts_a_process() {
        let r = Rig::new(json!({}));
        assert_eq!(r.w.transcribe_bytes(b"", None).unwrap(), "");
        assert_eq!(r.count("start"), 0);
        assert!(!r.w.running());
    }

    #[test]
    fn load_builds_the_model_once_per_process() {
        let r = Rig::new(json!({}));
        r.w.load().unwrap();
        r.w.load().unwrap();
        assert_eq!((r.count("start"), r.count("load")), (1, 2));
    }

    #[test]
    fn a_failure_in_the_child_is_reported_and_the_child_kept() {
        let r = Rig::new(json!({"fail": "no GPU"}));
        let err = r.w.transcribe_bytes(b"abc", None).unwrap_err();
        assert!(err.contains("no GPU"), "{err}");
        assert!(r.w.running());
    }

    #[test]
    fn a_child_that_died_is_started_again() {
        let r = Rig::new(json!({}));
        r.w.load().unwrap();
        {
            let mut st = r.w.state.lk();
            let p = st.proc.as_mut().unwrap();
            p.child.kill().unwrap(); // as if it had exited
            p.child.wait().unwrap();
        }
        assert_eq!(r.w.transcribe_bytes(b"a", None).unwrap(), "1 bytes, hint=None");
        assert_eq!(r.count("start"), 2);
    }

    #[test]
    fn an_idle_child_is_ended_and_a_busy_one_is_not() {
        let r = Rig::new(json!({"idle_unload": 60}));
        r.w.load().unwrap();
        r.w.reap();
        assert!(r.w.running()); // just used
        {
            let mut st = r.w.state.lk(); // mid-transcription
            st.used = now() - 61.0;
            drop(st);
            let _busy = r.w.state.lk();
            r.w.reap();
        }
        assert!(r.w.running());
        r.w.reap(); // idle past the limit, nothing holding it
        assert!(!r.w.running());
        r.w.load().unwrap(); // comes back on demand
        assert_eq!(r.count("start"), 2);
    }

    #[test]
    fn zero_means_never_unload() {
        let r = Rig::new(json!({"idle_unload": 0}));
        r.w.load().unwrap();
        r.w.state.lk().used = now() - 1e6;
        r.w.reap();
        assert!(r.w.running());
    }

    #[test]
    fn the_child_is_run_with_the_worker_flag_and_its_settings() {
        let r = Rig::new(json!({}));
        r.w.load().unwrap();
        let line = r.lines().into_iter().find(|l| l.starts_with("start")).unwrap();
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        assert_eq!(parts[1], "--worker");
        let sent: Value = serde_json::from_str(parts[2]).unwrap();
        assert_eq!(sent["model"], r.w.settings["model"]);
        assert_eq!(sent["idle_unload"], 0);
    }
}
