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

from . import subproc

WINDOWS = sys.platform == "win32"
TIMEOUT = 20.0
MAX_DIFF = 400 * 1024
WORKTREE_DIR = os.path.join(".claude", "worktrees")


class GitError(RuntimeError):
    pass


def git(cwd, *args, timeout=TIMEOUT, check=True):
    """stdout of `git -C cwd args`. Raises GitError on failure when `check`."""
    try:
        done = subproc.run(["git", "-C", cwd, *args], capture_output=True,
                           timeout=timeout)
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
    recorded = recorded_bases(project)
    out = []
    for wt in listed[1:]:
        entry = dict(wt, agent=is_agent_worktree(main["path"], wt["path"]),
                     exists=os.path.isdir(wt["path"]))
        # The branch it was made from, when Imperatorium made it; otherwise
        # the main checkout's, which is what Claude Code's own come from.
        own = recorded.get(wt.get("branch")) if wt.get("branch") else None
        entry["base"] = own or base
        entry["base_recorded"] = bool(own)
        if entry["exists"]:
            try:
                entry["dirty"] = bool(git(wt["path"], "status", "--porcelain").strip())
                if entry["base"] and wt.get("branch"):
                    entry["ahead"] = int(git(project, "rev-list", "--count",
                                             "%s..%s" % (entry["base"], wt["branch"])).strip() or 0)
                    entry["behind"] = int(git(project, "rev-list", "--count",
                                              "%s..%s" % (wt["branch"], entry["base"])).strip() or 0)
                entry["subject"] = git(wt["path"], "log", "-1", "--format=%s",
                                       check=False).strip()
            except (GitError, ValueError) as exc:
                entry["error"] = str(exc)
        out.append(entry)
    return {"ok": True, "repo": True, "base": base, "main": main["path"],
            "checkouts": {w["branch"]: w["path"] for w in listed if w.get("branch")},
            "worktrees": out}


def worktree_diff(project, path, max_diff=MAX_DIFF):
    """What a worktree's branch would bring to the base: the merge-base diff."""
    found = _find(project, path)
    if isinstance(found, dict) and not found.get("ok", True):
        return found
    wt, base, _ = found
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
            return wt, wt.get("base") or listed.get("base"), listed.get("checkouts") or {}
    return {"ok": False, "error": "no such worktree in %s" % project}


ACTIONS = ("merge", "pr", "discard")

# Where Imperatorium notes the branch a worktree was made from:
# `branch.<worktree branch>.imperatoriumBase` in the repository's config.
# Deleting the branch (`git branch -d`) removes it with the rest of its section.
BASE_KEY = "imperatoriumbase"


def recorded_bases(project):
    """{worktree branch: the branch it was made from}, as noted at creation."""
    got = git(project, "config", "--get-regexp", r"^branch\..*\.%s$" % BASE_KEY, check=False)
    out = {}
    for line in got.splitlines():
        key, _, value = line.partition(" ")
        if key.startswith("branch.") and key.endswith("." + BASE_KEY) and value.strip():
            out[key[len("branch."):-len("." + BASE_KEY)]] = value.strip()
    return out


def _branch_exists(project, branch):
    return bool(git(project, "rev-parse", "--verify", "--quiet", "refs/heads/" + branch,
                    check=False).strip())


def make_worktree(source, name):
    """A worktree for a background session, made from what `source` has
    checked out rather than from the remote's default branch, which is
    where Claude Code would branch its own.

    It goes where Claude Code puts its own, `.claude/worktrees/<name>` in
    the main checkout, on branch `worktree-<name>`, so the Worktrees tab
    manages it the same way; a session started inside it does not make
    another. The source's branch is noted as the worktree's base, which
    is what Diff, Merge and PR then compare with and go into. A source on
    a detached HEAD gives that commit, with no base noted.

    Returns {path, branch, base, start, main}, or None when `source` is not in a
    git repository (the session then runs there, as before). Raises
    GitError when git refuses.
    """
    if not is_repo(source):
        return None
    listed = parse_worktrees(git(source, "worktree", "list", "--porcelain"))
    if not listed or listed[0].get("bare"):
        return None
    main = listed[0]["path"]
    base = git(source, "symbolic-ref", "--quiet", "--short", "HEAD", check=False).strip() or None
    start = base or git(source, "rev-parse", "HEAD").strip()
    stem = re.sub(r"[^A-Za-z0-9-]+", "-", name or "").strip("-").lower()[:40] or "bg"
    root = os.path.join(main, WORKTREE_DIR)
    for n in range(1, 100):
        leaf = stem if n == 1 else "%s-%d" % (stem, n)
        path, branch = os.path.join(root, leaf), "worktree-" + leaf
        if not os.path.exists(path) and not _branch_exists(main, branch):
            break
    else:
        raise GitError("no free worktree name for %s" % stem)
    git(main, "worktree", "add", "-b", branch, path, start)
    if base:
        git(main, "config", "branch.%s.%s" % (branch, BASE_KEY), base)
    return {"path": path, "branch": branch, "base": base, "start": start, "main": main}


def drop_worktree(made):
    """Undo make_worktree when the session it was for never started."""
    git(made["main"], "worktree", "remove", "--force", made["path"], check=False)
    git(made["main"], "branch", "-D", made["branch"], check=False)


def finish(project, path, action, force=False, runner=None):
    """Merge, open a PR for, or discard one worktree. Returns {ok, ...}.

    Refusals are the point of this function, not an afterthought:
    - only worktrees Claude Code made under `.claude/worktrees/` are touched,
      so a worktree you made by hand is never removed from here;
    - merge goes into the worktree's base branch (the branch it was made
      from when Imperatorium made it, else the main checkout's), in whichever
      checkout has it, and needs that checkout clean and the worktree
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
    wt, base, checkouts = found
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
            # Into its base branch, wherever that is checked out: the main
            # checkout, or another worktree (a long-lived feature branch).
            into = checkouts.get(base)
            if not into:
                return {"ok": False, "error": "%s is not checked out anywhere - check it "
                        "out to merge into it" % (base,)}
            if git(into, "status", "--porcelain", "--untracked-files=no").strip():
                return {"ok": False, "error": "the checkout of %s has uncommitted changes"
                        % (base,)}
            if not wt.get("ahead"):
                return {"ok": False, "error": "nothing to merge - %s is not ahead of %s"
                        % (branch, base)}
            try:
                out = git(into, "merge", "--no-ff", "--no-edit", branch)
            except GitError as exc:
                # A conflict and a refusal (unrelated histories, a hook) both
                # land here; either way the checkout goes back as it was.
                conflicted = bool(git(into, "diff", "--name-only",
                                      "--diff-filter=U", check=False).strip())
                git(into, "merge", "--abort", check=False)
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
        # -D: what is unmerged was checked above, against the worktree's own
        # base; -d would check against the main checkout's branch instead and
        # keep a branch made from another one.
        git(project, "branch", "-D", branch, check=False)
        git(project, "config", "--unset", "branch.%s.%s" % (branch, BASE_KEY), check=False)
        return {"ok": True, "action": "discard", "branch": branch}
    except GitError as exc:
        return {"ok": False, "error": str(exc)}


def _run(cmd, cwd=None, timeout=60.0):
    try:
        done = subproc.run(cmd, cwd=cwd, capture_output=True, text=True,
                           timeout=timeout)
    except (OSError, subprocess.TimeoutExpired) as exc:
        return 1, str(exc)
    return done.returncode, (done.stdout or "") + (done.stderr or "")


if __name__ == "__main__":
    import json

    print(json.dumps(changes(sys.argv[1] if len(sys.argv) > 1 else "."),
                     indent=1)[:4000])
