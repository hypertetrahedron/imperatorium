"""The `imperatorium` binary (rust/), called from the Python that has not
been ported yet.

Phase 2 of the native port (docs/native-port.md): transcripts are read by
`imperatorium sessions ...` instead of the Agent SDK. This finds the binary
and runs it; callers fall back to the SDK where there is no binary (a
remote host's reader, until phase 4).

Where the binary is, first match:
  IMPERATORIUM_BIN                                  explicit (tests, conformance)
  %LOCALAPPDATA%\\Programs\\imperatorium\\imperatorium.exe   installed (Windows)
  ~/.local/bin/imperatorium                          installed (Linux, macOS)
  <checkout>/rust/target/release/imperatorium[.exe]  a local build
"""
import json
import os
import sys

from . import subproc

EXE = "imperatorium.exe" if sys.platform == "win32" else "imperatorium"
TIMEOUT = 60


class Unavailable(RuntimeError):
    """No binary, or it could not read what it was asked for."""


def candidates(env=None):
    env = os.environ if env is None else env
    found = []
    if env.get("IMPERATORIUM_BIN"):
        found.append(env["IMPERATORIUM_BIN"])
    if sys.platform == "win32" and env.get("LOCALAPPDATA"):
        found.append(os.path.join(env["LOCALAPPDATA"], "Programs", "imperatorium", EXE))
    found.append(os.path.join(os.path.expanduser("~"), ".local", "bin", "imperatorium"))
    here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    found.append(os.path.join(here, "rust", "target", "release", EXE))
    return found


def binary():
    """The binary's path, or None."""
    explicit = os.environ.get("IMPERATORIUM_BIN")
    if explicit:
        return explicit if os.path.isfile(explicit) else None
    for path in candidates():
        if os.path.isfile(path):
            return path
    return None


def available():
    return binary() is not None


def run_json(*args):
    """Run `imperatorium <args>` and return its JSON output."""
    exe = binary()
    if not exe:
        raise Unavailable("the imperatorium binary is not installed")
    try:
        done = subproc.run([exe] + list(args), capture_output=True, timeout=TIMEOUT)
    except (OSError, ValueError) as exc:
        raise Unavailable("could not run %s: %s" % (exe, exc)) from exc
    except Exception as exc:  # TimeoutExpired, and anything else odd
        raise Unavailable("%s %s: %s" % (exe, " ".join(args), exc)) from exc
    if done.returncode != 0:
        raise Unavailable((done.stderr or b"").decode("utf-8", "replace").strip()
                          or "%s exited %d" % (exe, done.returncode))
    try:
        return json.loads(done.stdout.decode("utf-8"))
    except ValueError as exc:
        raise Unavailable("unreadable output from %s: %s" % (exe, exc)) from exc


def session_chain(session_id):
    return run_json("sessions", "chain", session_id)


def session_info(session_id):
    return run_json("sessions", "info", session_id)


def session_titles(session_ids):
    """{id: title} in one process, for every tile at once."""
    if not session_ids:
        return {}
    return run_json("sessions", "titles", *session_ids)


def session_list():
    return run_json("sessions", "list")
