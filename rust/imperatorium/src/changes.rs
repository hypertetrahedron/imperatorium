//! What a session has changed, and the worktrees background sessions leave:
//! the read side of `imperatorium/changes.py` (status, diff, recent commits,
//! worktrees and their merge-base diffs), and finishing a worktree (merge,
//! PR, discard). Every git call is bounded in time
//! and output, as there.

use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::activity::splitlines;
use crate::paths;
use crate::pyish::take;

const TIMEOUT: f64 = 20.0;
pub(crate) const MAX_DIFF: usize = 400 * 1024;

/// What runs as `git`: the program and what to add to its environment.
///
/// Git for Windows puts `cmd\git.exe` on PATH, a wrapper that starts the real
/// `<arch>\bin\git.exe` and waits for it. Windows 11 25H2 (win32kfull
/// 10.0.26100.9974) leaks the token of any process that starts a console
/// program and is still running when it attaches to its console: freed only
/// by a reboot, ~2.5 KB each, ~14 MB/h at the dispatcher's git rate
/// (reported to Microsoft 2026-11-24). So the wrapper is skipped and the
/// real git run with what the wrapper would have given it: its `bin` and
/// `usr\bin` ahead on PATH (for sh, ssh, credential helpers), MSYSTEM and
/// PLINK_PROTOCOL. The same leak is why [`current_branch`] reads HEAD
/// rather than run git on every poll, and why [`prefer_real_git`] is given
/// to programs that run git themselves (gh).
fn git_command() -> Command {
    match found_real_git() {
        Some(real) => {
            let mut c = Command::new(&real.exe);
            real_git_env(&mut c, &real);
            c
        }
        None => Command::new("git"),
    }
}

/// For a program that runs git itself: the real git first on its PATH, with
/// the environment the wrapper would give it, so its git calls skip the
/// wrapper too. Nothing changes where PATH already finds the real git.
pub fn prefer_real_git(c: &mut Command) {
    if let Some(real) = found_real_git() {
        real_git_env(c, &real);
    }
}

fn found_real_git() -> Option<RealGit> {
    let found = crate::launch::which_real("git").map(std::path::PathBuf::from)?;
    real_git(&found)
}

fn real_git_env(c: &mut Command, real: &RealGit) {
    let mut path = std::ffi::OsString::from(real.bin.as_os_str());
    path.push(";");
    path.push(real.usr_bin.as_os_str());
    if let Some(p) = std::env::var_os("PATH") {
        path.push(";");
        path.push(p);
    }
    c.env("PATH", path);
    if std::env::var_os("MSYSTEM").is_none() {
        c.env("MSYSTEM", real.msystem);
    }
    if std::env::var_os("PLINK_PROTOCOL").is_none() {
        c.env("PLINK_PROTOCOL", "ssh");
    }
}

#[derive(Debug, PartialEq)]
struct RealGit {
    exe: std::path::PathBuf,
    bin: std::path::PathBuf,
    usr_bin: std::path::PathBuf,
    msystem: &'static str,
}

/// The git behind Git for Windows' `<top>\cmd\git.exe` wrapper, if `found`
/// is that wrapper and the real one is where an install puts it.
fn real_git(found: &std::path::Path) -> Option<RealGit> {
    if !cfg!(windows) || !found.file_name()?.to_str()?.eq_ignore_ascii_case("git.exe") {
        return None;
    }
    let cmd = found.parent()?;
    if !cmd.file_name()?.to_str()?.eq_ignore_ascii_case("cmd") {
        return None;
    }
    let top = cmd.parent()?;
    for (dir, msystem) in [("mingw64", "MINGW64"), ("clangarm64", "CLANGARM64"), ("mingw32", "MINGW32")] {
        let bin = top.join(dir).join("bin");
        let exe = bin.join("git.exe");
        if exe.is_file() {
            return Some(RealGit { exe, bin, usr_bin: top.join("usr").join("bin"), msystem });
        }
    }
    None
}

/// `git branch --show-current` for `cwd`, read from HEAD where that is a
/// plain file (no process, see [`git_command`]: the PR tracker asks for
/// every session's folder every 5 s); git itself otherwise.
/// Err when `cwd` is in no repository.
pub fn current_branch(cwd: &str) -> Result<String, String> {
    match head_branch(std::path::Path::new(cwd)) {
        Head::Branch(b) => Ok(b),
        Head::NoRepo => Err("not a git repository".into()),
        Head::AskGit => git(cwd, &["branch", "--show-current"], true)
            .map(|o| crate::pyish::strip(&o).to_string()),
    }
}

#[derive(Debug, PartialEq)]
enum Head {
    /// The branch checked out; empty when HEAD is detached.
    Branch(String),
    /// No `.git` in `cwd` or above it.
    NoRepo,
    /// Something only git can read (reftable, an odd HEAD, unreadable).
    AskGit,
}

fn head_branch(cwd: &std::path::Path) -> Head {
    match locate(cwd) {
        Located::Dir { git, .. } => head_in(&git),
        Located::NoRepo => Head::NoRepo,
        Located::AskGit => Head::AskGit,
    }
}

enum Located {
    /// The repository's git directory (a worktree's own, for a worktree) and
    /// the top of its working tree (None for a bare repository).
    Dir { git: std::path::PathBuf, top: Option<std::path::PathBuf> },
    NoRepo,
    AskGit,
}

fn locate(cwd: &std::path::Path) -> Located {
    let Ok(start) = std::path::absolute(cwd) else { return Located::AskGit };
    for dir in start.ancestors() {
        let dotgit = dir.join(".git");
        if dotgit.is_dir() {
            return Located::Dir { git: dotgit, top: Some(dir.to_path_buf()) };
        }
        if dotgit.is_file() {
            // A worktree or submodule: "gitdir: <path>", maybe relative.
            let Ok(text) = std::fs::read_to_string(&dotgit) else { return Located::AskGit };
            let Some(gitdir) = text.trim_end().strip_prefix("gitdir:") else {
                return Located::AskGit;
            };
            return Located::Dir { git: dir.join(gitdir.trim()), top: Some(dir.to_path_buf()) };
        }
        if dir.join("HEAD").is_file() && dir.join("objects").is_dir() && dir.join("refs").is_dir() {
            return Located::Dir { git: dir.to_path_buf(), top: None }; // a bare repository
        }
    }
    Located::NoRepo
}

/// `(OWNER/NAME, head branch)` for `gh pr view <branch> --repo OWNER/NAME`,
/// read from the repository's files, so gh need not run git to find them
/// (each git it runs is a console program started by a console program:
/// see [`git_command`]). Only where the answer is the one gh would reach
/// itself: a branch checked out, one github.com remote or the one gh
/// recorded as the base (`gh-resolved = base`), no `[include]`. None
/// otherwise, and gh works it out as before.
pub fn github_target(cwd: &str) -> Option<(String, String)> {
    let Located::Dir { git: gitdir, .. } = locate(std::path::Path::new(cwd)) else { return None };
    let Head::Branch(branch) = head_in(&gitdir) else { return None };
    // Empty is detached; a leading '-' (git refuses one) would read as a flag.
    if branch.is_empty() || branch.starts_with('-') {
        return None;
    }
    let config = std::fs::read_to_string(common_dir(&gitdir).join("config")).ok()?;
    target_from_config(&config, &branch)
}

/// Where a worktree's repository keeps what all its worktrees share (refs,
/// config): `commondir` names it; the git directory itself otherwise.
fn common_dir(gitdir: &std::path::Path) -> std::path::PathBuf {
    match std::fs::read_to_string(gitdir.join("commondir")) {
        Ok(rel) => gitdir.join(rel.trim()),
        Err(_) => gitdir.to_path_buf(),
    }
}

/// The commit HEAD names, read from the files: a detached id, a loose ref
/// or a `packed-refs` line. None for anything else (an unborn branch,
/// reftable), which only git can answer.
fn head_oid(gitdir: &std::path::Path) -> Option<String> {
    let is_oid = |s: &str| matches!(s.len(), 40 | 64) && s.bytes().all(|c| c.is_ascii_hexdigit());
    let text = std::fs::read_to_string(gitdir.join("HEAD")).ok()?;
    let head = text.trim_end();
    if is_oid(head) {
        return Some(head.to_string());
    }
    let target = head.strip_prefix("ref: ")?.trim();
    if !target.starts_with("refs/heads/") || target.contains("..") {
        return None;
    }
    let common = common_dir(gitdir);
    if let Ok(loose) = std::fs::read_to_string(common.join(target)) {
        return is_oid(loose.trim_end()).then(|| loose.trim_end().to_string());
    }
    let packed = std::fs::read_to_string(common.join("packed-refs")).ok()?;
    packed.lines().find_map(|line| {
        let (oid, name) = line.split_once(' ')?;
        (name.trim_end() == target && is_oid(oid)).then(|| oid.to_string())
    })
}

/// [`github_target`]'s reading of a git config file.
fn target_from_config(config: &str, branch: &str) -> Option<(String, String)> {
    struct Remote {
        slug: Option<String>,
        base: bool,
    }
    let mut remotes: Vec<Remote> = Vec::new();
    let mut merge: Option<String> = None;
    enum Sect {
        Remote,
        Branch,
        Other,
    }
    let mut sect = Sect::Other;
    for raw in config.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(head) = line.strip_prefix('[') {
            let head = head.split(']').next()?.trim();
            let (kind, sub) = match head.split_once(char::is_whitespace) {
                Some((k, s)) => (k.to_ascii_lowercase(), s.trim().trim_matches('"').to_string()),
                None => (head.to_ascii_lowercase(), String::new()),
            };
            sect = match kind.as_str() {
                "include" | "includeif" => return None,
                "remote" => {
                    remotes.push(Remote { slug: None, base: false });
                    Sect::Remote
                }
                "branch" if sub == branch => Sect::Branch,
                _ => Sect::Other,
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().trim_matches('"');
        match sect {
            Sect::Remote => {
                let r = remotes.last_mut()?;
                match key.as_str() {
                    "url" => r.slug = github_slug(value),
                    "gh-resolved" => r.base = value == "base",
                    _ => {}
                }
            }
            Sect::Branch if key == "merge" => {
                merge = value.strip_prefix("refs/heads/").map(str::to_string);
            }
            _ => {}
        }
    }
    let on_github: Vec<&Remote> = remotes.iter().filter(|r| r.slug.is_some()).collect();
    let chosen = match on_github.iter().find(|r| r.base) {
        Some(r) => *r,
        // One remote only: with a fork beside its upstream, gh's choice of
        // base depends on more than this file.
        None if on_github.len() == 1 && remotes.len() == 1 => on_github[0],
        None => return None,
    };
    Some((chosen.slug.clone()?, merge.unwrap_or_else(|| branch.to_string())))
}

/// OWNER/NAME of a github.com remote URL (https, ssh or scp form).
fn github_slug(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, name) = rest.split_once('/')?;
    let ok = |s: &str| {
        !s.is_empty()
            && !s.starts_with('-')
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    (ok(owner) && ok(name)).then(|| format!("{owner}/{name}"))
}

fn head_in(gitdir: &std::path::Path) -> Head {
    let Ok(text) = std::fs::read_to_string(gitdir.join("HEAD")) else { return Head::AskGit };
    let head = text.trim_end_matches(['\n', '\r']);
    if let Some(target) = head.strip_prefix("ref: ") {
        return match target.strip_prefix("refs/heads/") {
            // reftable keeps HEAD as a stub pointing here.
            Some(b) if !b.is_empty() && b != ".invalid" => Head::Branch(b.to_string()),
            _ => Head::AskGit,
        };
    }
    if matches!(head.len(), 40 | 64) && head.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Head::Branch(String::new()); // detached
    }
    Head::AskGit
}

/// stdout of `git -C cwd args`; Err(message) as changes.git raises it.
pub fn git(cwd: &str, args: &[&str], check: bool) -> Result<String, String> {
    let mut cmd = git_command();
    cmd.arg("-C").arg(cwd).args(args);
    let o = match crate::proc::run_timeout(&mut cmd, Duration::from_secs_f64(TIMEOUT)) {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("git is not on PATH".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            return Err(format!("git {} took longer than {}s", args[0], TIMEOUT))
        }
        Err(e) => return Err(e.to_string()),
    };
    let status = o.status;
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    if check && !status.success() {
        let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
        let stderr = crate::pyish::strip(&stderr);
        return Err(match splitlines(stderr).last() {
            Some(last) if !stderr.is_empty() => last.to_string(),
            _ => format!("git {} failed", args[0]),
        });
    }
    Ok(stdout)
}

pub fn is_repo(cwd: &str) -> bool {
    if cwd.is_empty() || !std::path::Path::new(cwd).is_dir() {
        return false;
    }
    git(cwd, &["rev-parse", "--is-inside-work-tree"], true)
        .is_ok_and(|o| crate::pyish::strip(&o) == "true")
}

fn branch_line() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(concat!(
            r"^## (?P<branch>[^.\s]+|HEAD \(no branch\)|No commits yet on \S+)",
            r"(?:\.\.\.(?P<upstream>\S+))?(?: \[(?P<track>[^\]]+)\])?"
        ))
        .unwrap()
    })
}

/// changes.parse_status.
pub fn parse_status(text: &str) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("branch".into(), Value::Null);
    out.insert("upstream".into(), Value::Null);
    out.insert("ahead".into(), json!(0));
    out.insert("behind".into(), json!(0));
    let mut files = Vec::new();
    for line in splitlines(text) {
        if line.starts_with("## ") {
            if let Some(m) = branch_line().captures(line) {
                out.insert(
                    "branch".into(),
                    m.name("branch")
                        .map(|x| json!(x.as_str()))
                        .unwrap_or(Value::Null),
                );
                out.insert(
                    "upstream".into(),
                    m.name("upstream")
                        .map(|x| json!(x.as_str()))
                        .unwrap_or(Value::Null),
                );
                for part in m.name("track").map(|t| t.as_str()).unwrap_or("").split(',') {
                    let part = crate::pyish::strip(part);
                    if let Some(n) = part.strip_prefix("ahead ") {
                        out.insert("ahead".into(), json!(n.parse::<i64>().unwrap_or(0)));
                    } else if let Some(n) = part.strip_prefix("behind ") {
                        out.insert("behind".into(), json!(n.parse::<i64>().unwrap_or(0)));
                    }
                }
            }
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        if chars.len() < 4 {
            continue;
        }
        let code: String = chars[..2].iter().collect();
        let mut path: String = chars[3..].iter().collect();
        if path.contains(" -> ") && matches!(chars[0], 'R' | 'C') {
            path = path
                .split_once(" -> ")
                .map(|x| x.1.to_string())
                .unwrap_or(path);
        }
        if path.len() >= 2 && path.starts_with('"') && path.ends_with('"') {
            path = path[1..path.len() - 1].to_string();
        } else if path == "\"" {
            path = String::new();
        }
        files.push(json!({
            "path": path,
            "code": code,
            "staged": !matches!(chars[0], ' ' | '?'),
            "unstaged": !matches!(chars[1], ' ' | '?'),
            "untracked": code == "??",
        }));
    }
    out.insert("files".into(), Value::Array(files));
    out
}

/// changes.parse_numstat: {path: (added, removed)}; binary is (None, None).
fn parse_numstat(text: &str) -> Map<String, Value> {
    let mut out = Map::new();
    for line in splitlines(text) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let v = if parts[0] == "-" {
            json!([null, null])
        } else {
            json!([
                parts[0].parse::<i64>().unwrap_or(0),
                parts[1].parse::<i64>().unwrap_or(0)
            ])
        };
        out.insert(parts[parts.len() - 1].to_string(), v);
    }
    out
}

/// What `changes` reads: status, numstat, diff and recent commits.
type Read = (Map<String, Value>, Map<String, Value>, String, Vec<Value>);

/// changes.changes(cwd): everything uncommitted, ready for the window.
/// What the last [`changes`] of a folder was read from: `git status`'s
/// output, the commit HEAD names and the size and time of every file status
/// lists. While all three hold, the diff, numstat and log would come out the
/// same, so only `git status` is run (the window asks every 6 s while the
/// Changes tab is open: five git a time without this).
#[derive(PartialEq)]
struct Seen {
    status: String,
    head: String,
    stamps: Vec<Option<(u64, std::time::SystemTime)>>,
}

fn seen(cwd: &str, status_out: &str, status: &Map<String, Value>) -> Option<Seen> {
    let Located::Dir { git, top: Some(top) } = locate(std::path::Path::new(cwd)) else {
        return None;
    };
    let head = head_oid(&git)?;
    let files = status.get("files").and_then(Value::as_array)?;
    let stamps = files
        .iter()
        .map(|f| {
            let m = std::fs::metadata(top.join(f["path"].as_str().unwrap_or(""))).ok()?;
            Some((m.len(), m.modified().ok()?))
        })
        .collect();
    Some(Seen { status: status_out.to_string(), head, stamps })
}

fn changes_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, (Seen, Value)>> {
    static C: OnceLock<std::sync::Mutex<std::collections::HashMap<String, (Seen, Value)>>> =
        OnceLock::new();
    C.get_or_init(Default::default)
}

/// `git rev-parse --is-inside-work-tree`, answered from the files where they
/// are plain (a `.git` folder or file above `cwd`), git otherwise.
fn in_work_tree(cwd: &str) -> bool {
    match locate(std::path::Path::new(cwd)) {
        Located::Dir { top: Some(_), .. } => true,
        Located::NoRepo => false,
        _ => is_repo(cwd),
    }
}

pub fn changes(cwd: &str) -> Value {
    use crate::locks::LockExt;
    if cwd.is_empty() || !std::path::Path::new(cwd).is_dir() {
        return json!({"ok": false, "error": "no working directory for this session"});
    }
    if !in_work_tree(cwd) {
        return json!({"ok": true, "repo": false, "files": [], "diff": ""});
    }
    let key = paths::abs_normcase(cwd).unwrap_or_else(|| cwd.to_string());
    // One read of a folder at a time: selecting a session and opening its
    // Changes tab ask within the same moment, and both missed the cache and
    // ran the whole read (10 git for 5; seen on a test dispatcher).
    let turn = {
        static TURNS: OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<std::sync::Mutex<()>>>>> =
            OnceLock::new();
        let mut turns = TURNS.get_or_init(Default::default).lk();
        if turns.len() > 64 {
            turns.retain(|_, t| std::sync::Arc::strong_count(t) > 1);
        }
        std::sync::Arc::clone(turns.entry(key.clone()).or_default())
    };
    let _mine = turn.lk();
    let status_out =
        match git(cwd, &["status", "--porcelain=v1", "-b", "--untracked-files=all"], true) {
            Ok(s) => s,
            Err(e) => return json!({"ok": false, "error": e}),
        };
    let now = seen(cwd, &status_out, &parse_status(&status_out));
    if let Some(now) = &now {
        if let Some((was, value)) = changes_cache().lk().get(&key) {
            if was == now {
                return value.clone();
            }
        }
    }
    let value = changes_read(cwd, &status_out);
    if let Some(now) = now.filter(|_| value["ok"] == json!(true)) {
        let mut cache = changes_cache().lk();
        if cache.len() >= 32 && !cache.contains_key(&key) {
            cache.clear();
        }
        cache.insert(key, (now, value.clone()));
    }
    value
}

/// [`changes`] in full, from `git status`'s output.
fn changes_read(cwd: &str, status_out: &str) -> Value {
    let run = || -> Result<Read, String> {
        let status = parse_status(status_out);
        let has_head =
            !crate::pyish::strip(&git(cwd, &["rev-parse", "--verify", "-q", "HEAD"], false)?)
                .is_empty();
        let (mut numstat, mut diff, mut recent) = (Map::new(), String::new(), Vec::new());
        if has_head {
            numstat = parse_numstat(&git(cwd, &["diff", "--numstat", "HEAD"], true)?);
            diff = git(cwd, &["diff", "--no-color", "--no-ext-diff", "HEAD"], true)?;
            for line in splitlines(&git(cwd, &["log", "-8", "--format=%h\x1f%s\x1f%ct"], true)?) {
                let bits: Vec<&str> = line.split('\x1f').collect();
                if bits.len() == 3 {
                    recent.push(json!({"hash": bits[0], "subject": bits[1],
                                       "when": bits[2].parse::<i64>().unwrap_or(0)}));
                }
            }
        }
        Ok((status, numstat, diff, recent))
    };
    let (mut status, numstat, diff, recent) = match run() {
        Ok(x) => x,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    if let Some(Value::Array(files)) = status.get_mut("files") {
        for f in files.iter_mut() {
            let path = f["path"].as_str().unwrap_or("").to_string();
            let (added, removed) = match numstat.get(&path) {
                Some(Value::Array(ar)) => (ar[0].clone(), ar[1].clone()),
                _ => (Value::Null, Value::Null),
            };
            f["added"] = added;
            f["removed"] = removed;
        }
    }
    let truncated = diff.chars().count() > MAX_DIFF;
    status.insert("ok".into(), json!(true));
    status.insert("repo".into(), json!(true));
    status.insert("diff".into(), json!(take(&diff, MAX_DIFF)));
    status.insert("truncated".into(), json!(truncated));
    status.insert("recent".into(), Value::Array(recent));
    Value::Object(status)
}

/// changes.parse_worktrees: `git worktree list --porcelain`.
pub(crate) fn parse_worktrees(text: &str) -> Vec<Map<String, Value>> {
    let mut out = Vec::new();
    let mut cur: Option<Map<String, Value>> = None;
    let mut lines: Vec<&str> = splitlines(text);
    lines.push("");
    for line in lines {
        if line.is_empty() {
            if let Some(c) = cur.take() {
                out.push(c);
            }
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if key == "worktree" {
            cur = Some(
                json!({"path": value, "head": null, "branch": null, "bare": false,
                       "detached": false, "locked": false})
                .as_object()
                .unwrap()
                .clone(),
            );
            continue;
        }
        let Some(c) = cur.as_mut() else { continue };
        match key {
            "HEAD" => {
                c.insert("head".into(), json!(value));
            }
            "branch" => {
                c.insert(
                    "branch".into(),
                    json!(value.strip_prefix("refs/heads/").unwrap_or(value)),
                );
            }
            "bare" | "detached" | "locked" => {
                c.insert(key.into(), json!(true));
            }
            _ => {}
        }
    }
    out
}

pub(crate) fn norm(p: &str) -> String {
    paths::abs_normcase(p).unwrap_or_default()
}

/// Is git's `listed` path the folder the caller named (`want` is that name
/// through norm)? Python's comparison first; then, for two spellings of one
/// folder that it misses - an 8.3 short name (`C:\Users\RUNNER~1`, as a
/// short TEMP gives) against the long one git reports - the canonical paths.
pub(crate) fn same_dir(listed: &str, want: &str, named: &str) -> bool {
    if norm(listed) == want {
        return true;
    }
    match (std::fs::canonicalize(listed), std::fs::canonicalize(named)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// True for a worktree Claude Code made for a background session.
pub(crate) fn is_agent_worktree(main: &str, path: &str) -> bool {
    let root = norm(
        &std::path::Path::new(main)
            .join(".claude")
            .join("worktrees")
            .to_string_lossy(),
    );
    norm(path).starts_with(&format!("{root}{}", paths::SEP))
}

/// changes.worktrees(project).
pub fn worktrees(project: &str) -> Value {
    worktrees_for(project, None)
}

/// The tip's subject of every local branch, in one call: a worktree on a
/// branch shows its branch's tip, so it needs no `log -1` of its own.
fn branch_subjects(project: &str) -> std::collections::HashMap<String, String> {
    let got = git(project, &["for-each-ref", "refs/heads", "--format=%(refname:short)%09%(subject)"], false)
        .unwrap_or_default();
    splitlines(&got)
        .into_iter()
        .filter_map(|l| l.split_once('\t').map(|(b, s)| (b.to_string(), crate::pyish::strip(s).to_string())))
        .collect()
}

/// What `measure_worktree` adds to a worktree's entry, in order.
type Measured = Result<Vec<(&'static str, Value)>, String>;

/// One worktree's own numbers, in the order the answer has always had them:
/// dirty, ahead, behind, subject. Ahead and behind come from one
/// `rev-list --left-right --count` (it was two).
fn measure_worktree(
    project: &str,
    path: &str,
    base: Option<&str>,
    branch: Option<&str>,
    subjects: &std::collections::HashMap<String, String>,
) -> Measured {
    let mut got = Vec::new();
    let status = git(path, &["status", "--porcelain"], true)?;
    let lines = splitlines(&status);
    let changed: Vec<&str> = lines.iter().copied().filter(|l| !crate::pyish::strip(l).is_empty()).collect();
    got.push(("dirty", json!(!changed.is_empty())));
    // Only new, untracked files: a merge or a push carries commits and
    // leaves them be, as the base checkout's own check does (2026-11-24: a
    // `__pycache__/` blocked Merge). Discard still counts them - it deletes
    // them. Said only when true, so other answers keep their shape.
    if !changed.is_empty() && changed.iter().all(|l| l.starts_with("??")) {
        got.push(("untracked_only", json!(true)));
    }
    if let (Some(b), Some(br)) = (base, branch) {
        let t = git(project, &["rev-list", "--left-right", "--count", &format!("{b}...{br}")], true)?;
        let mut n = crate::pyish::strip(&t).split_whitespace().map(|x| {
            x.parse::<i64>().map_err(|_| format!("invalid literal for int() with base 10: '{x}'"))
        });
        let behind = n.next().transpose()?.unwrap_or(0);
        let ahead = n.next().transpose()?.unwrap_or(0);
        got.push(("ahead", json!(ahead)));
        got.push(("behind", json!(behind)));
    }
    let subject = match branch.and_then(|br| subjects.get(br)) {
        Some(s) => s.clone(),
        None => crate::pyish::strip(&git(path, &["log", "-1", "--format=%s"], false)?).to_string(),
    };
    got.push(("subject", json!(subject)));
    Ok(got)
}

/// `worktrees`, measuring only the worktree at `only` (and leaving the
/// others out) when given: what `finish` needs to find one.
fn worktrees_for(project: &str, only: Option<&str>) -> Value {
    if !is_repo(project) {
        return json!({"ok": true, "repo": false, "worktrees": []});
    }
    let listed = match git(project, &["worktree", "list", "--porcelain"], true) {
        Ok(t) => parse_worktrees(&t),
        Err(e) => return json!({"ok": false, "error": e}),
    };
    if listed.is_empty() {
        return json!({"ok": true, "repo": true, "worktrees": []});
    }
    let main = &listed[0];
    let main_path = main["path"].as_str().unwrap_or("").to_string();
    let base = main
        .get("branch")
        .and_then(Value::as_str)
        .map(str::to_string);
    let recorded = recorded_bases(project);
    let want = only.map(|p| (norm(p), p));
    let mut entries = Vec::new();
    for wt in &listed[1..] {
        let path = wt["path"].as_str().unwrap_or("").to_string();
        if let Some((w, raw)) = &want {
            if !same_dir(&path, w, raw) {
                continue;
            }
        }
        let mut entry = wt.clone();
        entry.insert("agent".into(), json!(is_agent_worktree(&main_path, &path)));
        let exists = std::path::Path::new(&path).is_dir();
        entry.insert("exists".into(), json!(exists));
        // The branch it was made from, when Imperatorium made it; otherwise
        // the main checkout's, which is what Claude Code's own come from.
        let own = wt
            .get("branch")
            .and_then(Value::as_str)
            .filter(|b| !b.is_empty())
            .and_then(|b| recorded.get(b))
            .cloned();
        let base = own.clone().or_else(|| base.clone());
        entry.insert("base".into(), json!(base));
        entry.insert("base_recorded".into(), json!(own.is_some()));
        entries.push((entry, path, exists, base));
    }
    // Each worktree's git calls at once, not one worktree after another
    // (2026-11-21: ~34 git processes, one by one, per worktree request).
    let subjects = if entries.iter().any(|e| e.2) { branch_subjects(project) } else { Default::default() };
    let measured: Vec<Option<Measured>> = std::thread::scope(|s| {
        let jobs: Vec<_> = entries
            .iter()
            .map(|(entry, path, exists, base)| {
                let subjects = &subjects;
                exists.then(|| {
                    s.spawn(move || {
                        let base = base.as_deref().filter(|b| !b.is_empty());
                        let branch = entry.get("branch").and_then(Value::as_str).filter(|b| !b.is_empty());
                        measure_worktree(project, path, base, branch, subjects)
                    })
                })
            })
            .collect();
        jobs.into_iter()
            .map(|j| j.map(|h| h.join().unwrap_or_else(|_| Err("git measuring panicked".into()))))
            .collect()
    });
    let mut out = Vec::new();
    for ((mut entry, ..), got) in entries.into_iter().zip(measured) {
        match got {
            Some(Ok(fields)) => {
                for (k, v) in fields {
                    entry.insert(k.into(), v);
                }
            }
            Some(Err(e)) => {
                entry.insert("error".into(), json!(e));
            }
            None => {}
        }
        out.push(Value::Object(entry));
    }
    let checkouts: Map<String, Value> = listed
        .iter()
        .filter_map(|w| {
            let b = w.get("branch").and_then(Value::as_str).filter(|b| !b.is_empty())?;
            Some((b.to_string(), w["path"].clone()))
        })
        .collect();
    json!({"ok": true, "repo": true, "base": base, "main": main_path,
           "checkouts": checkouts, "worktrees": out})
}

// -- worktrees Imperatorium makes for background sessions ---------------------

/// changes.BASE_KEY: `branch.<worktree branch>.imperatoriumBase` notes the
/// branch a worktree was made from. `git branch -D` removes it with the rest.
const BASE_KEY: &str = "imperatoriumbase";

/// changes.recorded_bases: {worktree branch: the branch it was made from}.
pub fn recorded_bases(project: &str) -> std::collections::HashMap<String, String> {
    let pattern = format!(r"^branch\..*\.{BASE_KEY}$");
    let got = git(project, &["config", "--get-regexp", &pattern], false).unwrap_or_default();
    let suffix = format!(".{BASE_KEY}");
    let mut out = std::collections::HashMap::new();
    for line in splitlines(&got) {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        let value = crate::pyish::strip(value);
        if let Some(b) = key.strip_prefix("branch.").and_then(|k| k.strip_suffix(&suffix)) {
            if !value.is_empty() {
                out.insert(b.to_string(), value.to_string());
            }
        }
    }
    out
}

pub(crate) fn branch_exists(project: &str, branch: &str) -> bool {
    let r = format!("refs/heads/{branch}");
    git(project, &["rev-parse", "--verify", "--quiet", &r], false)
        .map(|t| !crate::pyish::strip(&t).is_empty())
        .unwrap_or(false)
}

/// changes.make_worktree: a worktree for a background session made from
/// what `source` has checked out - not from the remote's default branch,
/// where Claude Code would branch its own - at `.claude/worktrees/<name>`
/// in the main checkout, on `worktree-<name>`, with the source's branch
/// noted as its base. Ok(None) when `source` is not in a git repository.
pub fn make_worktree(source: &str, name: &str) -> Result<Option<Value>, String> {
    if !is_repo(source) {
        return Ok(None);
    }
    let listed = parse_worktrees(&git(source, &["worktree", "list", "--porcelain"], true)?);
    let Some(first) = listed.first().filter(|m| !crate::pyish::truthy(m.get("bare"))) else {
        return Ok(None);
    };
    let main = first["path"].as_str().unwrap_or("").to_string();
    let base = git(source, &["symbolic-ref", "--quiet", "--short", "HEAD"], false)
        .map(|t| crate::pyish::strip(&t).to_string())
        .unwrap_or_default();
    let base = (!base.is_empty()).then_some(base);
    let start = match &base {
        Some(b) => b.clone(),
        None => crate::pyish::strip(&git(source, &["rev-parse", "HEAD"], true)?).to_string(),
    };
    static NOT_STEM: OnceLock<Regex> = OnceLock::new();
    let stem: String = NOT_STEM
        .get_or_init(|| Regex::new("[^A-Za-z0-9-]+").expect("the stem pattern"))
        .replace_all(name, "-")
        .trim_matches('-')
        .to_lowercase()
        .chars()
        .take(40)
        .collect();
    let stem = if stem.is_empty() { "bg".to_string() } else { stem };
    let root = std::path::Path::new(&main).join(".claude").join("worktrees");
    let mut found = None;
    for n in 1..100 {
        let leaf = if n == 1 { stem.clone() } else { format!("{stem}-{n}") };
        let path = root.join(&leaf).to_string_lossy().into_owned();
        let branch = format!("worktree-{leaf}");
        if !std::path::Path::new(&path).exists() && !branch_exists(&main, &branch) {
            found = Some((path, branch));
            break;
        }
    }
    let Some((path, branch)) = found else {
        return Err(format!("no free worktree name for {stem}"));
    };
    git(&main, &["worktree", "add", "-b", &branch, &path, &start], true)?;
    if let Some(b) = &base {
        let key = format!("branch.{branch}.{BASE_KEY}");
        git(&main, &["config", &key, b], true)?;
    }
    Ok(Some(json!({"path": path, "branch": branch, "base": base, "start": start, "main": main})))
}

/// changes.drop_worktree: undo make_worktree when its session never started.
pub fn drop_worktree(made: &Value) {
    let main = made["main"].as_str().unwrap_or("");
    let path = made["path"].as_str().unwrap_or("");
    let branch = made["branch"].as_str().unwrap_or("");
    let _ = git(main, &["worktree", "remove", "--force", path], false);
    let _ = git(main, &["branch", "-D", branch], false);
}

/// changes.worktree_diff(project, path): what the branch would bring.
pub fn worktree_diff(project: &str, path: &str) -> Value {
    let listed = worktrees(project);
    if listed.get("ok") != Some(&json!(true)) {
        return listed;
    }
    let want = norm(path);
    let found = listed["worktrees"]
        .as_array()
        .and_then(|w| {
            w.iter()
                .find(|wt| same_dir(wt["path"].as_str().unwrap_or(""), &want, path))
        })
        .cloned();
    let Some(wt) = found else {
        return json!({"ok": false, "error": format!("no such worktree in {project}")});
    };
    let own = wt["base"].as_str().or(listed["base"].as_str());
    let (Some(base), Some(branch)) = (own, wt["branch"].as_str()) else {
        return json!({"ok": false, "error": "no branch to compare"});
    };
    let three = format!("{base}...{branch}");
    let two = format!("{base}..{branch}");
    let run = || -> Result<(String, String, String), String> {
        Ok((
            git(
                project,
                &["diff", "--no-color", "--no-ext-diff", &three],
                true,
            )?,
            git(project, &["diff", "--stat", &three], true)?,
            git(project, &["log", "--format=%h %s", &two], true)?,
        ))
    };
    match run() {
        Ok((diff, stat, log)) => json!({
            "ok": true, "branch": branch, "base": base, "stat": stat, "log": log,
            "diff": take(&diff, MAX_DIFF), "truncated": diff.chars().count() > MAX_DIFF,
        }),
        Err(e) => json!({"ok": false, "error": e}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_target_reads_one_remote_or_the_resolved_base() {
        let one = "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = https://github.com/o/r.git\n\
                   \tfetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"feat\"]\n\tremote = origin\n\
                   \tmerge = refs/heads/feat-pushed\n";
        assert_eq!(target_from_config(one, "feat"), Some(("o/r".into(), "feat-pushed".into())));
        assert_eq!(target_from_config(one, "main"), Some(("o/r".into(), "main".into())));
        let ssh = "[remote \"origin\"]\nurl = git@github.com:o/r\n";
        assert_eq!(target_from_config(ssh, "b"), Some(("o/r".into(), "b".into())));
        // A fork beside its upstream: only gh's recorded base is trusted.
        let two = "[remote \"origin\"]\nurl = git@github.com:me/r.git\n\
                   [remote \"upstream\"]\nurl = https://github.com/o/r\n";
        assert_eq!(target_from_config(two, "b"), None);
        let resolved = format!("{two}\tgh-resolved = base\n");
        assert_eq!(target_from_config(&resolved, "b"), Some(("o/r".into(), "b".into())));
        for none in [
            "[remote \"origin\"]\nurl = https://gitlab.com/o/r\n",
            "[include]\npath = other\n[remote \"origin\"]\nurl = https://github.com/o/r\n",
            "[remote \"origin\"]\nurl = https://github.com/-o/r\n",
            "",
        ] {
            assert_eq!(target_from_config(none, "b"), None, "{none:?}");
        }
    }

    /// This checkout: the same repository and branch gh would find itself.
    #[test]
    fn github_target_of_this_checkout_names_its_branch() {
        let here = env!("CARGO_MANIFEST_DIR");
        let Some((repo, branch)) = github_target(here) else { return }; // detached, or no single remote
        assert!(repo.contains('/') && !branch.is_empty(), "{repo} {branch}");
    }

    #[test]
    fn status_lines() {
        let s = parse_status(
            "## main...origin/main [ahead 2, behind 1]\n M a.py\nR  old -> new\n?? \"sp ace\"\n",
        );
        assert_eq!(s["branch"], "main");
        assert_eq!(s["upstream"], "origin/main");
        assert_eq!(
            (s["ahead"].clone(), s["behind"].clone()),
            (json!(2), json!(1))
        );
        let f = s["files"].as_array().unwrap();
        assert_eq!(f[0]["unstaged"], true);
        assert_eq!(f[1]["path"], "new");
        assert_eq!(f[2]["path"], "sp ace");
        assert_eq!(f[2]["untracked"], true);
    }

    #[test]
    fn worktree_listing() {
        let w = parse_worktrees("worktree C:/r\nHEAD abc\nbranch refs/heads/main\n\nworktree C:/r/.claude/worktrees/x\nHEAD def\ndetached\n");
        assert_eq!(w.len(), 2);
        assert_eq!(w[0]["branch"], "main");
        assert_eq!(w[1]["detached"], true);
        assert!(is_agent_worktree("C:/r", "C:/r/.claude/worktrees/x"));
    }

    fn sh(cwd: &str, args: &[&str]) {
        git(cwd, args, true).unwrap();
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("imp-head-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// HEAD read from the file agrees with `git branch --show-current`: on a
    /// branch, from a subfolder, detached, in a linked worktree.
    #[test]
    fn the_branch_is_read_from_head_as_git_reports_it() {
        let d = scratch("agree");
        let top = d.to_string_lossy().into_owned();
        sh(&top, &["init", "-q", "-b", "trunk"]);
        let asked = |cwd: &str| crate::pyish::strip(&git(cwd, &["branch", "--show-current"], true).unwrap()).to_string();
        assert_eq!(head_branch(&d), Head::Branch("trunk".into()));
        sh(&top, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "one"]);
        sh(&top, &["switch", "-q", "-c", "feature/x"]);
        std::fs::create_dir_all(d.join("sub").join("deeper")).unwrap();
        let sub = d.join("sub").join("deeper").to_string_lossy().into_owned();
        assert_eq!(current_branch(&sub).unwrap(), "feature/x");
        assert_eq!(current_branch(&sub).unwrap(), asked(&sub));
        sh(&top, &["switch", "-q", "--detach"]);
        assert_eq!(head_branch(&d), Head::Branch(String::new()));
        assert_eq!(current_branch(&top).unwrap(), asked(&top));
        let wt = d.join("wt");
        sh(&top, &["worktree", "add", "-q", "-b", "side", &wt.to_string_lossy()]);
        assert!(wt.join(".git").is_file());
        let w = wt.to_string_lossy().into_owned();
        assert_eq!(current_branch(&w).unwrap(), "side");
        assert_eq!(current_branch(&w).unwrap(), asked(&w));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_head_only_git_can_read_is_left_to_git() {
        let d = scratch("odd");
        let gd = d.join(".git");
        std::fs::create_dir_all(&gd).unwrap();
        for (head, want) in [
            ("ref: refs/heads/main\n", Head::Branch("main".into())),
            ("ref: refs/heads/main\r\n", Head::Branch("main".into())),
            ("ref: refs/heads/.invalid\n", Head::AskGit), // reftable's stub
            ("ref: refs/remotes/origin/x\n", Head::AskGit),
            (&format!("{}\n", "a".repeat(40)), Head::Branch(String::new())),
            ("garbage\n", Head::AskGit),
        ] {
            std::fs::write(gd.join("HEAD"), head).unwrap();
            assert_eq!(head_branch(&d), want, "{head:?}");
        }
        std::fs::remove_file(gd.join("HEAD")).unwrap();
        assert_eq!(head_branch(&d), Head::AskGit);
        // A gitdir file pointing somewhere relative.
        let wt = d.join("w");
        std::fs::create_dir_all(wt.join("meta")).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: meta\n").unwrap();
        std::fs::write(wt.join("meta").join("HEAD"), "ref: refs/heads/rel\n").unwrap();
        assert_eq!(head_branch(&wt), Head::Branch("rel".into()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_in_no_repository_needs_no_git() {
        let d = scratch("none");
        // Under the temp folder, which is in no repository here.
        if head_branch(&std::env::temp_dir()) == Head::NoRepo {
            assert_eq!(head_branch(&d), Head::NoRepo);
            assert!(current_branch(&d.to_string_lossy()).is_err());
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(windows)]
    #[test]
    fn the_wrapper_is_passed_over_for_the_git_it_starts() {
        let top = scratch("gfw");
        for p in ["cmd", "mingw64/bin", "usr/bin"] {
            std::fs::create_dir_all(top.join(p)).unwrap();
        }
        std::fs::write(top.join("cmd/git.exe"), "").unwrap();
        assert_eq!(real_git(&top.join("cmd/git.exe")), None, "no real git beside it yet");
        std::fs::write(top.join("mingw64/bin/git.exe"), "").unwrap();
        let real = real_git(&top.join("cmd").join("GIT.EXE")).unwrap();
        assert_eq!(real.exe, top.join("mingw64").join("bin").join("git.exe"));
        assert_eq!(real.usr_bin, top.join("usr").join("bin"));
        assert_eq!(real.msystem, "MINGW64");
        // The real one found first on PATH, or any other git, is run as found.
        assert_eq!(real_git(&top.join("mingw64/bin/git.exe")), None);
        assert_eq!(real_git(&top.join("cmd/gitk.exe")), None);
        let _ = std::fs::remove_dir_all(&top);
    }

    /// Through the real install: git run this way still finds sh (an alias
    /// with `!` needs it), as it does through the wrapper.
    #[cfg(windows)]
    #[test]
    fn the_real_git_keeps_what_the_wrapper_gave_it() {
        let Some(found) = crate::launch::which("git") else { return };
        if real_git(std::path::Path::new(&found)).is_none() {
            return; // PATH finds the real git already (a Git Bash shell)
        }
        let out = git(".", &["-c", "alias.e=!echo $MSYSTEM", "e"], true).unwrap();
        assert!(out.trim().starts_with("MINGW") || out.trim().starts_with("CLANG"), "{out:?}");
    }

    /// A repository on `main` with a long-lived `feat` beside it, ahead.
    fn repo_with_feature() -> (std::path::PathBuf, String, String) {
        let top = std::env::temp_dir().join(format!(
            "imp-wt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let main = top.join("r");
        std::fs::create_dir_all(&main).unwrap();
        let m = main.to_string_lossy().into_owned();
        sh(&m, &["init", "-q", "-b", "main"]);
        sh(&m, &["config", "user.email", "t@example.invalid"]);
        sh(&m, &["config", "user.name", "t"]);
        std::fs::write(main.join("f.txt"), "one\n").unwrap();
        sh(&m, &["add", "f.txt"]);
        sh(&m, &["commit", "-q", "-m", "init"]);
        let feat = top.join("feat").to_string_lossy().into_owned();
        sh(&m, &["worktree", "add", "-q", "-b", "feat", &feat]);
        std::fs::write(std::path::Path::new(&feat).join("feat.txt"), "x\n").unwrap();
        sh(&feat, &["add", "feat.txt"]);
        sh(&feat, &["commit", "-q", "-m", "feat work"]);
        (top, m, feat)
    }

    /// The cache answers only while nothing it was read from has moved: a
    /// second edit of a file already listed as modified, a commit, and a
    /// worktree's own HEAD each give a fresh read.
    #[test]
    fn changes_cache_follows_edits_commits_and_worktrees() {
        let (top, main, feat) = repo_with_feature();
        let f = std::path::Path::new(&main).join("f.txt");
        std::fs::write(&f, "two\n").unwrap();
        let first = changes(&main);
        assert!(first["diff"].as_str().unwrap().contains("+two"), "{first}");
        assert_eq!(changes(&main), first);
        std::fs::write(&f, "three, longer\n").unwrap();
        let second = changes(&main);
        assert!(second["diff"].as_str().unwrap().contains("+three"), "{second}");
        sh(&main, &["commit", "-qam", "three"]);
        let third = changes(&main);
        assert_eq!(third["files"], json!([]));
        assert_eq!(third["recent"][0]["subject"], "three");
        let wt = changes(&feat);
        assert_eq!(wt["branch"], "feat");
        assert_eq!(wt["recent"][0]["subject"], "feat work");
        let gitdir = match locate(std::path::Path::new(&feat)) {
            Located::Dir { git, .. } => git,
            _ => panic!("no git dir for the worktree"),
        };
        assert_eq!(head_oid(&gitdir).map(|o| o.len()), Some(40));
        let _ = std::fs::remove_dir_all(&top);
    }

    #[test]
    fn made_worktrees_branch_from_the_source_and_merge_back_into_it() {
        let (top, main, feat) = repo_with_feature();
        let made = make_worktree(&feat, "Fix The Thing").unwrap().unwrap();
        assert_eq!(made["branch"], "worktree-fix-the-thing");
        assert_eq!(made["base"], "feat");
        let path = made["path"].as_str().unwrap().to_string();
        assert!(std::path::Path::new(&path).join("feat.txt").is_file());
        let again = make_worktree(&feat, "Fix The Thing").unwrap().unwrap();
        assert_eq!(again["branch"], "worktree-fix-the-thing-2");
        drop_worktree(&again);
        assert!(!std::path::Path::new(again["path"].as_str().unwrap()).exists());

        let listed = worktrees(&main);
        let wt = listed["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["branch"] == "worktree-fix-the-thing")
            .unwrap()
            .clone();
        assert_eq!((wt["base"].clone(), wt["base_recorded"].clone()), (json!("feat"), json!(true)));

        std::fs::write(std::path::Path::new(&path).join("bg.txt"), "bg\n").unwrap();
        sh(&path, &["add", "bg.txt"]);
        sh(&path, &["commit", "-q", "-m", "bg change"]);
        let diff = worktree_diff(&main, &path);
        assert_eq!(diff["base"], "feat");
        assert!(!diff["stat"].as_str().unwrap().contains("feat.txt"));

        std::fs::write(std::path::Path::new(&feat).join("feat.txt"), "edited\n").unwrap();
        let refused = finish(&main, &path, "merge", false);
        assert!(refused["error"].as_str().unwrap().contains("checkout of feat has uncommitted"));
        sh(&feat, &["checkout", "-q", "--", "feat.txt"]);

        let merged = finish(&main, &path, "merge", false);
        assert_eq!(merged["ok"], json!(true), "{merged}");
        assert!(std::path::Path::new(&feat).join("bg.txt").is_file());
        assert!(!std::path::Path::new(&main).join("bg.txt").exists());

        let gone = finish(&main, &path, "discard", false);
        assert_eq!(gone["ok"], json!(true), "{gone}");
        assert!(!recorded_bases(&main).contains_key("worktree-fix-the-thing"));
        assert!(!branch_exists(&main, "worktree-fix-the-thing"));
        let _ = std::fs::remove_dir_all(top);
    }
}

// -- finishing a worktree: changes.finish -------------------------------------

pub const ACTIONS: &[&str] = &["merge", "pr", "discard"];

/// changes._find: the worktree at `path` under `project`, and the base branch.
type Found = (Map<String, Value>, Value, Map<String, Value>);
fn find(project: &str, path: &str) -> Result<Found, Value> {
    // Only that worktree measured: the others' git calls were wasted.
    let listed = worktrees_for(project, Some(path));
    if listed["ok"] != json!(true) {
        return Err(listed);
    }
    let want = norm(path);
    for wt in listed["worktrees"].as_array().into_iter().flatten() {
        if same_dir(wt["path"].as_str().unwrap_or(""), &want, path) {
            let base = match &wt["base"] {
                Value::String(b) if !b.is_empty() => wt["base"].clone(),
                _ => listed["base"].clone(),
            };
            return Ok((
                wt.as_object().cloned().unwrap_or_default(),
                base,
                listed["checkouts"].as_object().cloned().unwrap_or_default(),
            ));
        }
    }
    Err(json!({"ok": false, "error": format!("no such worktree in {project}")}))
}

/// changes._run: (exit code, stdout + stderr), 60 s at most.
fn run(cmd: &[&str], cwd: Option<&str>) -> (i32, String) {
    let mut c = if cmd[0] == "git" { git_command() } else { Command::new(cmd[0]) };
    c.args(&cmd[1..]);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    let o = match crate::proc::run_timeout(&mut c, Duration::from_secs(60)) {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            return (
                1,
                format!("Command '{}' timed out after 60.0 seconds", cmd.join(" ")),
            )
        }
        Err(e) => return (1, e.to_string()),
    };
    let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&o.stderr));
    (o.status.code().unwrap_or(1), text)
}

/// The last `n` characters, as Python's `s[-n:]`.
fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect()
}

/// changes.finish: merge, open a PR for, or discard one worktree. Only
/// worktrees Claude Code made under .claude/worktrees are touched; merge
/// needs both checkouts clean; discarding unsaved work needs `force`.
pub fn finish(project: &str, path: &str, action: &str, force: bool) -> Value {
    if !ACTIONS.contains(&action) {
        return json!({"ok": false, "error": format!("action must be one of {}", ACTIONS.join(", "))});
    }
    let (wt, base, checkouts) = match find(project, path) {
        Ok(f) => f,
        Err(e) => return e,
    };
    if !crate::pyish::truthy(wt.get("agent")) {
        return json!({"ok": false, "error": format!(
            "only background-session worktrees under {} are managed here",
            if cfg!(windows) { ".claude\\worktrees" } else { ".claude/worktrees" })});
    }
    let Some(branch) = wt
        .get("branch")
        .and_then(Value::as_str)
        .filter(|b| !b.is_empty())
        .map(str::to_string)
    else {
        return json!({"ok": false, "error": "that worktree has no branch (detached HEAD)"});
    };
    let dirty = crate::pyish::truthy(wt.get("dirty"));
    // What blocks a merge or a push: changes to tracked files only.
    let tracked_dirty = dirty && !crate::pyish::truthy(wt.get("untracked_only"));
    let ahead = wt.get("ahead").and_then(Value::as_i64).unwrap_or(0);
    let wt_path = wt
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let base_s = crate::pyish::py_str(&base);
    let result: Result<Value, String> = (|| {
        match action {
            "merge" => {
                if tracked_dirty {
                    return Ok(
                        json!({"ok": false, "error": "the worktree has uncommitted changes"}),
                    );
                }
                // Into its base branch, wherever that is checked out.
                let Some(into) = checkouts
                    .get(&base_s)
                    .and_then(Value::as_str)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                else {
                    return Ok(json!({"ok": false, "error": format!(
                        "{base_s} is not checked out anywhere - check it out to merge into it")}));
                };
                let status = git(&into, &["status", "--porcelain", "--untracked-files=no"], true)?;
                if !crate::pyish::strip(&status).is_empty() {
                    return Ok(json!({"ok": false, "error": format!(
                        "the checkout of {base_s} has uncommitted changes")}));
                }
                if ahead == 0 {
                    return Ok(json!({"ok": false, "error": format!(
                        "nothing to merge - {branch} is not ahead of {base_s}")}));
                }
                match git(&into, &["merge", "--no-ff", "--no-edit", &branch], true) {
                    Ok(out) => Ok(
                        json!({"ok": true, "action": "merge", "branch": branch, "base": base,
                                         "output": tail(crate::pyish::strip(&out), 400)}),
                    ),
                    Err(e) => {
                        // A conflict or a refusal: either way the checkout
                        // goes back as it was.
                        let conflicted = !crate::pyish::strip(&git(
                            &into,
                            &["diff", "--name-only", "--diff-filter=U"],
                            false,
                        )?)
                        .is_empty();
                        git(&into, &["merge", "--abort"], false)?;
                        Ok(json!({"ok": false, "error": if conflicted {
                            "merge conflicts - aborted, nothing changed".to_string()
                        } else {
                            format!("merge refused: {e}")
                        }}))
                    }
                }
            }
            "pr" => {
                if tracked_dirty {
                    return Ok(
                        json!({"ok": false, "error": "the worktree has uncommitted changes"}),
                    );
                }
                let pushed = run(
                    &["git", "-C", &wt_path, "push", "-u", "origin", &branch],
                    None,
                );
                if pushed.0 != 0 {
                    return Ok(
                        json!({"ok": false, "error": format!("push failed: {}", tail(&pushed.1, 300))}),
                    );
                }
                let made = run(
                    &[
                        "gh", "pr", "create", "--fill", "--head", &branch, "--base", &base_s,
                    ],
                    Some(&wt_path),
                );
                if made.0 != 0 {
                    return Ok(
                        json!({"ok": false, "error": format!("gh pr create failed: {}", tail(&made.1, 300))}),
                    );
                }
                let text = crate::pyish::strip(&made.1);
                let url = splitlines(text)
                    .last()
                    .map(|l| l.to_string())
                    .unwrap_or_default();
                Ok(json!({"ok": true, "action": "pr", "branch": branch, "url": url}))
            }
            _ => {
                if (dirty || ahead != 0) && !force {
                    let what = if dirty {
                        "uncommitted changes".to_string()
                    } else {
                        format!("{ahead} unmerged commit(s)")
                    };
                    return Ok(json!({"ok": false, "needs_force": true,
                                     "error": format!("that worktree has {what} - discarding loses it")}));
                }
                let mut args = vec!["worktree", "remove"];
                if force {
                    args.push("--force");
                }
                args.push(&wt_path);
                git(project, &args, true)?;
                // -D: what is unmerged was checked above, against the
                // worktree's own base; -d would check the main checkout's.
                git(project, &["branch", "-D", &branch], false)?;
                let key = format!("branch.{branch}.{BASE_KEY}");
                git(project, &["config", "--unset", &key], false)?;
                Ok(json!({"ok": true, "action": "discard", "branch": branch}))
            }
        }
    })();
    result.unwrap_or_else(|e| json!({"ok": false, "error": e}))
}

/// Ported from the Python suite (test_features.py): real temporary git
/// repositories, as there, because this module is a thin wrapper over git.
#[cfg(test)]
mod features_tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn sh(cwd: &str, args: &[&str]) -> String {
        git(cwd, args, true).unwrap_or_else(|e| panic!("git {args:?} in {cwd}: {e}"))
    }

    fn norm_p(p: &str) -> String {
        norm(p)
    }

    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    /// A repository on `main` with one commit; removed with its folder.
    struct Repo {
        top: PathBuf,
        dir: String,
    }

    impl Repo {
        fn new(label: &str) -> Repo {
            let top = std::env::temp_dir().join(format!(
                "imp-feat-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let main = top.join("r");
            std::fs::create_dir_all(&main).unwrap();
            let dir = main.to_string_lossy().into_owned();
            sh(&dir, &["init", "-q", "-b", "main"]);
            sh(&dir, &["config", "user.email", "test@example.com"]);
            sh(&dir, &["config", "user.name", "Test"]);
            sh(&dir, &["config", "core.autocrlf", "false"]);
            write(&main.join("f.txt"), "one\n");
            sh(&dir, &["add", "f.txt"]);
            sh(&dir, &["commit", "-q", "-m", "init"]);
            Repo { top, dir }
        }

        fn path(&self, rel: &str) -> PathBuf {
            Path::new(&self.dir).join(rel)
        }

        /// A worktree under .claude/worktrees/, the shape Claude Code makes.
        fn agent_worktree(&self, name: &str, branch: &str, commit: bool) -> String {
            let rel = format!(".claude/worktrees/{name}");
            sh(&self.dir, &["worktree", "add", &rel, "-b", branch, "-q"]);
            let full = self.path(&rel);
            let full_s = full.to_string_lossy().into_owned();
            if commit {
                let mut text = std::fs::read_to_string(full.join("f.txt")).unwrap();
                text.push_str("two\n");
                write(&full.join("f.txt"), &text);
                sh(&full_s, &["add", "f.txt"]);
                sh(&full_s, &["commit", "-q", "-m", "wt change"]);
            }
            full_s
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.top);
        }
    }

    /// A folder that is not a repository.
    fn plain_dir(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("imp-feat-plain-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn features_changes_of_a_clean_repo() {
        let r = Repo::new("clean");
        let got = changes(&r.dir);
        assert_eq!(got["ok"], true);
        assert_eq!(got["repo"], true);
        assert_eq!(got["files"], json!([]));
        assert_eq!(got["diff"], "");
        assert_eq!(got["branch"], "main");
    }

    #[test]
    fn features_changes_report_modified_and_untracked_files() {
        let r = Repo::new("dirty");
        write(&r.path("f.txt"), "one\nmodified\n");
        write(&r.path("new.txt"), "new\n");
        let got = changes(&r.dir);
        assert_eq!(got["ok"], true);
        let files = got["files"].as_array().unwrap();
        let mut paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
        paths.sort();
        assert_eq!(paths, ["f.txt", "new.txt"]);
        let by = |p: &str| files.iter().find(|f| f["path"] == p).unwrap().clone();
        assert_eq!(by("new.txt")["untracked"], true);
        assert_eq!(by("f.txt")["unstaged"], true);
        assert!(got["diff"].as_str().unwrap().contains("modified"));
    }

    #[test]
    fn features_changes_of_a_non_repo_and_a_missing_directory() {
        let other = plain_dir("changes");
        let got = changes(&other.to_string_lossy());
        assert_eq!(got["ok"], true);
        assert_eq!(got["repo"], false);
        assert_eq!(got["files"], json!([]));
        let r = Repo::new("missing");
        let gone = changes(&r.path("nope-nope").to_string_lossy());
        assert_eq!(gone["ok"], false);
        assert!(gone.get("error").is_some());
        let _ = std::fs::remove_dir_all(other);
    }

    #[test]
    fn features_worktrees_ahead_count_agent_flag_and_dirty() {
        let r = Repo::new("wts");
        assert_eq!(worktrees(&r.dir)["worktrees"], json!([])); // a plain repo
        r.agent_worktree("x", "feature", true);
        let got = worktrees(&r.dir);
        assert_eq!(got["ok"], true);
        assert_eq!(got["base"], "main");
        let list = got["worktrees"].as_array().unwrap();
        assert_eq!(list.len(), 1);
        let wt = &list[0];
        assert_eq!(wt["agent"], true);
        assert_eq!(wt["branch"], "feature");
        assert_eq!(wt["ahead"], 1);
        assert_eq!(wt["behind"], 0);
        assert_eq!(wt["dirty"], false);
        assert_eq!(wt["subject"], "wt change");

        // Made by hand: not an agent's.
        sh(&r.dir, &["worktree", "add", "sibling", "-b", "other", "-q"]);
        let got = worktrees(&r.dir);
        let by = |b: &str| {
            got["worktrees"].as_array().unwrap().iter().find(|w| w["branch"] == b).unwrap().clone()
        };
        assert_eq!(by("other")["agent"], false);

        // Uncommitted work is dirty.
        let y = r.agent_worktree("y", "dirtyfeat", false);
        write(&Path::new(&y).join("f.txt"), "one\nuncommitted\n");
        let got = worktrees(&r.dir);
        let dirty = got["worktrees"].as_array().unwrap().iter().find(|w| w["branch"] == "dirtyfeat").unwrap().clone();
        assert_eq!(dirty["dirty"], true);
    }

    #[test]
    fn features_worktrees_of_a_non_repo() {
        let other = plain_dir("wts");
        let got = worktrees(&other.to_string_lossy());
        assert_eq!(got["ok"], true);
        assert_eq!(got["repo"], false);
        let _ = std::fs::remove_dir_all(other);
    }

    #[test]
    fn features_pr_reports_a_push_failure_and_refuses_a_dirty_worktree_first() {
        let r = Repo::new("pr");
        let wt = r.agent_worktree("x", "feature", true);
        // No origin to push to: the failure is the push's, not gh's.
        let out = finish(&r.dir, &wt, "pr", false);
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("push failed"), "{out}");
        // Dirty: refused before any network.
        write(&Path::new(&wt).join("f.txt"), "one\ntwo\nuncommitted\n");
        let out = finish(&r.dir, &wt, "pr", false);
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("uncommitted changes"), "{out}");
    }

    #[test]
    fn features_a_merge_git_refuses_is_reported_and_changes_nothing() {
        // A refusal that leaves no conflict markers (here: unrelated
        // histories) was once reported as a successful merge.
        let r = Repo::new("orphan");
        sh(&r.dir, &["worktree", "add", ".claude/worktrees/x", "-q", "--detach"]);
        let wt = r.path(".claude/worktrees/x").to_string_lossy().into_owned();
        sh(&wt, &["checkout", "--orphan", "orphanfeat", "-q"]);
        sh(&wt, &["rm", "-f", "-q", "f.txt"]);
        write(&Path::new(&wt).join("g.txt"), "two\n");
        sh(&wt, &["add", "g.txt"]);
        sh(&wt, &["commit", "-q", "-m", "orphan"]);
        let before = sh(&r.dir, &["log", "--oneline"]);
        let out = finish(&r.dir, &wt, "merge", false);
        let after = sh(&r.dir, &["log", "--oneline"]);
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("refused"), "{out}");
        assert_eq!(after, before);
    }

    #[test]
    fn features_untracked_files_alone_do_not_block_a_merge() {
        // 2026-11-24: a `__pycache__/` left behind blocked Merge, while the
        // base checkout's own check has always ignored untracked files.
        let r = Repo::new("untracked");
        let wt = r.agent_worktree("u", "ufeat", true);
        write(&Path::new(&wt).join("scratch.txt"), "not tracked\n");
        let got = worktrees(&r.dir);
        let w = got["worktrees"].as_array().unwrap().iter().find(|w| w["branch"] == "ufeat").unwrap().clone();
        assert_eq!(w["dirty"], true);
        assert_eq!(w["untracked_only"], true);
        let out = finish(&r.dir, &wt, "merge", false);
        assert_eq!(out["ok"], true, "{out}");
        // A tracked change still blocks it, and Discard still counts both.
        write(&Path::new(&wt).join("f.txt"), "one\nchanged\n");
        let got = worktrees(&r.dir);
        let w = got["worktrees"].as_array().unwrap().iter().find(|w| w["branch"] == "ufeat").unwrap().clone();
        assert!(w.get("untracked_only").is_none());
        let out = finish(&r.dir, &wt, "pr", false);
        assert!(out["error"].as_str().unwrap_or("").contains("uncommitted"), "{out}");
    }

    /// `main` with a long-lived `feat` in a sibling worktree, ahead of it.
    fn with_feat(label: &str) -> (Repo, String) {
        let r = Repo::new(label);
        let feat = r.top.join("feat").to_string_lossy().into_owned();
        sh(&r.dir, &["worktree", "add", &feat, "-b", "feat", "-q"]);
        write(&Path::new(&feat).join("feat.txt"), "feature work\n");
        sh(&feat, &["add", "feat.txt"]);
        sh(&feat, &["commit", "-q", "-m", "feat work"]);
        (r, feat)
    }

    #[test]
    fn features_made_worktree_edge_cases() {
        let (r, feat) = with_feat("made");
        // A detached source gives its commit and no base.
        let head = sh(&feat, &["rev-parse", "HEAD"]).trim().to_string();
        sh(&feat, &["checkout", "-q", "--detach"]);
        let made = make_worktree(&feat, "d").unwrap().unwrap();
        assert_eq!(made["base"], Value::Null);
        assert_eq!(made["start"], head.as_str());
        let listed = worktrees(&r.dir);
        let wt = listed["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["branch"] == made["branch"])
            .unwrap()
            .clone();
        assert_eq!((wt["base"].clone(), wt["base_recorded"].clone()), (json!("main"), json!(false)));
        // Not a repository: None.
        let other = plain_dir("made");
        assert!(make_worktree(&other.to_string_lossy(), "x").unwrap().is_none());
        let _ = std::fs::remove_dir_all(other);
    }

    #[test]
    fn features_merge_refuses_a_base_checked_out_nowhere() {
        let (r, feat) = with_feat("nowhere");
        let made = make_worktree(&feat, "m").unwrap().unwrap();
        let path = made["path"].as_str().unwrap().to_string();
        write(&Path::new(&path).join("bg.txt"), "bg\n");
        sh(&path, &["add", "bg.txt"]);
        sh(&path, &["commit", "-q", "-m", "bg change"]);
        sh(&feat, &["checkout", "-q", "--detach"]);
        let out = finish(&r.dir, &path, "merge", false);
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("feat is not checked out anywhere"), "{out}");
        assert_eq!(norm_p(&path), norm_p(made["path"].as_str().unwrap()));
    }

    #[test]
    fn features_drop_worktree_undoes_it_including_the_branch() {
        let (r, feat) = with_feat("drop");
        let made = make_worktree(&feat, "m").unwrap().unwrap();
        let branch = made["branch"].as_str().unwrap().to_string();
        assert!(Path::new(made["path"].as_str().unwrap()).is_dir());
        drop_worktree(&made);
        assert!(!Path::new(made["path"].as_str().unwrap()).is_dir());
        assert_eq!(sh(&r.dir, &["branch", "--list", &branch]).trim(), "");
    }
}
