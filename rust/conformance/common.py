"""Shared pieces of the conformance suite: where the binary is, and a
persistent `imperatorium policy-decide` process.

The suite treats the Python implementation as the reference. Each check runs
the same input through both and compares, or runs the existing Python tests
with the Rust implementation patched in.

    IMPERATORIUM_BIN   the binary to test (default: rust/target/debug)
"""
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RUST = os.path.dirname(HERE)
REPO = os.path.dirname(RUST)

if REPO not in sys.path:
    sys.path.insert(0, REPO)


def binary():
    found = os.environ.get("IMPERATORIUM_BIN")
    if found:
        return found
    exe = "imperatorium.exe" if os.name == "nt" else "imperatorium"
    return os.path.join(RUST, "target", "debug", exe)


class RustDecider:
    """`policy.decide` answered by the binary, one JSON line per call.

    Runs in the caller's cwd, since relative paths resolve against it in both
    implementations.
    """

    def __init__(self):
        self.proc = subprocess.Popen(
            [binary(), "policy-decide"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            cwd=os.getcwd(), encoding="utf-8", bufsize=1)

    def __call__(self, request, policy=None):
        line = json.dumps({"request": request, "policy": policy}, ensure_ascii=False)
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()
        out = json.loads(self.proc.stdout.readline())
        if isinstance(out, dict):
            raise RuntimeError("binary refused the case: %s" % out.get("error"))
        return tuple(out)

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=10)
        except (OSError, subprocess.TimeoutExpired):
            self.proc.kill()
