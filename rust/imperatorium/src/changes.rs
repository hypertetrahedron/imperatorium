//! What a session has changed, and the worktrees background sessions leave:
//! the read side of `imperatorium/changes.py` (status, diff, recent commits,
//! worktrees and their merge-base diffs), and finishing a worktree (merge,
//! PR, discard). Every git call is bounded in time
//! and output, as there.

use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::activity::splitlines;
use crate::paths;
use crate::pyish::take;

const TIMEOUT: f64 = 20.0;
const MAX_DIFF: usize = 400 * 1024;

/// stdout of `git -C cwd args`; Err(message) as changes.git raises it.
pub fn git(cwd: &str, args: &[&str], check: bool) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("git is not on PATH".into())
        }
        Err(e) => return Err(e.to_string()),
    };
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let o = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = std::io::Read::read_to_end(&mut out, &mut b);
        b
    });
    let e = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = std::io::Read::read_to_end(&mut err, &mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed().as_secs_f64() < TIMEOUT => {
                std::thread::sleep(Duration::from_millis(5))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git {} took longer than {}s", args[0], TIMEOUT));
            }
        }
    };
    let stdout = String::from_utf8_lossy(&o.join().unwrap_or_default()).into_owned();
    if check && !status.success() {
        let stderr = String::from_utf8_lossy(&e.join().unwrap_or_default()).into_owned();
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
pub fn changes(cwd: &str) -> Value {
    if cwd.is_empty() || !std::path::Path::new(cwd).is_dir() {
        return json!({"ok": false, "error": "no working directory for this session"});
    }
    if !is_repo(cwd) {
        return json!({"ok": true, "repo": false, "files": [], "diff": ""});
    }
    let run = || -> Result<Read, String> {
        let status = parse_status(&git(
            cwd,
            &["status", "--porcelain=v1", "-b", "--untracked-files=all"],
            true,
        )?);
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
fn parse_worktrees(text: &str) -> Vec<Map<String, Value>> {
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

fn norm(p: &str) -> String {
    paths::abs_normcase(p).unwrap_or_default()
}

/// True for a worktree Claude Code made for a background session.
fn is_agent_worktree(main: &str, path: &str) -> bool {
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
    let mut out = Vec::new();
    for wt in &listed[1..] {
        let path = wt["path"].as_str().unwrap_or("").to_string();
        let mut entry = wt.clone();
        entry.insert("agent".into(), json!(is_agent_worktree(&main_path, &path)));
        let exists = std::path::Path::new(&path).is_dir();
        entry.insert("exists".into(), json!(exists));
        if exists {
            let run = |entry: &mut Map<String, Value>| -> Result<(), String> {
                let dirty =
                    !crate::pyish::strip(&git(&path, &["status", "--porcelain"], true)?).is_empty();
                entry.insert("dirty".into(), json!(dirty));
                if let (Some(b), Some(br)) = (&base, wt.get("branch").and_then(Value::as_str)) {
                    let count = |range: String| -> Result<i64, String> {
                        let t = git(project, &["rev-list", "--count", &range], true)?;
                        let t = crate::pyish::strip(&t);
                        if t.is_empty() {
                            return Ok(0);
                        }
                        t.parse::<i64>()
                            .map_err(|_| format!("invalid literal for int() with base 10: '{t}'"))
                    };
                    entry.insert("ahead".into(), json!(count(format!("{b}..{br}"))?));
                    entry.insert("behind".into(), json!(count(format!("{br}..{b}"))?));
                }
                let subject = git(&path, &["log", "-1", "--format=%s"], false)?;
                entry.insert("subject".into(), json!(crate::pyish::strip(&subject)));
                Ok(())
            };
            if let Err(e) = run(&mut entry) {
                entry.insert("error".into(), json!(e));
            }
        }
        out.push(Value::Object(entry));
    }
    json!({"ok": true, "repo": true, "base": base, "main": main_path, "worktrees": out})
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
                .find(|wt| norm(wt["path"].as_str().unwrap_or("")) == want)
        })
        .cloned();
    let Some(wt) = found else {
        return json!({"ok": false, "error": format!("no such worktree in {project}")});
    };
    let (Some(base), Some(branch)) = (listed["base"].as_str(), wt["branch"].as_str()) else {
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
}

// -- finishing a worktree: changes.finish -------------------------------------

pub const ACTIONS: &[&str] = &["merge", "pr", "discard"];

/// changes._find: the worktree at `path` under `project`, and the base branch.
fn find(project: &str, path: &str) -> Result<(Map<String, Value>, Value), Value> {
    let listed = worktrees(project);
    if listed["ok"] != json!(true) {
        return Err(listed);
    }
    let want = norm(path);
    for wt in listed["worktrees"].as_array().into_iter().flatten() {
        if norm(wt["path"].as_str().unwrap_or("")) == want {
            return Ok((
                wt.as_object().cloned().unwrap_or_default(),
                listed["base"].clone(),
            ));
        }
    }
    Err(json!({"ok": false, "error": format!("no such worktree in {project}")}))
}

/// changes._run: (exit code, stdout + stderr), 60 s at most.
fn run(cmd: &[&str], cwd: Option<&str>) -> (i32, String) {
    let mut c = Command::new(cmd[0]);
    c.args(&cmd[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => return (1, e.to_string()),
    };
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let o = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = std::io::Read::read_to_end(&mut out, &mut b);
        b
    });
    let e = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = std::io::Read::read_to_end(&mut err, &mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed().as_secs_f64() < 60.0 => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return (
                    1,
                    format!("Command '{}' timed out after 60.0 seconds", cmd.join(" ")),
                );
            }
        }
    };
    let mut text = String::from_utf8_lossy(&o.join().unwrap_or_default()).into_owned();
    text.push_str(&String::from_utf8_lossy(&e.join().unwrap_or_default()));
    (status.code().unwrap_or(1), text)
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
    let (wt, base) = match find(project, path) {
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
                if dirty {
                    return Ok(
                        json!({"ok": false, "error": "the worktree has uncommitted changes"}),
                    );
                }
                let main = git(
                    project,
                    &["status", "--porcelain", "--untracked-files=no"],
                    true,
                )?;
                if !crate::pyish::strip(&main).is_empty() {
                    return Ok(
                        json!({"ok": false, "error": "the main checkout has uncommitted changes"}),
                    );
                }
                if ahead == 0 {
                    return Ok(json!({"ok": false, "error": format!(
                        "nothing to merge - {branch} is not ahead of {base_s}")}));
                }
                match git(project, &["merge", "--no-ff", "--no-edit", &branch], true) {
                    Ok(out) => Ok(
                        json!({"ok": true, "action": "merge", "branch": branch, "base": base,
                                         "output": tail(crate::pyish::strip(&out), 400)}),
                    ),
                    Err(e) => {
                        // A conflict or a refusal: either way the checkout
                        // goes back as it was.
                        let conflicted = !crate::pyish::strip(&git(
                            project,
                            &["diff", "--name-only", "--diff-filter=U"],
                            false,
                        )?)
                        .is_empty();
                        git(project, &["merge", "--abort"], false)?;
                        Ok(json!({"ok": false, "error": if conflicted {
                            "merge conflicts - aborted, nothing changed".to_string()
                        } else {
                            format!("merge refused: {e}")
                        }}))
                    }
                }
            }
            "pr" => {
                if dirty {
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
                git(
                    project,
                    &["branch", if force { "-D" } else { "-d" }, &branch],
                    false,
                )?;
                Ok(json!({"ok": true, "action": "discard", "branch": branch}))
            }
        }
    })();
    result.unwrap_or_else(|e| json!({"ok": false, "error": e}))
}
