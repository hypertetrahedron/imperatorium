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
import threading
import time
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import RUST  # noqa: E402
from inbox_twin import record, window_token  # noqa: E402
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
    # The OS's wording of "no such pipe" is Python's or Windows' own.
    text = re.sub(r'"cannot open [^"]*"', '"<cannot open the inbox>"', text)
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
    roles = {}
    for side in SIDES:
        for role, stubborn in (("leaves", False), ("stubborn", True), ("replaced", False),
                               ("typing", False), ("busy", False)):
            p = console(bin_dir, logs[side], stubborn)
            consoles.append(p)
            roles[p.pid] = role
            sid = "%s%s-1111-4111-8111-111111111111" % ({"leaves": "1", "stubborn": "2", "replaced": "3",
                                                          "typing": "4", "busy": "5"}[role] * 4,
                                                           {"py": "aaaa", "rs": "bbbb"}[side])
            sessions[(side, role)] = sid
            ids[sid] = "<%s>" % role
            # A record as Claude Code writes it, with the console's pid.
            folder = os.path.join(root, "claude", "sessions")
            os.makedirs(folder, exist_ok=True)
            with open(os.path.join(folder, sid + ".json"), "w", encoding="utf-8") as fh:
                rec = {"pid": p.pid, "sessionId": sid, "cwd": work, "name": "twin-" + role,
                       "kind": "interactive", "status": "busy" if role == "busy" else "idle"}
                if role == "typing":
                    # Its inbox, which nothing serves: a token recorded
                    # against it makes the session reachable for routing.
                    rec["messagingSocketPath"] = r"\\.\pipe\imp-twin-nobody-" + side
                json.dump(rec, fh)
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

    procs = start(root, extra_config={"background": {"model": "sonnet", "permission_mode": "plan"},
                                      "chat": {"send_as_typed": True}},
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
        # Typing: Send As Typed and slash commands, into each twin's own
        # idle console; the busy one and a parked request refuse it.
        tokens = {port: window_token(port) for port in (PY_PORT, RS_PORT)}

        def typed(port, body, window=True):
            req = urllib.request.Request("http://127.0.0.1:%d/api/typed" % port,
                                         data=json.dumps(body).encode(), method="POST")
            req.add_header("Content-Type", "application/json")
            if window:
                req.add_header("Origin", "http://127.0.0.1:%d" % port)
                req.add_header("X-Imperatorium-Window", tokens[port])
            try:
                with urllib.request.urlopen(req, timeout=60) as fh:
                    return fh.status, json.loads(fh.read())
            except urllib.error.HTTPError as exc:
                return exc.code, json.loads(exc.read() or b"null")

        def parked(port, sid):
            box = {}
            t = threading.Thread(target=lambda: box.update(r=post(port, "/permission/park", {
                "session_id": sid, "tool_name": "Bash", "tool_input": {"command": "ls"}, "wait": 4})))
            t.start()
            time.sleep(0.5)
            return t

        typing_cases = [
            ("typed, not from the window", lambda s, port: typed(port, {"session_id": sessions[(s, "typing")],
                                                                        "prompt": "hi"}, window=False)),
            ("typed", lambda s, port: typed(port, {"session_id": sessions[(s, "typing")],
                                                   "prompt": "hello\nworld  !"})),
            ("typed, a command", lambda s, port: typed(port, {"session_id": sessions[(s, "typing")],
                                                              "prompt": "/model opus"})),
            ("typed, /exit", lambda s, port: typed(port, {"session_id": sessions[(s, "typing")], "prompt": "/exit"})),
            ("typed, a menu", lambda s, port: typed(port, {"session_id": sessions[(s, "typing")], "prompt": "/help"})),
            ("typed, shell mode", lambda s, port: typed(port, {"session_id": sessions[(s, "typing")],
                                                               "prompt": "!rm -rf build"})),
            ("typed, blank", lambda s, port: typed(port, {"session_id": sessions[(s, "typing")], "prompt": " \n "})),
            ("typed, busy", lambda s, port: typed(port, {"session_id": sessions[(s, "busy")], "prompt": "hi"})),
            ("typed, nobody", lambda s, port: typed(port, {"session_id": "nobody", "prompt": "hi"})),
            ("typed, parked", lambda s, port: (lambda t: (typed(port, {"session_id": sessions[(s, "typing")],
                                                                        "prompt": "hi"}), t.join())[0])(
                                                   parked(port, sessions[(s, "typing")]))),
            ("queue /exit", lambda s, port: post(port, "/api/queue", {"session_id": sessions[(s, "typing")],
                                                                       "prompt": "/exit"})),
            ("queue /compact", lambda s, port: post(port, "/api/queue", {"session_id": sessions[(s, "typing")],
                                                                          "prompt": "/compact"})),
        ]
        for label, call in typing_cases:
            got = {}
            for side, port in (("py", PY_PORT), ("rs", RS_PORT)):
                got[side] = neutral(json.dumps(list(call(side, port)), sort_keys=True), ids)
            answer = json.loads(got["py"])[1]
            if isinstance(answer, dict) and "queued" in answer:
                answer = dict(answer, queued="<item>")
            steps.append((label, answer))
            a, b = (re.sub(r'"(id|at)": [^,}]+', r'"\1": 0', got[k]) for k in ("py", "rs"))
            if a != b:
                failures.append((label, "%s\n  vs %s" % (got["py"], got["rs"])))

        # Dispatch: each twin's own typing console becomes reachable (a token
        # and an inbox that is not there), so routing can find it. Never
        # mode launch, nor auto with a failing target: either would open a
        # real Claude window through the deep link.
        for side, port in (("py", PY_PORT), ("rs", RS_PORT)):
            post(port, "/hook", {"hook_event_name": "SessionStart", "session_id": sessions[(side, "typing")],
                                 "cwd": work, "source": "startup", "messaging_token": "tok",
                                 "messaging_socket": r"\\.\pipe\imp-twin-nobody-" + side})
        dispatch_cases = [
            ("dispatch, nothing", lambda s: {}),
            ("dispatch, no project", lambda s: {"prompt": "x"}),
            ("dispatch, no such path", lambda s: {"path": os.path.join(root, "nope"), "prompt": "x"}),
            ("dispatch, said, dry run", lambda s: {"text": "project: work, check the build", "mode": "dry-run"}),
            ("dispatch, misheard", lambda s: {"project": "wrok", "prompt": "x"}),
            ("dispatch, nothing like it", lambda s: {"project": "zzqqxx", "prompt": "x"}),
            ("dispatch, a command by name", lambda s: {"project": "work", "prompt": "/model sonnet"}),
            ("dispatch, a menu by name", lambda s: {"project": "work", "prompt": "/config"}),
            ("dispatch, inject by path", lambda s: {"path": work, "prompt": "hello", "mode": "inject"}),
            ("dispatch, background", lambda s: {"path": work, "prompt": "build the thing", "mode": "background"}),
            ("dispatch, to a session", lambda s: {"session_id": sessions[(s, "typing")], "prompt": "hi"}),
            ("dispatch, a command to a session", lambda s: {"session_id": sessions[(s, "typing")],
                                                            "prompt": "/model haiku"}),
            ("dispatch, unreachable session", lambda s: {"session_id": sessions[(s, "busy")], "prompt": "hi"}),
            ("dispatch, no such session", lambda s: {"session_id": "nobody", "prompt": "hi"}),
        ]
        for label, body in dispatch_cases:
            got = {}
            for side, port in (("py", PY_PORT), ("rs", RS_PORT)):
                got[side] = neutral(json.dumps(list(post(port, "/api/dispatch", body(side))), sort_keys=True), ids)
            answer = json.loads(got["py"])[1]
            steps.append((label, {k: answer.get(k) for k in ("ok", "error", "action", "confidence", "command",
                                                             "bg_id", "needs_confirmation") if k in answer}))
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
        closed = [c for c in consoles if roles[c.pid] in ("leaves", "stubborn", "replaced")]
        alive = [c.pid for c in closed if c.poll() is None]
        print("closed consoles still running: %d of %d" % (len(alive), len(closed)))
        if alive:
            failures.append(("consoles", "still running: %r" % alive))
        lines = [e.get("line") for e in read_log(logs["py"]) if "line" in e]
        want = ["hello world !", "/model opus", "rm -rf build"]
        if [l for l in lines if l in want] != want:
            failures.append(("typed lines", "%r, expected %r among them" % (lines, want)))
        print("typed into the idle console: %r" % [l for l in lines if l != "/exit"])
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
