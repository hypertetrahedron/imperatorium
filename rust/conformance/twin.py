"""Two dispatchers, Python and the binary, fed the same events.

For the endpoints that change things (step 2), which the shadow cannot test
against the live dispatcher. Each twin gets a state folder of its own in a
temporary directory, a port of its own (8813, 8814) and no config.json, so
neither touches the live state, a real session or herdr-core. Both see the
machine's real sessions read-only, as any dispatcher would.

A scripted sequence of hook events for made-up session ids goes to both; the
answer to every POST, the state files and the GET answers that read them
must agree, timestamps masked.

    python rust/conformance/twin.py
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, binary  # noqa: E402
from sessions_compare import first_difference  # noqa: E402

PY_PORT, RS_PORT = 8813, 8814
A = "11111111-1111-4111-8111-111111111111"
B = "22222222-2222-4222-8222-222222222222"
C = "33333333-3333-4333-8333-333333333333"
# Fields that hold a clock reading, masked before comparing.
CLOCKS = {"seen_at", "last_event_at", "attention_since", "at", "start", "end", "since"}


def env():
    out = {k: v for k, v in os.environ.items() if not k.upper().startswith("CLAUDE")}
    out.update({"HC_SESSION": "imptest", "PYTHONUTF8": "1"})
    return out


def start(state_dir):
    py = subprocess.Popen(
        [sys.executable, "-m", "imperatorium.dispatcher", "--port", str(PY_PORT),
         "--state", os.path.join(state_dir, "py", "state.json")],
        cwd=REPO, env=dict(env(), LOCALAPPDATA=os.path.join(state_dir, "py-local")),
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    rs = subprocess.Popen(
        [binary(), "serve", "--port", str(RS_PORT), "--home", REPO,
         "--state", os.path.join(state_dir, "rs", "state.json")],
        cwd=REPO, env=dict(env(), LOCALAPPDATA=os.path.join(state_dir, "rs-local")),
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for port in (PY_PORT, RS_PORT):
        for _ in range(100):
            try:
                urllib.request.urlopen("http://127.0.0.1:%d/api/health" % port, timeout=2).read()
                break
            except OSError:
                time.sleep(0.2)
        else:
            raise SystemExit("twin on %d did not start" % port)
    return py, rs


def post(port, path, body, ctype="application/json", origin=None, raw=None):
    data = raw if raw is not None else json.dumps(body).encode()
    req = urllib.request.Request("http://127.0.0.1:%d%s" % (port, path), data=data, method="POST")
    if ctype:
        req.add_header("Content-Type", ctype)
    if origin:
        req.add_header("Origin", origin)
    try:
        with urllib.request.urlopen(req, timeout=30) as fh:
            return fh.status, json.loads(fh.read() or b"null")
    except urllib.error.HTTPError as exc:
        return exc.code, json.loads(exc.read() or b"null")


def get(port, path):
    with urllib.request.urlopen("http://127.0.0.1:%d%s" % (port, path), timeout=60) as fh:
        return json.loads(fh.read())


def masked(value):
    if isinstance(value, dict):
        return {k: ("<clock>" if k in CLOCKS and isinstance(v, (int, float)) else masked(v))
                for k, v in value.items()}
    if isinstance(value, list):
        return [masked(v) for v in value]
    return value


def hook(event, sid, **extra):
    return dict({"hook_event_name": event, "session_id": sid, "cwd": "C:\\work\\twin"}, **extra)


SCRIPT = [
    hook("SessionStart", A, session_name="twin-a", source="startup",
         messaging_socket="\\\\.\\pipe\\twin-a", messaging_token="tok-a"),
    hook("Notification", A, notification_type="permission_prompt", message="Claude needs permission"),
    hook("Notification", A, notification_type="idle_prompt"),
    hook("PreToolUse", A, tool_name="Bash", tool_use_id="t1", tool_input={"command": "make test"}),
    hook("PostToolUse", A, tool_name="Bash", tool_use_id="t1", tool_input={"command": "make test"}),
    hook("PreToolUse", A, tool_name="Edit", tool_use_id="t2", tool_input={"file_path": "a.py"}),
    hook("PostToolUseFailure", A, tool_name="Edit", tool_use_id="t2", error="String to replace not found\nmore"),
    hook("PostToolUse", A, tool_name="Read", tool_use_id="t9"),            # its Pre was missed
    hook("PreToolUse", A, tool_name="Task", tool_use_id="t3", agent_id="sub1"),  # a subagent's
    hook("Stop", A, last_assistant_message="Done: tests pass."),
    hook("SessionStart", B, messaging_socket="\\\\.\\pipe\\twin-b"),
    hook("StopFailure", B, error="overloaded", error_details="529 Overloaded"),
    hook("StopFailure", B, error="invalid_request", error_details="Prompt is too long"),
    hook("StopFailure", B, error="max_output_tokens"),
    hook("StopFailure", B, last_assistant_message="no error field at all"),
    hook("Notification", B, notification_type="auth_success", messaging_token="tok-b"),
    hook("SubagentStop", B),
    hook("TaskCompleted", B, last_assistant_message="x" * 400),
    hook("SessionEnd", A, reason="prompt_input_exit"),
    hook("SomethingNew", B),
    {"hook_event_name": "Stop"},
    {"hook_event_name": "imperatorium_statusline", "session_id": B, "model": {"display_name": "Opus"}},
    # C keeps its tool record and is left waiting, so both are compared.
    hook("SessionStart", C, messaging_socket="\\\\.\\pipe\\twin-c", messaging_token="tok-c"),
    hook("PreToolUse", C, tool_name="Bash", tool_use_id="c1", tool_input={"command": "pytest -q"}),
    hook("PostToolUseFailure", C, tool_name="Bash", tool_use_id="c1",
         tool_response="exit code 1\nFAILED test_x"),
    hook("PreToolUse", C, tool_name="Grep", tool_use_id="c2", tool_input={"pattern": "TODO", "path": "src"}),
    hook("Notification", C, notification_type="permission_prompt"),
    hook("StopFailure", C, error="rate_limit"),
]

# Malformed or refused POSTs: (path, body, ctype, origin, raw)
REFUSED = [
    ("/hook", [1, 2], "application/json", None, None),
    ("/hook", None, "text/plain", None, b"{}"),
    ("/hook", None, "application/json; charset=utf-8", None, b'{"hook_event_name": "Stop"}'),
    ("/hook", None, "application/json", "http://evil.example", b"{}"),
    ("/hook", None, "application/json", "http://127.0.0.1:%d", b"{}"),
    ("/hook", None, "application/json", None, b"not json"),
    ("/hook", None, "application/json", None, b"\xff\xfe"),
]


def main():
    root = tempfile.mkdtemp(prefix="imp-twin-")
    py, rs = start(root)
    failures = []
    try:
        for i, body in enumerate(SCRIPT):
            a, b = post(PY_PORT, "/hook", body), post(RS_PORT, "/hook", body)
            if a != b:
                failures.append(("POST #%d %s" % (i, body.get("hook_event_name")), "%r vs %r" % (a, b)))
        for path, body, ctype, origin, raw in REFUSED:
            outs = []
            for port in (PY_PORT, RS_PORT):
                o = origin % port if origin and "%d" in origin else origin
                outs.append(post(port, path, body, ctype, o, raw))
            if outs[0] != outs[1]:
                failures.append(("refused %s %r" % (ctype, raw or body), "%r vs %r" % tuple(outs)))
        # What the events left behind.
        states = []
        for side in ("py", "rs"):
            with open(os.path.join(root, side, "state.json"), encoding="utf-8") as fh:
                states.append(masked(json.load(fh)))
        print("state: %d sessions (%s), %d tokens, %d events; attention on C: %r"
              % (len(states[0]["sessions"]), ", ".join(sorted(states[0]["sessions"]))[:60],
                 len(states[0]["tokens"]), len(states[0]["events"]),
                 states[0]["sessions"].get(C, {}).get("attention_reason")))
        diff = first_difference(states[0], states[1], "state")
        if diff:
            failures.append(("state.json", diff))
        for path in ("/api/events", "/api/queue", "/api/prefs",
                     "/api/activity?session=" + A, "/api/activity?session=" + B,
                     "/api/activity?session=" + C):
            pa, pb = masked(get(PY_PORT, path)), masked(get(RS_PORT, path))
            if path.startswith("/api/activity"):
                print("%s: source %s, %d calls" % (path[:40], pa.get("source"), len(pa.get("timeline") or [])))
            diff = first_difference(pa, pb, "$")
            if diff:
                failures.append((path, diff))
    finally:
        for p in (py, rs):
            p.terminate()
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()
        shutil.rmtree(root, ignore_errors=True)
    print("twin: %d hook events, %d refused POSTs, state and %d GETs compared"
          % (len(SCRIPT), len(REFUSED), 6))
    for what, diff in failures:
        print("DIFF %s\n     %s" % (what, diff))
    print("twin: %s" % ("%d differences" % len(failures) if failures else "identical"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
