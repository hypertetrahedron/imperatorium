"""Background dispatch into a worktree made from the chosen session's branch,
Python and the binary, each on its own copy of a throwaway project.

Each side gets a repository on `main` with a long-lived `feat` checked out
in a sibling worktree, ahead of main, and a session whose folder is that
`feat` checkout. fakecli stands in for claude.exe: it logs `claude --bg` and
where it ran, and starts nothing.

Compared: every answer (paths made neutral), where `claude --bg` ran, the
event each records, and what is left in each repository - worktrees, their
branches, the base each noted, whether a worktree carries feat's work.

    python rust/conformance/bg_worktree_twin.py      (Windows)
    python rust/conformance/bg_worktree_twin.py --record DIR

--record (golden.py): both twins are Python; each dispatch (the py side's
body, paths neutral), the Python side's answer, what claude --bg was run
with, the events and what is left go to DIR/steps.jsonl.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import RUST, stand_in_path, stop_tree, stop_under  # noqa: E402
from notify_twin import PY_PORT, RS_PORT, start  # noqa: E402
from twin import post  # noqa: E402
import golden  # noqa: E402
from golden import rec  # noqa: E402

FAKE = os.path.join(RUST, "target", "release", "fakecli.exe")
SIDES = ("py", "rs")
SID = {"py": "6666aaaa-1111-4111-8111-111111111111", "rs": "6666bbbb-1111-4111-8111-111111111111"}


def git(cwd, *args):
    return subprocess.run(["git", "-C", cwd, *args], check=True, capture_output=True,
                          text=True).stdout


def project(root, side):
    """proj-<side> on main, proj-<side>-feat on feat (one commit ahead)."""
    main = os.path.join(root, "proj-" + side)
    os.makedirs(main)
    git(main, "init", "-q", "-b", "main")
    git(main, "config", "user.email", "twin@example.invalid")
    git(main, "config", "user.name", "twin")
    with open(os.path.join(main, "f.txt"), "w", encoding="utf-8") as fh:
        fh.write("one\n")
    git(main, "add", "f.txt")
    git(main, "commit", "-q", "-m", "start")
    feat = main + "-feat"
    git(main, "worktree", "add", "-q", "-b", "feat", feat)
    with open(os.path.join(feat, "feat.txt"), "w", encoding="utf-8") as fh:
        fh.write("feature\n")
    git(feat, "add", "feat.txt")
    git(feat, "commit", "-q", "-m", "feat work")
    return main, feat


def left(main):
    """What the dispatches left in a repository."""
    out = []
    for block in git(main, "worktree", "list", "--porcelain").strip().split("\n\n"):
        lines = dict(line.partition(" ")[::2] for line in block.splitlines())
        path = lines.get("worktree", "")
        if ".claude/worktrees" not in path.replace("\\", "/"):
            continue
        branch = lines.get("branch", "").replace("refs/heads/", "")
        base = subprocess.run(["git", "-C", main, "config", "branch.%s.imperatoriumbase" % branch],
                              capture_output=True, text=True).stdout.strip()
        out.append({"leaf": os.path.basename(path), "branch": branch, "base": base,
                    "has_feat_work": os.path.isfile(os.path.join(path, "feat.txt"))})
    return out


def main():
    if os.name != "nt":
        print("bg_worktree_twin: Windows only; skipped")
        return 0
    if not os.path.exists(FAKE):
        raise SystemExit("build it first: cargo build --release (fakecli)")
    root = tempfile.mkdtemp(prefix="imp-bgwt-twin-")
    golden.start_recording(tmp=root)
    bin_dir = os.path.join(root, "bin")
    os.makedirs(bin_dir)
    shutil.copy(FAKE, os.path.join(bin_dir, "claude.exe"))
    logs = {side: os.path.join(root, "%s-launches.jsonl" % side) for side in SIDES}
    repos = {side: project(root, side) for side in SIDES}
    folder = os.path.join(root, "claude", "sessions")
    os.makedirs(folder)
    for side in SIDES:
        # The chosen session: in the feat checkout, alive as this script.
        with open(os.path.join(folder, SID[side] + ".json"), "w", encoding="utf-8") as fh:
            json.dump({"pid": os.getpid(), "sessionId": SID[side], "cwd": repos[side][1],
                       "name": "twin-feat", "kind": "interactive", "status": "idle"}, fh)

    def env(side):
        return {"PATH": stand_in_path(bin_dir), "FAKE_LOG": logs[side]}

    def neutral(text, side):
        main, feat = repos[side]
        for real, name in ((feat, "<feat>"), (main, "<proj>")):
            for form in (real, real.replace("\\", "/"), json.dumps(real)[1:-1]):
                text = text.replace(form, name)
        return text.replace("proj-" + side, "proj-<side>")

    procs = start(root, extra_config={"background": {"model": "sonnet"}}, extra_env=env)
    failures, steps = [], []
    try:
        cases = [
            ("from the chosen session's branch", lambda s: {
                "session_id": SID[s], "path": repos[s][0], "prompt": "build the thing",
                "mode": "background"}),
            ("again: a name of its own", lambda s: {
                "session_id": SID[s], "path": repos[s][0], "prompt": "build the thing",
                "mode": "background"}),
            ("no session: the project's branch", lambda s: {
                "path": repos[s][0], "prompt": "tidy up", "mode": "background"}),
            ("an unknown session: the project's branch", lambda s: {
                "session_id": "nobody", "path": repos[s][0], "prompt": "other work",
                "mode": "background"}),
        ]
        for label, body in cases:
            got = {}
            for side, port in (("py", PY_PORT), ("rs", RS_PORT)):
                code, answer = post(port, "/api/dispatch", body(side))
                answer = dict(answer, bg_id="<id>") if isinstance(answer, dict) and "bg_id" in answer \
                    else answer
                got[side] = neutral(json.dumps([code, answer], sort_keys=True), side)
            steps.append((label, json.loads(got["py"])[1]))
            rec("dispatch", label=label, path="/api/dispatch",
                body=json.loads(neutral(json.dumps(body("py")), "py")), want=json.loads(got["py"]))
            if got["py"] != got["rs"]:
                failures.append((label, "%s\n  vs %s" % (got["py"], got["rs"])))
        launched = {}
        for side in SIDES:
            with open(logs[side], encoding="utf-8") as fh:
                calls = [json.loads(x) for x in fh if x.strip()]
            launched[side] = [neutral(json.dumps({"args": c.get("args"), "cwd": c.get("cwd")}), side)
                              for c in calls if c.get("prog") == "claude"]
        rec("launched", note="claude calls in fakecli's log: args and cwd",
            want=[json.loads(x) for x in launched["py"]])
        if launched["py"] != launched["rs"]:
            failures.append(("claude --bg", "%s\n  vs %s" % (launched["py"], launched["rs"])))
        events = {}
        for side in SIDES:
            state = os.path.join(root, side, "state.json")
            with open(state, encoding="utf-8") as fh:
                evs = [e for e in json.load(fh).get("events", []) if e.get("kind") == "dispatch_background"]
            events[side] = [neutral(json.dumps({k: v for k, v in e.items() if k not in ("at", "bg_id")},
                                               sort_keys=True), side) for e in evs]
        rec("events", note="dispatch_background events without at/bg_id",
            want=[json.loads(x) for x in events["py"]])
        if events["py"] != events["rs"]:
            failures.append(("events", "%s\n  vs %s" % (events["py"], events["rs"])))
        state = {side: left(repos[side][0]) for side in SIDES}
        rec("left", want=json.loads(neutral(json.dumps(state["py"]), "py")))
        if state["py"] != state["rs"]:
            failures.append(("left", "%s\n  vs %s" % (state["py"], state["rs"])))
    finally:
        for p in procs:
            stop_tree(p)
        for p in procs:
            p.wait()
        stop_under(bin_dir)
    for label, answer in steps:
        brief = {k: answer.get(k) for k in ("ok", "error", "action", "branch", "base", "worktree")
                 if isinstance(answer, dict) and answer.get(k) is not None}
        print("  %-42s %s" % (label, brief))
    print("launched:", *launched["py"], sep="\n  ")
    print("left:", state["py"])
    for label, diff in failures:
        print("DIFF", label)
        print("    ", diff)
    print("bg_worktree_twin: %d dispatches on each; %s" % (
        len(cases), "%d differences" % len(failures) if failures else "identical"))
    shutil.rmtree(root, ignore_errors=True)
    if golden.REC is not None:
        golden.REC.save(subst={SID["py"]: "<sid>"})
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
