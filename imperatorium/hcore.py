"""herdr-core (`hc`): terminals that outlive their window.

herdr-core hosts each Claude Code session in a terminal of its own, which
keeps running when the herdr window (or any viewer) closes, and comes back
as a shell in the same folder after a reboot (`--keep`). herdr-app shows
those terminals; this module is how the dispatcher starts them, finds them,
and types into them. See docs/hc-spike.md for what was verified.

Two rules from the spike are enforced here rather than left to callers:

- **A clean environment.** `hc` hands its own environment to every terminal
  it starts, and any `hc` call auto-starts the core when it is down. A core
  started from inside a Claude Code session gave every new terminal that
  session's inbox socket and token. So every call here runs with
  `clean_env()`, and `Core.ensure()` refuses a core whose environment still
  carries `CLAUDE*` variables.
- **Typed input is the person.** `hc send` and `hc keys` arrive exactly as
  if typed, so they can approve a permission prompt or spend money. Only
  the dispatcher's window-origin paths call `send`/`keys` (see
  Control.send_typed); this module does not decide who may.

The CLI is used rather than the JSON-RPC pipe: it is the same API, it is
what the spike exercised, and an argv list needs no client of our own.
"""
import json
import os
import shutil
import subprocess
import sys

WINDOWS = sys.platform == "win32"

# Labels herdr-app reads: a terminal carrying both shows up in that space
# and tab (defaults/AGENTS.md, "A terminal with those labels shows up there").
SPACE_LABEL = "herdr.space"
TAB_LABEL = "herdr.tab"
# Ours: which Claude Code session a terminal was started for.
SESSION_LABEL = "imperatorium.session"

# Variables a terminal must not inherit from whoever started the core or a
# terminal: Claude Code's own (a session's inbox credentials among them), and
# hc's per-terminal ones, which would make a new terminal the child of the
# terminal the dispatcher happens to run in.
HC_TERMINAL_VARS = ("HC_TERMINAL_ID", "HC_TERMINAL_SOCKET", "HC_SCOPE")
OTHER_AGENT_VARS = ("AI_AGENT",)


class HcError(RuntimeError):
    pass


def tainted_names(env):
    """The Claude Code variables in `env` that would leak into terminals."""
    return sorted(k for k in env if k.upper().startswith("CLAUDE"))


def clean_env(env=None):
    env = dict(os.environ if env is None else env)
    for name in tainted_names(env):
        env.pop(name, None)
    for name in HC_TERMINAL_VARS + OTHER_AGENT_VARS:
        env.pop(name, None)
    return env


def find_exe():
    found = shutil.which("hc")
    if found:
        return found
    if WINDOWS:
        local = os.environ.get("LOCALAPPDATA") or ""
        guess = os.path.join(local, "Programs", "herdr-core", "hc.exe")
        if os.path.isfile(guess):
            return guess
    return None


def _no_window():
    return getattr(subprocess, "CREATE_NO_WINDOW", 0) if WINDOWS else 0


class Core:
    """One herdr-core: the default one, or `session` (HC_SESSION) for tests."""

    def __init__(self, exe=None, session=None, runner=None, env=None):
        self.exe = exe or find_exe()
        self.session = session
        self._run = runner or subprocess.run
        self._env = env

    # -- plumbing ------------------------------------------------------------
    def env(self):
        env = clean_env(self._env)
        if self.session:
            env["HC_SESSION"] = self.session
        return env

    def available(self):
        return bool(self.exe)

    def call(self, sub, *args, timeout=20.0, json_out=True):
        """`hc <sub> [--json] <args>`; the parsed JSON, or the text.

        `--json` goes straight after the subcommand so that a trailing argv
        (`run -- claude ...`) never swallows it.
        """
        if not self.exe:
            raise HcError("herdr-core (hc) is not installed")
        argv = [self.exe] + list(sub if isinstance(sub, (list, tuple)) else [sub])
        if json_out:
            argv.append("--json")
        argv += [str(a) for a in args]
        try:
            got = self._run(argv, capture_output=True, timeout=timeout, env=self.env(),
                            stdin=subprocess.DEVNULL, creationflags=_no_window())
        except (OSError, subprocess.SubprocessError) as exc:
            raise HcError("hc %s: %s" % (" ".join(argv[1:3]), exc)) from exc
        out = _text(got.stdout)
        if got.returncode != 0:
            raise HcError(_error_text(out, _text(got.stderr)) or
                          "hc %s exited %s" % (argv[1], got.returncode))
        if not json_out:
            return out
        try:
            return json.loads(out) if out.strip() else {}
        except ValueError:
            # Some commands print several JSON lines; the last is the answer.
            for line in reversed(out.strip().splitlines()):
                try:
                    return json.loads(line)
                except ValueError:
                    continue
            raise HcError("hc %s: not JSON: %s" % (argv[1], out[:200]))

    # -- the core ------------------------------------------------------------
    def status(self):
        """The core's status, or None when hc can't give one.

        Like every hc command, `server status` starts the core when it is
        down (seen with hc 0.1.18), which is safe here only because every
        call runs with `clean_env()`.
        """
        try:
            got = self.call(["server", "status"], timeout=10.0)
        except HcError:
            return None
        return got if isinstance(got, dict) and got.get("pid") else None

    def leaked(self):
        """Claude Code variables in the core's own environment.

        `hc exec` runs a program with the core's environment and no
        terminal, so it sees exactly what a new terminal would inherit.
        """
        probe = [sys.executable, "-I", "-c",
                 "import os,json;print(json.dumps(sorted(k for k in os.environ "
                 "if k.upper().startswith('CLAUDE'))))"]
        got = self.call("exec", "--timeout", "30", "--", *probe, timeout=40.0)
        # `--json` gives stdout base64-encoded (it is bytes, exactly).
        text = _b64(got.get("stdout")) if isinstance(got, dict) else got
        try:
            names = json.loads((text or "[]").strip().splitlines()[-1])
        except (ValueError, IndexError):
            raise HcError("could not read the core's environment: %r" % (got,))
        return names

    def ensure(self):
        """A running core with a clean environment, started here if needed.

        Raises HcError when the running core leaks Claude Code variables: it
        was started from inside a session, and every terminal it starts
        would get that session's credentials. Restarting it is the person's
        call, since its terminals are their running work.
        """
        st = self.status()
        if st is None:
            self.call(["server", "start"], timeout=30.0)
            st = self.status()
            if st is None:
                raise HcError("herdr-core did not start")
        names = self.leaked()
        if names:
            raise HcError("herdr-core was started with Claude Code's variables (%s); "
                          "every terminal it starts would inherit them. Stop it with "
                          "`hc server stop` from a plain terminal and start it again."
                          % ", ".join(names))
        return st

    # -- terminals -----------------------------------------------------------
    def terminals(self, all_=False):
        got = self.call("ls", *(["--all"] if all_ else []))
        return list((got or {}).get("terminals") or [])

    def get(self, address):
        return self.call("get", address)

    def run(self, argv, cwd, labels=None, keep=True, name=None):
        """Start `argv` in a new terminal and return its facts (`address`).

        Not a child of any terminal (the dispatcher may itself run in one),
        and it outlives whoever started it.
        """
        args = ["--bg", "--no-parent", "--cwd", cwd]
        if keep:
            args.append("--keep")
        if name:
            args += ["--name", name]
        for k, v in sorted((labels or {}).items()):
            args += ["--label", "%s=%s" % (k, v)]
        got = self.call("run", *args, "--", *argv, timeout=30.0)
        if not isinstance(got, dict) or not got.get("address"):
            raise HcError("hc run gave no terminal: %r" % (got,))
        return got

    def label(self, address, **labels):
        args = [address] + ["%s=%s" % kv for kv in sorted(labels.items())]
        return self.call("label", *args)

    def read(self, address, lines=50):
        return self.call("read", address, "--lines", str(int(lines)), json_out=False)

    def send(self, address, text, force=False, only_if=None):
        args = [address, text]
        if force:
            args.insert(0, "--force")
        if only_if:
            args = ["--if", only_if] + args
        return self.call("send", *args, timeout=30.0)

    def keys(self, address, *names):
        return self.call("keys", address, *names, timeout=15.0)

    def interrupt(self, address):
        return self.call("interrupt", address, timeout=15.0)

    def kill(self, address):
        return self.call("kill", address, timeout=30.0)


def _b64(s):
    import base64
    import binascii
    if not s:
        return ""
    try:
        return base64.b64decode(s, validate=True).decode("utf-8", "replace")
    except (binascii.Error, ValueError):
        return s


def _text(b):
    if b is None:
        return ""
    if isinstance(b, bytes):
        return b.decode("utf-8", "replace")
    return b


def _error_text(out, err):
    for blob in (out, err):
        blob = (blob or "").strip()
        if not blob:
            continue
        try:
            got = json.loads(blob.splitlines()[-1])
            if isinstance(got, dict):
                e = got.get("error")
                if isinstance(e, dict):
                    return e.get("message") or json.dumps(e)
                if e:
                    return str(e)
        except ValueError:
            pass
        return blob.splitlines()[-1][:300]
    return ""


def session_terminal(payload_env):
    """(hc session, terminal address) from a hook's environment, or None."""
    t = (payload_env or {}).get("HC_TERMINAL_ID")
    if not t:
        return None
    return {"address": t, "core": (payload_env or {}).get("HC_SESSION") or "default"}
