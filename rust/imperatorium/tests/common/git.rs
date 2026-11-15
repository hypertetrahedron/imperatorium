//! Git helpers for the worktree twins: a fixed author and date, a `git`
//! that fails loudly, and the throwaway repositories the twins' scripts
//! built (rust/conformance/worktree_twin.py and bg_worktree_twin.py).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Environment for every git the tests run, so commits are reproducible.
pub const GIT_ENV: &[(&str, &str)] = &[
    ("GIT_AUTHOR_NAME", "twin"),
    ("GIT_AUTHOR_EMAIL", "twin@example.invalid"),
    ("GIT_COMMITTER_NAME", "twin"),
    ("GIT_COMMITTER_EMAIL", "twin@example.invalid"),
    ("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z"),
    ("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
];

/// `git -C cwd args...` stdout; panics with git's stderr on failure.
pub fn git(cwd: &Path, args: &[&str]) -> String {
    let out = git_raw(cwd, args);
    if !out.status.success() {
        panic!(
            "git {args:?} in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Like `git`, but the caller looks at the status.
pub fn git_raw(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(cwd).args(args);
    for (k, v) in GIT_ENV {
        cmd.env(k, v);
    }
    cmd.output().expect("run git")
}

/// Write `text` to `path` (parents made), LF line ends.
pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// worktree_twin.py `project()`: `<root>/projects/<name>` with a bare
/// origin at `<root>/origins/<name>.git` and one worktree per case under
/// .claude/worktrees, plus `byhand-<name>`; main moves on at the end.
pub fn worktree_project(root: &Path, name: &str) -> PathBuf {
    let main = root.join("projects").join(name);
    let origin = root.join("origins").join(format!("{name}.git"));
    std::fs::create_dir_all(&main).unwrap();
    std::fs::create_dir_all(origin.parent().unwrap()).unwrap();
    let ok = Command::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(&origin)
        .status()
        .unwrap();
    assert!(ok.success());
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "core.autocrlf", "false"]);
    write(&main.join("a.txt"), "one\n");
    write(&main.join(".gitignore"), ".claude/\n");
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "start"]);
    git(&main, &["remote", "add", "origin", &origin.to_string_lossy()]);
    let trees = main.join(".claude").join("worktrees");
    let tree = |case: &str, commit: Option<(&str, &str)>, dirty: bool| {
        let path = trees.join(case);
        git(
            &main,
            &["worktree", "add", "-q", "-b", &format!("agent-{case}"), &path.to_string_lossy()],
        );
        if let Some((file, text)) = commit {
            write(&path.join(file), text);
            git(&path, &["add", "."]);
            git(&path, &["commit", "-q", "-m", &format!("{case}: {file}")]);
        }
        if dirty {
            write(&path.join("a.txt"), "edited, not committed\n");
        }
    };
    tree("merge", Some(("b.txt", "from merge\n")), false);
    tree("dirty", Some(("c.txt", "c\n")), true);
    tree("conflict", Some(("a.txt", "the worktree's line\n")), false);
    tree("pr", Some(("d.txt", "for a PR\n")), false);
    tree("clean", None, false);
    tree("unmerged", Some(("e.txt", "never merged\n")), false);
    tree("dirtydiscard", None, true);
    tree("mainbusy", Some(("f.txt", "f\n")), false);
    git(
        &main,
        &["worktree", "add", "-q", "--detach", &trees.join("detached").to_string_lossy()],
    );
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "byhand",
            &root.join(format!("byhand-{name}")).to_string_lossy(),
        ],
    );
    // The conflict: main moves the same line on.
    write(&main.join("a.txt"), "main's line\n");
    git(&main, &["commit", "-q", "-am", "main moves on"]);
    main
}

/// bg_worktree_twin.py `project()`: `<root>/proj-<side>` on main and
/// `proj-<side>-feat` on `feat`, one commit ahead. Returns (main, feat).
pub fn bg_project(root: &Path, side: &str) -> (PathBuf, PathBuf) {
    let main = root.join(format!("proj-{side}"));
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "user.email", "twin@example.invalid"]);
    git(&main, &["config", "user.name", "twin"]);
    write(&main.join("f.txt"), "one\n");
    git(&main, &["add", "f.txt"]);
    git(&main, &["commit", "-q", "-m", "start"]);
    let feat = root.join(format!("proj-{side}-feat"));
    git(&main, &["worktree", "add", "-q", "-b", "feat", &feat.to_string_lossy()]);
    write(&feat.join("feat.txt"), "feature\n");
    git(&feat, &["add", "feat.txt"]);
    git(&feat, &["commit", "-q", "-m", "feat work"]);
    (main, feat)
}
