//! The integration tests' harness: the binary under test, goldens recorded
//! from the Python reference (tests/golden/, see its README), temporary
//! folders, a stripped environment, `imperatorium serve` on a free port,
//! HTTP helpers, the fake CLI on a stand-in PATH, and a JSON comparison with
//! Python's equality.
//!
//! Everything a test starts is its own: a temp folder per test, port 0
//! (the system picks), no fixed sleeps. Safe to run in parallel.
#![allow(dead_code)]

pub mod git;

use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

pub mod console;
pub mod fakessh;
pub mod safety;

// ---------------------------------------------------------------- places

/// The binary under test: the one cargo builds for the integration tests,
/// or $IMPERATORIUM_TEST_BIN (e.g. the release build, for the local corpus
/// at release speed).
pub fn bin() -> PathBuf {
    std::env::var_os("IMPERATORIUM_TEST_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_imperatorium")))
}

/// The checkout: rust/imperatorium/../..
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("checkout")
        .to_path_buf()
}

pub fn golden_dir(part: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(part)
}

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

pub fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The system temp folder as Python's tempfile.gettempdir() spells it (no
/// trailing separator).
pub fn system_temp() -> PathBuf {
    let t = std::env::temp_dir();
    // The long spelling: a TEMP given in 8.3 form (C:\Users\RUNNER~1 on
    // GitHub's runners) is not what git, or the binary through git, reports,
    // so paths the tests map back to placeholders would not match.
    #[cfg(windows)]
    let t = std::fs::canonicalize(&t)
        .ok()
        .and_then(|c| c.to_str().map(|s| PathBuf::from(s.trim_start_matches(r"\\?\"))))
        .unwrap_or(t);
    let s = t.to_string_lossy();
    PathBuf::from(s.trim_end_matches(['\\', '/']).to_string())
}

pub fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn read_jsonl(path: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("{} line {}: {e}", path.display(), i + 1))
        })
        .collect()
}

// ---------------------------------------------------------- placeholders

/// Puts back what tests/golden's placeholders stand for (rust/conformance/
/// golden.py `Places`): `<name>`, `<name-lc>`, `<name/>`, `<name/-lc>`, and
/// the JSON-escaped `<j:...>` and percent-encoded `<u:...>` forms of each.
pub struct Places {
    pairs: Vec<(String, String)>,
}

fn normcase(s: &str) -> String {
    if cfg!(windows) {
        s.to_lowercase().replace('/', "\\")
    } else {
        s.to_string()
    }
}

/// urllib.parse.quote(s, safe="").
pub fn quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn json_escaped(s: &str) -> String {
    let t = serde_json::to_string(&Value::String(s.to_string())).unwrap();
    t[1..t.len() - 1].to_string()
}

impl Places {
    /// repo, repo-parent and home, plus `extra` (name, this run's path).
    pub fn new(extra: &[(&str, &Path)]) -> Places {
        let repo = repo();
        let mut bases: Vec<(String, String)> = vec![
            ("repo".into(), repo.to_string_lossy().into_owned()),
            (
                "repo-parent".into(),
                repo.parent().unwrap().to_string_lossy().into_owned(),
            ),
            ("home".into(), home_dir().to_string_lossy().into_owned()),
        ];
        for (n, p) in extra {
            bases.push((n.to_string(), p.to_string_lossy().into_owned()));
        }
        let mut pairs = Vec::new();
        for (name, base) in bases {
            let variants = [
                base.clone(),
                normcase(&base),
                base.replace('\\', "/"),
                normcase(&base).replace('\\', "/"),
            ];
            let tags = [
                name.to_string(),
                format!("{name}-lc"),
                format!("{name}/"),
                format!("{name}/-lc"),
            ];
            for (tag, v) in tags.iter().zip(variants.iter()) {
                pairs.push((format!("<{tag}>"), v.clone()));
                pairs.push((format!("<j:{tag}>"), json_escaped(v)));
                pairs.push((format!("<u:{tag}>"), quote(v)));
            }
        }
        Places { pairs }
    }

    pub fn text(&self, s: &str) -> String {
        if !s.contains('<') {
            return s.to_string();
        }
        let mut out = s.to_string();
        for (tag, v) in &self.pairs {
            if out.contains(tag.as_str()) {
                out = out.replace(tag.as_str(), v);
            }
        }
        out
    }

    /// `value` with every placeholder in its strings (keys too) put back.
    pub fn put(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.text(s)),
            Value::Array(a) => Value::Array(a.iter().map(|v| self.put(v)).collect()),
            Value::Object(o) => {
                Value::Object(o.iter().map(|(k, v)| (self.text(k), self.put(v))).collect())
            }
            other => other.clone(),
        }
    }
}

// ------------------------------------------------------------ comparing

fn is_float(n: &serde_json::Number) -> bool {
    let s = n.to_string();
    s.contains(['.', 'e', 'E']) || s.contains("inf") || s.contains("NaN")
}

/// Python's `==` on two parsed JSON numbers, its type check included
/// (an int and a float are different types to first_difference()).
fn num_same(a: &serde_json::Number, b: &serde_json::Number) -> bool {
    let (fa, fb) = (is_float(a), is_float(b));
    if fa != fb {
        return false;
    }
    let (sa, sb) = (a.to_string(), b.to_string());
    if sa == sb {
        return true;
    }
    if fa {
        match (sa.parse::<f64>(), sb.parse::<f64>()) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    } else {
        match (sa.parse::<i128>(), sb.parse::<i128>()) {
            (Ok(x), Ok(y)) => x == y,
            _ => sa.trim_start_matches('-') == "0" && sb.trim_start_matches('-') == "0",
        }
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "None",
        Value::Bool(_) => "bool",
        Value::Number(n) if is_float(n) => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn brief(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    s.chars().take(80).collect()
}

/// The first place `a` (Python's) and `b` (the binary's) differ, as
/// rust/conformance/sessions_compare.py first_difference() reports it.
/// `ordered`: dict key order counts too (it does there; it does not where a
/// script compared json.dumps(sort_keys=True) or used ==).
pub fn first_difference(a: &Value, b: &Value, path: &str, ordered: bool) -> Option<String> {
    if kind(a) != kind(b) {
        return Some(format!("{path}: {} vs {}", kind(a), kind(b)));
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => {
                        if let Some(d) = first_difference(p, q, &format!("{path}.{k}"), ordered) {
                            return Some(d);
                        }
                    }
                    _ => return Some(format!("{path}.{k} missing on one side")),
                }
            }
            if ordered && !x.keys().eq(y.keys()) {
                let first = |m: &serde_json::Map<String, Value>| {
                    m.keys().take(8).cloned().collect::<Vec<_>>()
                };
                return Some(format!(
                    "{path}: key order {:?} vs {:?}",
                    first(x),
                    first(y)
                ));
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{path}: {} vs {} items", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (p, q))| first_difference(p, q, &format!("{path}[{i}]"), ordered))
        }
        (Value::Number(x), Value::Number(y)) => {
            (!num_same(x, y)).then(|| format!("{path}: {} vs {}", x, y))
        }
        (Value::String(x), Value::String(y)) if x != y => {
            // Where they part, with a little of each side from there.
            let at = x.chars().zip(y.chars()).take_while(|(p, q)| p == q).count();
            let from = at.saturating_sub(20);
            let piece = |s: &str| s.chars().skip(from).take(70).collect::<String>();
            Some(format!(
                "{path}: strings part at char {at} ({} vs {} chars): ...{:?} vs ...{:?}",
                x.chars().count(),
                y.chars().count(),
                piece(x),
                piece(y)
            ))
        }
        _ => (a != b).then(|| format!("{path}: {:?} vs {:?}", brief(a), brief(b))),
    }
}

/// Collects failures and panics once at the end with the first few, so a
/// run reports how many cases differ rather than only the first.
#[derive(Default)]
pub struct Failures {
    pub items: Vec<String>,
    pub checked: usize,
}

impl Failures {
    pub fn check(&mut self, label: impl FnOnce() -> String, diff: Option<String>) {
        self.checked += 1;
        if let Some(d) = diff {
            self.items.push(format!("{}\n     {d}", label()));
        }
    }

    pub fn finish(&self, what: &str) {
        if self.items.is_empty() {
            eprintln!("{what}: {} checks, identical", self.checked);
            return;
        }
        let shown: Vec<&String> = self.items.iter().take(15).collect();
        panic!(
            "{what}: {} of {} checks differ from the Python goldens:\n{}",
            self.items.len(),
            self.checked,
            shown
                .iter()
                .map(|s| format!("  {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

// --------------------------------------------------------- temp folders

static SEQ: AtomicU32 = AtomicU32::new(0);

/// A folder of the test's own, removed (read-only files included) on drop.
/// Named imp-test-*, never imp-*\bin like a conformance twin's.
pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new(label: &str) -> TempDir {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let path = system_temp().join(format!(
            "imp-test-{label}-{}-{n}-{nanos:08x}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        TempDir { path }
    }

    pub fn join(&self, p: impl AsRef<Path>) -> PathBuf {
        self.path.join(p)
    }
}

pub fn remove_tree(path: &Path) {
    for _ in 0..20 {
        if std::fs::remove_dir_all(path).is_ok() || !path.exists() {
            return;
        }
        // git's object files are read-only, and a process that just exited
        // may hold a handle for a moment.
        make_writable(path);
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn make_writable(path: &Path) {
    if let Ok(entries) = std::fs::read_dir(path) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                make_writable(&p);
            } else if let Ok(meta) = std::fs::metadata(&p) {
                let mut perm = meta.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                perm.set_readonly(false);
                let _ = std::fs::set_permissions(&p, perm);
            }
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        remove_tree(&self.path);
    }
}

pub fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

/// Every file under `root`/projects gets a fixed mtime, in sorted
/// relative-path order: as golden.py pin_mtimes(), so the session list's
/// order and last_modified match the goldens.
pub fn pin_mtimes(root: &Path) {
    const BASE: u64 = 1767225600;
    const STEP: u64 = 60;
    let mut files = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, root, out);
                } else {
                    let rel = p
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((rel, p));
                }
            }
        }
    }
    walk(&root.join("projects"), root, &mut files);
    files.sort();
    for (i, (_, p)) in files.iter().enumerate() {
        let t = std::time::UNIX_EPOCH + Duration::from_secs(BASE + STEP * i as u64);
        let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
        f.set_modified(t).unwrap();
    }
}

// ---------------------------------------------------------- environment

/// The prefixes stripped from every environment a test gives a process: a
/// real session's variables, herdr's, the API's, Imperatorium's own.
pub const STRIPPED: &[&str] = &["CLAUDE", "HC_", "ANTHROPIC_", "IMPERATORIUM_", "AI_AGENT"];

/// Programs that must never be found on a test's PATH: a missing stand-in
/// then fails instead of opening a real terminal or session.
const REAL: &[&str] = &[
    "wt.exe",
    "claude.exe",
    "wt",
    "claude",
    "hc.exe",
    "hc",
    "herdr.exe",
    "herdr",
    "herdr-app.exe",
];

/// `bin_dir`, then this PATH without any folder holding a real wt, claude
/// or hc (rust/conformance/common.py stand_in_path, hc added).
pub fn stand_in_path(bin_dir: Option<&Path>) -> std::ffi::OsString {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut keep: Vec<PathBuf> = bin_dir.map(|b| vec![b.to_path_buf()]).unwrap_or_default();
    for d in std::env::split_paths(&path) {
        if d.as_os_str().is_empty() || REAL.iter().any(|n| d.join(n).is_file()) {
            continue;
        }
        keep.push(d);
    }
    std::env::join_paths(keep).unwrap()
}

/// The environment minus STRIPPED, PATH a stand-in one.
pub fn clean_env(cmd: &mut Command, bin_dir: Option<&Path>) {
    cmd.env_clear();
    for (k, v) in std::env::vars_os() {
        let key = k.to_string_lossy().to_uppercase();
        if key == "PATH" || STRIPPED.iter().any(|p| key.starts_with(p)) {
            continue;
        }
        cmd.env(k, v);
    }
    cmd.env("PATH", stand_in_path(bin_dir));
}

/// fakecli (rust/fakecli), built beside the binary by
/// `cargo test --workspace`, or by `cargo build [--release] -p fakecli`.
pub fn fakecli() -> PathBuf {
    let exe = if cfg!(windows) {
        "fakecli.exe"
    } else {
        "fakecli"
    };
    let here = PathBuf::from(env!("CARGO_BIN_EXE_imperatorium"));
    let dir = here.parent().unwrap();
    let target = dir.parent().unwrap();
    for c in [
        dir.join(exe),
        target.join("debug").join(exe),
        target.join("release").join(exe),
    ] {
        if c.is_file() {
            return c;
        }
    }
    panic!("fakecli is not built: cargo build -p fakecli (in rust/)");
}

// ------------------------------------------------------------ processes

/// Stop `child` and everything it started.
pub fn kill_tree(child: &mut Child) {
    if let Ok(Some(_)) = child.try_wait() {
        return;
    }
    let pid = child.id();
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
    sweep_orphaned_git(pid);
}

/// End the git processes a killed dispatcher `parent` left behind. A
/// dispatcher killed while it was starting a git leaves that git created
/// but never resumed - suspended for good, holding its test folder - and
/// taskkill /T, which ran before it existed, does not see it. Only git.exe
/// (and what it started) is taken: a reused pid's own children are not.
/// Twice, a moment apart, for one that was still being created.
pub fn sweep_orphaned_git(parent: u32) {
    #[cfg(windows)]
    for pass in 0..2 {
        if pass > 0 {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        for pid in orphaned_git(parent) {
            end_process(pid);
        }
    }
    #[cfg(not(windows))]
    let _ = parent;
}

/// git.exe processes whose parent is `parent`, with everything under them.
#[cfg(windows)]
pub fn orphaned_git(parent: u32) -> Vec<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut all: Vec<(u32, u32, String)> = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut e) != 0;
        while more {
            let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            all.push((e.th32ProcessID, e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..len])));
            more = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    let mut out: Vec<u32> = all
        .iter()
        .filter(|(_, pp, name)| *pp == parent && name.eq_ignore_ascii_case("git.exe"))
        .map(|(p, _, _)| *p)
        .collect();
    let mut i = 0;
    while i < out.len() {
        let p = out[i];
        out.extend(all.iter().filter(|(c, pp, _)| *pp == p && !out.contains(c)).map(|(c, _, _)| *c).collect::<Vec<_>>());
        i += 1;
    }
    out
}

#[cfg(windows)]
fn end_process(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            TerminateProcess(h, 1);
            CloseHandle(h);
        }
    }
}

/// Stop every process whose executable lives under `folder` (a test's
/// stand-ins, which can outlive the dispatcher that started them).
pub fn stop_under(folder: &Path) {
    #[cfg(windows)]
    {
        let script = "$f = [IO.Path]::GetFullPath($env:STOP_UNDER).TrimEnd('\\') + '\\'; \
            Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and \
            $_.ExecutablePath.StartsWith($f, [StringComparison]::OrdinalIgnoreCase) } | \
            ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }";
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("STOP_UNDER", folder)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    let _ = folder;
}

/// Run the binary with `args`, `input` on stdin (written from a thread, so
/// a large input cannot deadlock against a full stdout), and return its
/// exit code, stdout and stderr.
pub fn run(cmd: &mut Command, input: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut stdin = child.stdin.take().unwrap();
    let data = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let mut err = child.stderr.take().unwrap();
    let errs = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = err.read_to_end(&mut v);
        v
    });
    let mut out = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut out).unwrap();
    let _ = writer.join();
    let status = child.wait().unwrap();
    (
        status.code().unwrap_or(-1),
        out,
        errs.join().unwrap_or_default(),
    )
}

/// A `*-check` / `policy-decide` / `sessions batch` subcommand: one JSON
/// line per case in, one answer per line out.
pub fn check_lines(args: &[&str], cwd: &Path, cases: &[Value]) -> Vec<Value> {
    check_lines_env(args, cwd, cases, &[])
}

/// check_lines() with further environment (after the isolation).
pub fn check_lines_env(
    args: &[&str],
    cwd: &Path,
    cases: &[Value],
    env: &[(&str, &std::ffi::OsStr)],
) -> Vec<Value> {
    let mut input = Vec::new();
    for c in cases {
        input.extend_from_slice(serde_json::to_string(c).unwrap().as_bytes());
        input.push(b'\n');
    }
    let mut cmd = Command::new(bin());
    cmd.args(args).current_dir(cwd);
    clean_env(&mut cmd, None);
    isolate(&mut cmd, None);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let (code, out, err) = run(&mut cmd, &input);
    assert_eq!(
        code,
        0,
        "{args:?} exited {code}: {}",
        String::from_utf8_lossy(&err)
    );
    let got: Vec<Value> = String::from_utf8(out)
        .expect("utf-8 answers")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("answer {l:?}: {e}")))
        .collect();
    assert_eq!(
        got.len(),
        cases.len(),
        "{args:?}: {} answers for {} cases",
        got.len(),
        cases.len()
    );
    got
}

/// Point what a process could write to (or read the live state from) at
/// `dir`, or at a throwaway folder: LOCALAPPDATA, APPDATA, XDG_*, the park
/// endpoint at a port nothing serves.
pub fn isolate(cmd: &mut Command, dir: Option<&Path>) {
    let dir = dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| system_temp().join("imp-test-nowhere"));
    cmd.env("LOCALAPPDATA", dir.join("local"))
        .env("APPDATA", dir.join("roaming"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("CLAUDE_CONFIG_DIR", dir.join("claude"))
        .env(
            "IMPERATORIUM_PARK_ENDPOINT",
            "http://127.0.0.1:9/permission/park",
        )
        .env("IMPERATORIUM_ENDPOINT", "http://127.0.0.1:9/hook")
        .env("HC_SESSION", "imptest");
}

// ----------------------------------------------------------- dispatcher

/// What a dispatcher is started with. `Dispatcher::start(Options::new())`.
#[derive(Default)]
pub struct Options {
    /// config.json in its --home.
    pub config: Option<Value>,
    /// policy.json (IMPERATORIUM_POLICY); none: a path that does not exist.
    pub policy: Option<Value>,
    /// Copies of fakecli under these names in a bin folder first on PATH.
    pub fake_cli: Vec<&'static str>,
    pub env: Vec<(String, String)>,
    /// --home: default a folder of its own; Some(repo()) as the twins did.
    pub home: Option<PathBuf>,
}

impl Options {
    pub fn new() -> Options {
        Options::default()
    }
}

/// `imperatorium serve` on a free port with a temp --home, --state,
/// LOCALAPPDATA and CLAUDE_CONFIG_DIR, a stripped environment and a
/// stand-in PATH. Stopped (tree and stand-ins) on drop.
pub struct Dispatcher {
    child: Child,
    pub port: u16,
    pub root: TempDir,
    pub home: PathBuf,
    pub state: PathBuf,
    pub claude: PathBuf,
    pub bin_dir: PathBuf,
    pub log: PathBuf,
    agent: ureq::Agent,
}

impl Dispatcher {
    pub fn start(opts: Options) -> Dispatcher {
        let root = TempDir::new("serve");
        let home = opts.home.clone().unwrap_or_else(|| root.join("home"));
        let state = root.join("state").join("state.json");
        let claude = root.join("claude");
        let bin_dir = root.join("bin");
        for d in [
            &home,
            &state.parent().unwrap().to_path_buf(),
            &claude.join("sessions"),
            &bin_dir,
        ] {
            std::fs::create_dir_all(d).unwrap();
        }
        if let Some(c) = &opts.config {
            std::fs::write(home.join("config.json"), serde_json::to_vec(c).unwrap()).unwrap();
        }
        let policy = root.join("policy.json");
        if let Some(p) = &opts.policy {
            std::fs::write(&policy, serde_json::to_vec(p).unwrap()).unwrap();
        }
        let fake = (!opts.fake_cli.is_empty()).then(fakecli);
        for name in &opts.fake_cli {
            std::fs::copy(fake.as_ref().unwrap(), bin_dir.join(name)).unwrap();
        }
        let log = root.join("serve.log");
        let out = std::fs::File::create(&log).unwrap();
        let mut cmd = Command::new(bin());
        cmd.args(["serve", "--port", "0", "--host", "127.0.0.1"])
            .arg("--home")
            .arg(&home)
            .arg("--state")
            .arg(&state)
            .current_dir(root.path.clone());
        clean_env(&mut cmd, Some(&bin_dir));
        isolate(&mut cmd, Some(&root.path));
        cmd.env("CLAUDE_CONFIG_DIR", &claude)
            .env("IMPERATORIUM_POLICY", &policy)
            .env("FAKE_LOG", root.join("fake.jsonl"));
        for (k, v) in &opts.env {
            cmd.env(k, v);
        }
        let child = cmd
            .stdin(Stdio::null())
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .spawn()
            .expect("start imperatorium serve");
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .build();
        let mut d = Dispatcher {
            child,
            port: 0,
            root,
            home,
            state,
            claude,
            bin_dir,
            log,
            agent,
        };
        d.port = d.wait_listening(Duration::from_secs(30));
        let health = d.url("/api/health");
        let agent = d.agent.clone();
        wait_until(Duration::from_secs(30), || {
            agent.get(&health).call().ok().map(|_| ())
        })
        .unwrap_or_else(|| panic!("dispatcher on {} never answered: {}", d.port, d.log_text()));
        d
    }

    fn wait_listening(&mut self, limit: Duration) -> u16 {
        let log = self.log.clone();
        let found = wait_until(limit, || {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            text.lines().find_map(|l| {
                l.split("listening on http://")
                    .nth(1)
                    .and_then(|a| a.rsplit(':').next())
                    .and_then(|p| p.trim().parse::<u16>().ok())
            })
        });
        match found {
            Some(p) => p,
            None => {
                let exited = self.child.try_wait().ok().flatten();
                panic!(
                    "serve never said where it listens (exit {exited:?}): {}",
                    self.log_text()
                )
            }
        }
    }

    pub fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// GET; the status and the body as JSON (null when empty, a string
    /// when not JSON).
    pub fn get(&self, path: &str) -> (u16, Value) {
        reply(self.agent.get(&self.url(path)).call())
    }

    pub fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        self.request(
            path,
            Some(serde_json::to_vec(body).unwrap()),
            Some("application/json"),
            &[],
        )
    }

    /// POST with an exact body, Content-Type (None: none sent) and headers.
    pub fn request(
        &self,
        path: &str,
        body: Option<Vec<u8>>,
        content_type: Option<&str>,
        headers: &[(&str, &str)],
    ) -> (u16, Value) {
        let mut req = self.agent.post(&self.url(path));
        if let Some(ct) = content_type {
            req = req.set("Content-Type", ct);
        }
        for (k, v) in headers {
            req = req.set(k, v);
        }
        reply(req.send_bytes(&body.unwrap_or_default()))
    }

    /// The window's page token (`<meta name=imp-window content=...>`).
    pub fn window_token(&self) -> String {
        let page = self
            .agent
            .get(&self.url("/"))
            .call()
            .unwrap()
            .into_string()
            .unwrap();
        let key = "<meta name=imp-window content=\"";
        let start = page.find(key).expect("window token") + key.len();
        page[start..start + page[start..].find('"').unwrap()].to_string()
    }

    /// POST as the window does: its Origin and token.
    pub fn post_window(&self, path: &str, body: &Value, token: &str) -> (u16, Value) {
        let origin = format!("http://127.0.0.1:{}", self.port);
        self.request(
            path,
            Some(serde_json::to_vec(body).unwrap()),
            Some("application/json"),
            &[("Origin", &origin), ("X-Imperatorium-Window", token)],
        )
    }

    /// The state file as JSON (null if not written yet).
    pub fn state_json(&self) -> Value {
        std::fs::read_to_string(&self.state)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null)
    }

    /// A made-up session record under CLAUDE_CONFIG_DIR/sessions, as Claude
    /// Code writes it; `pid` this test's, so it is alive while the test runs.
    pub fn session_record(&self, sid: &str, fields: Value) {
        let mut rec = json!({"pid": std::process::id(), "sessionId": sid});
        if let (Some(r), Some(f)) = (rec.as_object_mut(), fields.as_object()) {
            for (k, v) in f {
                r.insert(k.clone(), v.clone());
            }
        }
        std::fs::write(
            self.claude.join("sessions").join(format!("{sid}.json")),
            serde_json::to_vec(&rec).unwrap(),
        )
        .unwrap();
    }

    /// fakecli's log: one JSON object per call.
    pub fn fake_log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.join("fake.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

impl Drop for Dispatcher {
    fn drop(&mut self) {
        kill_tree(&mut self.child);
        if std::fs::read_dir(&self.bin_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
        {
            stop_under(&self.bin_dir);
        }
    }
}

fn reply(r: Result<ureq::Response, ureq::Error>) -> (u16, Value) {
    let resp = match r {
        Ok(resp) => resp,
        Err(ureq::Error::Status(_, resp)) => resp,
        Err(e) => panic!("request failed: {e}"),
    };
    let code = resp.status();
    let mut body = Vec::new();
    let _ = resp.into_reader().read_to_end(&mut body);
    let value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()))
    };
    (code, value)
}

/// Poll `f` every 20 ms until it gives Some, or `limit` passes.
pub fn wait_until<T>(limit: Duration, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let end = Instant::now() + limit;
    loop {
        if let Some(v) = f() {
            return Some(v);
        }
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

// --------------------------------------------------------- fake server

/// A recording HTTP server on a free port (the hooks' dispatcher, ntfy):
/// every request kept as (method, path, Content-Type, body); answers from
/// `answer(path)` -> (status, body), 200 `{}` by default.
type Seen = std::sync::Arc<std::sync::Mutex<Vec<(String, String, Option<String>, Vec<u8>)>>>;
type Answers = std::sync::Arc<std::sync::Mutex<Vec<(String, u16, Vec<u8>)>>>;

pub struct FakeServer {
    pub port: u16,
    pub seen: Seen,
    pub answer: Answers,
    server: std::sync::Arc<tiny_http::Server>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FakeServer {
    pub fn start() -> FakeServer {
        let server = std::sync::Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let port = server.server_addr().to_ip().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let answer: Answers =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (s, seen2, answer2) = (server.clone(), seen.clone(), answer.clone());
        let thread = std::thread::spawn(move || {
            for mut req in s.incoming_requests() {
                let mut body = Vec::new();
                let _ = req.as_reader().read_to_end(&mut body);
                let ctype = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Content-Type"))
                    .map(|h| h.value.as_str().to_string());
                let path = req.url().to_string();
                seen2
                    .lock()
                    .unwrap()
                    .push((req.method().to_string(), path.clone(), ctype, body));
                let (status, reply) = answer2
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(p, _, _)| *p == path)
                    .map(|(_, s, b)| (*s, b.clone()))
                    .unwrap_or((200, b"{}".to_vec()));
                let resp = tiny_http::Response::from_data(reply)
                    .with_status_code(status)
                    .with_header(
                        "Content-Type: application/json"
                            .parse::<tiny_http::Header>()
                            .unwrap(),
                    );
                let _ = req.respond(resp);
            }
        });
        FakeServer {
            port,
            seen,
            answer,
            server,
            thread: Some(thread),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn set_answer(&self, path: &str, status: u16, body: &[u8]) {
        let mut a = self.answer.lock().unwrap();
        a.retain(|(p, _, _)| p != path);
        a.push((path.to_string(), status, body.to_vec()));
    }

    pub fn take(&self) -> Vec<(String, String, Option<String>, Vec<u8>)> {
        std::mem::take(&mut *self.seen.lock().unwrap())
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Read stdout lines from a reader until `pred` matches one (for a process
/// that announces itself).
pub fn read_until(r: &mut impl BufRead, pred: impl Fn(&str) -> bool) -> Option<String> {
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if pred(&line) {
            return Some(line);
        }
    }
}

pub mod dispatch;

pub mod setup;

#[path = "core.rs"]
pub mod core_support;

pub mod features;
