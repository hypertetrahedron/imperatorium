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
const MAX_DIFF: usize = 400 * 1024;

/// stdout of `git -C cwd args`; Err(message) as changes.git raises it.
pub fn git(cwd: &str, args: &[&str], check: bool) -> Result<String, String> {
    let mut cmd = Command::new("git");
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
    let recorded = recorded_bases(project);
    let mut out = Vec::new();
    for wt in &listed[1..] {
        let path = wt["path"].as_str().unwrap_or("").to_string();
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
        if exists {
            let run = |entry: &mut Map<String, Value>| -> Result<(), String> {
                let dirty =
                    !crate::pyish::strip(&git(&path, &["status", "--porcelain"], true)?).is_empty();
                entry.insert("dirty".into(), json!(dirty));
                if let (Some(b), Some(br)) = (
                    base.as_ref().filter(|b| !b.is_empty()),
                    wt.get("branch").and_then(Value::as_str).filter(|b| !b.is_empty()),
                ) {
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

fn branch_exists(project: &str, branch: &str) -> bool {
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
                .find(|wt| norm(wt["path"].as_str().unwrap_or("")) == want)
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
fn find(project: &str, path: &str) -> Result<(Map<String, Value>, Value, Map<String, Value>), Value> {
    let listed = worktrees(project);
    if listed["ok"] != json!(true) {
        return Err(listed);
    }
    let want = norm(path);
    for wt in listed["worktrees"].as_array().into_iter().flatten() {
        if norm(wt["path"].as_str().unwrap_or("")) == want {
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
    let mut c = Command::new(cmd[0]);
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
