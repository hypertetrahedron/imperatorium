"""Notifications, Python and the binary, pushing to a fake ntfy server.

Each twin gets a config.json of its own (Python's from a temporary copy of
the package, since it reads config.json beside itself; the binary's through
--home) with ntfy on, pointing at a server this script runs on 127.0.0.1 -
never a real ntfy server - under a topic per twin. Speech stays off: it
would talk out loud. The sessions are made up, as in inbox_twin.py.

Compared: what each pushes (title, priority, tags, token, message) for a
permission prompt, a failed turn, a usage threshold and the dispatcher's own
idle check a minute after Stop; that a prompt answered within the delay and
a channel switched off push nothing; and /api/notify's counters.

    python rust/conformance/notify_twin.py      (about 75 s: the idle check)
"""
import http.server
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, binary, require_free  # noqa: E402
from inbox_twin import record  # noqa: E402
from twin import env, get, post  # noqa: E402

PY_PORT, RS_PORT, FAKE_PORT = 8817, 8818, 8819
ASKS = "a1a1a1a1-1111-4111-8111-111111111111"     # permission prompt, left waiting
ANSWERED = "a2a2a2a2-2222-4222-8222-222222222222"  # answered within the delay
FAILS = "a3a3a3a3-3333-4333-8333-333333333333"     # a failed turn
QUIET = "a4a4a4a4-4444-4444-8444-444444444444"     # idle a minute after Stop
LATER = "a5a5a5a5-5555-4555-8555-555555555555"     # after ntfy is switched off

PUSHES = []


class Fake(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        PUSHES.append({"topic": self.path.strip("/"), "body": body.decode("utf-8", "replace"),
                       **{h: self.headers.get(h) for h in ("Title", "Priority", "Tags", "Authorization")}})
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"{}")

    def log_message(self, *a):
        pass


def config(topic):
    return {"notify": {"ntfy": {"enabled": True, "server": "http://127.0.0.1:%d/" % FAKE_PORT,
                                "topic": topic, "delay": 1.5, "priority": "high", "token": "tk"},
                       "speak": {"enabled": False}}}


def start(root, extra_config=None, extra_env=None):
    require_free(PY_PORT, RS_PORT)
    py_home, rs_home = os.path.join(root, "py-home"), os.path.join(root, "rs-home")
    shutil.copytree(os.path.join(REPO, "imperatorium"), os.path.join(py_home, "imperatorium"),
                    ignore=shutil.ignore_patterns("__pycache__"))
    os.makedirs(rs_home)
    for home, topic in ((py_home, "twin-py"), (rs_home, "twin-rs")):
        with open(os.path.join(home, "config.json"), "w", encoding="utf-8") as fh:
            json.dump(dict(config(topic), **(extra_config or {})), fh)
    procs = []
    for side, port in (("py", PY_PORT), ("rs", RS_PORT)):
        e = dict(env(), CLAUDE_CONFIG_DIR=os.path.join(root, "claude"),
                 LOCALAPPDATA=os.path.join(root, side + "-local"),
                 IMPERATORIUM_POLICY=os.path.join(root, "no-policy.json"),
                 **((extra_env(side) if callable(extra_env) else extra_env) or {}))
        state = os.path.join(root, side, "state.json")
        if side == "py":
            cmd, cwd = [sys.executable, "-m", "imperatorium.dispatcher", "--port", str(port),
                        "--state", state], py_home
        else:
            cmd, cwd = [binary(), "serve", "--port", str(port), "--home", rs_home, "--state", state], REPO
        procs.append(subprocess.Popen(cmd, cwd=cwd, env=e,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    for port in (PY_PORT, RS_PORT):
        for _ in range(150):
            try:
                urllib.request.urlopen("http://127.0.0.1:%d/api/health" % port, timeout=2).read()
                break
            except OSError:
                time.sleep(0.2)
        else:
            raise SystemExit("twin on %d did not start" % port)
    return procs


def hook(event, sid, root, **extra):
    return dict({"hook_event_name": event, "session_id": sid,
                 "cwd": os.path.join(root, "work")}, **extra)


def both(path, body):
    got = [post(port, path, body) for port in (PY_PORT, RS_PORT)]
    return got


def pushes(topic):
    return [{k: v for k, v in p.items() if k != "topic"} for p in PUSHES if p["topic"] == topic]


def main():
    root = tempfile.mkdtemp(prefix="imp-notify-twin-")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", FAKE_PORT), Fake)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    for sid, name in ((ASKS, "twin-asks"), (ANSWERED, "twin-answered"), (FAILS, "twin-fails"),
                      (QUIET, "twin-quiet"), (LATER, "twin-later")):
        record(root, sid, "idle", "\\\\.\\pipe\\imp-notify-twin-nobody", name)
    procs = start(root)
    failures = []
    try:
        answers = []
        # The idle check takes a minute: start its clock first.
        answers.append(both("/hook", hook("Stop", QUIET, root, last_assistant_message="all done")))
        stop_at = time.time()
        answers.append(both("/hook", hook("Notification", ASKS, root, notification_type="permission_prompt",
                                          message="Claude needs your permission to use Bash")))
        answers.append(both("/hook", hook("Notification", ANSWERED, root, notification_type="permission_prompt")))
        time.sleep(0.3)
        answers.append(both("/hook", hook("Stop", ANSWERED, root)))  # answered: Stop clears it
        answers.append(both("/hook", hook("StopFailure", FAILS, root, error="rate_limit")))
        answers.append(both("/hook", {"hook_event_name": "imperatorium_statusline", "session_id": ASKS,
                                      "rate_limits": {"five_hour": {"used_percentage": 91,
                                                                    "resets_at": 4102444800}}}))
        time.sleep(3)
        # Switched off from the window: nothing more is pushed.
        answers.append(both("/api/notify", {"channel": "ntfy", "enabled": False}))
        answers.append(both("/hook", hook("Notification", LATER, root, notification_type="permission_prompt")))
        time.sleep(3)
        answers.append(both("/api/notify", {"channel": "ntfy", "enabled": True}))
        # The idle check is due 60 s after its Stop; its push follows once
        # the still-waiting check has looked again (claude agents: seconds).
        while time.time() < stop_at + 100 and min(len(pushes("twin-py")), len(pushes("twin-rs"))) < 5:
            time.sleep(0.5)
        print("idle push after %.1f s" % (time.time() - stop_at))
        time.sleep(2)
        for a, b in answers:
            if json.dumps(a, sort_keys=True) != json.dumps(b, sort_keys=True) and "enabled" not in json.dumps(a):
                failures.append(("answer", "%r vs %r" % (a, b)))
        statuses = [get(port, "/api/notify") for port in (PY_PORT, RS_PORT)]
        if statuses[0] != statuses[1]:
            failures.append(("/api/notify", "%r vs %r" % tuple(statuses)))
        py, rs = pushes("twin-py"), pushes("twin-rs")
        print("pushes: %d from Python, %d from the binary" % (len(py), len(rs)))
        for p in py:
            print("   %s | %s | %s" % (p["Title"], p["Tags"], p["body"]))
        if sorted(map(json.dumps, py)) != sorted(map(json.dumps, rs)):
            failures.append(("pushes", "%r\n  vs %r" % (py, rs)))
        bodies = sorted(p["body"] for p in py)
        want = ["5-hour limit at 91%, resets " + time.strftime("%H:%M", time.localtime(4102444800)),
                "work: needs permission", "work: rate limited - wait and retry",
                # Both sessions that stopped and then sat idle for a minute:
                # the one left alone and the one whose prompt was answered.
                "work: waiting for you", "work: waiting for you"]
        if bodies != sorted(want):
            failures.append(("what Python pushed", "%r, expected %r" % (bodies, sorted(want))))
        states = []
        for side in ("py", "rs"):
            with open(os.path.join(root, side, "state.json"), encoding="utf-8") as fh:
                st = json.load(fh)
            states.append(sorted((e.get("kind"), e.get("session_id"), e.get("summary"))
                                 for e in st.get("events") or []))
        if states[0] != states[1]:
            failures.append(("events", "%r\n  vs %r" % tuple(states)))
        print("events: %s" % sorted({k for k, _, _ in states[0]}))
        print("notify: %r" % {k: statuses[0].get(k) for k in ("sent", "last_error")})
    finally:
        for p in procs:
            p.terminate()
        for p in procs:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()
        server.shutdown()
        shutil.rmtree(root, ignore_errors=True)
    for what, diff in failures:
        print("DIFF %s\n     %s" % (what, diff))
    print("notify_twin: %s" % ("identical" if not failures else "%d differences" % len(failures)))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
