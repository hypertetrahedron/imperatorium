"""What a session has changed, and the worktrees background sessions leave.

A conversation says what Claude *did*; this says what is different on disk,
which is the thing you actually review. It is plain `git` in the session's
working directory - status, then the diff against HEAD - so it answers the
same way a terminal would and needs nothing from Claude Code.

Background sessions work in a linked worktree under `.claude/worktrees/`.
When one finishes, its branch still has to be merged, turned into a PR, or
thrown away, and until now that meant going to a terminal. `worktrees()`
lists them per project and `finish()` does one of the three.

Every git call is bounded in time and output: this runs on the request path
of a window that polls, and a repository with a vendored binary tree can
produce a diff of hundreds of megabytes.
"""
import os
import re
import subprocess
import sys

WINDOWS = sys.platform == "win32"
TIMEOUT = 20.0
MAX_DIFF = 400 * 1024
WORKTREE_DIR = os.path.join(".claude", "worktrees")


class GitError(RuntimeError):
    pass


def git(cwd, *args, timeout=TIMEOUT, check=True):
    """stdout of `git -C cwd args`. Raises GitError on failure when `check`."""
    try:
        done = subprocess.run(
            ["git", "-C", cwd, *args],
            capture_output=True,
            timeout=timeout,
            # A console window would flash on Windows once per call.
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0) if WINDOWS else 0,
        )
    except FileNotFoundError as exc:
        raise GitError("git is not on PATH") from exc
    except subprocess.TimeoutExpired as exc:
        raise GitError("git %s took longer than %gs" % (args[0], timeout)) from exc
    out = done.stdout.decode("utf-8", "replace")
    if check and done.returncode != 0:
        err = done.stderr.decode("utf-8", "replace").strip()
        raise GitError(err.splitlines()[-1] if err else "git %s failed" % args[0])
    return out


def is_repo(cwd):
    if not cwd or not os.path.isdir(cwd):
        return False
    try:
        return git(cwd, "rev-parse", "--is-inside-work-tree").strip() == "true"
    except GitError:
        return False


BRANCH_LINE = re.compile(
    r"^## (?P<branch>[^.\s]+|HEAD \(no branch\)|No commits yet on \S+)"
    r"(?:\.\.\.(?P<upstream>\S+))?(?: \[(?P<track>[^\]]+)\])?")


def parse_status(text):
    """`git status --porcelain=v1 -b -z`-free parse of the line format.

    Returns {branch, upstream, ahead, behind, files:[{path, code, staged,
    unstaged, untracked}]}. Pure, so it is tested without a repository.
    """
    out = {"branch": None, "upstream": None, "ahead": 0, "behind": 0, "files": []}
    for line in text.splitlines():
        if line.startswith("## "):
            m = BRANCH_LINE.match(line)
            if m:
                out["branch"] = m.group("branch")
                out["upstream"] = m.group("upstream")
                for part in (m.group("track") or "").split(","):
                    part = part.strip()
                    if part.startswith("ahead "):
                        out["ahead"] = int(part[6:])
                    elif part.startswith("behind "):
                        out["behind"] = int(part[7:])
            continue
        if len(line) < 4:
            continue
        code, path = line[:2], line[3:]
        # A rename is `R  old -> new`; the new name is the one on disk.
        if " -> " in path and code[0] in "RC":
            path = path.split(" -> ", 1)[1]
        if path.startswith('"') and path.endswith('"'):
            path = path[1:-1]
        out["files"].append({
            "path": path,
            "code": code,
            "staged": code[0] not in " ?",
            "unstaged": code[1] not in " ?",
            "untracked": code == "??",
        })
    return out


def parse_numstat(text):
    """{path: (added, removed)} from `git diff --numstat`. Binary is (None, None)."""
    out = {}
    for line in text.splitlines():
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        added, removed, path = parts[0], parts[1], parts[-1]
        out[path] = (None, None) if added == "-" else (int(added), int(removed))
    return out


def changes(cwd, max_diff=MAX_DIFF):
    """Everything uncommitted in `cwd`, ready for the window.

    {ok, repo, branch, upstream, ahead, behind, files:[...], diff, truncated,
     recent:[{hash, subject, when}]}. `diff` covers tracked files against
    HEAD, staged and unstaged together, because the question is "what is
    different", not "what is in the index".
    """
    if not cwd or not os.path.isdir(cwd):
        return {"ok": False, "error": "no working directory for this session"}
    if not is_repo(cwd):
        return {"ok": True, "repo": False, "files": [], "diff": ""}
    try:
        status = parse_status(git(cwd, "status", "--porcelain=v1", "-b",
                                  "--untracked-files=all"))
        has_head = bool(git(cwd, "rev-parse", "--verify", "-q", "HEAD",
                            check=False).strip())
        base = ["HEAD"] if has_head else []
        numstat = parse_numstat(git(cwd, "diff", "--numstat", *base)) if has_head else {}
        diff = git(cwd, "diff", "--no-color", "--no-ext-diff", *base) if has_head else ""
        recent = []
        if has_head:
            for line in git(cwd, "log", "-8", "--format=%h\x1f%s\x1f%ct").splitlines():
                bits = line.split("\x1f")
                if len(bits) == 3:
                    recent.append({"hash": bits[0], "subject": bits[1],
                                   "when": int(bits[2])})
    except GitError as exc:
        return {"ok": False, "error": str(exc)}
    for f in status["files"]:
        added, removed = numstat.get(f["path"], (None, None))
        f["added"], f["removed"] = added, removed
    truncated = len(diff) > max_diff
    return dict(status, ok=True, repo=True, diff=diff[:max_diff],
                truncated=truncated, recent=recent)


# -- worktrees ------------------------------------------------------------
def parse_worktrees(text):
    """`git worktree list --porcelain` -> [{path, head, branch, bare, detached}]."""
    out, cur = [], None
    for line in text.splitlines() + [""]:
        if not line:
            if cur:
                out.append(cur)
            cur = None
            continue
        key, _, value = line.partition(" ")
        if key == "worktree":
            cur = {"path": value, "head": None, "branch": None,
                   "bare": False, "detached": False, "locked": False}
        elif cur is None:
            continue
        elif key == "HEAD":
            cur["head"] = value
        elif key == "branch":
            cur["branch"] = value[len("refs/heads/"):] if value.startswith(
                "refs/heads/") else value
        elif key in ("bare", "detached", "locked"):
            cur[key] = True
    return out


def _norm(path):
    return os.path.normcase(os.path.abspath(path))


def is_agent_worktree(main, path):
    """True for a worktree Claude Code made for a background session."""
    root = _norm(os.path.join(main, WORKTREE_DIR))
    return _norm(path).startswith(root + os.sep)


def worktrees(project):
    """Linked worktrees of `project`, with how far each has moved.

    The main checkout is reported separately as `base` because it is what a
    merge lands on. Each worktree says how many commits it is ahead of the
    base branch and whether it has uncommitted changes, which is what
    decides between merge, PR and discard.
    """
    if not is_repo(project):
        return {"ok": True, "repo": False, "worktrees": []}
    try:
        listed = parse_worktrees(git(project, "worktree", "list", "--porcelain"))
    except GitError as exc:
        return {"ok": False, "error": str(exc)}
    if not listed:
        return {"ok": True, "repo": True, "worktrees": []}
    main = listed[0]
    base = main.get("branch")
    out = []
    for wt in listed[1:]:
        entry = dict(wt, agent=is_agent_worktree(main["path"], wt["path"]),
                     exists=os.path.isdir(wt["path"]))
        if entry["exists"]:
            try:
                entry["dirty"] = bool(git(wt["path"], "status", "--porcelain").strip())
                if base and wt.get("branch"):
                    entry["ahead"] = int(git(project, "rev-list", "--count",
                                             "%s..%s" % (base, wt["branch"])).strip() or 0)
                    entry["behind"] = int(git(project, "rev-list", "--count",
                                              "%s..%s" % (wt["branch"], base)).strip() or 0)
                entry["subject"] = git(wt["path"], "log", "-1", "--format=%s",
                                       check=False).strip()
            except (GitError, ValueError) as exc:
                entry["error"] = str(exc)
        out.append(entry)
    return {"ok": True, "repo": True, "base": base, "main": main["path"],
            "worktrees": out}


def worktree_diff(project, path, max_diff=MAX_DIFF):
    """What a worktree's branch would bring to the base: the merge-base diff."""
    found = _find(project, path)
    if isinstance(found, dict) and not found.get("ok", True):
        return found
    wt, base = found
    if not base or not wt.get("branch"):
        return {"ok": False, "error": "no branch to compare"}
    try:
        diff = git(project, "diff", "--no-color", "--no-ext-diff",
                   "%s...%s" % (base, wt["branch"]))
        stat = git(project, "diff", "--stat", "%s...%s" % (base, wt["branch"]))
        log = git(project, "log", "--format=%h %s", "%s..%s" % (base, wt["branch"]))
    except GitError as exc:
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "branch": wt["branch"], "base": base, "stat": stat,
            "log": log, "diff": diff[:max_diff], "truncated": len(diff) > max_diff}


def _find(project, path):
    """(worktree, base branch) for `path` under `project`, or an error dict."""
    listed = worktrees(project)
    if not listed.get("ok"):
        return listed
    want = _norm(path)
    for wt in listed.get("worktrees", []):
        if _norm(wt["path"]) == want:
            return wt, listed.get("base")
    return {"ok": False, "error": "no such worktree in %s" % project}


ACTIONS = ("merge", "pr", "discard")


def finish(project, path, action, force=False, runner=None):
    """Merge, open a PR for, or discard one worktree. Returns {ok, ...}.

    Refusals are the point of this function, not an afterthought:
    - only worktrees Claude Code made under `.claude/worktrees/` are touched,
      so a worktree you made by hand is never removed from here;
    - merge needs the main checkout clean, on its branch, and the worktree
      committed - a merge that stops half way in a checkout you are working
      in is worse than not merging;
    - discard of a worktree with uncommitted or unmerged work needs `force`,
      because that work exists nowhere else.

    `runner` replaces `gh`/`git push` in tests.
    """
    if action not in ACTIONS:
        return {"ok": False, "error": "action must be one of %s" % ", ".join(ACTIONS)}
    found = _find(project, path)
    if isinstance(found, dict):
        return found
    wt, base = found
    # `agent` was worked out against git's own path for the main checkout.
    # Comparing with `project` instead fails on a mapped drive: git reports
    # the Y: drive as //server/share, so nothing would ever match.
    if not wt.get("agent"):
        return {"ok": False, "error": "only background-session worktrees under "
                "%s are managed here" % WORKTREE_DIR}
    branch = wt.get("branch")
    if not branch:
        return {"ok": False, "error": "that worktree has no branch (detached HEAD)"}
    try:
        if action == "merge":
            if wt.get("dirty"):
                return {"ok": False, "error": "the worktree has uncommitted changes"}
            if git(project, "status", "--porcelain", "--untracked-files=no").strip():
                return {"ok": False, "error": "the main checkout has uncommitted changes"}
            if not wt.get("ahead"):
                return {"ok": False, "error": "nothing to merge - %s is not ahead of %s"
                        % (branch, base)}
            try:
                out = git(project, "merge", "--no-ff", "--no-edit", branch)
            except GitError as exc:
                # A conflict and a refusal (unrelated histories, a hook) both
                # land here; either way the checkout goes back as it was.
                conflicted = bool(git(project, "diff", "--name-only",
                                      "--diff-filter=U", check=False).strip())
                git(project, "merge", "--abort", check=False)
                return {"ok": False, "error": "merge conflicts - aborted, nothing changed"
                        if conflicted else "merge refused: %s" % exc}
            return {"ok": True, "action": "merge", "branch": branch, "base": base,
                    "output": out.strip()[-400:]}
        if action == "pr":
            if wt.get("dirty"):
                return {"ok": False, "error": "the worktree has uncommitted changes"}
            run = runner or _run
            pushed = run(["git", "-C", wt["path"], "push", "-u", "origin", branch])
            if pushed[0] != 0:
                return {"ok": False, "error": "push failed: " + pushed[1][-300:]}
            made = run(["gh", "pr", "create", "--fill", "--head", branch,
                        "--base", base], cwd=wt["path"])
            if made[0] != 0:
                return {"ok": False, "error": "gh pr create failed: " + made[1][-300:]}
            return {"ok": True, "action": "pr", "branch": branch,
                    "url": made[1].strip().splitlines()[-1] if made[1].strip() else ""}
        # discard
        unmerged = bool(wt.get("ahead"))
        if (wt.get("dirty") or unmerged) and not force:
            return {"ok": False, "needs_force": True, "error":
                    "that worktree has %s - discarding loses it" % (
                        "uncommitted changes" if wt.get("dirty")
                        else "%d unmerged commit(s)" % wt["ahead"])}
        args = ["worktree", "remove"] + (["--force"] if force else []) + [wt["path"]]
        git(project, *args)
        git(project, "branch", "-D" if force else "-d", branch, check=False)
        return {"ok": True, "action": "discard", "branch": branch}
    except GitError as exc:
        return {"ok": False, "error": str(exc)}


def _run(cmd, cwd=None, timeout=60.0):
    try:
        done = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True,
                              timeout=timeout,
                              creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)
                              if WINDOWS else 0)
    except (OSError, subprocess.TimeoutExpired) as exc:
        return 1, str(exc)
    return done.returncode, (done.stdout or "") + (done.stderr or "")


if __name__ == "__main__":
    import json

    print(json.dumps(changes(sys.argv[1] if len(sys.argv) > 1 else "."),
                     indent=1)[:4000])
