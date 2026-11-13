"""How long each hook process takes, start to exit, Python vs Rust.

Everything points at a local fake server and a temporary state folder: both
endpoints are set (the park default is the live dispatcher), the policy has
parking off, and the audit log goes to a temp dir.

    python rust/conformance/bench_hooks.py [--runs N]
"""
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, binary  # noqa: E402


class Ok(BaseHTTPRequestHandler):
    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length") or 0))
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"{}")

    def log_message(self, *args):
        pass


def main():
    runs = int(sys.argv[sys.argv.index("--runs") + 1]) if "--runs" in sys.argv else 25
    server = ThreadingHTTPServer(("127.0.0.1", 0), Ok)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = "http://127.0.0.1:%d" % server.server_address[1]
    state = tempfile.mkdtemp(prefix="imp-bench-")
    try:
        policy = os.path.join(state, "policy.json")
        with open(policy, "w", encoding="utf-8") as fh:
            json.dump({"enabled": True, "decision_format": "object",
                       "park": {"enabled": False}}, fh)
        env = {k: v for k, v in os.environ.items()
               if not k.upper().startswith(("CLAUDE", "IMPERATORIUM_", "HC_"))}
        env.update({"IMPERATORIUM_ENDPOINT": base + "/hook",
                    "IMPERATORIUM_PARK_ENDPOINT": base + "/permission/park",
                    "IMPERATORIUM_POLICY": policy, "LOCALAPPDATA": state,
                    "XDG_STATE_HOME": state})
        report = json.dumps({"hook_event_name": "Stop", "session_id": "bench"}).encode()
        ask = json.dumps({"tool_name": "Bash", "tool_input": {"command": "git status"},
                          "cwd": REPO, "session_id": "bench"}).encode()
        line = json.dumps({"session_id": "bench", "model": {"display_name": "Opus"}}).encode()
        exe = binary()
        hooks = os.path.join(REPO, "hooks")
        rows = [
            ("hook", [sys.executable, os.path.join(hooks, "cc_hook.py")], [exe, "hook"], report),
            ("statusline", [sys.executable, os.path.join(hooks, "cc_statusline.py")],
             [exe, "statusline"], line),
            ("permission", [sys.executable, os.path.join(hooks, "cc_permission.py")],
             [exe, "permission"], ask),
        ]
        print("%-11s %14s %14s" % ("", "python median", "rust median"))
        for name, py, rs, data in rows:
            out = []
            for cmd in (py, rs):
                times = []
                for _ in range(runs + 3):
                    t = time.perf_counter()
                    subprocess.run(cmd, input=data, capture_output=True, env=env, timeout=30)
                    times.append((time.perf_counter() - t) * 1000)
                out.append(statistics.median(times[3:]))
            print("%-11s %11.1f ms %11.1f ms   (%.0fx)" % (name, out[0], out[1], out[0] / out[1]))
    finally:
        server.shutdown()
        shutil.rmtree(state, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
