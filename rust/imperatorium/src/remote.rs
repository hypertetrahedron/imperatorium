//! Sessions on other machines, over SSH: `imperatorium/remote.py`, with the
//! host's side done by this same binary instead of python3 (phase 4).
//!
//! * **Polling.** `claude agents --json --all` and the host's
//!   `~/.claude/sessions/*.json` in one SSH call, merged by the same
//!   `registry::merge` as local rows. Shell only: no binary needed.
//! * **The binary.** Pushed to `~/.claude/imperatorium/bin/imperatorium` on
//!   first use, by target triple (`uname -m`), and checked by SHA-256 before
//!   and after; a host whose copy already matches is not sent it again.
//! * **Channels.** One long-lived `imperatorium remote-channel` per host for
//!   conversation reads and one for transcript numbers, JSON lines over the
//!   SSH channel, as Python's Reader and Stats - but the reader is this
//!   binary's own (phase 2), so the host needs no Agent SDK venv.
//! * **Injection.** `imperatorium inbox-send SOCKET` on the host, the frames
//!   (token included) on stdin, never on a command line `ps` would show.
//! * **Hooks.** A `Tunnel` holds `ssh -N -R` open so the host's hooks reach
//!   a hook-only listener here (the server owns the listener).
//!
//! * **Containers.** A host entry with `"container": NAME` is a container:
//!   every command runs as `docker exec -i NAME sh -c ...`, on this machine
//!   (`"ssh": false`) or on the SSH host named by `"ssh"`. A container cannot
//!   reach the dispatcher's loopback port, so its hooks come back through
//!   `imperatorium hook-relay` inside it: a listener on the container's own
//!   127.0.0.1:8792 that prints each hook as a JSON line on the exec's
//!   stdout, which the `Tunnel` hands to the hook-only listener here.
//!
//! A dead host must never stall a poll: every call is bounded, each host is
//! polled on its own thread, and a failing host is parked with a backoff.

use crate::locks::LockExt;
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::pyish::{py_str, strip, take, truthy};

type Row = Map<String, Value>;

pub const DEFAULT_CLAUDE: &str = "$HOME/.local/bin/claude";
pub const DEFAULT_REMOTE_HOOK_PORT: u16 = 8792;
pub const FIRST_TUNNEL_PORT: u16 = 8793;
const CONNECT_TIMEOUT: u32 = 8;
const CALL_TIMEOUT: f64 = 25.0;
pub const READ_TIMEOUT: f64 = 15.0;
const STATS_TIMEOUT: f64 = 20.0;
// The waits below are timing::t() (scaled by IMPERATORIUM_TEST_TIME_SCALE).
pub fn poll_every() -> f64 {
    crate::timing::t().remote_poll_every
}
const FILES_MARK: &str = "--- imperatorium session files ---";
/// Where the binary lives on a host. Written in double quotes so the
/// host's shell expands $HOME.
pub const REMOTE_BIN: &str = "\"$HOME/.claude/imperatorium/bin/imperatorium\"";

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Python's "%g" for the timeouts in error text: 25.0 -> "25".
fn g(f: f64) -> String {
    if f.fract() == 0.0 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

/// shlex.quote.
pub fn sh_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[derive(Clone, Debug)]
pub struct Host {
    pub name: String,
    pub claude: String,
    pub label: String,
    /// (local_port, remote_port)
    pub tunnel: Option<(u16, u16)>,
}

impl Host {
    /// As `/api/hosts` lists it. Old configs carry `python` and
    /// `reader_python` per host (the interpreter the Python remote ran on
    /// the host); they are accepted and ignored - the binary runs there.
    pub fn public(&self) -> Map<String, Value> {
        let tunnel = match self.tunnel {
            Some((l, r)) => json!({"local_port": l, "remote_port": r}),
            None => Value::Null,
        };
        json!({"name": self.name, "claude": self.claude, "label": self.label,
               "tunnel": tunnel})
        .as_object()
        .unwrap()
        .clone()
    }
}

/// remote.load_hosts: the `remote` block of config.json. `"tunnel": true`
/// takes the next free port from FIRST_TUNNEL_PORT in config order.
pub fn load_hosts(config: &Value) -> Vec<Host> {
    let block = config.get("remote").cloned().unwrap_or(json!({}));
    if block.get("enabled").is_some_and(|e| !truthy(Some(e))) {
        return vec![];
    }
    let mut hosts = Vec::new();
    let mut wanted: Vec<Option<Map<String, Value>>> = Vec::new();
    for entry in block
        .get("hosts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let entry = match entry {
            Value::String(s) => json!({"name": s}),
            other => other,
        };
        let Some(name) = entry
            .get("name")
            .filter(|n| truthy(Some(n)))
            .map(py_str)
        else {
            continue;
        };
        let pick = |k: &str, d: &str| {
            entry
                .get(k)
                .filter(|v| truthy(Some(v)))
                .map(py_str)
                .unwrap_or_else(|| d.to_string())
        };
        let container = entry
            .get("container")
            .filter(|c| truthy(Some(c)))
            .map(py_str);
        // "ssh": false (or null) for a container on this machine; a name
        // for the SSH host it runs on; absent: the entry's own name, except
        // that a container with no "ssh" is local.
        let ssh = match entry.get("ssh") {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            Some(_) => None,
            None if container.is_some() => None,
            None => Some(name.clone()),
        };
        set_via(&name, Via { ssh, container, docker: pick("docker", "docker") });
        hosts.push(Host {
            claude: pick("claude", DEFAULT_CLAUDE),
            label: pick("label", &name),
            tunnel: None,
            name,
        });
        wanted.push(match entry.get("tunnel") {
            Some(t) if truthy(Some(t)) => Some(t.as_object().cloned().unwrap_or_default()),
            _ => None,
        });
    }
    let port_of = |v: Option<&Value>| -> Option<u16> {
        v.filter(|x| truthy(Some(x)))
            .and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))
            .map(|p| p as u16)
    };
    let mut used: Vec<u16> = wanted
        .iter()
        .flatten()
        .filter_map(|w| port_of(w.get("local_port")))
        .collect();
    let mut port = FIRST_TUNNEL_PORT;
    for (host, want) in hosts.iter_mut().zip(wanted) {
        let Some(want) = want else { continue };
        let local = match port_of(want.get("local_port")) {
            Some(p) => p,
            None => {
                while used.contains(&port) {
                    port += 1;
                }
                used.push(port);
                port
            }
        };
        let remote = port_of(want.get("remote_port")).unwrap_or(DEFAULT_REMOTE_HOOK_PORT);
        host.tunnel = Some((local, remote));
    }
    hosts
}

// -- running ssh ----------------------------------------------------------

/// Every ssh this process starts is put in one kill-on-close job, so it
/// dies with the dispatcher: an orphaned tunnel would keep the host's port,
/// and every later tunnel would fail with "remote port forwarding failed".
#[cfg(windows)]
fn bind_to_us(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return 0;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        job as usize
    });
    if job != 0 {
        unsafe {
            AssignProcessToJobObject(job as *mut core::ffi::c_void, child.as_raw_handle() as _);
        }
    }
}

#[cfg(not(windows))]
fn bind_to_us(_child: &Child) {}

/// The program run as `ssh`. IMPERATORIUM_SSH replaces it for tests (a fake
/// that runs the remote command in a sandbox).
fn ssh_program() -> String {
    crate::paths::env_nonempty("IMPERATORIUM_SSH").unwrap_or_else(|| "ssh".into())
}

pub fn ssh_base() -> Vec<String> {
    vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        format!("ConnectTimeout={CONNECT_TIMEOUT}"),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
    ]
}

/// How a host is reached: over SSH, inside a container, or both.
#[derive(Clone, Debug, PartialEq)]
pub struct Via {
    pub ssh: Option<String>,
    pub container: Option<String>,
    pub docker: String,
}

fn vias() -> &'static Mutex<HashMap<String, Via>> {
    static VIAS: std::sync::OnceLock<Mutex<HashMap<String, Via>>> = std::sync::OnceLock::new();
    VIAS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_via(host: &str, via: Via) {
    vias().lk().insert(host.to_string(), via);
}

/// A host load_hosts never saw is an SSH destination by that name.
pub fn via_of(host: &str) -> Via {
    vias().lk().get(host).cloned().unwrap_or(Via {
        ssh: Some(host.to_string()),
        container: None,
        docker: "docker".into(),
    })
}

/// The program and arguments that run `cmd` (a POSIX shell command line)
/// on `host`.
pub fn argv_for(host: &str, cmd: &str) -> (String, Vec<String>) {
    let via = via_of(host);
    match (&via.ssh, &via.container) {
        (Some(dest), None) => {
            let mut a = ssh_base();
            a.extend([dest.clone(), cmd.to_string()]);
            (ssh_program(), a)
        }
        (Some(dest), Some(c)) => {
            let mut a = ssh_base();
            a.push(dest.clone());
            a.push(format!("{} exec -i {} sh -c {}", via.docker, sh_quote(c), sh_quote(cmd)));
            (ssh_program(), a)
        }
        (None, Some(c)) => (
            via.docker.clone(),
            vec!["exec".into(), "-i".into(), c.clone(), "sh".into(), "-c".into(), cmd.to_string()],
        ),
        // "ssh": false with no container: there is nothing else to use.
        (None, None) => {
            let mut a = ssh_base();
            a.extend([host.to_string(), cmd.to_string()]);
            (ssh_program(), a)
        }
    }
}

fn command(args: &[String]) -> Command {
    command_of(&ssh_program(), args)
}

fn command_of(program: &str, args: &[String]) -> Command {
    let mut c = Command::new(program);
    c.args(args);
    crate::proc::no_window(&mut c);
    c
}

fn spawn(program: &str, args: &[String], stdin: bool) -> Result<Child, String> {
    let child = command_of(program, args)
        .stdin(if stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program} could not run: {e}"))?;
    bind_to_us(&child);
    Ok(child)
}

pub struct Done {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// One ssh call to `host` running `remote_cmd`, `input` on its stdin,
/// bounded by `timeout` seconds.
pub fn run(host: &str, remote_cmd: &str, input: Option<Vec<u8>>, timeout: f64) -> Result<Done, String> {
    let (program, args) = argv_for(host, remote_cmd);
    let mut child = spawn(&program, &args, input.is_some())?;
    if let (Some(data), Some(mut sin)) = (input, child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = sin.write_all(&data);
        });
    }
    let drain = |s: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut s) = s {
                let _ = s.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(Done {
                    code: status.code().unwrap_or(-1),
                    stdout: out.join().unwrap_or_default(),
                    stderr: err.join().unwrap_or_default(),
                })
            }
            Ok(None) if started.elapsed().as_secs_f64() < timeout => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", g(timeout)));
            }
        }
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The last non-blank line of what ssh said, or `fallback`.
fn last_line(said: &str, fallback: String) -> String {
    said.lines()
        .filter(|l| !strip(l).is_empty())
        .last()
        .map(|l| take(strip(l), 200))
        .filter(|l| !l.is_empty())
        .unwrap_or(fallback)
}

// -- polling --------------------------------------------------------------

/// remote.agents: live sessions on `host`, tagged with it.
pub fn agents(host: &Host) -> Result<Vec<Row>, String> {
    let cmd = format!(
        "{} agents --json --all && {{ echo; echo '{FILES_MARK}'; \
         for f in $HOME/.claude/sessions/*.json; do \
         [ -f \"$f\" ] && cat \"$f\" && echo; done; true; }}",
        host.claude
    );
    let done = run(&host.name, &cmd, None, CALL_TIMEOUT)?;
    let stdout = text(&done.stdout);
    if done.code != 0 {
        let said = text(if done.stderr.is_empty() { &done.stdout } else { &done.stderr });
        let said = take(strip(&said), 200);
        return Err(if said.is_empty() {
            format!("ssh exited {}", done.code)
        } else {
            said
        });
    }
    let (head, tail) = match stdout.find(FILES_MARK) {
        Some(i) => (&stdout[..i], &stdout[i + FILES_MARK.len()..]),
        None => (stdout.as_str(), ""),
    };
    let head = strip(head);
    let parsed: Value = serde_json::from_str(if head.is_empty() { "[]" } else { head })
        .map_err(|_| format!("did not return JSON: {}", take(head, 120)))?;
    let Value::Array(rows) = parsed else {
        return Ok(vec![]);
    };
    let rows: Vec<Value> = rows.into_iter().filter(Value::is_object).collect();
    let live: Vec<String> = rows
        .iter()
        .filter_map(|r| r.get("sessionId").map(py_str))
        .collect();
    let mut files: HashMap<String, Row> = HashMap::new();
    for line in tail.split('\n') {
        let line = strip(line);
        if line.is_empty() {
            continue;
        }
        if let Ok(Value::Object(rec)) = serde_json::from_str::<Value>(line) {
            if let Some(sid) = rec.get("sessionId").filter(|v| truthy(Some(v))).map(py_str) {
                if live.contains(&sid) {
                    files.insert(sid, rec);
                }
            }
        }
    }
    let mut out = crate::registry::merge(&rows, &files, &HashMap::new());
    for row in out.iter_mut() {
        row.insert("host".into(), json!(host.name));
        row.insert("label".into(), json!(host.label));
        row.insert("remote".into(), json!(true));
    }
    Ok(out)
}

// -- the binary on the host -----------------------------------------------

/// The target triple a host's `uname -m` needs.
pub fn triple_for(arch: &str) -> Option<&'static str> {
    match arch {
        "x86_64" | "amd64" => Some("x86_64-unknown-linux-musl"),
        "aarch64" | "arm64" => Some("aarch64-unknown-linux-musl"),
        _ => None,
    }
}

/// Where this machine keeps the build for `triple`: beside the installed
/// exe (`targets/<triple>/imperatorium`), or, run from a cargo target dir,
/// that dir's own cross build. IMPERATORIUM_REMOTE_BUILDS overrides both.
pub fn local_build(triple: &str) -> Option<PathBuf> {
    let mut places = Vec::new();
    if let Some(dir) = crate::paths::env_nonempty("IMPERATORIUM_REMOTE_BUILDS") {
        places.push(Path::new(&dir).join(triple).join("imperatorium"));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        places.push(dir.join("targets").join(triple).join("imperatorium"));
        if let Some(target) = dir.parent() {
            places.push(target.join(triple).join("release").join("imperatorium"));
        }
    }
    places.into_iter().find(|p| p.is_file())
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn hash_word(line: &str) -> Option<String> {
    let w = line.split_whitespace().next()?;
    (w.len() == 64 && w.chars().all(|c| c.is_ascii_hexdigit())).then(|| w.to_ascii_lowercase())
}

/// Puts this binary on one host, once: the host's copy is used while its
/// hash matches the local build, and is replaced when it does not.
pub struct Deploy {
    host: String,
    ready: Mutex<Option<String>>,
}

impl Deploy {
    pub fn new(host: &str) -> Self {
        Deploy {
            host: host.into(),
            ready: Mutex::new(None),
        }
    }

    /// Forget the check, so the next use looks again (a channel died).
    pub fn doubt(&self) {
        *self.ready.lk() = None;
    }

    pub fn status(&self) -> Value {
        match self.ready.lk().as_ref() {
            Some(h) => json!({"binary": "ok", "sha256": h}),
            None => json!({"binary": null}),
        }
    }

    /// Ok once the host has the same binary as this machine's build for it.
    pub fn ensure(&self) -> Result<(), String> {
        let mut ready = self.ready.lk();
        if ready.is_some() {
            return Ok(());
        }
        let probe = format!(
            "uname -m; sha256sum {REMOTE_BIN} 2>/dev/null || shasum -a 256 {REMOTE_BIN} 2>/dev/null; true"
        );
        let done = run(&self.host, &probe, None, CALL_TIMEOUT)?;
        if done.code != 0 {
            return Err(last_line(&text(&done.stderr), format!("ssh exited {}", done.code)));
        }
        let said = text(&done.stdout);
        let mut lines = said.lines();
        let arch = strip(lines.next().unwrap_or("")).to_string();
        let theirs = lines.find_map(hash_word);
        let triple = triple_for(&arch).ok_or_else(|| format!("no imperatorium build for {arch:?}"))?;
        let path = local_build(triple)
            .ok_or_else(|| format!("no {triple} build of imperatorium on this machine"))?;
        let data = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let ours = sha256_hex(&data);
        if theirs.as_deref() == Some(ours.as_str()) {
            *ready = Some(ours);
            return Ok(());
        }
        // Written beside, then renamed over, so a channel still running
        // the old copy keeps its file and a cut-off push leaves no half
        // binary behind.
        let push = "set -e; d=\"$HOME/.claude/imperatorium/bin\"; mkdir -p \"$d\"; \
                    cat > \"$d/.imperatorium.new\"; chmod 755 \"$d/.imperatorium.new\"; \
                    mv -f \"$d/.imperatorium.new\" \"$d/imperatorium\"; \
                    sha256sum \"$d/imperatorium\" 2>/dev/null || shasum -a 256 \"$d/imperatorium\"";
        let done = run(&self.host, push, Some(data), 180.0)?;
        if done.code != 0 {
            return Err(format!(
                "pushing the binary: {}",
                last_line(&text(&done.stderr), format!("ssh exited {}", done.code))
            ));
        }
        let got = text(&done.stdout).lines().find_map(hash_word);
        if got.as_deref() != Some(ours.as_str()) {
            return Err(format!(
                "pushed the binary, but the host's copy hashes to {}",
                got.unwrap_or_else(|| "nothing".into())
            ));
        }
        eprintln!("remote {}: imperatorium {triple} installed ({})", self.host, &ours[..12]);
        *ready = Some(ours);
        Ok(())
    }
}

// -- injection and background ---------------------------------------------

/// remote.inject: `content` into a session's inbox on `host`.
pub fn inject(host: &Host, deploy: &Deploy, socket: &str, token: Option<&str>, content: &str) -> Result<(), String> {
    if strip(content).is_empty() {
        return Err("prompt is empty".into());
    }
    let n = content.chars().count();
    if n > crate::inbox::MAX_CONTENT {
        return Err(format!("prompt is {n} chars; the cap is {}", crate::inbox::MAX_CONTENT));
    }
    if !socket.starts_with('/') {
        return Err(format!(
            "not a Unix socket path: {}",
            crate::pyish::py_repr(&Value::from(socket))
        ));
    }
    let Some(token) = token.filter(|t| !t.is_empty()) else {
        return Err("no token for that session".into());
    };
    deploy.ensure()?;
    let cmd = format!("{REMOTE_BIN} inbox-send {}", sh_quote(socket));
    let done = run(&host.name, &cmd, Some(crate::inbox::frames(Some(token), content)), 30.0)?;
    if done.code != 0 {
        return Err(last_line(&text(&done.stderr), format!("ssh exited {}", done.code)));
    }
    Ok(())
}

/// remote.start_background: `claude --bg` in `cwd` on `host`.
pub fn start_background(host: &Host, cwd: &str, prompt: &str, options: &Map<String, Value>) -> Result<Value, String> {
    if strip(prompt).is_empty() {
        return Err("prompt is empty".into());
    }
    let argv = crate::background::build_command(prompt, options);
    // The claude path is deliberately not quoted: it comes from config,
    // written with $HOME, and quoting would stop the host expanding it.
    let cmd = format!(
        "cd {} && {} {}",
        sh_quote(cwd),
        host.claude,
        argv[1..].iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" ")
    );
    let done = run(&host.name, &cmd, None, 120.0)?;
    let stdout = text(&done.stdout);
    let output = strip(&(stdout.clone() + &text(&done.stderr))).to_string();
    if done.code != 0 {
        return Err(if output.is_empty() {
            format!("ssh exited {}", done.code)
        } else {
            take(&output, 300)
        });
    }
    let Some(id) = crate::background::parse_id(&stdout) else {
        return Err(format!("started, but no session id was printed: {}", take(&output, 200)));
    };
    Ok(json!({"id": id, "host": host.name, "output": take(&output, 400)}))
}

// -- channels -------------------------------------------------------------

enum Line {
    Out(String),
    Err(String),
    OutEnd,
    ErrEnd,
}

struct Live {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Line>,
}

struct ChannelState {
    live: Option<Live>,
    seq: u64,
    failed_at: f64,
    error: Option<String>,
}

/// One long-lived `imperatorium remote-channel` on a host. Requests are
/// serialised; a request that times out kills the channel rather than
/// leaving its reply to be mistaken for the next one's.
pub struct Channel {
    host: String,
    deploy: Arc<Deploy>,
    state: Mutex<ChannelState>,
}

impl Channel {
    pub fn new(host: &str, deploy: Arc<Deploy>) -> Self {
        Channel {
            host: host.into(),
            deploy,
            state: Mutex::new(ChannelState {
                live: None,
                seq: 0,
                failed_at: 0.0,
                error: None,
            }),
        }
    }

    fn start(&self) -> Result<Live, String> {
        self.deploy.ensure()?;
        let (program, args) = argv_for(&self.host, &format!("{REMOTE_BIN} remote-channel"));
        let mut child = spawn(&program, &args, true)?;
        let stdin = child.stdin.take().unwrap();
        let (tx, rx) = channel();
        let out = child.stdout.take().unwrap();
        let err = child.stderr.take().unwrap();
        let t1 = tx.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(out).lines() {
                match line {
                    Ok(l) => {
                        if t1.send(Line::Out(l)).is_err() {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = t1.send(Line::OutEnd);
        });
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(err).lines() {
                match line {
                    Ok(l) => {
                        if tx.send(Line::Err(l)).is_err() {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(Line::ErrEnd);
        });
        Ok(Live {
            child,
            stdin,
            lines: rx,
        })
    }

    fn stop(st: &mut ChannelState, error: Option<String>) {
        if let Some(mut live) = st.live.take() {
            let _ = live.child.kill();
            let _ = live.child.wait();
        }
        st.failed_at = now();
        st.error = error;
    }

    /// What a dying channel wrote to stderr, waiting briefly for it.
    fn last_words(lines: &Receiver<Line>, said: &mut Vec<String>) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match lines.recv_timeout(left) {
                Ok(Line::Err(l)) if !strip(&l).is_empty() => said.push(strip(&l).to_string()),
                Ok(Line::ErrEnd) | Err(_) => break,
                Ok(_) => {}
            }
        }
    }

    /// One request and its reply.
    pub fn call(&self, mut req: Map<String, Value>, timeout: f64) -> Result<Map<String, Value>, String> {
        let mut st = self.state.lk();
        if st.live.is_none() {
            // A host that just failed is not re-dialled on every poll.
            if let Some(e) = &st.error {
                if now() - st.failed_at < crate::timing::t().remote_channel_retry {
                    return Err(e.clone());
                }
            }
            match self.start() {
                Ok(live) => st.live = Some(live),
                Err(e) => {
                    Self::stop(&mut st, Some(e.clone()));
                    return Err(e);
                }
            }
        }
        st.seq += 1;
        let seq = st.seq;
        req.insert("id".into(), json!(seq));
        let line = serde_json::to_string(&Value::Object(req)).unwrap() + "\n";
        let wrote = {
            let live = st.live.as_mut().unwrap();
            live.stdin.write_all(line.as_bytes()).and_then(|_| live.stdin.flush())
        };
        if let Err(e) = wrote {
            let mut said = Vec::new();
            Self::last_words(&st.live.as_ref().unwrap().lines, &mut said);
            let why = said.last().cloned().unwrap_or_else(|| format!("channel closed: {e}"));
            let why = take(&why, 200);
            Self::stop(&mut st, Some(why.clone()));
            self.deploy.doubt();
            return Err(why);
        }
        let deadline = Instant::now() + Duration::from_secs_f64(timeout);
        let mut stderr = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let got = st.live.as_ref().unwrap().lines.recv_timeout(left);
            match got {
                Err(RecvTimeoutError::Timeout) => {
                    let why = format!("no reply in {}s", g(timeout));
                    Self::stop(&mut st, Some(why.clone()));
                    return Err(why);
                }
                Ok(Line::Err(l)) => {
                    if !strip(&l).is_empty() {
                        stderr.push(strip(&l).to_string());
                    }
                }
                Ok(Line::ErrEnd) => {}
                Ok(Line::OutEnd) | Err(RecvTimeoutError::Disconnected) => {
                    Self::last_words(&st.live.as_ref().unwrap().lines, &mut stderr);
                    let why = take(stderr.last().map(String::as_str).unwrap_or("reader exited"), 200);
                    Self::stop(&mut st, Some(why.clone()));
                    // Perhaps the binary went missing: check it again.
                    self.deploy.doubt();
                    return Err(why);
                }
                Ok(Line::Out(l)) => {
                    let Ok(Value::Object(reply)) = serde_json::from_str::<Value>(&l) else {
                        continue; // stray output from the host's shell
                    };
                    if reply.get("id").and_then(Value::as_u64) != Some(seq) {
                        continue; // a reply to a request that already timed out
                    }
                    st.error = None;
                    if !truthy(reply.get("ok")) {
                        return Err(reply
                            .get("error")
                            .filter(|e| truthy(Some(e)))
                            .map(py_str)
                            .unwrap_or_else(|| "read failed".into()));
                    }
                    return Ok(reply);
                }
            }
        }
    }

    /// Reader.read: {items, title} for a session on this host.
    pub fn read(&self, session_id: &str, limit: usize, cwd: Option<&str>) -> Result<(Vec<Value>, String), String> {
        let mut req = json!({"op": "read", "session": session_id, "limit": limit})
            .as_object()
            .unwrap()
            .clone();
        if let Some(c) = cwd.filter(|c| !c.is_empty()) {
            req.insert("cwd".into(), json!(c));
        }
        let reply = self.call(req, READ_TIMEOUT)?;
        let items = reply.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        let title = reply
            .get("title")
            .filter(|t| truthy(Some(t)))
            .map(py_str)
            .unwrap_or_default();
        Ok((items, title))
    }

    /// The task list, read on the host (taskout.awaited there).
    pub fn awaited(&self, session_id: &str) -> Result<Vec<Value>, String> {
        let req = json!({"op": "awaited", "session": session_id}).as_object().unwrap().clone();
        let reply = self.call(req, READ_TIMEOUT)?;
        Ok(reply.get("tasks").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// A subagent as a chat, read on the host (taskout.subagent_view there).
    pub fn subagent(&self, session_id: &str, agent: &str) -> Result<Value, String> {
        let req = json!({"op": "subagent", "session": session_id, "agent": agent})
            .as_object()
            .unwrap()
            .clone();
        let mut reply = self.call(req, READ_TIMEOUT)?;
        reply.remove("id");
        Ok(Value::Object(reply))
    }

    /// The session timeline, read on the host (timeline.of_session there).
    pub fn timeline(&self, session_id: &str, known: &str) -> Result<Value, String> {
        let req = json!({"op": "timeline", "session": session_id, "sig": known}).as_object().unwrap().clone();
        let mut reply = self.call(req, READ_TIMEOUT)?;
        reply.remove("id");
        Ok(Value::Object(reply))
    }

    /// Stats.read: {session id: numbers} for those it has a transcript of.
    pub fn stats(&self, ids: &[String]) -> Result<Map<String, Value>, String> {
        let req = json!({"op": "stats", "sessions": ids}).as_object().unwrap().clone();
        let reply = self.call(req, STATS_TIMEOUT)?;
        Ok(reply.get("stats").and_then(Value::as_object).cloned().unwrap_or_default())
    }
}

// -- the fleet ------------------------------------------------------------

struct PerHost {
    host: Host,
    deploy: Arc<Deploy>,
    reader: Channel,
    stats: Channel,
    wake: Arc<(Mutex<bool>, Condvar)>,
}

/// remote.Fleet: polls a set of hosts, parking the ones that are failing.
pub struct Fleet {
    pub hosts: Vec<Host>,
    every: f64,
    each: Vec<PerHost>,
    state: Mutex<HashMap<String, Map<String, Value>>>,
    rows: Mutex<HashMap<String, Vec<Row>>>,
}

impl Fleet {
    pub fn new(hosts: Vec<Host>, every: f64) -> Arc<Fleet> {
        let each = hosts
            .iter()
            .map(|h| {
                let deploy = Arc::new(Deploy::new(&h.name));
                PerHost {
                    host: h.clone(),
                    reader: Channel::new(&h.name, deploy.clone()),
                    stats: Channel::new(&h.name, deploy.clone()),
                    deploy,
                    wake: Arc::new((Mutex::new(false), Condvar::new())),
                }
            })
            .collect();
        Arc::new(Fleet {
            hosts,
            every,
            each,
            state: Mutex::new(HashMap::new()),
            rows: Mutex::new(HashMap::new()),
        })
    }

    fn per(&self, name: &str) -> Option<&PerHost> {
        self.each.iter().find(|p| p.host.name == name)
    }

    pub fn host(&self, name: &str) -> Option<&Host> {
        self.per(name).map(|p| &p.host)
    }

    pub fn status(&self) -> Map<String, Value> {
        let st = self.state.lk();
        let mut out = Map::new();
        for h in &self.hosts {
            if let Some(rec) = st.get(&h.name) {
                let mut rec = rec.clone();
                if let Some(p) = self.per(&h.name) {
                    if let Value::Object(d) = p.deploy.status() {
                        rec.extend(d);
                    }
                }
                out.insert(h.name.clone(), Value::Object(rec));
            }
        }
        out
    }

    fn due(&self, name: &str) -> bool {
        match self.state.lk().get(name) {
            None => true,
            Some(rec) => now() >= rec.get("next_attempt").and_then(Value::as_f64).unwrap_or(0.0),
        }
    }

    fn ok(&self, name: &str) {
        let rec = json!({"fails": 0, "next_attempt": 0, "error": null, "polled_at": now()});
        self.state
            .lk()
            .insert(name.into(), rec.as_object().unwrap().clone());
    }

    fn failed(&self, name: &str, error: &str) {
        let mut st = self.state.lk();
        let rec = st.get(name).cloned().unwrap_or_default();
        let fails = rec.get("fails").and_then(Value::as_u64).unwrap_or(0) + 1;
        let delay = (crate::timing::t().remote_backoff_start * 2f64.powi(fails as i32 - 1)).min(crate::timing::t().remote_backoff_max);
        let out = json!({"fails": fails, "next_attempt": now() + delay,
                         "error": take(error, 200), "parked_for": delay,
                         "polled_at": rec.get("polled_at").cloned().unwrap_or(Value::Null)});
        st.insert(name.into(), out.as_object().unwrap().clone());
    }

    /// Fleet.poll_host: never fails; a failure parks the host and its
    /// sessions leave the board until it answers.
    fn poll_host(&self, p: &PerHost) {
        let name = &p.host.name;
        let rows = match agents(&p.host) {
            Ok(mut rows) => {
                self.ok(name);
                Self::attach_stats(p, &mut rows);
                rows
            }
            Err(e) => {
                self.failed(name, &e);
                vec![]
            }
        };
        self.rows.lk().insert(name.clone(), rows);
    }

    /// Each row's transcript numbers as `row["stat"]`: extra, never needed.
    fn attach_stats(p: &PerHost, rows: &mut [Row]) {
        let ids: Vec<String> = rows
            .iter()
            .filter_map(|r| r.get("sessionId").filter(|v| truthy(Some(v))).map(py_str))
            .collect();
        if ids.is_empty() {
            return;
        }
        let Ok(got) = p.stats.stats(&ids) else {
            return;
        };
        for row in rows.iter_mut() {
            let sid = row.get("sessionId").map(py_str).unwrap_or_default();
            if let Some(s) = got.get(&sid) {
                row.insert("stat".into(), s.clone());
            }
        }
    }

    /// Every host's latest rows, in config order.
    pub fn rows(&self) -> Vec<Row> {
        let rows = self.rows.lk();
        self.hosts
            .iter()
            .flat_map(|h| rows.get(&h.name).cloned().unwrap_or_default())
            .collect()
    }

    /// A polling thread per host; `on_change` after each poll.
    pub fn start(self: &Arc<Self>, on_change: Arc<dyn Fn() + Send + Sync>) {
        for i in 0..self.each.len() {
            let me = Arc::clone(self);
            let on_change = on_change.clone();
            std::thread::Builder::new()
                .name(format!("remote-{}", self.each[i].host.name))
                .spawn(move || me.run_loop(i, on_change))
                .ok();
        }
    }

    fn run_loop(&self, i: usize, on_change: Arc<dyn Fn() + Send + Sync>) {
        let p = &self.each[i];
        loop {
            if self.due(&p.host.name) {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.poll_host(p)));
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_change()));
            }
            let parked = self
                .state
                .lk()
                .get(&p.host.name)
                .and_then(|r| r.get("next_attempt").and_then(Value::as_f64))
                .unwrap_or(0.0);
            let wait = if parked > 0.0 { (parked - now()).max(crate::timing::secs(1.0)) } else { self.every };
            let (lock, cv) = &*p.wake;
            let woken = lock.lk();
            let (mut woken, _) = cv
                .wait_timeout_while(woken, Duration::from_secs_f64(wait), |w| !*w)
                .unwrap();
            *woken = false;
        }
    }

    /// Fleet.refresh: ask every host again now, parked or not.
    pub fn refresh(&self) -> usize {
        for rec in self.state.lk().values_mut() {
            rec.insert("next_attempt".into(), json!(0));
        }
        for p in &self.each {
            let (lock, cv) = &*p.wake;
            *lock.lk() = true;
            cv.notify_all();
        }
        self.hosts.len()
    }

    /// The conversation reader's channel for `host`.
    pub fn read(&self, host: &str, session_id: &str, limit: usize, cwd: Option<&str>) -> Result<(Vec<Value>, String), String> {
        let p = self.per(host).ok_or_else(|| format!("host {host} is not configured"))?;
        p.reader.read(session_id, limit, cwd)
    }

    pub fn inject(&self, host: &str, socket: &str, token: Option<&str>, content: &str) -> Result<(), String> {
        let p = self.per(host).ok_or_else(|| format!("host {host} is not configured"))?;
        inject(&p.host, &p.deploy, socket, token, content)
    }

    pub fn awaited(&self, host: &str, session_id: &str) -> Result<Vec<Value>, String> {
        let p = self.per(host).ok_or_else(|| format!("host {host} is not configured"))?;
        p.reader.awaited(session_id)
    }

    pub fn subagent(&self, host: &str, session_id: &str, agent: &str) -> Result<Value, String> {
        let p = self.per(host).ok_or_else(|| format!("host {host} is not configured"))?;
        p.reader.subagent(session_id, agent)
    }

    pub fn timeline(&self, host: &str, session_id: &str, known: &str) -> Result<Value, String> {
        let p = self.per(host).ok_or_else(|| format!("host {host} is not configured"))?;
        p.reader.timeline(session_id, known)
    }
}

// -- tunnels --------------------------------------------------------------

/// remote.Tunnel: keeps `ssh -N -R` open so the host's hooks reach this
/// machine - `127.0.0.1:<remote_port>` there leads to the hook-only
/// listener on `127.0.0.1:<local_port>` here, never the dispatcher's port.
pub struct Tunnel {
    pub host: String,
    pub local_port: u16,
    pub remote_port: u16,
    state: Mutex<Map<String, Value>>,
}

impl Tunnel {
    pub fn new(host: &str, local_port: u16, remote_port: u16) -> Arc<Tunnel> {
        Arc::new(Tunnel {
            host: host.into(),
            local_port,
            remote_port,
            state: Mutex::new(
                json!({"up": false, "since": null, "fails": 0, "error": null})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        })
    }

    pub fn argv(&self) -> Vec<String> {
        let mut a = ssh_base();
        for x in [
            "-N",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-R",
        ] {
            a.push(x.into());
        }
        a.push(format!("127.0.0.1:{}:127.0.0.1:{}", self.remote_port, self.local_port));
        a.push(self.host.clone());
        a
    }

    pub fn status(&self) -> Value {
        let mut out = self.state.lk().clone();
        out.insert("local_port".into(), json!(self.local_port));
        out.insert("remote_port".into(), json!(self.remote_port));
        Value::Object(out)
    }

    pub fn start(self: &Arc<Self>) {
        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("tunnel-{}", self.host))
            .spawn(move || me.run())
            .ok();
    }

    fn down(&self, reason: &str) -> f64 {
        let mut st = self.state.lk();
        let fails = st.get("fails").and_then(Value::as_u64).unwrap_or(0) + 1;
        let delay = (crate::timing::t().remote_relay_backoff_start * 2f64.powi(fails as i32 - 1)).min(crate::timing::t().remote_backoff_max);
        st.insert("up".into(), json!(false));
        st.insert("since".into(), Value::Null);
        st.insert("fails".into(), json!(fails));
        st.insert("error".into(), json!(reason));
        st.insert("retry_in".into(), json!(delay));
        delay
    }

    /// A container's hooks: `hook-relay` inside it, each line it prints
    /// posted to the hook-only listener as a host's tunnel would deliver it.
    fn run_relay(&self) {
        let url = format!("http://127.0.0.1:{}/hook", self.local_port);
        // The relay is the binary, which a fresh container does not have
        // until something opens a channel; its hooks would be lost before.
        let deploy = Deploy::new(&self.host);
        loop {
            let started = now();
            if let Err(e) = deploy.ensure() {
                let delay = self.down(&format!("the binary: {e}"));
                std::thread::sleep(Duration::from_secs_f64(delay));
                continue;
            }
            deploy.doubt();
            let (program, args) =
                argv_for(&self.host, &format!("{REMOTE_BIN} hook-relay --port {}", self.remote_port));
            // stdin is held open: the relay leaves when it closes, since
            // killing `docker exec` does not end what it started.
            let child = command_of(&program, &args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            let mut child = match child {
                Ok(c) => c,
                Err(e) => {
                    self.down(&format!("{program} could not run: {e}"));
                    std::thread::sleep(Duration::from_secs_f64(crate::timing::t().remote_backoff_start));
                    continue;
                }
            };
            bind_to_us(&child);
            let _stdin = child.stdin.take();
            let err = child.stderr.take().map(|mut e| {
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    let _ = e.read_to_end(&mut buf);
                    buf
                })
            });
            if let Some(out) = child.stdout.take() {
                for line in std::io::BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    if line == "ready" {
                        let mut st = self.state.lk();
                        st.insert("up".into(), json!(true));
                        st.insert("since".into(), json!(started));
                        st.insert("error".into(), Value::Null);
                        continue;
                    }
                    if serde_json::from_str::<Value>(&line).is_ok() {
                        let _ = crate::http::post_json(&url, &line, Duration::from_secs(5));
                    }
                }
            }
            let _ = child.kill();
            let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            let said = err.and_then(|h| h.join().ok()).unwrap_or_default();
            let reason = last_line(&text(&said), format!("hook relay exited {code}"));
            if now() - started > 120.0 {
                self.state.lk().insert("fails".into(), json!(0));
            }
            let delay = self.down(&reason);
            std::thread::sleep(Duration::from_secs_f64(delay));
        }
    }

    fn run(&self) {
        if via_of(&self.host).container.is_some() {
            return self.run_relay();
        }
        loop {
            let started = now();
            let child = command(&self.argv())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn();
            let mut child = match child {
                Ok(c) => c,
                Err(e) => {
                    self.down(&format!("ssh could not run: {e}"));
                    std::thread::sleep(Duration::from_secs_f64(crate::timing::t().remote_backoff_start));
                    continue;
                }
            };
            bind_to_us(&child);
            {
                let mut st = self.state.lk();
                st.insert("up".into(), json!(true));
                st.insert("since".into(), json!(started));
                st.insert("error".into(), Value::Null);
            }
            let mut err = Vec::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = e.read_to_end(&mut err);
            }
            let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            let reason = last_line(&text(&err), format!("ssh exited {code}"));
            // A tunnel that held a while and dropped is a network blip, not
            // a broken host: no long backoff for it.
            if now() - started > 120.0 {
                self.state.lk().insert("fails".into(), json!(0));
            }
            let delay = self.down(&reason);
            std::thread::sleep(Duration::from_secs_f64(delay));
        }
    }
}

// -- probing from the command line ----------------------------------------

/// `imperatorium remote-probe [--home DIR] [--read SESSION] [HOST...]`: one
/// poll of each host (deploying the binary on the way, for the stats), as
/// Python's `python -m imperatorium.remote`.
pub fn probe_main(args: &[String], home: Option<&str>) -> i32 {
    let mut names = Vec::new();
    let mut read = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--home" => {
                it.next();
            }
            "--read" => read = it.next().cloned(),
            x if x.starts_with("--") => {}
            x => names.push(x.to_string()),
        }
    }
    let hosts = if names.is_empty() {
        let path = crate::paths::config_home(home).join("config.json");
        let cfg = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(json!({}));
        load_hosts(&cfg)
    } else {
        load_hosts(&json!({"remote": {"hosts": names}}))
    };
    if hosts.is_empty() {
        eprintln!("no hosts configured (see the `remote` block of config.json)");
        return 1;
    }
    let fleet = Fleet::new(hosts, poll_every());
    let mut total = 0;
    for p in &fleet.each {
        fleet.poll_host(p);
    }
    let rows = fleet.rows();
    println!("{} session(s) across {} host(s)", rows.len(), fleet.hosts.len());
    for row in &rows {
        total += 1;
        let s = |k: &str| row.get(k).filter(|v| truthy(Some(v))).map(py_str).unwrap_or_default();
        let name = if s("name").is_empty() { s("sessionId") } else { s("name") };
        let state = if s("state").is_empty() { s("status") } else { s("state") };
        println!(
            "  {:<16} {:<26} {:<11} {:<8} {} {}",
            s("host"),
            take(&name, 26),
            s("kind"),
            state,
            if s("socket").is_empty() { "no inbox" } else { "inbox" },
            if row.contains_key("stat") { "stat" } else { "no stat" }
        );
    }
    for (name, rec) in fleet.status() {
        println!("  {name:<16} {}", Value::Object(rec.as_object().cloned().unwrap_or_default()));
    }
    if let Some(sid) = read {
        let host = rows
            .iter()
            .find(|r| r.get("sessionId").map(py_str).as_deref() == Some(sid.as_str()))
            .and_then(|r| r.get("host").map(py_str))
            .unwrap_or_else(|| fleet.hosts[0].name.clone());
        match fleet.read(&host, &sid, 0, None) {
            Ok((items, title)) => println!("read {sid} on {host}: {} items, title {title:?}", items.len()),
            Err(e) => println!("read {sid} on {host}: error {e}"),
        }
    }
    let _ = total;
    0
}

// -- the host's side ------------------------------------------------------

/// `imperatorium remote-channel`: run on a host over SSH. JSON lines in,
/// JSON lines out: {"id", "op": "read", "session", "limit"} answers
/// {"id", "ok", "items", "title"}; {"id", "op": "stats", "sessions"}
/// answers {"id", "ok", "stats": {sid: numbers}}.
pub fn channel_main() -> i32 {
    const KEEP: &[&str] = &[
        "cache",
        "context_tokens",
        "model_id",
        "last_reply_at",
        "tokens",
        "agents",
        "shells",
        "monitors",
        "wakeups",
    ];
    let stats = crate::sessionstat::Reader::new();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if strip(&line).is_empty() {
            continue;
        }
        let req: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let reply = match req.get("op").and_then(Value::as_str) {
            Some("read") => {
                let sid = req.get("session").map(py_str).unwrap_or_default();
                let limit = req.get("limit").and_then(Value::as_u64).unwrap_or(60) as usize;
                match crate::sessions::chain_json(&sid) {
                    Ok(chain) => {
                        let chain = chain.as_array().cloned().unwrap_or_default();
                        let items = crate::conversation::to_items(
                            &crate::conversation::messages(&chain),
                            limit,
                            true,
                        );
                        let info = crate::sessions::info_json(&sid);
                        let title = info
                            .get("custom_title")
                            .filter(|t| truthy(Some(t)))
                            .or_else(|| info.get("summary").filter(|t| truthy(Some(t))))
                            .map(py_str)
                            .unwrap_or_default();
                        json!({"id": id, "ok": true, "items": items, "title": title})
                    }
                    Err(e) => json!({"id": id, "ok": false, "error": format!("RuntimeError: {e}")}),
                }
            }
            Some("stats") => {
                let t = now();
                let mut got = Map::new();
                for sid in req.get("sessions").and_then(Value::as_array).into_iter().flatten() {
                    let sid = py_str(sid);
                    if let Some(Value::Object(one)) = stats.read(&sid, t) {
                        let kept: Map<String, Value> = KEEP
                            .iter()
                            .filter_map(|k| one.get(*k).map(|v| (k.to_string(), v.clone())))
                            .collect();
                        got.insert(sid, Value::Object(kept));
                    }
                }
                json!({"id": id, "ok": true, "stats": got})
            }
            Some("awaited") => {
                let sid = req.get("session").map(py_str).unwrap_or_default();
                let t = now();
                let stat = stats.read(&sid, t).unwrap_or_else(|| json!({}));
                let ran = stat.get("finished").and_then(Value::as_array).is_some_and(|f| {
                    f.iter().any(|x| x.get("kind").and_then(Value::as_str) == Some("agents"))
                });
                let agents = if truthy(stat.get("agents")) || ran {
                    crate::taskout::subagents_dir(crate::sessionstat::transcript_path(&sid).as_deref())
                } else {
                    None
                };
                let folder = crate::taskout::tasks_dir(&sid);
                let tasks = crate::taskout::awaited(&stat, folder.as_deref(), agents.as_deref());
                json!({"id": id, "ok": true, "tasks": tasks})
            }
            Some("subagent") => {
                let sid = req.get("session").map(py_str).unwrap_or_default();
                let agent = req.get("agent").map(py_str).unwrap_or_default();
                let mut v = crate::taskout::subagent_view(&sid, &agent, now());
                v.as_object_mut().unwrap().insert("id".into(), id.clone());
                v
            }
            Some("timeline") => {
                let sid = req.get("session").map(py_str).unwrap_or_default();
                let known = req.get("sig").map(py_str).unwrap_or_default();
                let mut v = crate::timeline::of_session(&sid, &known);
                v.as_object_mut().unwrap().insert("id".into(), id.clone());
                v
            }
            _ => json!({"id": id, "ok": false, "error": "ValueError: unknown op"}),
        };
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
    0
}

/// `imperatorium hook-relay [--port N]`, inside a container: the hooks'
/// endpoint (127.0.0.1:N/hook, the hooks' default 8792), each body printed
/// as one JSON line for the dispatcher reading this process's stdout. It
/// prints `ready` once listening and leaves when its stdin closes.
pub fn hook_relay_main(args: &[String]) -> i32 {
    let port = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(DEFAULT_REMOTE_HOOK_PORT);
    let server = match tiny_http::Server::http(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("hook-relay: cannot listen on 127.0.0.1:{port}: {e}");
            return 1;
        }
    };
    std::thread::spawn(|| {
        let mut sink = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut sink);
        std::process::exit(0);
    });
    let out = Arc::new(Mutex::new(std::io::stdout()));
    {
        let mut o = out.lk();
        if writeln!(o, "ready").and_then(|_| o.flush()).is_err() {
            return 1;
        }
    }
    let reply = |request: tiny_http::Request, code: u16, body: Value| {
        let header = tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
        let _ = request.respond(
            tiny_http::Response::from_string(body.to_string()).with_status_code(code).with_header(header),
        );
    };
    for mut request in server.incoming_requests() {
        let path = request.url().split('?').next().unwrap_or("").trim_end_matches('/').to_string();
        if request.method() != &tiny_http::Method::Post || path != "/hook" {
            reply(request, 404, json!({"error": "no such path"}));
            continue;
        }
        let mut raw = Vec::new();
        let _ = Read::take(request.as_reader(), 4 * 1024 * 1024 + 1).read_to_end(&mut raw);
        let Ok(v) = serde_json::from_slice::<Value>(&raw) else {
            reply(request, 400, json!({"error": "body is not JSON"}));
            continue;
        };
        if raw.len() > 4 * 1024 * 1024 || !v.is_object() {
            reply(request, 413, json!({"error": "body must be a JSON object under 4 MiB"}));
            continue;
        }
        {
            let mut o = out.lk();
            if writeln!(o, "{v}").and_then(|_| o.flush()).is_err() {
                return 0;
            }
        }
        reply(request, 200, json!({"ok": true}));
    }
    0
}

/// `imperatorium inbox-send SOCKET`: the frames on stdin, written to a
/// session's inbox socket on this host.
pub fn inbox_send_main(args: &[String]) -> i32 {
    let Some(socket) = args.first() else {
        eprintln!("usage: imperatorium inbox-send SOCKET < frames");
        return 2;
    };
    let mut data = Vec::new();
    if std::io::stdin().read_to_end(&mut data).is_err() || data.is_empty() {
        eprintln!("nothing to send");
        return 2;
    }
    match crate::inbox::write_raw(socket, &data) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containers_run_through_docker_exec() {
        let hosts = load_hosts(&json!({"remote": {"hosts": [
            "plainhost",
            {"name": "box-local", "container": "box1", "ssh": false},
            {"name": "box-remote", "container": "box 2", "ssh": "devhost", "docker": "podman"},
            {"name": "box-default", "container": "box3"}
        ]}}));
        assert_eq!(hosts.len(), 4);
        let (p, a) = argv_for("plainhost", "uname -m");
        assert_eq!(p, ssh_program());
        assert_eq!(&a[a.len() - 2..], ["plainhost", "uname -m"]);
        assert_eq!(
            argv_for("box-local", "echo $HOME"),
            ("docker".into(), ["exec", "-i", "box1", "sh", "-c", "echo $HOME"].map(String::from).to_vec())
        );
        let (p, a) = argv_for("box-remote", "echo $HOME");
        assert_eq!(p, ssh_program());
        assert_eq!(&a[a.len() - 2..], ["devhost", "podman exec -i 'box 2' sh -c 'echo $HOME'"]);
        assert_eq!(argv_for("box-default", "x").0, "docker");
        // Never configured: an SSH destination, as before.
        assert_eq!(argv_for("elsewhere", "x").1.last().unwrap(), "x");
    }

    #[test]
    fn quotes_like_shlex() {
        assert_eq!(sh_quote(""), "''");
        assert_eq!(sh_quote("/tmp/a.sock"), "/tmp/a.sock");
        assert_eq!(sh_quote("it's here"), "'it'\"'\"'s here'");
        assert_eq!(sh_quote("$HOME"), "'$HOME'");
    }

    #[test]
    fn loads_hosts_like_python() {
        let cfg = json!({"remote": {"hosts": [
            {"name": "a", "tunnel": true},
            "b",
            {"name": "c", "tunnel": {"local_port": 8793}},
            {"name": "d", "label": "Dee", "tunnel": {"remote_port": 9000}},
            {"label": "no name"}
        ]}});
        let hosts = load_hosts(&cfg);
        let names: Vec<_> = hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "d"]);
        assert_eq!(hosts[0].tunnel, Some((8794, 8792))); // 8793 is c's
        assert_eq!(hosts[1].tunnel, None);
        assert_eq!(hosts[2].tunnel, Some((8793, 8792)));
        assert_eq!(hosts[3].tunnel, Some((8795, 9000)));
        assert_eq!(hosts[3].label, "Dee");
        assert_eq!(hosts[1].claude, DEFAULT_CLAUDE);
        assert!(load_hosts(&json!({"remote": {"enabled": false, "hosts": ["a"]}})).is_empty());
    }

    #[test]
    fn old_python_fields_are_accepted_and_ignored() {
        let cfg = json!({"remote": {"hosts": [
            {"name": "a", "python": "/usr/bin/python3.11", "reader_python": "/opt/py", "claude": "cl"}
        ]}});
        let hosts = load_hosts(&cfg);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].claude, "cl");
        let shown = hosts[0].public();
        assert!(!shown.contains_key("python") && !shown.contains_key("reader_python"));
    }

    #[test]
    fn triples() {
        assert_eq!(triple_for("x86_64"), Some("x86_64-unknown-linux-musl"));
        assert_eq!(triple_for("aarch64"), Some("aarch64-unknown-linux-musl"));
        assert_eq!(triple_for("armv7l"), None);
        assert_eq!(hash_word(&format!("{} /x", "A".repeat(64))), Some("a".repeat(64)));
        assert_eq!(hash_word("nope /x"), None);
    }
}
