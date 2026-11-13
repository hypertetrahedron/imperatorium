"""Finishing a background session's worktree (merge, PR, discard), Python and
the binary, each on its own copy of a throwaway project.

The project and its worktrees are made here in a temporary folder: agent
worktrees under .claude/worktrees for each case, plus one made by hand and
one detached, which must be refused. `origin` is a bare repository beside
it, so a PR's push lands there; `gh` runs with an empty config folder and no
token, so it cannot reach GitHub and fails as it would unauthenticated.

Compared: every answer (paths made neutral), the event each records, and
what is left in each repository - worktrees, branches, the main history.

    python rust/conformance/worktree_twin.py
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from notify_twin import PY_PORT, RS_PORT, start  # noqa: E402
from twin import post  # noqa: E402

GIT_ENV = dict(os.environ, GIT_AUTHOR_NAME="twin", GIT_AUTHOR_EMAIL="twin@example.invalid",
               GIT_COMMITTER_NAME="twin", GIT_COMMITTER_EMAIL="twin@example.invalid",
               GIT_AUTHOR_DATE="2026-01-01T00:00:00Z", GIT_COMMITTER_DATE="2026-01-01T00:00:00Z")


def git(cwd, *args):
    out = subprocess.run(["git", "-C", cwd, *args], capture_output=True, text=True, env=GIT_ENV)
    if out.returncode != 0:
        raise SystemExit("git %s in %s: %s" % (args, cwd, out.stderr))
    return out.stdout


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)


def project(root, name):
    """A project with an origin and one worktree per case."""
    main = os.path.join(root, "projects", name)
    origin = os.path.join(root, "origins", name + ".git")
    os.makedirs(main)
    subprocess.run(["git", "init", "-q", "--bare", "-b", "main", origin], check=True)
    git(main, "init", "-q", "-b", "main")
    git(main, "config", "core.autocrlf", "false")
    write(os.path.join(main, "a.txt"), "one\n")
    write(os.path.join(main, ".gitignore"), ".claude/\n")
    git(main, "add", ".")
    git(main, "commit", "-q", "-m", "start")
    git(main, "remote", "add", "origin", origin)
    trees = os.path.join(main, ".claude", "worktrees")

    def tree(case, commit=None, dirty=False):
        path = os.path.join(trees, case)
        git(main, "worktree", "add", "-q", "-b", "agent-" + case, path)
        if commit:
            write(os.path.join(path, commit[0]), commit[1])
            git(path, "add", ".")
            git(path, "commit", "-q", "-m", case + ": " + commit[0])
        if dirty:
            write(os.path.join(path, "a.txt"), "edited, not committed\n")
        return path

    tree("merge", ("b.txt", "from merge\n"))
    tree("dirty", ("c.txt", "c\n"), dirty=True)
    tree("conflict", ("a.txt", "the worktree's line\n"))
    tree("pr", ("d.txt", "for a PR\n"))
    tree("clean")
    tree("unmerged", ("e.txt", "never merged\n"))
    tree("dirtydiscard", dirty=True)
    tree("mainbusy", ("f.txt", "f\n"))
    git(main, "worktree", "add", "-q", "--detach", os.path.join(trees, "detached"))
    git(main, "worktree", "add", "-q", "-b", "byhand", os.path.join(root, "byhand-" + name))
    # The conflict: main moves the same line on.
    write(os.path.join(main, "a.txt"), "main's line\n")
    git(main, "commit", "-q", "-am", "main moves on")
    return main


def state(main):
    return {"worktrees": sorted(os.path.basename(l.split(" ", 1)[1])
                                for l in git(main, "worktree", "list", "--porcelain").splitlines()
                                if l.startswith("worktree ")),
            "branches": sorted(git(main, "branch", "--format=%(refname:short)").split()),
            "log": git(main, "log", "--format=%s", "main").splitlines(),
            "status": git(main, "status", "--porcelain")}


def main():
    root = tempfile.mkdtemp(prefix="imp-worktree-twin-")
    mains = {"py": project(root, "proj-py"), "rs": project(root, "proj-rs")}
    procs = start(root, extra_config={"roots": [os.path.join(root, "projects")]},
                  extra_env={"GH_CONFIG_DIR": os.path.join(root, "gh"), "GH_TOKEN": "",
                             "GITHUB_TOKEN": "", "GH_PROMPT_DISABLED": "1"})
    failures, steps = [], []
    try:
        def tree(side, case):
            return os.path.join(mains[side], ".claude", "worktrees", case)

        cases = [
            ("bad action", lambda s: {"project": mains[s], "path": tree(s, "merge"), "action": "rebase"}),
            ("unknown project", lambda s: {"project": os.path.join(root, "elsewhere"), "path": "x", "action": "merge"}),
            ("no project", lambda s: {"path": "x", "action": "merge"}),
            ("no such worktree", lambda s: {"project": mains[s], "path": tree(s, "nope"), "action": "merge"}),
            ("made by hand", lambda s: {"project": mains[s], "path": os.path.join(root, "byhand-proj-" + s),
                                        "action": "discard", "force": True}),
            ("detached", lambda s: {"project": mains[s], "path": tree(s, "detached"), "action": "merge"}),
            ("merge dirty", lambda s: {"project": mains[s], "path": tree(s, "dirty"), "action": "merge"}),
            ("merge", lambda s: {"project": mains[s], "path": tree(s, "merge"), "action": "merge"}),
            ("merge again", lambda s: {"project": mains[s], "path": tree(s, "merge"), "action": "merge"}),
            ("merge conflict", lambda s: {"project": mains[s], "path": tree(s, "conflict"), "action": "merge"}),
            ("main busy", "dirty-main"),
            ("merge, main dirty", lambda s: {"project": mains[s], "path": tree(s, "mainbusy"), "action": "merge"}),
            ("main clean", "clean-main"),
            ("pr dirty", lambda s: {"project": mains[s], "path": tree(s, "dirty"), "action": "pr"}),
            ("pr", lambda s: {"project": mains[s], "path": tree(s, "pr"), "action": "pr"}),
            ("discard clean", lambda s: {"project": mains[s], "path": tree(s, "clean"), "action": "discard"}),
            ("discard unmerged", lambda s: {"project": mains[s], "path": tree(s, "unmerged"), "action": "discard"}),
            ("discard unmerged, forced", lambda s: {"project": mains[s], "path": tree(s, "unmerged"),
                                                    "action": "discard", "force": True}),
            ("discard dirty", lambda s: {"project": mains[s], "path": tree(s, "dirtydiscard"), "action": "discard"}),
            ("discard dirty, forced", lambda s: {"project": mains[s], "path": tree(s, "dirtydiscard"),
                                                 "action": "discard", "force": True}),
        ]
        for label, body in cases:
            if body == "dirty-main":
                for m in mains.values():
                    write(os.path.join(m, "a.txt"), "being edited in main\n")
                continue
            if body == "clean-main":
                for m in mains.values():
                    git(m, "checkout", "--", "a.txt")
                continue
            got = {}
            for side, port in (("py", PY_PORT), ("rs", RS_PORT)):
                code, answer = post(port, "/api/worktree", body(side))
                text = json.dumps([code, answer], sort_keys=True)
                for name in ("proj-py", "proj-rs"):
                    text = text.replace(name, "proj-<side>")
                got[side] = text
            steps.append((label, json.loads(got["py"])[1]))
            if got["py"] != got["rs"]:
                failures.append((label, "%s\n  vs %s" % (got["py"], got["rs"])))
        for label, answer in steps:
            print("  %-26s %s" % (label, answer.get("error") or "ok: %s %s" % (answer.get("action"), answer.get("branch"))))
        left = {side: state(m) for side, m in mains.items()}
        print("left: %r" % left["py"])
        if json.dumps(left["py"]).replace("proj-py", "x") != json.dumps(left["rs"]).replace("proj-rs", "x"):
            failures.append(("what is left", "%r\n  vs %r" % (left["py"], left["rs"])))
        if git(os.path.join(root, "origins", "proj-py.git"), "branch", "--list", "agent-pr").strip() != \
                git(os.path.join(root, "origins", "proj-rs.git"), "branch", "--list", "agent-pr").strip():
            failures.append(("pushed", "agent-pr reached one origin and not the other"))
        events = []
        for side in ("py", "rs"):
            with open(os.path.join(root, side, "state.json"), encoding="utf-8") as fh:
                events.append([(e.get("kind"), e.get("summary")) for e in json.load(fh).get("events") or []
                               if str(e.get("kind")).startswith("worktree_")])
        print("events: %r" % events[0])
        if events[0] != events[1]:
            failures.append(("events", "%r vs %r" % tuple(events)))
    finally:
        for p in procs:
            p.terminate()
        for p in procs:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()
        shutil.rmtree(root, ignore_errors=True)
    for what, diff in failures:
        print("DIFF %s\n     %s" % (what, diff))
    print("worktree_twin: %d actions on each; %s" % (
        len(steps), "identical" if not failures else "%d differences" % len(failures)))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
