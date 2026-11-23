//! Where a new session runs (2026-11-22): the Send button's "new session"
//! options, the New session dialog and Resume all ask here first, so two
//! running sessions never share a checkout unless the person says so.
//!
//! The rules (ROADMAP "Worktree rules for new sessions"):
//! - a fresh session runs in the project's main checkout, unless a live
//!   session is already there: then in a new worktree from what it has
//!   checked out;
//! - a session with context (a fork) runs beside a live parent, so always
//!   in a new worktree from the parent's branch; a fork of a parent that
//!   has ended takes the parent's own folder when nobody else is in it;
//! - a resumed session goes back to its own folder, or to a new worktree
//!   when that folder is taken (its uncommitted edits stay behind);
//! - a folder that is not a git repository cannot have worktrees: the
//!   session shares it, with a warning.
//!
//! Uncommitted files in the source folder are a warning for a fork (its
//! memory would not match a worktree made from the last commit, so going on
//! copies them in) and only a note for a fresh session.

use std::path::Path;

use serde_json::{json, Map, Value};

use crate::changes::git;

/// What is being started.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// No context: a fresh `claude`.
    New,
    /// With context: a fork of the parent's conversation.
    Fork,
    /// The same conversation, continued (only for a session that ended).
    Resume,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "new" => Some(Kind::New),
            "fork" => Some(Kind::Fork),
            "resume" => Some(Kind::Resume),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::New => "new",
            Kind::Fork => "fork",
            Kind::Resume => "resume",
        }
    }
}

/// What the rules need to know about the folder, gathered by `facts`.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// The folder the session would start from: the parent's (fork,
    /// resume) or the main checkout (new).
    pub source: String,
    pub repo: bool,
    /// A live session (other than the one being resumed) runs in `source`.
    pub occupied: bool,
    /// The parent is still running (fork only).
    pub parent_live: bool,
    /// Uncommitted files in `source`, as `git status` names them.
    pub dirty: Vec<String>,
}

/// The decision, as the window shows it before anything happens.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// "here" (in `folder`) or "worktree" (a new one made from `folder`).
    pub placement: &'static str,
    pub folder: String,
    /// Why, in a sentence the dialog shows.
    pub reason: String,
    /// "dirty" (a fork from uncommitted work: going on copies it),
    /// "behind" (a resume into a worktree leaves edits behind),
    /// "shared" (not git, and another session is in the folder), or "".
    pub warning: &'static str,
}

/// The rules, pure. `facts.source` is already the right folder for `kind`.
pub fn decide(kind: Kind, f: &Facts) -> Plan {
    let here = |reason: &str, warning: &'static str| Plan {
        placement: "here",
        folder: f.source.clone(),
        reason: reason.to_string(),
        warning,
    };
    let tree = |reason: &str, warning: &'static str| Plan {
        placement: "worktree",
        folder: f.source.clone(),
        reason: reason.to_string(),
        warning,
    };
    let busy = match kind {
        Kind::Fork => f.parent_live || f.occupied,
        Kind::New | Kind::Resume => f.occupied,
    };
    if !busy {
        return match kind {
            Kind::New => here("no other session is running in this folder", ""),
            Kind::Fork => here("the session it comes from has ended and nobody else is in its folder", ""),
            Kind::Resume => here("its folder is free", ""),
        };
    }
    if !f.repo {
        return here("this folder is not a git repository, so it cannot have a worktree", "shared");
    }
    let dirty = !f.dirty.is_empty();
    match kind {
        Kind::New => tree("another session is running in this folder", ""),
        Kind::Fork => tree(
            if f.parent_live {
                "the session it comes from is still running in that folder"
            } else {
                "another session is running in that folder"
            },
            if dirty { "dirty" } else { "" },
        ),
        Kind::Resume => tree("another session is running in its folder now", if dirty { "behind" } else { "" }),
    }
}

impl Plan {
    pub fn to_json(&self, f: &Facts) -> Value {
        json!({"placement": self.placement, "folder": self.folder, "reason": self.reason,
               "warning": self.warning, "repo": f.repo, "occupied": f.occupied,
               "parent_live": f.parent_live,
               "dirty_count": f.dirty.len(),
               "dirty": f.dirty.iter().take(12).collect::<Vec<_>>()})
    }
}

/// The main checkout of the repository `folder` is in (the first entry of
/// `git worktree list`), or None outside git.
pub fn main_checkout(folder: &str) -> Option<String> {
    if !crate::changes::is_repo(folder) {
        return None;
    }
    let listed = crate::changes::parse_worktrees(&git(folder, &["worktree", "list", "--porcelain"], true).ok()?);
    let first = listed.first().filter(|m| !crate::pyish::truthy(m.get("bare")))?;
    first.get("path").and_then(Value::as_str).map(|p| p.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// Whether a `git status` path is ours to count: not a worktree folder
/// (they live inside the main checkout) and not a directory entry (a nested
/// repository shows as one).
fn counts(path: &str) -> bool {
    let p = path.replace('\\', "/");
    !p.ends_with('/') && !p.starts_with(".claude/worktrees/") && p != ".claude/worktrees"
}

/// Pure: the paths `git status --porcelain=v1 -z` names (a rename's new
/// path; the old one follows it and is skipped).
pub fn parse_dirty(z: &str) -> Vec<String> {
    let mut out = vec![];
    let mut parts = z.split('\0').filter(|s| !s.is_empty());
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        if code.starts_with('R') || code.starts_with('C') {
            parts.next();
        }
        if counts(path) {
            out.push(path.to_string());
        }
    }
    out
}

/// Uncommitted files in `folder`: changed, staged, deleted and new ones;
/// ignored files do not count.
pub fn dirty(folder: &str) -> Vec<String> {
    git(folder, &["status", "--porcelain=v1", "-z", "--untracked-files=all"], true)
        .map(|t| parse_dirty(&t))
        .unwrap_or_default()
}

/// Copy `source`'s uncommitted state into the worktree at `dest` (made from
/// the same commit): tracked changes as a patch, new files byte for byte.
/// How many files it brought.
pub fn copy_changes(source: &str, dest: &str) -> Result<usize, String> {
    let patch = git(source, &["diff", "HEAD", "--binary"], true)?;
    if !patch.trim().is_empty() {
        let file = std::env::temp_dir().join(format!("imperatorium-copy-{}.patch", std::process::id()));
        std::fs::write(&file, patch.as_bytes()).map_err(|e| e.to_string())?;
        let f = file.to_string_lossy().into_owned();
        let got = git(dest, &["apply", "--binary", "--whitespace=nowarn", &f], true);
        let _ = std::fs::remove_file(&file);
        got.map_err(|e| format!("could not copy the changes: {e}"))?;
    }
    let mut n = git(source, &["diff", "HEAD", "--name-only"], true)?.lines().filter(|l| !l.is_empty()).count();
    let new = git(source, &["ls-files", "--others", "--exclude-standard", "-z"], true)?;
    for rel in new.split('\0').filter(|s| !s.is_empty() && counts(s)) {
        let from = Path::new(source).join(rel);
        if !from.is_file() {
            continue;
        }
        let to = Path::new(dest).join(rel);
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::copy(&from, &to).map_err(|e| format!("could not copy {rel}: {e}"))?;
        n += 1;
    }
    Ok(n)
}

/// Pure: where a fork of a session that is mid-turn should end - the last
/// entry of its last finished turn (an assistant message that ended the
/// turn), so the fork never sees the turn still running. None when no turn
/// has finished yet.
pub fn last_finished_turn(entries: &[Map<String, Value>]) -> Option<String> {
    entries
        .iter()
        .rev()
        .filter(|e| !crate::pyish::truthy(e.get("isSidechain")))
        .find(|e| {
            e.get("type").and_then(Value::as_str) == Some("assistant")
                && e.get("message").and_then(|m| m.get("stop_reason")).and_then(Value::as_str) == Some("end_turn")
        })
        .and_then(|e| e.get("uuid").and_then(Value::as_str).map(str::to_string))
}

/// The note put in front of the first message, so the new session knows
/// where it is and what else is going on. "" when there is nothing to say.
/// `shared`: it runs in a folder another session is working in.
pub fn note(kind: Kind, plan_placement: &str, folder: &str, made: Option<&Value>, copied: Option<usize>, shared: bool) -> String {
    let mut parts: Vec<String> = vec![];
    if kind == Kind::Fork {
        parts.push("You are a fork of an earlier conversation. That session may still be working on its own task, \
                    in its own folder: do not carry on with it. Work only on the message below."
            .into());
    }
    if let ("worktree", Some(m)) = (plan_placement, made) {
        let base = m["base"].as_str().unwrap_or("its starting commit");
        parts.push(format!(
            "You are in a new git worktree at {} on branch {}, made from {}{}. Commit your work on this branch; \
             it is merged back later.",
            m["path"].as_str().unwrap_or(""),
            m["branch"].as_str().unwrap_or(""),
            base,
            if copied.is_some() { "" } else { "'s last commit" }
        ));
        if let Some(n) = copied.filter(|n| *n > 0) {
            parts.push(format!(
                "{n} uncommitted file(s) were copied in from {folder}; some may be another session's unfinished changes."
            ));
        }
        if kind == Kind::Resume {
            parts.push(format!(
                "This conversation used to run in {folder}; any edits it left uncommitted there stay there."
            ));
        }
    }
    if shared {
        parts.push(
            "Another session is working in this same folder. Do not revert, stash or reformat files you did not \
             change, and do not switch branches."
                .into(),
        );
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("[Imperatorium] {}", parts.join(" "))
}

/// The first message with its note: the note, a blank line, the prompt.
pub fn with_note(note: &str, prompt: &str) -> String {
    match (note.is_empty(), prompt.trim().is_empty()) {
        (true, _) => prompt.to_string(),
        (false, true) => String::new(),
        (false, false) => format!("{note}\n\n{prompt}"),
    }
}

/// A worktree's name: the prompt's first words, else the kind and the
/// parent's short id.
pub fn worktree_name(kind: Kind, prompt: &str, parent: Option<&str>) -> String {
    crate::background::short_name(prompt).unwrap_or_else(|| match parent {
        Some(p) if kind != Kind::New => format!("{}-{}", kind.name(), p.chars().take(8).collect::<String>()),
        _ => "session".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(occupied: bool, parent_live: bool, repo: bool, dirty: &[&str]) -> Facts {
        Facts {
            source: "C:\\p".into(),
            repo,
            occupied,
            parent_live,
            dirty: dirty.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn the_rules() {
        // 1, 2: a fresh session.
        assert_eq!(decide(Kind::New, &facts(false, false, true, &[])).placement, "here");
        let p = decide(Kind::New, &facts(true, false, true, &["a.rs"]));
        assert_eq!((p.placement, p.warning), ("worktree", ""));
        // 3, 4, 5: with context, beside a live parent.
        let p = decide(Kind::Fork, &facts(true, true, true, &[]));
        assert_eq!((p.placement, p.warning), ("worktree", ""));
        let p = decide(Kind::Fork, &facts(true, true, true, &["a.rs"]));
        assert_eq!((p.placement, p.warning), ("worktree", "dirty"));
        // A parent in a worktree is live in that worktree: the same rule.
        assert_eq!(decide(Kind::Fork, &facts(false, true, true, &[])).placement, "worktree");
        // 6: the parent ended and its folder is free.
        let p = decide(Kind::Fork, &facts(false, false, true, &["a.rs"]));
        assert_eq!((p.placement, p.warning), ("here", ""));
        // ...but taken by someone else.
        assert_eq!(decide(Kind::Fork, &facts(true, false, true, &["a.rs"])).warning, "dirty");
        // 7, 8: resume.
        assert_eq!(decide(Kind::Resume, &facts(false, false, true, &["a.rs"])).placement, "here");
        let p = decide(Kind::Resume, &facts(true, false, true, &["a.rs"]));
        assert_eq!((p.placement, p.warning), ("worktree", "behind"));
        assert_eq!(decide(Kind::Resume, &facts(true, false, true, &[])).warning, "");
        // 9: not git.
        let p = decide(Kind::New, &facts(true, false, false, &[]));
        assert_eq!((p.placement, p.warning), ("here", "shared"));
        assert_eq!(decide(Kind::New, &facts(false, false, false, &[])).warning, "");
    }

    #[test]
    fn dirty_paths() {
        let z = " M src/a.rs\0?? new.txt\0R  b.rs\0old_b.rs\0?? .claude/worktrees/x/\0?? sub/\0D  gone.md\0";
        assert_eq!(parse_dirty(z), ["src/a.rs", "new.txt", "b.rs", "gone.md"]);
    }

    fn entry(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn the_cut_is_the_end_of_the_last_finished_turn() {
        let e = vec![
            entry(json!({"type": "user", "uuid": "u1"})),
            entry(json!({"type": "assistant", "uuid": "a1", "message": {"stop_reason": "end_turn"}})),
            entry(json!({"type": "system", "subtype": "stop_hook_summary", "uuid": "s1"})),
            entry(json!({"type": "user", "uuid": "u2"})),
            entry(json!({"type": "assistant", "uuid": "a2", "message": {"stop_reason": "tool_use"}})),
            entry(json!({"type": "assistant", "uuid": "x", "isSidechain": true, "message": {"stop_reason": "end_turn"}})),
            entry(json!({"type": "user", "uuid": "r2"})),
        ];
        assert_eq!(last_finished_turn(&e).as_deref(), Some("a1"));
        assert_eq!(last_finished_turn(&e[..1]), None);
    }

    #[test]
    fn notes() {
        let made = json!({"path": "C:\\p\\.claude\\worktrees\\x", "branch": "worktree-x", "base": "main"});
        let n = note(Kind::Fork, "worktree", "C:\\p", Some(&made), Some(2), false);
        assert!(n.starts_with("[Imperatorium] You are a fork"), "{n}");
        assert!(n.contains("branch worktree-x, made from main.") && n.contains("2 uncommitted file(s)"), "{n}");
        let n = note(Kind::New, "worktree", "C:\\p", Some(&made), None, false);
        assert!(n.contains("made from main's last commit") && !n.contains("fork"), "{n}");
        assert_eq!(note(Kind::New, "here", "C:\\p", None, None, false), "");
        assert!(note(Kind::Fork, "here", "C:\\p", None, None, true).contains("same folder"));
        assert!(note(Kind::Resume, "worktree", "C:\\p", Some(&made), None, false).contains("stay there"));
        assert_eq!(with_note("", "go"), "go");
        assert_eq!(with_note("[I] x", "go"), "[I] x\n\ngo");
        assert_eq!(with_note("[I] x", "  "), "");
    }

    fn sh(cwd: &str, args: &[&str]) {
        git(cwd, args, true).unwrap();
    }

    #[test]
    fn uncommitted_work_is_found_and_copied_into_a_worktree() {
        let top = std::env::temp_dir().join(format!("imp-spawn-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let main = top.join("r");
        std::fs::create_dir_all(&main).unwrap();
        let m = main.to_string_lossy().into_owned();
        sh(&m, &["init", "-q", "-b", "main"]);
        sh(&m, &["config", "user.email", "t@example.invalid"]);
        sh(&m, &["config", "user.name", "t"]);
        std::fs::write(main.join("f.txt"), "one\n").unwrap();
        std::fs::write(main.join("gone.txt"), "bye\n").unwrap();
        std::fs::write(main.join(".gitignore"), "build/\n").unwrap();
        sh(&m, &["add", "."]);
        sh(&m, &["commit", "-q", "-m", "init"]);
        assert!(dirty(&m).is_empty());
        // Canonical on both sides: a short TEMP (`C:\Users\RUNNER~1`) names
        // the folder git reports by its long name.
        let found = main_checkout(&m).expect("a main checkout");
        assert_eq!(std::fs::canonicalize(&found).unwrap(), std::fs::canonicalize(&main).unwrap());

        std::fs::write(main.join("f.txt"), "one\ntwo\n").unwrap();
        std::fs::remove_file(main.join("gone.txt")).unwrap();
        std::fs::create_dir_all(main.join("sub")).unwrap();
        std::fs::write(main.join("sub").join("new.txt"), "new\n").unwrap();
        std::fs::create_dir_all(main.join("build")).unwrap();
        std::fs::write(main.join("build").join("out.bin"), "ignored\n").unwrap();
        let mut d = dirty(&m);
        d.sort();
        assert_eq!(d, ["f.txt", "gone.txt", "sub/new.txt"]);

        let made = crate::changes::make_worktree(&m, "copy test").unwrap().unwrap();
        let wt = made["path"].as_str().unwrap().to_string();
        // A worktree inside the main checkout is not uncommitted work.
        let mut d = dirty(&m);
        d.sort();
        assert_eq!(d, ["f.txt", "gone.txt", "sub/new.txt"]);
        assert_eq!(copy_changes(&m, &wt).unwrap(), 3);
        let w = Path::new(&wt);
        assert_eq!(std::fs::read_to_string(w.join("f.txt")).unwrap().replace("\r\n", "\n"), "one\ntwo\n");
        assert!(!w.join("gone.txt").exists());
        assert_eq!(std::fs::read_to_string(w.join("sub").join("new.txt")).unwrap(), "new\n");
        assert!(!w.join("build").exists());
        crate::changes::drop_worktree(&made);
        let _ = std::fs::remove_dir_all(&top);
    }
}
