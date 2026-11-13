"""Resume, fork, new and close, Python and the binary, with stand-ins for
claude.exe and wt.exe.

fakecli (rust/conformance/fakecli) is copied to `claude.exe` and `wt.exe` in
a folder put first on each twin's PATH; it logs every call to that twin's
own log and starts nothing. The sessions to close are fakecli consoles too
(`--fake-console`, hidden windows): one leaves on `/exit` as Claude Code
does, one is stubborn and has to be killed. Each twin closes its own.

Compared: every answer (ids made neutral), what each launched (Windows
Terminal's arguments, `claude --bg`'s, and that no session variable leaked
into them), what was typed into each console, and the events.

    python rust/conformance/session_twin.py      (Windows)
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import RUST  # noqa: E402
from inbox_twin import record  # noqa: E402
from notify_twin import PY_PORT, RS_PORT, start  # noqa: E402
from twin import post  # noqa: E402

FAKE = os.path.join(RUST, "target", "release", "fakecli.exe")
GONE = "c0c0c0c0-0000-4000-8000-000000000000"   # never ran here
SIDES = ("py", "rs")


def console(bin_dir, log, stubborn=False):
    """A hidden console running the fake claude; its Popen."""
    si = subprocess.STARTUPINFO()
    si.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    si.wShowWindow = 0  # SW_HIDE
    args = [os.path.join(bin_dir, "claude.exe"), "--fake-console"] + (["--stubborn"] if stubborn else [])
    return subprocess.Popen(args, env=dict(os.environ, FAKE_LOG=log), startupinfo=si,
                            creationflags=subprocess.CREATE_NEW_CONSOLE)


def read_log(path):
    try:
        with open(path, encoding="utf-8") as fh:
            return [json.loads(x) for x in fh if x.strip()]
    except OSError:
        return []


def neutral(text, ids):
    for real, name in ids.items():
        text = text.replace(real, name)
    text = re.sub(r"[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}", "<new-uuid>", text)
    return text


def main():
    if os.name != "nt":
        print("session_twin: Windows only; skipped")
        return 0
    if not os.path.exists(FAKE):
        raise SystemExit("build it first: cargo build --release (fakecli)")
    root = tempfile.mkdtemp(prefix="imp-session-twin-")
    bin_dir = os.path.join(root, "bin")
    os.makedirs(bin_dir)
    for name in ("claude.exe", "wt.exe"):
        shutil.copy(FAKE, os.path.join(bin_dir, name))
    work = os.path.join(root, "work")
    os.makedirs(work)
    logs = {side: os.path.join(root, "%s-launches.jsonl" % side) for side in SIDES}
    consoles, ids, sessions = [], {GONE: "<gone>"}, {}
    for side in SIDES:
        for role, stubborn in (("leaves", False), ("stubborn", True), ("replaced", False)):
            p = console(bin_dir, logs[side], stubborn)
            consoles.append(p)
            sid = "%s%s-1111-4111-8111-111111111111" % ({"leaves": "1", "stubborn": "2", "replaced": "3"}[role] * 4,
                                                           {"py": "aaaa", "rs": "bbbb"}[side])
            sessions[(side, role)] = sid
            ids[sid] = "<%s>" % role
            # A record as Claude Code writes it, with the console's pid.
            folder = os.path.join(root, "claude", "sessions")
            os.makedirs(folder, exist_ok=True)
            with open(os.path.join(folder, sid + ".json"), "w", encoding="utf-8") as fh:
                json.dump({"pid": p.pid, "sessionId": sid, "cwd": work, "name": "twin-" + role,
                           "kind": "interactive", "status": "idle"}, fh)
    record(root, GONE, "", None, "twin-gone")  # a record whose process is gone
    with open(os.path.join(root, "claude", "sessions", GONE + ".json"), "r+", encoding="utf-8") as fh:
        rec = json.load(fh)
        rec.pop("pid", None)  # its process is gone
        fh.seek(0)
        json.dump(rec, fh)
        fh.truncate()

    def env(side):
        return {"PATH": bin_dir + os.pathsep + os.environ["PATH"], "FAKE_LOG": logs[side],
                # A session's own variables, which must not reach what it launches.
                "AI_AGENT": "claude-code", "CLAUDE_CODE_ENTRYPOINT": "cli", "CLAUDE_PID": "1"}

    procs = start(root, extra_config={"background": {"model": "sonnet", "permission_mode": "plan"}},
                  extra_env=env)
    failures, steps = [], []
    try:
        cases = [
            ("resume, no id", "/api/resume", lambda s: {"session_id": ""}),
            ("resume, nowhere to run", "/api/resume", lambda s: {"session_id": GONE + "x"}),
            ("resume in a tab", "/api/resume", lambda s: {"session_id": GONE, "cwd": work}),
            ("fork in the background", "/api/resume", lambda s: {"session_id": GONE, "cwd": work, "fork": True,
                                                                 "where": "background", "prompt": "carry on"}),
            ("resume in the background", "/api/resume", lambda s: {"session_id": GONE, "cwd": work,
                                                                   "where": "background"}),
            ("resume, where?", "/api/resume", lambda s: {"session_id": GONE, "cwd": work, "where": "moon"}),
            ("resume a running one", "/api/resume", lambda s: {"session_id": sessions[(s, "leaves")]}),
            ("fork a running one", "/api/resume", lambda s: {"session_id": sessions[(s, "leaves")], "fork": True}),
            ("resume, folder gone", "/api/resume", lambda s: {"session_id": GONE, "cwd": os.path.join(root, "nope")}),
            ("new", "/api/new", lambda s: {"cwd": work}),
            ("new beside", "/api/new", lambda s: {"session_id": sessions[(s, "leaves")]}),
            ("new and close, not running", "/api/new", lambda s: {"session_id": GONE, "close": True}),
            ("close, unknown", "/api/close", lambda s: {"session_id": "nobody"}),
            ("close, not running", "/api/close", lambda s: {"session_id": GONE}),
            ("close", "/api/close", lambda s: {"session_id": sessions[(s, "leaves")]}),
            ("close, stubborn", "/api/close", lambda s: {"session_id": sessions[(s, "stubborn")]}),
            ("new and close the old", "/api/new", lambda s: {"session_id": sessions[(s, "replaced")], "close": True}),
        ]
        for label, path, body in cases:
            got = {}
            for side, port in (("py", PY_PORT), ("rs", RS_PORT)):
                code, answer = post(port, path, body(side))
                got[side] = neutral(json.dumps([code, answer], sort_keys=True), ids)
            steps.append((label, json.loads(got["py"])[1]))
            if got["py"] != got["rs"]:
                failures.append((label, "%s\n  vs %s" % (got["py"], got["rs"])))
        time.sleep(1.5)  # a launch is fire-and-forget: let the last ones log
        for label, answer in steps:
            print("  %-28s %s" % (label, answer.get("error") or "ok: %s" % {
                k: answer.get(k) for k in ("action", "where", "how", "tab_closed", "command") if k in answer}))
        launched = {}
        for side in SIDES:
            entries = read_log(logs[side])
            for e in entries:
                if e.get("args"):
                    e["args"] = [os.path.basename(a) if a.lower().endswith("wt.exe") else a for a in e["args"]]
            launched[side] = neutral(json.dumps(entries, sort_keys=True), ids)
        print("launched/typed (python): %d entries" % len(json.loads(launched["py"])))
        for e in json.loads(launched["py"]):
            print("    %s" % e)
        if launched["py"] != launched["rs"]:
            failures.append(("launched", "%s\n  vs %s" % (launched["py"], launched["rs"])))
        leaked = [e for side in SIDES for e in read_log(logs[side]) if e.get("session_env")]
        if leaked:
            failures.append(("session variables reached a launch", repr(leaked)))
        events = []
        for side in SIDES:
            with open(os.path.join(root, side, "state.json"), encoding="utf-8") as fh:
                events.append(neutral(json.dumps([(e.get("kind"), e.get("session_id"), e.get("summary"))
                                                  for e in json.load(fh).get("events") or []]), ids))
        if events[0] != events[1]:
            failures.append(("events", "%s\n  vs %s" % tuple(events)))
        alive = [c.pid for c in consoles if c.poll() is None]
        print("consoles still running: %d of %d" % (len(alive), len(consoles)))
        if alive:
            failures.append(("consoles", "still running: %r" % alive))
    finally:
        for p in procs:
            p.terminate()
        for c in consoles:
            if c.poll() is None:
                c.kill()
        for p in procs:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()
        time.sleep(0.5)
        shutil.rmtree(root, ignore_errors=True)
    for what, diff in failures:
        print("DIFF %s\n     %s" % (what, diff))
    print("session_twin: %d actions on each; %s" % (
        len(steps), "identical" if not failures else "%d differences" % len(failures)))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
