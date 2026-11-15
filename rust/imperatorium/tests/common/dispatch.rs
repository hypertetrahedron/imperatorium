//! Helpers for the dispatch_* integration tests (the ports of
//! test_dispatcher.py, test_closing.py, test_finished.py and test_public.py):
//! a dispatcher with a project catalogue, made-up sessions that are alive,
//! the inbox pipes served by the test, a console that reads what is typed
//! into it, and a `claude agents --json` that answers what a test says.
//!
//! Nothing here reaches the machine's own sessions: every dispatcher is the
//! harness's (temp --home, --state, CLAUDE_CONFIG_DIR, stand-in PATH).
#![allow(dead_code)]

use super::*;
use std::path::Path;

pub const SID: &str = "11111111-1111-1111-1111-111111111111";
pub const OTHER: &str = "22222222-2222-2222-2222-222222222222";

/// Timers at 1/50 of production: the 5 s stream look is 0.1 s, the 15 s
/// queue sweep 0.3 s, the 60 s idle check 1.2 s.
pub const SCALE: &str = "0.02";

/// The harness's dispatcher with a catalogue of two projects, laser-ledger
/// and gem-trip, as folders under its own temp root.
pub struct Rig {
    pub d: Dispatcher,
    pub projects: PathBuf,
    /// This rig's laser-ledger, as a session's cwd.
    pub cwd: String,
    pipes: std::cell::RefCell<Vec<PipeServer>>,
    consoles: std::cell::RefCell<Vec<Console>>,
}

impl std::ops::Deref for Rig {
    type Target = Dispatcher;
    fn deref(&self) -> &Dispatcher {
        &self.d
    }
}

impl Rig {
    pub fn start() -> Rig {
        Rig::with(json!({}), |_| {})
    }

    /// `config` is merged over {"roots": [the catalogue]}; `tweak` adjusts
    /// the Options (fake_cli, env) before the start.
    pub fn with(config: Value, tweak: impl FnOnce(&mut Options)) -> Rig {
        // The catalogue lives beside the dispatcher's own folders, in a
        // folder this rig removes (the Dispatcher's root is created inside
        // start, so the projects get a TempDir of their own).
        let holder = TempDir::new("projects");
        let projects = holder.path.clone();
        std::mem::forget(holder); // removed with the rig's Drop below
        for name in ["laser-ledger", "gem-trip"] {
            std::fs::create_dir_all(projects.join(name)).unwrap();
        }
        let mut cfg = json!({"roots": [projects.to_string_lossy()]});
        if let (Some(c), Some(extra)) = (cfg.as_object_mut(), config.as_object()) {
            for (k, v) in extra {
                c.insert(k.clone(), v.clone());
            }
        }
        let mut opts = Options::new();
        opts.config = Some(cfg);
        opts.env
            .push(("IMPERATORIUM_TEST_TIME_SCALE".to_string(), SCALE.to_string()));
        tweak(&mut opts);
        let d = Dispatcher::start(opts);
        let cwd = projects.join("laser-ledger").to_string_lossy().into_owned();
        Rig {
            d,
            projects,
            cwd,
            pipes: Default::default(),
            consoles: Default::default(),
        }
    }

    /// A pipe this rig serves, as a session's messagingSocketPath.
    pub fn pipe(&self) -> String {
        let p = PipeServer::start();
        let path = p.path.clone();
        self.pipes.borrow_mut().push(p);
        path
    }

    /// What was written to `path`'s pipe: one entry per connection.
    pub fn received(&self, path: &str) -> Vec<String> {
        self.pipes
            .borrow()
            .iter()
            .find(|p| p.path == path)
            .map(|p| p.got.lock().unwrap().clone())
            .unwrap_or_default()
    }

    /// A console that reads what is typed into it (fakecli's), and its pid.
    pub fn console(&self, stubborn: bool) -> u32 {
        let c = Console::start(&self.d, stubborn);
        let pid = c.pid;
        self.consoles.borrow_mut().push(c);
        pid
    }

    /// Make the next look at the sessions a new one.
    pub fn invalidate(&self) {
        self.d.post("/api/unqueue", &json!({"session": "-", "id": 0}));
    }

    /// A made-up local session as `claude agents` and its record show it:
    /// alive (this process's pid unless `extra` says otherwise), with an
    /// inbox pipe. Returns the pipe.
    pub fn live(&self, sid: &str, status: &str, extra: Value) -> String {
        let socket = self.pipe();
        let mut rec = json!({"cwd": self.cwd, "kind": "interactive", "name": "ll-1",
                             "status": status, "messagingSocketPath": socket});
        merge(&mut rec, &extra);
        self.d.session_record(sid, rec);
        self.invalidate();
        socket
    }

    /// Rewrite a session's record (the fields given), as Claude Code does
    /// when it changes status.
    pub fn touch(&self, sid: &str, extra: Value) {
        let path = self.d.claude.join("sessions").join(format!("{sid}.json"));
        let mut rec: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        merge(&mut rec, &extra);
        std::fs::write(&path, serde_json::to_vec(&rec).unwrap()).unwrap();
        self.invalidate();
    }

    /// POST /hook.
    pub fn hook(&self, event: &str, sid: &str, extra: Value) -> Value {
        let mut body = json!({"hook_event_name": event, "session_id": sid, "cwd": self.cwd});
        merge(&mut body, &extra);
        let (code, got) = self.d.post("/hook", &body);
        assert_eq!(code, 200, "{got}");
        got
    }

    /// SessionStart with the credential of `socket`.
    pub fn start_hook(&self, sid: &str, token: &str, socket: &str) -> Value {
        self.hook(
            "SessionStart",
            sid,
            json!({"session_name": "ll-1", "messaging_socket": socket, "messaging_token": token}),
        )
    }

    /// The rows of GET /api/sessions (what the window sees, never a token).
    pub fn rows(&self) -> Vec<Value> {
        let (code, body) = self.d.get("/api/sessions");
        assert_eq!(code, 200);
        body["sessions"].as_array().cloned().unwrap_or_default()
    }

    /// The row for `sid`, looked at afresh.
    pub fn row(&self, sid: &str) -> Value {
        self.invalidate();
        self.rows()
            .into_iter()
            .find(|r| r["sessionId"] == sid)
            .unwrap_or_else(|| panic!("no row for {sid}"))
    }

    /// POST /api/dispatch as a plain API caller.
    pub fn dispatch(&self, body: Value) -> Value {
        self.d.post("/api/dispatch", &body).1
    }

    /// The recorded tokens: {sid: token} from state.json.
    pub fn tokens(&self) -> Value {
        self.d.state_json()["tokens"].clone()
    }

    /// The newest events first, as GET /api/events answers.
    pub fn events(&self) -> Vec<Value> {
        self.d.get("/api/events").1["events"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// `claude agents --json --all` answers `rows` from now on (and every
    /// call is counted: see `agent_calls`).
    pub fn agents(&self, rows: Value) {
        std::fs::write(
            self.d.bin_dir.join("agents.json"),
            serde_json::to_vec(&rows).unwrap(),
        )
        .unwrap();
        self.invalidate();
    }

    /// How many times the dispatcher has run `claude agents`.
    pub fn agent_calls(&self) -> usize {
        std::fs::read_to_string(self.d.bin_dir.join("stub.log"))
            .map(|t| t.lines().filter(|l| l.starts_with("agents")).count())
            .unwrap_or(0)
    }

    /// A rig whose `claude` is the agents stub rather than fakecli.
    pub fn with_agents() -> Rig {
        Rig::with_agents_config(json!({}))
    }

    pub fn with_agents_config(config: Value) -> Rig {
        let rig = Rig::with(config, |_| {});
        rig.install_stub();
        rig
    }

    fn install_stub(&self) {
        let exe = agents_stub();
        std::fs::copy(&exe, self.d.bin_dir.join("claude.exe")).unwrap();
    }

    /// A transcript for `sid` under the rig's CLAUDE_CONFIG_DIR/projects:
    /// the lines as given.
    pub fn transcript(&self, sid: &str, lines: &[Value]) -> PathBuf {
        let dir = self.d.claude.join("projects").join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{sid}.jsonl"));
        let mut text = String::new();
        for l in lines {
            text.push_str(&l.to_string());
            text.push('\n');
        }
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consoles.borrow_mut().clear();
        self.pipes.borrow_mut().clear();
        remove_tree(&self.projects);
    }
}

/// `into` with every field of `from` set.
pub fn merge(into: &mut Value, from: &Value) {
    if let (Some(a), Some(b)) = (into.as_object_mut(), from.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
}

// ------------------------------------------------------------- the agents

/// The program run as `claude` when a test wants to say what `claude agents
/// --json --all` lists: prints agents.json beside itself (else `[]`) for
/// `agents`, and logs every call's arguments to stub.log there. Built once
/// with the compiler that built the tests, kept in the temp folder.
pub fn agents_stub() -> PathBuf {
    const SRC: &str = r#"
use std::io::Write;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let here = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(here.join("stub.log")) {
        let _ = writeln!(f, "{}", args.join(" "));
    }
    if args.first().map(String::as_str) == Some("agents") {
        let text = std::fs::read_to_string(here.join("agents.json"))
            .unwrap_or_else(|_| "[]".to_string());
        print!("{}", if text.trim().is_empty() { "[]" } else { &text });
    }
}
"#;
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            let dir = std::env::temp_dir().join("imp-dispatch-stub");
            std::fs::create_dir_all(&dir).unwrap();
            let exe = dir.join(format!("agents-stub-{:x}.exe", fnv(SRC)));
            if !exe.is_file() {
                let src = dir.join(format!("agents-stub-{}.rs", std::process::id()));
                std::fs::write(&src, SRC).unwrap();
                let rustc = std::env::var_os("RUSTC")
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("CARGO").and_then(|c| {
                            let r = Path::new(&c).with_file_name("rustc.exe");
                            r.is_file().then_some(r)
                        })
                    })
                    .unwrap_or_else(|| PathBuf::from("rustc"));
                let part = dir.join(format!("part-{}.exe", std::process::id()));
                let out = Command::new(rustc)
                    .args(["--edition", "2021", "-O", "-o"])
                    .arg(&part)
                    .arg(&src)
                    .output()
                    .expect("run rustc");
                assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
                let _ = std::fs::remove_file(&src);
                if std::fs::rename(&part, &exe).is_err() {
                    let _ = std::fs::remove_file(&part); // another test process got there first
                }
            }
            exe
        })
        .clone()
}

fn fnv(s: &str) -> u64 {
    s.bytes()
        .fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

// ---------------------------------------------------------------- consoles

/// A fakecli `claude --fake-console` in a console of its own: the process a
/// dispatcher can type into. It is started through `cmd /c start /wait`, so
/// that it has the new console's own handles (a child of this test would
/// have the test's pipes for stdin, and read nothing). Stopped on drop.
pub struct Console {
    launcher: Child,
    pub pid: u32,
}

impl Console {
    pub fn start(d: &Dispatcher, stubborn: bool) -> Console {
        let dir = d.root.join(if stubborn { "console-stubborn" } else { "console" });
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("claude.exe");
        std::fs::copy(fakecli(), &exe).unwrap();
        let launcher = start_in_new_console(
            d,
            &format!("\"{}\" --fake-console{}", exe.display(), if stubborn { " --stubborn" } else { "" }),
        );
        let lp = launcher.id();
        let pid = wait_until(Duration::from_secs(20), || {
            children_of(lp).into_iter().find(|(_, n)| n.eq_ignore_ascii_case("claude.exe")).map(|c| c.0)
        })
        .expect("the fake console started");
        wait_typeable(pid);
        Console { launcher, pid }
    }

    /// Has the process gone?
    pub fn gone(&self) -> bool {
        !running(self.pid, "claude.exe")
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        kill_tree(&mut self.launcher);
        let _ = self.launcher.wait();
    }
}

/// Until `imperatorium type-into PID` (no keys) can attach to the process's
/// console: a console that has only just been made refuses for a moment.
pub fn wait_typeable(pid: u32) {
    let ok = wait_until(Duration::from_secs(20), || {
        Command::new(bin())
            .args(["type-into", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .filter(|s| s.success())
    });
    assert!(ok.is_some(), "console of {pid} never took a keystroke");
}

/// `cmd /c start "" /wait <command line>`: the program in a new console
/// window, with that console's handles. The returned child is the cmd that
/// waits for it.
pub fn start_in_new_console(d: &Dispatcher, command_line: &str) -> Child {
    let mut cmd = Command::new("cmd");
    cmd.env("FAKE_LOG", d.root.join("fake.jsonl"))
        .current_dir(&d.root.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(format!("/c start \"\" /wait {command_line}"));
    }
    cmd.spawn().expect("start in a new console")
}

// ------------------------------------------------------------------ pipes

static PIPES: AtomicU32 = AtomicU32::new(0);

/// A named pipe a session's inbox would be: accepts any number of writers
/// and keeps each connection's bytes. Windows only.
pub struct PipeServer {
    pub path: String,
    pub got: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(windows)]
mod ffi {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateNamedPipeW(
            name: *const u16,
            open_mode: u32,
            pipe_mode: u32,
            max_instances: u32,
            out_buffer: u32,
            in_buffer: u32,
            default_timeout: u32,
            security: *const c_void,
        ) -> isize;
        pub fn ConnectNamedPipe(pipe: isize, overlapped: *mut c_void) -> i32;
        pub fn DisconnectNamedPipe(pipe: isize) -> i32;
        pub fn ReadFile(
            file: isize,
            buffer: *mut u8,
            len: u32,
            read: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        pub fn CloseHandle(handle: isize) -> i32;
    }
}

impl PipeServer {
    #[cfg(windows)]
    pub fn start() -> PipeServer {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex};
        let n = PIPES.fetch_add(1, Ordering::SeqCst);
        let path = format!(r"\\.\pipe\imp-dispatch-{}-{n}", std::process::id());
        let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
        let got = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (g, s) = (got.clone(), stop.clone());
        // The first instance exists before this returns, so a client never
        // finds the pipe missing.
        let first = unsafe {
            ffi::CreateNamedPipeW(wide.as_ptr(), 1, 0, 255, 4096, 4096, 0, std::ptr::null())
        };
        assert!(first != -1, "could not create {path}");
        std::thread::spawn(move || {
            let mut pipe = first;
            loop {
                unsafe { ffi::ConnectNamedPipe(pipe, std::ptr::null_mut()) };
                if s.load(Ordering::SeqCst) {
                    unsafe { ffi::CloseHandle(pipe) };
                    return;
                }
                let next = unsafe {
                    ffi::CreateNamedPipeW(wide.as_ptr(), 1, 0, 255, 4096, 4096, 0, std::ptr::null())
                };
                let g = g.clone();
                let done = pipe;
                std::thread::spawn(move || {
                    let mut bytes = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        let mut n = 0u32;
                        let ok = unsafe {
                            ffi::ReadFile(done, buf.as_mut_ptr(), buf.len() as u32, &mut n, std::ptr::null_mut())
                        };
                        if ok == 0 || n == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&buf[..n as usize]);
                    }
                    g.lock().unwrap().push(String::from_utf8_lossy(&bytes).into_owned());
                    unsafe {
                        ffi::DisconnectNamedPipe(done);
                        ffi::CloseHandle(done);
                    }
                });
                if next == -1 {
                    return;
                }
                pipe = next;
            }
        });
        PipeServer { path, got, stop }
    }

    #[cfg(not(windows))]
    pub fn start() -> PipeServer {
        panic!("the inbox pipes are Windows named pipes");
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the listener so its thread ends.
        let _ = std::fs::OpenOptions::new().write(true).open(&self.path);
    }
}

/// The messages written to a pipe, as JSON: every line of every connection.
pub fn lines_of(received: &[String]) -> Vec<Value> {
    received
        .iter()
        .flat_map(|c| c.lines())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// The prompt text of the `user` message in a connection, with the origin
/// note still on it.
pub fn user_texts(received: &[String]) -> Vec<String> {
    lines_of(received)
        .iter()
        .filter(|l| l["type"] == "user")
        .filter_map(|l| l["message"]["content"].as_str().map(str::to_string))
        .collect()
}

/// The tokens the pipe was shown, in order.
pub fn auth_tokens(received: &[String]) -> Vec<String> {
    lines_of(received)
        .iter()
        .filter(|l| l["type"] == "auth")
        .filter_map(|l| l["token"].as_str().map(str::to_string))
        .collect()
}

/// A transcript line for an assistant message: `usage` as given.
pub fn assistant_line(id: &str, text: &str, input: u64, output: u64) -> Value {
    json!({"type": "assistant", "uuid": format!("a-{id}"), "timestamp": "2026-01-01T00:00:00.000Z",
           "message": {"id": id, "role": "assistant", "model": "claude-sonnet-4-5",
                       "content": [{"type": "text", "text": text}],
                       "usage": {"input_tokens": input, "output_tokens": output}}})
}

pub fn user_line(text: &str) -> Value {
    json!({"type": "user", "uuid": format!("u-{}", text.len()), "timestamp": "2026-01-01T00:00:00.000Z",
           "message": {"role": "user", "content": text}})
}

/// Poll until `f` holds (five seconds), panicking with `what`.
pub fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    assert!(
        wait_until(Duration::from_secs(10), || f().then_some(())).is_some(),
        "never happened: {what}"
    );
}

// ------------------------------------------------------------ process trees

/// (pid, exe file name) of the processes whose parent is `pid`.
#[cfg(windows)]
pub fn children_of(pid: u32) -> Vec<(u32, String)> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            if e.th32ParentProcessID == pid {
                let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
                out.push((e.th32ProcessID, String::from_utf16_lossy(&e.szExeFile[..len])));
            }
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    out
}

/// Is a process with this pid and exe name still there? (A freed pid is
/// reused quickly, so the name is what says it is the same one.)
#[cfg(windows)]
pub fn running(pid: u32, exe: &str) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut found = false;
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            if e.th32ProcessID == pid {
                let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
                found = String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case(exe);
                break;
            }
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    found
}

/// Is a process with this pid still there?
#[cfg(windows)]
pub fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code);
        CloseHandle(h);
        ok != 0 && code == STILL_ACTIVE as u32
    }
}

/// A terminal tab as the process table sees it: `<host>` (a copy of cmd.exe
/// named for a terminal, or for an editor) running `<shell>` (another copy,
/// named pwsh, kept open with /k) running a fake `claude` console. All in
/// one console of their own, which is what `imperatorium type-into`
/// attaches to. Stopped on drop.
pub struct Tab {
    launcher: Child,
    pub host: u32,
    pub shell: u32,
    pub claude: u32,
}

impl Tab {
    pub fn start(d: &Dispatcher, host_exe: &str) -> Tab {
        let dir = d.root.join(format!("tab-{host_exe}"));
        std::fs::create_dir_all(&dir).unwrap();
        let cmd_exe = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or(r"C:\Windows".into()))
            .join("System32")
            .join("cmd.exe");
        let host_path = dir.join(format!("{host_exe}.exe"));
        let shell_path = dir.join("pwsh.exe");
        let claude_path = dir.join("claude.exe");
        std::fs::copy(&cmd_exe, &host_path).unwrap();
        std::fs::copy(&cmd_exe, &shell_path).unwrap();
        std::fs::copy(fakecli(), &claude_path).unwrap();
        let launcher = start_in_new_console(
            d,
            &format!(
                "\"{}\" /c \"\"{}\" /k \"{} --fake-console\"\"",
                host_path.display(),
                shell_path.display(),
                claude_path.display()
            ),
        );
        let lp = launcher.id();
        let found = wait_until(Duration::from_secs(20), || {
            let host = children_of(lp).into_iter().find(|(_, n)| n.eq_ignore_ascii_case(&format!("{host_exe}.exe")))?;
            let shell = children_of(host.0).into_iter().find(|(_, n)| n.eq_ignore_ascii_case("pwsh.exe"))?;
            let claude = children_of(shell.0).into_iter().find(|(_, n)| n.eq_ignore_ascii_case("claude.exe"))?;
            Some((host.0, shell.0, claude.0))
        });
        let (host, shell, claude) = found.expect("the tab's host, shell and claude");
        wait_typeable(claude);
        Tab { launcher, host, shell, claude }
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        kill_tree(&mut self.launcher);
        let _ = self.launcher.wait();
    }
}
