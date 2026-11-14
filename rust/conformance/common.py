"""Shared pieces of the conformance suite: where the binary is, and a
persistent `imperatorium policy-decide` process.

The suite treats the Python implementation as the reference. Each check runs
the same input through both and compares, or runs the existing Python tests
with the Rust implementation patched in.

    IMPERATORIUM_BIN   the binary to test (default: rust/target/debug)
"""
import json
import os
import socket
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RUST = os.path.dirname(HERE)
REPO = os.path.dirname(RUST)

if REPO not in sys.path:
    sys.path.insert(0, REPO)


def require_free(*ports):
    """Refuse to start a twin on a port something already answers on.

    The twins use fixed ports. A second run started beside the first would
    otherwise send its requests to the first run's dispatchers, which go on
    launching after that run has removed its stand-ins - and a launch whose
    stand-in is gone reaches the real Windows Terminal and claude.
    """
    for port in ports:
        with socket.socket() as s:
            s.settimeout(0.5)
            if s.connect_ex(("127.0.0.1", port)) == 0:
                raise SystemExit("port %d is in use (another conformance run?); not starting" % port)


def stand_in_path(bin_dir):
    """`bin_dir`, then this PATH without any folder holding a real wt or
    claude, so a missing stand-in fails instead of opening a real terminal."""
    names = ("wt.exe", "claude.exe", "wt", "claude")
    keep = [d for d in os.environ.get("PATH", "").split(os.pathsep)
            if d and not any(os.path.isfile(os.path.join(d, n)) for n in names)]
    return os.pathsep.join([bin_dir] + keep)


def stop_tree(proc):
    """Stop `proc` and everything it started.

    A twin's Python dispatcher runs under the venv's launcher, so
    terminate() would stop only the launcher and leave the dispatcher
    serving - with a PATH whose stand-ins are about to be deleted.
    """
    if proc.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(["taskkill", "/T", "/F", "/PID", str(proc.pid)],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    else:
        proc.terminate()


def stop_under(folder):
    """Stop every process whose executable lives under `folder` (a twin's
    stand-ins, which can outlive the Popen that started them). Windows only;
    matched by path, so a real claude elsewhere is never touched."""
    if os.name != "nt":
        return
    script = ("$f = [IO.Path]::GetFullPath($env:STOP_UNDER).TrimEnd('\') + '\'; "
              "Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and "
              "$_.ExecutablePath.StartsWith($f, [StringComparison]::OrdinalIgnoreCase) } | "
              "ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }")
    subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
                   env=dict(os.environ, STOP_UNDER=folder),
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=60)


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
