"""Sessions on other machines, over SSH.

The same `claude agents --json` that answers locally answers over SSH, so a
remote host needs no agent, no daemon and no open port - just a key you
already use. Rows come back tagged with the host they came from. The same
call also reads the host's `~/.claude/sessions/*.json`, which is where local
rows get their status and inbox path, so a remote row is built by the same
`registry.merge`.

A host can be made as reachable as a local session, still over SSH only:

  * **Hooks.** A `Tunnel` holds `ssh -N -R` open, so `127.0.0.1:8792` on the
    host leads to a hook-only listener here - one local port per host, so the
    host an event came from is known from the connection, not claimed by it.
    The host runs the same `cc_hook.py` (see `hooks/install.py --remote`).
  * **Injection.** `inject` pipes the two inbox lines over SSH to a one-line
    python3 writer on the host. Nothing runs there between messages.

  * **Conversations.** A `Reader` keeps one SSH channel per host running
    this project's own `conversation.py` against the Agent SDK there, so a
    remote session's pane is built by the same code as a local one.

Focus still does not cross the wire: there is no window here to raise.

A dead host must never stall a poll, so every call is bounded and a host that
fails is parked with a growing backoff - the pattern `ai-metric-tracking`
already uses on this machine for the same reason.
"""
import json
import os
import shlex
import subprocess
import sys
import threading
import time

WINDOWS = sys.platform == "win32"

DEFAULT_CLAUDE = "$HOME/.local/bin/claude"
DEFAULT_PYTHON = "python3"
# Where cc_hook.py posts by default, so the host needs no configuration.
DEFAULT_REMOTE_HOOK_PORT = 8792
FIRST_TUNNEL_PORT = 8793
CONNECT_TIMEOUT = 8
CALL_TIMEOUT = 25.0
MAX_PARALLEL = 4

# A host that is off, asleep or unreachable is ordinary. Back off rather than
# paying the connect timeout on every poll.
BACKOFF_START = 60.0
BACKOFF_MAX = 15 * 60.0

SSH_BASE = [
    "ssh",
    "-o", "BatchMode=yes",
    "-o", "ConnectTimeout=%d" % CONNECT_TIMEOUT,
    "-o", "StrictHostKeyChecking=accept-new",
]


class RemoteError(RuntimeError):
    pass


def load_hosts(config):
    """[{name, claude, label, python, tunnel}] from the `remote` block of config.json.

    `tunnel` is None, or {local_port, remote_port}. `"tunnel": true` takes the
    next free port from FIRST_TUNNEL_PORT in config order.
    """
    block = (config or {}).get("remote") or {}
    if not block.get("enabled", True):
        return []
    hosts = []
    for entry in block.get("hosts") or ():
        if isinstance(entry, str):
            entry = {"name": entry}
        name = (entry or {}).get("name")
        if not name:
            continue
        hosts.append({
            "name": name,
            "claude": entry.get("claude") or DEFAULT_CLAUDE,
            "label": entry.get("label") or name,
            "python": entry.get("python") or DEFAULT_PYTHON,
            "reader_python": entry.get("reader_python") or DEFAULT_READER_PYTHON,
            "tunnel": entry.get("tunnel"),
        })
    used = {h["tunnel"].get("local_port") for h in hosts
            if isinstance(h["tunnel"], dict)}
    port = FIRST_TUNNEL_PORT
    for host in hosts:
        want = host["tunnel"]
        if not want:
            host["tunnel"] = None
            continue
        want = dict(want) if isinstance(want, dict) else {}
        if not want.get("local_port"):
            while port in used:
                port += 1
            want["local_port"] = port
            used.add(port)
        want["local_port"] = int(want["local_port"])
        want["remote_port"] = int(want.get("remote_port")
                                  or DEFAULT_REMOTE_HOOK_PORT)
        host["tunnel"] = want
    return hosts


def _run(argv, timeout):
    return subprocess.run(
        argv,
        capture_output=True,
        text=True,
        timeout=timeout,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0) if WINDOWS else 0,
    )


# Separates `claude agents` output from the session files in one SSH call.
FILES_MARK = "--- ccontrol session files ---"


def agents(host, timeout=CALL_TIMEOUT):
    """Live sessions on `host`, tagged with it. Raises RemoteError on failure.

    Rows carry status and inbox path when the host's Claude Code writes them,
    but never a token: that comes from the host's hooks, and the dispatcher
    pairs the two.
    """
    from . import registry

    claude = host.get("claude") or DEFAULT_CLAUDE
    # `claude` is not on PATH for a non-interactive SSH session on Ubuntu -
    # .bashrc returns early - so it is always called by absolute path.
    remote_cmd = (
        "%s agents --json --all && { echo; echo '%s'; "
        "for f in $HOME/.claude/sessions/*.json; do "
        "[ -f \"$f\" ] && cat \"$f\" && echo; done; true; }" % (claude, FILES_MARK)
    )
    argv = SSH_BASE + [host["name"], remote_cmd]
    try:
        done = _run(argv, timeout)
    except subprocess.TimeoutExpired as exc:
        raise RemoteError("timed out after %gs" % timeout) from exc
    except OSError as exc:
        raise RemoteError("ssh could not run: %s" % exc) from exc
    if done.returncode != 0:
        raise RemoteError((done.stderr or done.stdout or "").strip()[:200]
                          or "ssh exited %d" % done.returncode)
    head, _, tail = (done.stdout or "").partition(FILES_MARK)
    try:
        rows = json.loads(head.strip() or "[]")
    except ValueError:
        raise RemoteError("did not return JSON: %s" % head[:120])
    if not isinstance(rows, list):
        return []
    rows = [r for r in rows if isinstance(r, dict)]
    files = {}
    for line in tail.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rec = json.loads(line)
        except ValueError:
            continue
        if isinstance(rec, dict) and rec.get("sessionId"):
            files[rec["sessionId"]] = rec
    # Only sessions `agents` reports: a session file outliving its process
    # must not put a ghost on the board.
    live = {r.get("sessionId") for r in rows}
    rows = registry.merge(rows, {k: v for k, v in files.items() if k in live})
    for row in rows:
        row["host"] = host["name"]
        row["label"] = host.get("label") or host["name"]
        row["remote"] = True
    return rows


# Runs on the host. The socket path is argv[1]; the auth and user lines arrive
# on stdin, already built here by `inbox`, so the host needs nothing but
# python3.
INJECT_SCRIPT = (
    "import socket,sys\n"
    "s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)\n"
    "s.settimeout(10)\n"
    "s.connect(sys.argv[1])\n"
    "s.sendall(sys.stdin.buffer.read())\n"
    "s.close()\n"
)


def _run_input(argv, data, timeout):
    return subprocess.run(
        argv,
        input=data,
        capture_output=True,
        timeout=timeout,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0) if WINDOWS else 0,
    )


def inject(host, socket_path, token, content, timeout=30.0):
    """Deliver `content` to a session's inbox on `host`. Raises RemoteError."""
    from . import inbox

    if not isinstance(content, str) or not content.strip():
        raise RemoteError("prompt is empty")
    if len(content) > inbox.MAX_CONTENT:
        raise RemoteError("prompt is %d chars; the cap is %d"
                          % (len(content), inbox.MAX_CONTENT))
    if not socket_path or not socket_path.startswith("/"):
        raise RemoteError("not a Unix socket path: %r" % (socket_path,))
    if not token:
        raise RemoteError("no token for that session")
    python = host.get("python") or DEFAULT_PYTHON
    # The token travels on stdin, never on a command line where `ps` on the
    # host would show it.
    remote_cmd = "%s -c %s %s" % (python, shlex.quote(INJECT_SCRIPT),
                                  shlex.quote(socket_path))
    try:
        done = _run_input(SSH_BASE + [host["name"], remote_cmd],
                          inbox._frames(token, content), timeout)
    except subprocess.TimeoutExpired as exc:
        raise RemoteError("timed out after %gs" % timeout) from exc
    except OSError as exc:
        raise RemoteError("ssh could not run: %s" % exc) from exc
    if done.returncode != 0:
        err = (done.stderr or done.stdout or b"").decode("utf-8", "replace")
        lines = [l for l in err.strip().splitlines() if l.strip()]
        raise RemoteError((lines[-1] if lines else "")[:200]
                          or "ssh exited %d" % done.returncode)


def start_background(host, cwd, prompt, options=None, timeout=120.0):
    """`claude --bg` in `cwd` on `host`. Returns {id, host, output}."""
    from . import background

    if not prompt or not prompt.strip():
        raise RemoteError("prompt is empty")
    claude = host.get("claude") or DEFAULT_CLAUDE
    argv = background.build_command(prompt, **{
        k: v for k, v in (options or {}).items() if k in background.OPTION_KEYS
    })
    # The arguments are quoted so a prompt with spaces or quotes survives the
    # remote shell. The interpreter path is deliberately NOT quoted: it comes
    # from config and is normally written with $HOME or ~, and quoting stops
    # the remote shell expanding it. That is exactly how the first remote
    # dispatch failed, while polling worked - polling builds its command as a
    # plain unquoted string.
    remote_cmd = "cd %s && %s %s" % (
        shlex.quote(cwd),
        claude,
        " ".join(shlex.quote(a) for a in argv[1:]),
    )
    try:
        done = _run(SSH_BASE + [host["name"], remote_cmd], timeout)
    except subprocess.TimeoutExpired as exc:
        raise RemoteError("timed out after %gs" % timeout) from exc
    except OSError as exc:
        raise RemoteError("ssh could not run: %s" % exc) from exc
    output = ((done.stdout or "") + (done.stderr or "")).strip()
    if done.returncode != 0:
        raise RemoteError(output[:300] or "ssh exited %d" % done.returncode)
    session = background.parse_id(done.stdout or "")
    if not session:
        raise RemoteError("started, but no session id was printed: %s"
                          % output[:200])
    return {"id": session, "host": host["name"], "output": output[:400]}


class Fleet:
    """Polls a set of hosts, parking the ones that are failing."""

    def __init__(self, hosts=None):
        self.hosts = list(hosts or ())
        self._state = {}  # name -> {fails, next_attempt, error}
        self._lock = threading.Lock()

    def status(self):
        with self._lock:
            return {name: dict(rec) for name, rec in self._state.items()}

    def _due(self, name):
        with self._lock:
            rec = self._state.get(name)
            return not rec or time.time() >= rec.get("next_attempt", 0)

    def _ok(self, name):
        with self._lock:
            self._state[name] = {"fails": 0, "next_attempt": 0, "error": None}

    def _failed(self, name, error):
        with self._lock:
            rec = self._state.get(name) or {"fails": 0}
            fails = rec.get("fails", 0) + 1
            delay = min(BACKOFF_START * (2 ** (fails - 1)), BACKOFF_MAX)
            self._state[name] = {
                "fails": fails,
                "next_attempt": time.time() + delay,
                "error": str(error)[:200],
                "parked_for": delay,
            }

    def poll(self):
        """Rows from every host that is due. Never raises."""
        due = [h for h in self.hosts if self._due(h["name"])]
        if not due:
            return []
        results = []
        threads = []
        lock = threading.Lock()

        def work(host):
            try:
                rows = agents(host)
            except RemoteError as exc:
                self._failed(host["name"], exc)
                return
            self._ok(host["name"])
            with lock:
                results.extend(rows)

        for host in due[:MAX_PARALLEL * 8]:
            thread = threading.Thread(target=work, args=(host,), daemon=True)
            thread.start()
            threads.append(thread)
            # Bound concurrency so a fleet of sleeping hosts does not open a
            # connection per host at once.
            if len(threads) % MAX_PARALLEL == 0:
                for t in threads[-MAX_PARALLEL:]:
                    t.join(timeout=CALL_TIMEOUT + 5)
        for thread in threads:
            thread.join(timeout=CALL_TIMEOUT + 5)
        return results


class Tunnel:
    """Keeps `ssh -N -R` open so the host's hooks can reach this machine.

    On the host, `127.0.0.1:<remote_port>` leads to `127.0.0.1:<local_port>`
    here, which is a hook-only listener - never the dispatcher's own port,
    which would hand every process on that host `/api/dispatch` into the
    sessions on this one.

    ssh exits when the link dies (ServerAlive) or the host's port is taken
    (ExitOnForwardFailure); either way it is started again after a backoff.
    """

    def __init__(self, host, local_port, remote_port=DEFAULT_REMOTE_HOOK_PORT):
        self.host = host
        self.local_port = local_port
        self.remote_port = remote_port
        self._lock = threading.Lock()
        self._proc = None
        self._stop = threading.Event()
        self._state = {"up": False, "since": None, "fails": 0, "error": None}

    def argv(self):
        return SSH_BASE + [
            "-N",
            "-o", "ExitOnForwardFailure=yes",
            "-o", "ServerAliveInterval=15",
            "-o", "ServerAliveCountMax=3",
            "-R", "127.0.0.1:%d:127.0.0.1:%d" % (self.remote_port, self.local_port),
            self.host["name"],
        ]

    def status(self):
        with self._lock:
            out = dict(self._state)
        out.update(local_port=self.local_port, remote_port=self.remote_port)
        return out

    def start(self):
        threading.Thread(target=self._run, name="tunnel-%s" % self.host["name"],
                         daemon=True).start()
        return self

    def stop(self):
        self._stop.set()
        with self._lock:
            proc = self._proc
        if proc and proc.poll() is None:
            proc.terminate()

    def _run(self):
        while not self._stop.is_set():
            started = time.time()
            try:
                proc = subprocess.Popen(
                    self.argv(),
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.PIPE,
                    creationflags=(getattr(subprocess, "CREATE_NO_WINDOW", 0)
                                   if WINDOWS else 0),
                )
            except OSError as exc:
                self._down("ssh could not run: %s" % exc)
                self._stop.wait(BACKOFF_START)
                continue
            _bind_to_this_process(proc)
            with self._lock:
                self._proc = proc
                self._state.update(up=True, since=started, error=None)
            err = proc.communicate()[1] or b""
            lines = [l for l in err.decode("utf-8", "replace").splitlines()
                     if l.strip()]
            reason = (lines[-1] if lines else "ssh exited %s" % proc.returncode)
            # A tunnel that held for a while and then dropped is a network
            # blip, not a broken host; do not make it wait out a long backoff.
            if time.time() - started > 120:
                with self._lock:
                    self._state["fails"] = 0
            delay = self._down(reason[:200])
            self._stop.wait(delay)

    def _down(self, reason):
        with self._lock:
            fails = self._state.get("fails", 0) + 1
            delay = min(5.0 * (2 ** (fails - 1)), BACKOFF_MAX)
            self._state.update(up=False, since=None, fails=fails, error=reason,
                               retry_in=delay)
            return delay


# The host's side of a Reader. Its first stdin line is the source of
# `conversation.py`, JSON-encoded - the same code the local reader runs, so
# there is one parser, not two - and every line after that is a request.
READER_BOOT = "import sys,json;exec(json.loads(sys.stdin.readline()))"
READER_LOOP = r'''
import json as _json, sys as _sys
for _line in _sys.stdin:
    _req = {}
    try:
        _req = _json.loads(_line)
        _items = read(_req["session"], limit=_req.get("limit", 60),
                      directory=_req.get("cwd"))
        _out = {"id": _req.get("id"), "ok": True, "items": _items,
                "title": title(_req["session"])}
    except Exception as _exc:
        _out = {"id": _req.get("id") if isinstance(_req, dict) else None,
                "ok": False, "error": "%s: %s" % (type(_exc).__name__, _exc)}
    _sys.stdout.write(_json.dumps(_out) + "\n")
    _sys.stdout.flush()
'''
DEFAULT_READER_PYTHON = "$HOME/.claude/ccontrol/venv/bin/python"
READ_TIMEOUT = 15.0
# The first read on a host pays a one-off ~1s inside the SDK; after that a
# read is ~2ms. A fresh SSH connection per 1.5s poll would pay the connect
# and that second every time, so one channel per host is kept open.
READER_RETRY = 30.0


class Reader:
    """A long-lived SSH channel that reads conversations on a host.

    Not a daemon: it is a child of the dispatcher (bound to it by the same
    job object as the tunnel), runs only while something asks, and needs only
    the Agent SDK in a venv on the host. Requests are serialised; a read that
    times out kills the channel rather than leaving a reply to be mistaken
    for the next request's.
    """

    def __init__(self, host):
        self.host = host
        self._lock = threading.Lock()
        self._proc = None
        self._lines = None
        self._seq = 0
        self._failed_at = 0.0
        self._error = None

    def argv(self):
        python = self.host.get("reader_python") or DEFAULT_READER_PYTHON
        # Unquoted interpreter so $HOME expands; the boot line is quoted.
        return SSH_BASE + [self.host["name"],
                           "%s -u -c %s" % (python, shlex.quote(READER_BOOT))]

    def _start(self):
        import queue

        here = os.path.dirname(os.path.abspath(__file__))
        with open(os.path.join(here, "conversation.py"), encoding="utf-8") as fh:
            source = fh.read() + "\n" + READER_LOOP
        proc = subprocess.Popen(
            self.argv(), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0) if WINDOWS else 0,
        )
        _bind_to_this_process(proc)
        lines = queue.Queue()

        def pump(stream, tag):
            for raw in iter(stream.readline, b""):
                lines.put((tag, raw.decode("utf-8", "replace")))
            lines.put((tag, None))

        threading.Thread(target=pump, args=(proc.stdout, "out"), daemon=True).start()
        threading.Thread(target=pump, args=(proc.stderr, "err"), daemon=True).start()
        self._proc, self._lines = proc, lines
        self._send(json.dumps(source))

    def _send(self, line):
        """Write one line. An ssh that already died is reported by its own
        words - "Permission denied (publickey)" - not as a broken pipe."""
        try:
            self._proc.stdin.write((line + "\n").encode("utf-8"))
            self._proc.stdin.flush()
        except OSError as exc:
            said = self._last_words([])
            self._stop((said[-1] if said else "channel closed: %s" % exc)[:200])
            raise RemoteError(self._error) from exc

    def _last_words(self, said):
        """What a dying channel wrote to stderr, waiting briefly for it."""
        deadline = time.time() + 3
        while time.time() < deadline:
            try:
                tag, text = self._lines.get(timeout=max(0.0, deadline - time.time()))
            except Exception:
                break
            if tag == "err":
                if text is None:
                    break
                if text.strip():
                    said.append(text.strip())
        return said

    def _stop(self, error):
        proc, self._proc, self._lines = self._proc, None, None
        if proc:
            if proc.poll() is None:
                proc.kill()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
            for stream in (proc.stdin, proc.stdout, proc.stderr):
                try:
                    stream.close()
                except (OSError, AttributeError):
                    pass
        self._failed_at = time.time()
        self._error = error

    def close(self):
        with self._lock:
            self._stop(None)

    def read(self, session_id, limit=60, cwd=None, timeout=READ_TIMEOUT):
        """{items, title} for a session on this host. Raises RemoteError."""
        import queue

        with self._lock:
            if self._proc is None:
                # A host that just failed is not re-dialled on every 1.5s poll.
                if self._error and time.time() - self._failed_at < READER_RETRY:
                    raise RemoteError(self._error)
                try:
                    self._start()
                except (OSError, ValueError) as exc:
                    self._stop("ssh could not run: %s" % exc)
                    raise RemoteError(self._error) from exc
            self._seq += 1
            req = {"id": self._seq, "session": session_id, "limit": limit}
            if cwd:
                req["cwd"] = cwd
            self._send(json.dumps(req))
            deadline = time.time() + timeout
            stderr = []
            while True:
                left = deadline - time.time()
                try:
                    tag, line = self._lines.get(timeout=max(0.0, left))
                except queue.Empty:
                    self._stop("no reply in %gs" % timeout)
                    raise RemoteError(self._error)
                if tag == "err":
                    if line is not None and line.strip():
                        stderr.append(line.strip())
                    continue
                if line is None:
                    said = self._last_words(stderr)
                    self._stop((said[-1] if said else "reader exited")[:200])
                    raise RemoteError(self._error)
                try:
                    reply = json.loads(line)
                except ValueError:
                    continue  # stray output from the host's shell
                if reply.get("id") != self._seq:
                    continue  # a reply to a request that already timed out
                self._error = None
                if not reply.get("ok"):
                    raise RemoteError(reply.get("error") or "read failed")
                return {"items": reply.get("items") or [],
                        "title": reply.get("title") or ""}


_job = None


def _bind_to_this_process(proc):
    """Make `proc` die with this process on Windows.

    The dispatcher is restarted by killing it. An orphaned ssh would keep the
    host's port, so every later tunnel would fail with "remote port forwarding
    failed" and the host's hooks would still be feeding a dead listener's
    port - working by accident until the orphan went too. A job object with
    KILL_ON_JOB_CLOSE ties the child to this process's lifetime.
    """
    global _job
    if not WINDOWS:
        return
    try:
        import ctypes
        from ctypes import wintypes

        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        if _job is None:
            class BASIC(ctypes.Structure):
                _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64),
                            ("PerJobUserTimeLimit", ctypes.c_int64),
                            ("LimitFlags", wintypes.DWORD),
                            ("MinimumWorkingSetSize", ctypes.c_size_t),
                            ("MaximumWorkingSetSize", ctypes.c_size_t),
                            ("ActiveProcessLimit", wintypes.DWORD),
                            ("Affinity", ctypes.c_size_t),
                            ("PriorityClass", wintypes.DWORD),
                            ("SchedulingClass", wintypes.DWORD)]

            class IO(ctypes.Structure):
                _fields_ = [(n, ctypes.c_uint64) for n in (
                    "ReadOperationCount", "WriteOperationCount",
                    "OtherOperationCount", "ReadTransferCount",
                    "WriteTransferCount", "OtherTransferCount")]

            class EXTENDED(ctypes.Structure):
                _fields_ = [("BasicLimitInformation", BASIC),
                            ("IoInfo", IO),
                            ("ProcessMemoryLimit", ctypes.c_size_t),
                            ("JobMemoryLimit", ctypes.c_size_t),
                            ("PeakProcessMemoryUsed", ctypes.c_size_t),
                            ("PeakJobMemoryUsed", ctypes.c_size_t)]

            k32.CreateJobObjectW.restype = wintypes.HANDLE
            job = k32.CreateJobObjectW(None, None)
            if not job:
                return
            info = EXTENDED()
            info.BasicLimitInformation.LimitFlags = 0x2000  # KILL_ON_JOB_CLOSE
            k32.SetInformationJobObject.argtypes = [
                wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
            if not k32.SetInformationJobObject(job, 9, ctypes.byref(info),
                                               ctypes.sizeof(info)):
                return
            _job = job
        k32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        k32.AssignProcessToJobObject(_job, int(proc._handle))
    except Exception:
        # Best effort: without it a restart can orphan one ssh, which the
        # next tunnel reports as a forwarding failure rather than hiding.
        pass


if __name__ == "__main__":
    import argparse

    ap = argparse.ArgumentParser(description="Probe remote hosts")
    ap.add_argument("host", nargs="*", help="host names (default: config.json)")
    args = ap.parse_args()

    if args.host:
        hosts = [{"name": h, "claude": DEFAULT_CLAUDE, "label": h}
                 for h in args.host]
    else:
        path = os.path.join(os.path.dirname(os.path.dirname(
            os.path.abspath(__file__))), "config.json")
        try:
            with open(path, encoding="utf-8") as fh:
                hosts = load_hosts(json.load(fh))
        except (OSError, ValueError):
            hosts = []
    if not hosts:
        raise SystemExit("no hosts configured (see the `remote` block of config.json)")

    fleet = Fleet(hosts)
    rows = fleet.poll()
    print("%d session(s) across %d host(s)" % (len(rows), len(hosts)))
    for row in rows:
        print("  %-16s %-26s %-11s %-8s %s" % (
            row.get("host"),
            (row.get("name") or row.get("sessionId") or "?")[:26],
            row.get("kind"), row.get("state") or row.get("status"),
            "inbox" if row.get("socket") else "no inbox"))
    for name, rec in fleet.status().items():
        if rec.get("error"):
            print("  %-16s PARKED %.0fs: %s" % (name, rec.get("parked_for", 0),
                                                rec["error"]))
