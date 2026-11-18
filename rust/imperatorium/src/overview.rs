//! Project Changes (2026-11-17): every line of work in one project at once -
//! the main checkout, each worktree, and each local branch ahead of its base
//! that no worktree has checked out - with the files each touches, and the
//! files more than one of them touches (the overlaps, where a merge will
//! meet another's work). Files only, not lines. Read-only; every git call
//! is the bounded one in `changes`.

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Map, Value};

use crate::activity::splitlines;
use crate::changes::{
    branch_exists, git, is_agent_worktree, norm, parse_status, parse_worktrees,
    recorded_bases, same_dir, MAX_DIFF,
};
use crate::pyish::{strip, take};

/// Files listed for one line of work; past this the list says it is cut.
const MAX_FILES: usize = 2000;
/// Branches without a worktree looked at, the most recently committed first.
const MAX_BRANCHES: usize = 30;

/// Paths as git writes them, not quoted, so the same file is the same
/// string in every line.
const RAW: [&str; 2] = ["-c", "core.quotepath=false"];

fn with_raw<'a>(args: &[&'a str]) -> Vec<&'a str> {
    RAW.iter().copied().chain(args.iter().copied()).collect()
}

fn names(text: &str) -> Vec<String> {
    splitlines(text)
        .into_iter()
        .map(str::to_string)
        .filter(|l| !l.is_empty())
        .collect()
}

/// (ahead, behind): what `tip` has that `base` has not, and the other way
/// round, in one git call (each costs ~100 ms on Windows, bounded).
fn ahead_behind(cwd: &str, base: &str, tip: &str) -> Result<(i64, i64), String> {
    let t = git(cwd, &["rev-list", "--left-right", "--count", &format!("{base}...{tip}")], true)?;
    let mut n = strip(&t).split_whitespace().map(|x| x.parse::<i64>().unwrap_or(0));
    let behind = n.next().unwrap_or(0);
    Ok((n.next().unwrap_or(0), behind))
}

fn capped(mut files: Vec<String>, line: &mut Map<String, Value>, key: &str) {
    files.sort();
    files.dedup();
    if files.len() > MAX_FILES {
        line.insert("truncated".into(), json!(true));
        files.truncate(MAX_FILES);
    }
    line.insert(key.into(), json!(files));
}

/// One checked-out line: the main checkout (`main` true) or a worktree.
/// `others` are the other worktrees' folders, normalised: git lists one
/// inside this folder (as Claude Code's are, under .claude/worktrees) as an
/// untracked folder of this one, and it is not.
fn checkout_line(wt: &Map<String, Value>, main: bool, main_path: &str, base: Option<&str>,
                 subject: Option<&str>, others: &HashSet<String>) -> Map<String, Value> {
    let path = wt["path"].as_str().unwrap_or("").to_string();
    let branch = wt.get("branch").and_then(Value::as_str).filter(|b| !b.is_empty());
    let mut line = Map::new();
    line.insert("kind".into(), json!(if main { "main" } else { "worktree" }));
    line.insert("path".into(), json!(path));
    line.insert("branch".into(), json!(branch));
    line.insert("head".into(), wt.get("head").cloned().unwrap_or(Value::Null));
    // The main checkout on the base itself has no base to be ahead of.
    let base = base.filter(|b| !(main && Some(*b) == branch));
    line.insert("base".into(), json!(base));
    line.insert("agent".into(), json!(!main && is_agent_worktree(main_path, &path)));
    line.insert("committed".into(), json!([]));
    line.insert("uncommitted".into(), json!([]));
    if !std::path::Path::new(&path).is_dir() {
        line.insert("error".into(), json!("its folder is gone"));
        return line;
    }
    let run = |line: &mut Map<String, Value>| -> Result<(), String> {
        let status = parse_status(&git(
            &path,
            &with_raw(&["status", "--porcelain=v1", "-b", "--untracked-files=all"]),
            true,
        )?);
        let dirty: Vec<String> = status["files"]
            .as_array()
            .map(|f| f.iter().filter_map(|x| x["path"].as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let dirty: Vec<String> = dirty
            .into_iter()
            .filter(|f| {
                !(f.ends_with('/') && others.contains(&norm(&std::path::Path::new(&path).join(f).to_string_lossy())))
            })
            .collect();
        capped(dirty, line, "uncommitted");
        if main && base.is_none() {
            // The base itself: how it stands against its upstream.
            line.insert("upstream".into(), status["upstream"].clone());
            line.insert("ahead".into(), status["ahead"].clone());
            line.insert("behind".into(), status["behind"].clone());
        } else if let Some(b) = base {
            let three = format!("{b}...HEAD");
            capped(names(&git(&path, &with_raw(&["diff", "--name-only", &three]), true)?), line, "committed");
            let (ahead, behind) = ahead_behind(&path, b, "HEAD")?;
            line.insert("ahead".into(), json!(ahead));
            line.insert("behind".into(), json!(behind));
        }
        // A branch's last subject came with the branch list; detached, ask.
        let subject = match subject {
            Some(s) => s.to_string(),
            None => strip(&git(&path, &["log", "-1", "--format=%s"], false)?).to_string(),
        };
        line.insert("subject".into(), json!(subject));
        Ok(())
    };
    if let Err(e) = run(&mut line) {
        line.insert("error".into(), json!(e));
    }
    line
}

/// A local branch no worktree has checked out, if it is ahead of its base.
fn branch_line(project: &str, branch: &str, subject: &str, base: &str) -> Option<Map<String, Value>> {
    let mut line = Map::new();
    line.insert("kind".into(), json!("branch"));
    line.insert("path".into(), Value::Null);
    line.insert("branch".into(), json!(branch));
    line.insert("base".into(), json!(base));
    line.insert("agent".into(), json!(false));
    line.insert("subject".into(), json!(subject));
    line.insert("uncommitted".into(), json!([]));
    line.insert("committed".into(), json!([]));
    let run = |line: &mut Map<String, Value>| -> Result<bool, String> {
        let (ahead, behind) = ahead_behind(project, base, branch)?;
        if ahead == 0 {
            return Ok(false); // merged, or never moved: nothing of its own
        }
        line.insert("ahead".into(), json!(ahead));
        line.insert("behind".into(), json!(behind));
        let three = format!("{base}...{branch}");
        capped(names(&git(project, &with_raw(&["diff", "--name-only", &three]), true)?), line, "committed");
        Ok(true)
    };
    match run(&mut line) {
        Ok(true) => Some(line),
        Ok(false) => None,
        Err(e) => {
            line.insert("error".into(), json!(e));
            Some(line)
        }
    }
}

fn set(l: &Value, k: &str) -> HashSet<String> {
    l[k].as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Pure: the pairs of lines worth a closer look - those whose files, each
/// against the base, meet.
pub fn candidate_pairs(lines: &[Value]) -> Vec<(usize, usize)> {
    let all: Vec<HashSet<String>> =
        lines.iter().map(|l| &set(l, "committed") | &set(l, "uncommitted")).collect();
    let mut out = Vec::new();
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            if !all[i].is_disjoint(&all[j]) {
                out.push((i, j));
            }
        }
    }
    out
}

/// What each of two lines committed since they parted from each other
/// (not from the base): for a pair, (i's files, j's files).
pub type Sides = (HashSet<String>, HashSet<String>);
pub type Since = std::collections::HashMap<(usize, usize), Sides>;

/// Pure: the files two lines both change, each with the lines that do and
/// how ("committed", "uncommitted" or "both"); the most shared first.
/// Where `since` has a pair, its commits count only from where the two
/// parted: a branch made from another's unmerged work does not overlap it
/// in the commits they share. A pair without it is compared against the
/// base. What is not committed always counts.
pub fn overlaps(lines: &[Value], since: &Since) -> Vec<Value> {
    let committed: Vec<HashSet<String>> = lines.iter().map(|l| set(l, "committed")).collect();
    let uncommitted: Vec<HashSet<String>> = lines.iter().map(|l| set(l, "uncommitted")).collect();
    let mut by: BTreeMap<String, std::collections::BTreeSet<usize>> = BTreeMap::new();
    for (i, j) in candidate_pairs(lines) {
        let (ci, cj) = match since.get(&(i, j)) {
            Some((a, b)) => (a.clone(), b.clone()),
            None => (committed[i].clone(), committed[j].clone()),
        };
        let a = &ci | &uncommitted[i];
        let b = &cj | &uncommitted[j];
        for f in a.intersection(&b) {
            let e = by.entry(f.clone()).or_default();
            e.insert(i);
            e.insert(j);
        }
    }
    let mut out: Vec<(String, Vec<usize>)> =
        by.into_iter().map(|(f, v)| (f, v.into_iter().collect())).collect();
    out.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    out.into_iter()
        .map(|(path, v)| {
            let how = |i: usize| match (committed[i].contains(&path), uncommitted[i].contains(&path)) {
                (true, true) => "both",
                (false, true) => "uncommitted",
                _ => "committed",
            };
            json!({"path": path, "in": v.iter().map(|&i| json!({"line": i, "how": how(i)})).collect::<Vec<_>>()})
        })
        .collect()
}

/// Pure: for each line, the lines that hold every commit of its own - it
/// was branched from theirs, or merged into it - as `since` shows it.
pub fn held_by(n: usize, since: &Since, tips: &[Option<String>]) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new(); n];
    for (&(i, j), (a, b)) in since {
        if tips[i] == tips[j] {
            continue;
        }
        if a.is_empty() && !b.is_empty() {
            out[i].push(j);
        } else if b.is_empty() && !a.is_empty() {
            out[j].push(i);
        }
    }
    for v in &mut out {
        v.sort();
    }
    out
}

/// Pairs looked at more closely, at most; past it, against the base.
const MAX_PAIRS: usize = 40;

/// Each candidate pair's files since the two parted: two git calls a pair,
/// eight pairs at a time.
fn read_since(cwd: &str, lines: &[Value], tips: &[Option<String>]) -> Since {
    let pairs: Vec<(usize, usize)> = candidate_pairs(lines)
        .into_iter()
        .filter(|&(i, j)| tips[i].is_some() && tips[j].is_some())
        .take(MAX_PAIRS)
        .collect();
    let mut out = Since::new();
    for chunk in pairs.chunks(8) {
        let got: Vec<_> = std::thread::scope(|sc| {
            let hs: Vec<_> = chunk
                .iter()
                .map(|&(i, j)| {
                    let (ti, tj) = (tips[i].clone().unwrap(), tips[j].clone().unwrap());
                    sc.spawn(move || -> Option<((usize, usize), Sides)> {
                        if ti == tj {
                            return Some(((i, j), (HashSet::new(), HashSet::new())));
                        }
                        let side = |a: &str, b: &str| {
                            git(cwd, &with_raw(&["diff", "--name-only", &format!("{a}...{b}")]), true)
                                .ok()
                                .map(|t| names(&t).into_iter().collect::<HashSet<String>>())
                        };
                        Some(((i, j), (side(&tj, &ti)?, side(&ti, &tj)?)))
                    })
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
        });
        out.extend(got);
    }
    out
}

/// What the project's work merges into: the remote's default branch (as a
/// local branch when there is one), else main or master, else whatever the
/// main checkout has out. Not simply the last: a main checkout on a
/// feature branch would hide that branch's own work.
/// Pure, from `Refs`.
fn default_branch(refs: &Refs, checked_out: Option<&str>) -> Option<String> {
    let has = |b: &str| refs.heads.iter().any(|(h, _)| h == b);
    if let Some(remote) = &refs.origin_head {
        if let Some(local) = remote.strip_prefix("origin/").filter(|b| !b.is_empty()) {
            return Some(if has(local) { local.to_string() } else { remote.clone() });
        }
    }
    ["main", "master"]
        .into_iter()
        .find(|b| has(b))
        .map(str::to_string)
        .or_else(|| checked_out.map(str::to_string))
}

/// The local branches, the most recently committed first, each with its
/// last commit's subject, and where origin/HEAD points (`origin/main`).
#[derive(Default)]
struct Refs {
    heads: Vec<(String, String)>,
    origin_head: Option<String>,
}

fn read_refs(cwd: &str) -> Refs {
    let text = git(
        cwd,
        &["for-each-ref", "--sort=-committerdate", "--format=%(refname)\x1f%(symref)\x1f%(subject)",
          "refs/heads/", "refs/remotes/origin/HEAD"],
        false,
    )
    .unwrap_or_default();
    let mut out = Refs::default();
    for l in splitlines(&text) {
        let mut bits = l.splitn(3, '\x1f');
        let (name, symref, subject) = (bits.next().unwrap_or(""), bits.next().unwrap_or(""), bits.next().unwrap_or(""));
        if let Some(h) = name.strip_prefix("refs/heads/") {
            out.heads.push((h.to_string(), subject.to_string()));
        } else if name == "refs/remotes/origin/HEAD" {
            out.origin_head = symref.strip_prefix("refs/remotes/").map(str::to_string);
        }
    }
    out
}

/// Everything in flight in `project` (any folder of it: a worktree's finds
/// the same lines).
pub fn project_changes(project: &str) -> Value {
    if project.is_empty() || !std::path::Path::new(project).is_dir() {
        return json!({"ok": false, "error": "no such folder"});
    }
    // Refs and config are the whole repository's, so any of its folders
    // answers; the three reads go side by side.
    let (listed, refs, recorded) = std::thread::scope(|sc| {
        let a = sc.spawn(|| git(project, &["worktree", "list", "--porcelain"], true));
        let b = sc.spawn(|| read_refs(project));
        let c = sc.spawn(|| recorded_bases(project));
        (a.join().unwrap_or_else(|_| Err("git failed".into())), b.join().unwrap_or_default(), c.join().unwrap_or_default())
    });
    let listed = match listed {
        Ok(t) => parse_worktrees(&t),
        Err(e) if e.contains("not a git repository") => {
            return json!({"ok": true, "repo": false, "lines": [], "overlaps": []})
        }
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let Some(first) = listed.first() else {
        return json!({"ok": true, "repo": true, "lines": [], "overlaps": []});
    };
    let main_path = first["path"].as_str().unwrap_or("").to_string();
    let base = default_branch(&refs, first.get("branch").and_then(Value::as_str).filter(|b| !b.is_empty()));
    let base_of = |branch: Option<&str>| -> Option<String> {
        branch.and_then(|b| recorded.get(b)).cloned().or_else(|| base.clone())
    };

    let checked: HashSet<String> = listed
        .iter()
        .filter_map(|w| w.get("branch").and_then(Value::as_str).map(str::to_string))
        .collect();
    let mut candidates = Vec::new();
    let mut unlisted = 0usize;
    if base.is_some() {
        for (b, subject) in &refs.heads {
            if b.is_empty() || checked.contains(b) {
                continue;
            }
            if candidates.len() < MAX_BRANCHES {
                candidates.push((b.clone(), subject.clone()));
            } else {
                unlisted += 1;
            }
        }
    }
    let subject_of = |b: Option<&str>| b.and_then(|b| refs.heads.iter().find(|(h, _)| h == b)).map(|(_, s)| s.as_str());

    let folders: HashSet<String> = listed.iter().filter_map(|w| w["path"].as_str()).map(norm).collect();
    // Each line's git calls are its own; they run side by side.
    let (checkouts, branches) = std::thread::scope(|sc| {
        let cs: Vec<_> = listed
            .iter()
            .filter(|w| !crate::pyish::truthy(w.get("bare")))
            .enumerate()
            .map(|(i, w)| {
                let br = w.get("branch").and_then(Value::as_str);
                let b = base_of(br);
                let subject = subject_of(br);
                let mp = main_path.clone();
                let others = &folders;
                sc.spawn(move || checkout_line(w, i == 0, &mp, b.as_deref(), subject, others))
            })
            .collect();
        let bs: Vec<_> = candidates
            .iter()
            .map(|(b, subject)| {
                let bb = base_of(Some(b)).unwrap_or_default();
                let mp = main_path.clone();
                sc.spawn(move || branch_line(&mp, b, subject, &bb))
            })
            .collect();
        (
            cs.into_iter().filter_map(|h| h.join().ok()).collect::<Vec<_>>(),
            bs.into_iter().filter_map(|h| h.join().ok()).flatten().collect::<Vec<_>>(),
        )
    });
    let mut lines: Vec<Value> = checkouts.into_iter().chain(branches).map(Value::Object).collect();
    // A line's tip: its branch, or a detached worktree's commit.
    let tips: Vec<Option<String>> = lines
        .iter()
        .map(|l| l["branch"].as_str().or(l["head"].as_str()).filter(|t| !t.is_empty()).map(str::to_string))
        .collect();
    let since = read_since(&main_path, &lines, &tips);
    let overlaps = overlaps(&lines, &since);
    for (l, held) in lines.iter_mut().zip(held_by(tips.len(), &since, &tips)) {
        l["held_by"] = json!(held);
    }
    json!({"ok": true, "repo": true, "main": main_path, "base": base,
           "lines": lines, "overlaps": overlaps, "unlisted_branches": unlisted})
}

/// One file's diff in one line of work, against what that line would merge
/// into: a worktree's from where it left its base (commits and what is not
/// committed yet), the main checkout's uncommitted, a branch's commits.
/// `at` is the worktree's folder, or the branch for kind "branch".
pub fn file_diff(project: &str, kind: &str, at: &str, file: &str) -> Value {
    if file.is_empty() || std::path::Path::new(file).is_absolute() || file.split(['/', '\\']).any(|p| p == "..") {
        return json!({"ok": false, "error": "not a file of the project"});
    }
    let listed = match git(project, &["worktree", "list", "--porcelain"], true) {
        Ok(t) => parse_worktrees(&t),
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let Some(first) = listed.first() else {
        return json!({"ok": false, "error": "not a git repository"});
    };
    let main_path = first["path"].as_str().unwrap_or("").to_string();
    let main_base = default_branch(&read_refs(&main_path), first.get("branch").and_then(Value::as_str).filter(|b| !b.is_empty()));
    let recorded = recorded_bases(&main_path);
    let base_of = |b: Option<&str>| b.and_then(|b| recorded.get(b)).cloned().or_else(|| main_base.clone());
    let diff = |cwd: &str, args: &[&str]| -> Value {
        let mut all = with_raw(&["diff", "--no-color", "--no-ext-diff"]);
        all.extend_from_slice(args);
        all.extend_from_slice(&["--", file]);
        match git(cwd, &all, true) {
            Ok(d) => json!({"ok": true, "diff": take(&d, MAX_DIFF), "truncated": d.chars().count() > MAX_DIFF}),
            Err(e) => json!({"ok": false, "error": e}),
        }
    };
    let untracked = |cwd: &str| -> bool {
        git(cwd, &with_raw(&["status", "--porcelain=v1", "--untracked-files=all", "--", file]), false)
            .map(|t| t.starts_with("??"))
            .unwrap_or(false)
    };
    match kind {
        "branch" => {
            if !branch_exists(&main_path, at) {
                return json!({"ok": false, "error": format!("no branch {at}")});
            }
            let Some(b) = base_of(Some(at)) else {
                return json!({"ok": false, "error": "no base branch to compare with"});
            };
            diff(&main_path, &[&format!("{b}...{at}")])
        }
        "main" | "worktree" => {
            let want = norm(at);
            let Some(wt) = listed.iter().find(|w| same_dir(w["path"].as_str().unwrap_or(""), &want, at)) else {
                return json!({"ok": false, "error": format!("no such worktree in {project}")});
            };
            let path = wt["path"].as_str().unwrap_or("").to_string();
            if untracked(&path) {
                return json!({"ok": true, "untracked": true, "diff": ""});
            }
            let branch = wt.get("branch").and_then(Value::as_str);
            let is_main = same_dir(&main_path, &want, at);
            let b = if is_main { main_base.clone() } else { base_of(branch) };
            // The main checkout on the base: only what is not committed.
            let Some(b) = b.filter(|b| !(is_main && Some(b.as_str()) == branch)) else {
                return diff(&path, &["HEAD"]);
            };
            match git(&path, &["merge-base", &b, "HEAD"], true) {
                Ok(mb) => diff(&path, &[strip(&mb)]),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        _ => json!({"ok": false, "error": format!("unknown kind {kind}")}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn sh(cwd: &str, args: &[&str]) -> String {
        git(cwd, args, true).unwrap_or_else(|e| panic!("git {args:?} in {cwd}: {e}"))
    }

    fn write(p: &Path, text: &str) {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(p, text).unwrap();
    }

    struct Repo {
        top: PathBuf,
        dir: String,
    }

    impl Repo {
        fn new(label: &str) -> Repo {
            let top = std::env::temp_dir().join(format!(
                "imp-overview-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
            ));
            let main = top.join("r");
            std::fs::create_dir_all(&main).unwrap();
            let dir = main.to_string_lossy().into_owned();
            sh(&dir, &["init", "-q", "-b", "main"]);
            sh(&dir, &["config", "user.email", "test@example.com"]);
            sh(&dir, &["config", "user.name", "Test"]);
            sh(&dir, &["config", "core.autocrlf", "false"]);
            for f in ["a.txt", "b.txt", "c.txt"] {
                write(&main.join(f), "one\n");
            }
            sh(&dir, &["add", "."]);
            sh(&dir, &["commit", "-q", "-m", "init"]);
            Repo { top, dir }
        }
        fn p(&self, rel: &str) -> PathBuf {
            Path::new(&self.dir).join(rel)
        }
        fn commit_in(&self, cwd: &str, file: &str, text: &str, msg: &str) {
            write(&Path::new(cwd).join(file), text);
            sh(cwd, &["add", file]);
            sh(cwd, &["commit", "-q", "-m", msg]);
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.top);
        }
    }

    fn line<'a>(got: &'a Value, branch: &str) -> &'a Value {
        got["lines"].as_array().unwrap().iter().find(|l| l["branch"] == branch).unwrap_or_else(|| panic!("no line {branch}: {got}"))
    }

    #[test]
    fn overlaps_are_files_in_more_than_one_line_most_shared_first() {
        let lines = vec![
            json!({"committed": [], "uncommitted": ["a", "b"]}),
            json!({"committed": ["a", "c"], "uncommitted": ["c"]}),
            json!({"committed": ["a", "b"], "uncommitted": []}),
            json!({"committed": ["z"], "uncommitted": []}),
        ];
        let got = overlaps(&lines, &Since::new());
        assert_eq!(
            got,
            vec![
                json!({"path": "a", "in": [{"line": 0, "how": "uncommitted"}, {"line": 1, "how": "committed"}, {"line": 2, "how": "committed"}]}),
                json!({"path": "b", "in": [{"line": 0, "how": "uncommitted"}, {"line": 2, "how": "committed"}]}),
            ]
        );
        assert!(overlaps(&[json!({"committed": ["c"], "uncommitted": ["c"]})], &Since::new()).is_empty());
        // Told what each committed since they parted, shared commits drop out.
        let mut since = Since::new();
        let none = HashSet::new();
        since.insert((0, 2), (none.clone(), ["b".to_string()].into()));
        since.insert((1, 2), (["c".to_string()].into(), none.clone()));
        since.insert((0, 1), (none.clone(), ["a".to_string(), "c".to_string()].into()));
        let got = overlaps(&lines, &since);
        assert_eq!(
            got,
            vec![
                json!({"path": "a", "in": [{"line": 0, "how": "uncommitted"}, {"line": 1, "how": "committed"}]}),
                json!({"path": "b", "in": [{"line": 0, "how": "uncommitted"}, {"line": 2, "how": "committed"}]}),
            ]
        );
        let tips = vec![Some("x".to_string()), Some("y".to_string()), Some("z".to_string())];
        // 0 has nothing since parting from 1 or 2, nor 2 from 1: theirs hold it.
        assert_eq!(held_by(4, &since, &[tips, vec![None]].concat()), vec![vec![1usize, 2], vec![], vec![1], vec![]]);
    }

    #[test]
    fn every_line_of_work_with_its_files_and_the_overlaps() {
        let r = Repo::new("lines");
        // The main checkout: a.txt not committed yet.
        write(&r.p("a.txt"), "one\nmain edit\n");
        // A worktree Claude Code's shape: a.txt and b.txt committed, new.txt untracked.
        sh(&r.dir, &["worktree", "add", ".claude/worktrees/x", "-b", "wt-x", "-q"]);
        let x = r.p(".claude/worktrees/x").to_string_lossy().into_owned();
        r.commit_in(&x, "a.txt", "one\nx\n", "x a");
        r.commit_in(&x, "b.txt", "one\nx\n", "x b");
        write(&Path::new(&x).join("new.txt"), "n\n");
        // A branch with no worktree, touching b.txt; one already merged.
        sh(&r.dir, &["branch", "side"]);
        sh(&r.dir, &["branch", "stale"]);
        sh(&r.dir, &["worktree", "add", "../tmp-side", "side", "-q"]);
        let side = r.top.join("tmp-side").to_string_lossy().into_owned();
        r.commit_in(&side, "b.txt", "one\nside\n", "side b");
        sh(&r.dir, &["worktree", "remove", &side]);

        let got = project_changes(&x); // from a worktree's folder: the same project
        assert_eq!(got["ok"], true, "{got}");
        assert_eq!(got["base"], "main");
        let lines = got["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 3, "{got}"); // main, wt-x, side - not stale
        let main = line(&got, "main");
        assert_eq!(main["kind"], "main");
        assert_eq!(main["uncommitted"], json!(["a.txt"]));
        let wt = line(&got, "wt-x");
        assert_eq!((wt["kind"].clone(), wt["agent"].clone(), wt["ahead"].clone()), (json!("worktree"), json!(true), json!(2)));
        assert_eq!(wt["committed"], json!(["a.txt", "b.txt"]));
        assert_eq!(wt["uncommitted"], json!(["new.txt"]));
        let sd = line(&got, "side");
        assert_eq!((sd["kind"].clone(), sd["committed"].clone(), sd["subject"].clone()), (json!("branch"), json!(["b.txt"]), json!("side b")));

        let ov = got["overlaps"].as_array().unwrap();
        let paths: Vec<&str> = ov.iter().map(|o| o["path"].as_str().unwrap()).collect();
        assert_eq!(paths, ["a.txt", "b.txt"]);

        // One file's diff in each kind of line.
        let d = file_diff(&r.dir, "worktree", &x, "a.txt");
        assert!(d["diff"].as_str().unwrap().contains("+x"), "{d}");
        let d = file_diff(&r.dir, "main", &r.dir, "a.txt");
        assert!(d["diff"].as_str().unwrap().contains("+main edit"), "{d}");
        let d = file_diff(&r.dir, "branch", "side", "b.txt");
        assert!(d["diff"].as_str().unwrap().contains("+side"), "{d}");
        assert_eq!(file_diff(&r.dir, "worktree", &x, "new.txt")["untracked"], true);
        assert_eq!(file_diff(&r.dir, "branch", "nope", "b.txt")["ok"], false);
        assert_eq!(file_diff(&r.dir, "main", &r.dir, "../etc/passwd")["ok"], false);
        assert_eq!(file_diff(&r.dir, "worktree", &r.top.to_string_lossy(), "a.txt")["ok"], false);
    }

    #[test]
    fn the_base_is_the_default_branch_even_with_the_main_checkout_off_it() {
        let r = Repo::new("offbase");
        sh(&r.dir, &["checkout", "-q", "-b", "feature"]);
        r.commit_in(&r.dir, "c.txt", "one
feature
", "feature c");
        write(&r.p("a.txt"), "one
feature edit
");
        let got = project_changes(&r.dir);
        assert_eq!(got["base"], "main", "{got}");
        let main = line(&got, "feature");
        assert_eq!(main["kind"], "main");
        assert_eq!((main["base"].clone(), main["ahead"].clone()), (json!("main"), json!(1)));
        assert_eq!(main["committed"], json!(["c.txt"]));
        assert_eq!(main["uncommitted"], json!(["a.txt"]));
        // Its diff of a file is from where it left main: commits and edits.
        let d = file_diff(&r.dir, "main", &r.dir, "c.txt");
        assert!(d["diff"].as_str().unwrap().contains("+feature"), "{d}");
        // main itself is behind nothing and not listed as a branch of its own.
        assert_eq!(got["lines"].as_array().unwrap().len(), 1, "{got}");
    }

    #[test]
    fn the_default_branch_is_origins_then_main_or_master_then_what_is_out() {
        let refs = |heads: &[&str], origin: Option<&str>| Refs {
            heads: heads.iter().map(|h| (h.to_string(), String::new())).collect(),
            origin_head: origin.map(str::to_string),
        };
        assert_eq!(default_branch(&refs(&["dev", "main"], Some("origin/dev")), Some("x")).as_deref(), Some("dev"));
        assert_eq!(default_branch(&refs(&["main"], Some("origin/trunk")), Some("x")).as_deref(), Some("origin/trunk"));
        assert_eq!(default_branch(&refs(&["x", "master"], None), Some("x")).as_deref(), Some("master"));
        assert_eq!(default_branch(&refs(&["x"], None), Some("x")).as_deref(), Some("x"));
        assert_eq!(default_branch(&refs(&[], None), None), None);
    }

    #[test]
    fn a_branch_made_from_another_does_not_overlap_it_in_their_shared_commits() {
        let r = Repo::new("stacked");
        sh(&r.dir, &["checkout", "-q", "-b", "first"]);
        r.commit_in(&r.dir, "a.txt", "one\nfirst\n", "first a");
        sh(&r.dir, &["branch", "second"]);
        sh(&r.dir, &["worktree", "add", "../second", "second", "-q"]);
        let second = r.top.join("second").to_string_lossy().into_owned();
        r.commit_in(&second, "b.txt", "one\nsecond\n", "second b");
        let got = project_changes(&r.dir);
        assert_eq!(got["overlaps"], json!([]), "{got}");
        let first = got["lines"].as_array().unwrap().iter().position(|l| l["branch"] == "first").unwrap();
        let sec = got["lines"].as_array().unwrap().iter().position(|l| l["branch"] == "second").unwrap();
        assert_eq!(got["lines"][first]["held_by"], json!([sec]), "{got}");
        // Both changing a.txt after they parted is an overlap again.
        r.commit_in(&second, "a.txt", "one\nfirst\nsecond\n", "second a");
        let got = project_changes(&r.dir);
        let paths: Vec<&str> = got["overlaps"].as_array().unwrap().iter().map(|o| o["path"].as_str().unwrap()).collect();
        assert_eq!(paths, Vec::<&str>::new(), "first's a.txt is in second's history: {got}");
        r.commit_in(&r.dir, "a.txt", "one\nfirst again\n", "first a again");
        let got = project_changes(&r.dir);
        let paths: Vec<&str> = got["overlaps"].as_array().unwrap().iter().map(|o| o["path"].as_str().unwrap()).collect();
        assert_eq!(paths, ["a.txt"], "{got}");
    }

    #[test]
    fn a_folder_that_is_not_a_repository() {
        let d = std::env::temp_dir().join(format!("imp-overview-plain-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let got = project_changes(&d.to_string_lossy());
        assert_eq!((got["ok"].clone(), got["repo"].clone()), (json!(true), json!(false)));
        let _ = std::fs::remove_dir_all(d);
    }
}

