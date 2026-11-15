"""The hook processes, Python vs Rust, as Claude Code sees them.

Each case runs `hooks/cc_*.py` and the matching `imperatorium` subcommand
with the same stdin and environment against a fake dispatcher, and compares
what matters to Claude Code and to the dispatcher, byte for byte:

  - the exit code and stdout (the permission reply, the statusline text);
  - every HTTP request the hook made (path and body);
  - the permission audit log (decisions.jsonl), minus its timestamps.

Python runs with PYTHONUTF8=1: without it, Python reads hook stdin as cp1252
and garbles non-ASCII text, a known bug the port fixes (see hooks.rs).

    python rust/conformance/hooks_blackbox.py
    python rust/conformance/hooks_blackbox.py --record DIR

--record runs the Python hooks only and writes each case (stdin, env,
policy, the fake dispatcher's park answer) with what they did, paths made
neutral (golden.py; tests/hooks_blackbox.rs replays them).
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, binary  # noqa: E402
from golden import Places, record_dir, write_jsonl  # noqa: E402

PY = sys.executable
# The cases --record collects, in the order the tests run.
RECORDING = None
SCRIPTS = {"hook": "cc_hook.py", "statusline": "cc_statusline.py", "permission": "cc_permission.py"}


class Fake(BaseHTTPRequestHandler):
    """Records every request; answers /permission/park as the case says."""

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length)
        server = self.server
        server.seen.append((self.path, self.headers.get("Content-Type"), body))
        status, reply = 200, b"{}"
        if self.path == "/permission/park":
            status, reply = server.park
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(reply)))
        self.end_headers()
        self.wfile.write(reply)

    def log_message(self, *args):
        pass


def clean_env():
    """The environment minus anything a real session would set."""
    drop = ("CLAUDE", "HC_", "ANTHROPIC_", "IMPERATORIUM_")
    return {k: v for k, v in os.environ.items() if not k.upper().startswith(drop)}


class Blackbox(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), Fake)
        cls.server.seen = []
        cls.server.park = (200, b"{}")
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.base = "http://127.0.0.1:%d" % cls.server.server_address[1]

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()

    def run_one(self, impl, which, stdin, env, policy):
        state = tempfile.mkdtemp(prefix="imp-bb-")
        try:
            full = clean_env()
            full.update({
                "IMPERATORIUM_ENDPOINT": self.base + "/hook",
                "IMPERATORIUM_PARK_ENDPOINT": self.base + "/permission/park",
                "LOCALAPPDATA": state, "XDG_STATE_HOME": state, "PYTHONUTF8": "1",
            })
            if policy is not None:
                path = os.path.join(state, "policy.json")
                with open(path, "w", encoding="utf-8") as fh:
                    fh.write(policy if isinstance(policy, str) else json.dumps(policy))
                full["IMPERATORIUM_POLICY"] = path
            full.update(env or {})
            if impl == "python":
                cmd = [PY, os.path.join(REPO, "hooks", SCRIPTS[which])]
            else:
                cmd = [binary(), which]
            self.server.seen = []
            proc = subprocess.run(cmd, input=stdin, capture_output=True, env=full,
                                  cwd=REPO, timeout=60)
            audit = []
            log = os.path.join(state, "imperatorium", "decisions.jsonl")
            if os.path.exists(log):
                with open(log, encoding="utf-8") as fh:
                    for line in fh:
                        record = json.loads(line)
                        record.pop("at", None)
                        audit.append(record)
            return {"state": state, "code": proc.returncode, "stdout": proc.stdout,
                    "requests": [(p, b) for p, _, b in self.server.seen],
                    "content_types": sorted({c for _, c, _ in self.server.seen}),
                    "audit": audit, "stderr": proc.stderr}
        finally:
            shutil.rmtree(state, ignore_errors=True)

    def same(self, which, stdin, env=None, policy=None, park=(200, b"{}")):
        if isinstance(stdin, (dict, list)):
            stdin = json.dumps(stdin, ensure_ascii=False).encode("utf-8")
        elif isinstance(stdin, str):
            stdin = stdin.encode("utf-8")
        self.server.park = park
        py = self.run_one("python", which, stdin, env, policy)
        if RECORDING is not None:
            self.assertEqual(py["code"], 0, "python failed: %s" % py["stderr"][-400:])
            places = Places(state=py["state"])
            text = lambda b: places(b.decode("utf-8"))  # noqa: E731
            RECORDING.append({
                "test": self.id().rsplit(".", 1)[-1], "which": which, "stdin": text(stdin),
                "env": places(env or {}), "policy": places(policy),
                "park": [park[0], text(park[1])],
                "want": {"code": py["code"], "stdout": text(py["stdout"]),
                         "requests": [[p, text(b)] for p, b in py["requests"]],
                         "content_types": py["content_types"], "audit": places(py["audit"])}})
            return py
        rs = self.run_one("rust", which, stdin, env, policy)
        label = "%s %r" % (which, stdin[:120])
        self.assertEqual(py["code"], 0, "python failed: %s" % py["stderr"][-400:])
        for key in ("code", "stdout", "requests", "content_types", "audit"):
            self.assertEqual(rs[key], py[key], "%s differs for %s" % (key, label))
        return py

    # -- cc_hook --------------------------------------------------------------

    def test_session_start_carries_the_environment(self):
        got = self.same("hook", {"hook_event_name": "SessionStart", "session_id": "s1",
                                 "cwd": "C:/w/p", "source": "startup"},
                        env={"CLAUDE_CODE_MESSAGING_SOCKET": "\\\\.\\pipe\\x",
                             "CLAUDE_CODE_MESSAGING_TOKEN": "tok", "HC_TERMINAL_ID": "t9",
                             "CLAUDE_PID": "1234", "HC_SESSION": ""})
        self.assertEqual(len(got["requests"]), 1)

    def test_tool_events_are_trimmed(self):
        self.same("hook", {"hook_event_name": "PreToolUse", "session_id": "s", "tool_name": "Write",
                           "tool_use_id": "t", "tool_input": {"file_path": "a.py", "content": "x" * 50000}})
        self.same("hook", {"hook_event_name": "PostToolUse", "session_id": "s", "tool_name": "Bash",
                           "tool_input": {"command": "é" * 400, "description": ["a", 1],
                                          "timeout": 5, "query": 3.5, "url": True},
                           "tool_response": {"stdout": "y" * 9000}})

    def test_failures_keep_a_short_reason(self):
        self.same("hook", {"hook_event_name": "PostToolUseFailure", "session_id": "s",
                           "tool_name": "Bash", "tool_input": {"command": "make"},
                           "error": "exit 2", "tool_response": "y" * 5000})
        self.same("hook", {"hook_event_name": "PostToolUseFailure", "session_id": "s",
                           "tool_response": {"error": "nö", "code": [1, 2.25, None]}})
        self.same("hook", {"hook_event_name": "PostToolUseFailure", "tool_response": ""})

    def test_other_events_and_odd_input(self):
        self.same("hook", {"hook_event_name": "Notification", "message": "needs you ✓",
                           "nested": {"a": [1e16, 0.0001, -0.0, 12345678901234]}})
        self.same("hook", "")
        self.same("hook", "   \n")
        self.same("hook", "not json")
        self.same("hook", [1, 2])
        self.same("hook", "\"a string\"")
        self.same("hook", {"emoji": "😀", "ctrl": "\x01\x7f", "tab": "\t"})

    # -- cc_statusline --------------------------------------------------------

    def test_statusline(self):
        full = {"session_id": "s", "model": {"display_name": "Opus 5.5"},
                "context_window": {"used_percentage": 22.5},
                "rate_limits": {"five_hour": {"used_percentage": 41.5},
                                "seven_day": {"used_percentage": True}}}
        got = self.same("statusline", full)
        self.assertEqual(got["stdout"].decode("utf-8"), "Opus 5.5 · ctx 22% · 5h 42% · 7d 1%")
        self.same("statusline", {"model": {"display_name": "Sonnet"}})  # no session: no post
        self.same("statusline", {"session_id": "s", "model": "flat"})    # raises: prints nothing
        self.same("statusline", {"session_id": "s", "rate_limits": {}},
                  env={"ANTHROPIC_API_KEY": "k"})
        self.same("statusline", {"session_id": "s"}, env={"CLAUDE_CODE_USE_BEDROCK": " TRUE "})
        self.same("statusline", {"session_id": "s"}, env={"CLAUDE_CODE_USE_VERTEX": "0",
                                                          "ANTHROPIC_BASE_URL": "https://gw"})
        self.same("statusline", "garbage")
        self.same("statusline", {"session_id": "s", "context_window": {"used_percentage": "n/a"}})

    # -- cc_permission --------------------------------------------------------

    ON = {"enabled": True, "decision_format": "object", "defaults": {}}
    PARK = {"enabled": True, "wait_seconds": 2, "background_wait_seconds": 3}

    def bash(self, command, **extra):
        return dict({"session_id": "s", "cwd": REPO, "tool_name": "Bash",
                     "tool_input": {"command": command}}, **extra)

    def test_policy_answers_without_the_network(self):
        got = self.same("permission", self.bash("git status"), policy=self.ON)
        self.assertIn(b'"allow"', got["stdout"])
        self.assertEqual(got["requests"], [])
        self.same("permission", self.bash("git status"),
                  policy={"enabled": True, "defaults": {}})  # legacy reply
        got = self.same("permission", self.bash("type policy.json"), policy=self.ON)
        self.assertIn(b'"deny"', got["stdout"])

    def test_no_or_broken_policy_says_nothing(self):
        self.same("permission", self.bash("git status"), policy=None)
        self.same("permission", self.bash("git status"), policy="{broken")
        self.same("permission", self.bash("git status"), policy="[1, 2]")
        self.same("permission", "", policy=self.ON)
        self.same("permission", "nope", policy=self.ON)

    def test_parked_and_answered(self):
        pol = dict(self.ON, park=self.PARK)
        for park in ((200, b'{"verdict": "allow"}'), (200, b'{"verdict": "deny"}'),
                     (200, b'{"verdict": null}'), (200, b'{"verdict": "pass"}'),
                     (200, b"[1]"), (200, b"not json"), (500, b"{}")):
            got = self.same("permission", self.bash("rm -rf build"), policy=pol, park=park)
            self.assertEqual(len(got["requests"]), 1, park)

    def test_park_settings(self):
        req = self.bash("make deploy")
        for park in ({"enabled": False, "wait_seconds": 5}, {"enabled": True, "wait_seconds": 0},
                     {"enabled": True, "wait_seconds": "2"},
                     {"enabled": True, "wait_seconds": 1, "question_wait_seconds": 500},
                     {"enabled": True, "background_wait_seconds": 4}):
            self.same("permission", req, policy=dict(self.ON, park=park),
                      park=(200, b'{"verdict": "allow"}'))

    def test_questions(self):
        question = {"session_id": "s", "cwd": REPO, "tool_name": "AskUserQuestion",
                    "tool_input": {"questions": [{"question": "Which colour?", "header": "C",
                                                  "multiSelect": False,
                                                  "options": [{"label": "Red"}, {"label": "Grün"}]}],
                                   "answers": {"old": "x"}, "z": 1}}
        pol = dict(self.ON, park=self.PARK)
        got = self.same("permission", question, policy=pol,
                        park=(200, json.dumps({"verdict": "allow",
                                               "answers": {"Which colour?": "Grün"}}).encode()))
        self.assertIn(b"updatedInput", got["stdout"])
        for park in ((200, b'{"verdict": "allow"}'), (200, b'{"verdict": "allow", "answers": {}}'),
                     (200, b'{"verdict": "deny"}')):
            self.same("permission", question, policy=pol, park=park)
        # The legacy reply cannot carry answers: the terminal asks.
        self.same("permission", question, policy={"enabled": True, "park": self.PARK},
                  park=(200, b'{"verdict": "allow", "answers": {"q": "a"}}'))


if __name__ == "__main__":
    folder = record_dir()
    if folder:
        i = sys.argv.index("--record")
        del sys.argv[i:i + 2]
        RECORDING = []
        result = unittest.main(verbosity=1, exit=False).result
        write_jsonl(os.path.join(folder, "cases.jsonl"), RECORDING)
        print("recorded %d cases" % len(RECORDING))
        sys.exit(0 if result.wasSuccessful() else 1)
    unittest.main(verbosity=2)
