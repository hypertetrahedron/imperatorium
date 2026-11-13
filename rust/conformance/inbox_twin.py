"""Prompt delivery, Python and the binary, with no real session anywhere.

twin.py's dispatchers see this machine's real sessions, so they cannot be
asked to deliver anything. Here each twin also gets a CLAUDE_CONFIG_DIR of its
own: a temporary folder whose sessions/ holds made-up session records, each
pointing at a named pipe this script serves. The pipe only records what
arrives. A real session's inbox is never named, its token never known, so
nothing can reach one.

Both twins get the same SessionStart hooks (sockets and tokens), then the
same queue requests one twin at a time; the bytes each delivers, its answers
and the state it keeps must agree. Covered: an idle session gets its prompt
at once; a busy one or one waiting on you holds it until Stop, one per turn;
a session whose pipe is gone keeps the prompt with the error; queued from the
window (Origin plus the page's token) is marked as the person's own.

    python rust/conformance/inbox_twin.py      (Windows: named pipes)
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
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, binary  # noqa: E402
from sessions_compare import first_difference  # noqa: E402
from twin import env, get, masked, post  # noqa: E402

PY_PORT, RS_PORT = 8815, 8816
IDLE = "aaaaaaaa-1111-4111-8111-111111111111"
BUSY = "bbbbbbbb-2222-4222-8222-222222222222"
GONE = "cccccccc-3333-4333-8333-333333333333"
WAITS = "dddddddd-4444-4444-8444-444444444444"
LATE = "eeeeeeee-5555-4555-8555-555555555555"
BG = "ffffffff-6666-4666-8666-666666666666"


class Pipe:
    """A named pipe that accepts any number of writers and keeps what each
    wrote, one entry per connection."""

    def __init__(self, name):
        import _winapi
        self.w = _winapi
        self.path = "\\\\.\\pipe\\" + name
        self.got = []
        self.lock = threading.Lock()
        threading.Thread(target=self._serve, daemon=True).start()

    def _serve(self):
        w = self.w
        while True:
            # PIPE_ACCESS_INBOUND; byte type, byte read mode, blocking (all 0);
            # PIPE_UNLIMITED_INSTANCES.
            h = w.CreateNamedPipe(self.path, 0x1, 0, 255, 65536, 65536, 0, w.NULL)
            try:
                w.ConnectNamedPipe(h, False)
            except OSError:
                pass  # ERROR_PIPE_CONNECTED: the writer came first
            threading.Thread(target=self._read, args=(h,), daemon=True).start()

    def _read(self, h):
        w, data = self.w, b""
        while True:
            try:
                chunk, _ = w.ReadFile(h, 65536)
            except OSError:
                break
            if not chunk:
                break
            data += chunk
        w.CloseHandle(h)
        with self.lock:
            self.got.append(data)

    def take(self, wait=5.0):
        """What arrived since the last take, waiting briefly for a reader."""
        end = time.time() + wait
        while time.time() < end:
            with self.lock:
                if self.got:
                    out, self.got = self.got, []
                    return out
            time.sleep(0.05)
        return []


def start(root):
    ports = {"py": PY_PORT, "rs": RS_PORT}
    procs = []
    for side, port in ports.items():
        e = dict(env(), CLAUDE_CONFIG_DIR=os.path.join(root, "claude"),
                 IMPERATORIUM_POLICY=os.path.join(root, "policy.json"),
                 LOCALAPPDATA=os.path.join(root, side + "-local"))
        state = os.path.join(root, side, "state.json")
        cmd = ([sys.executable, "-m", "imperatorium.dispatcher", "--port", str(port), "--state", state]
               if side == "py" else
               [binary(), "serve", "--port", str(port), "--home", REPO, "--state", state])
        procs.append(subprocess.Popen(cmd, cwd=REPO, env=e,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    for port in ports.values():
        for _ in range(150):
            try:
                urllib.request.urlopen("http://127.0.0.1:%d/api/health" % port, timeout=2).read()
                break
            except OSError:
                time.sleep(0.2)
        else:
            raise SystemExit("twin on %d did not start" % port)
    return procs


def record(root, sid, status, socket, name, kind="interactive"):
    """A session record as Claude Code writes it under sessions/; the pid is
    this script's, so it is alive for as long as the test runs."""
    folder = os.path.join(root, "claude", "sessions")
    os.makedirs(folder, exist_ok=True)
    rec = {"pid": os.getpid(), "sessionId": sid, "cwd": os.path.join(root, "work"),
           "name": name, "kind": kind, "status": status,
           "messagingSocketPath": socket, "startedAt": int(time.time() * 1000)}
    with open(os.path.join(folder, "%s.json" % sid), "w", encoding="utf-8") as fh:
        json.dump(rec, fh)


def window_token(port):
    with urllib.request.urlopen("http://127.0.0.1:%d/" % port, timeout=30) as fh:
        page = fh.read().decode("utf-8")
    return re.search(r'<meta name=imp-window content="([^"]*)"', page).group(1)


def post_window(port, path, body, token):
    req = urllib.request.Request("http://127.0.0.1:%d%s" % (port, path),
                                 data=json.dumps(body).encode(), method="POST")
    req.add_header("Content-Type", "application/json")
    req.add_header("Origin", "http://127.0.0.1:%d" % port)
    req.add_header("X-Imperatorium-Window", token)
    with urllib.request.urlopen(req, timeout=30) as fh:
        return fh.status, json.loads(fh.read())


IDS = {"id", "delivered", "mod_prompt"}


def scrub(value):
    """Clocks and random ids masked; an error's wording is the OS's, so only
    whether there is one is compared. Tuples are lists here."""
    value = masked(json.loads(json.dumps(value, default=repr)))
    if isinstance(value, dict):
        return {k: ("<id>" if k in IDS and isinstance(v, str) else
                    "<error>" if k == "error" and isinstance(v, str) and "pipe" in v.lower() else
                    scrub(v)) for k, v in value.items()}
    if isinstance(value, list):
        return [scrub(v) for v in value]
    return value


def post_mod(port, path, body, token, raw=None):
    req = urllib.request.Request("http://127.0.0.1:%d%s" % (port, path),
                                 data=raw if raw is not None else json.dumps(body).encode(),
                                 method="POST")
    req.add_header("Content-Type", "application/json")
    if token is not None:
        req.add_header("X-Imperatorium-Mod", token)
    try:
        with urllib.request.urlopen(req, timeout=30) as fh:
            return fh.status, json.loads(fh.read())
    except urllib.error.HTTPError as exc:
        return exc.code, json.loads(exc.read() or b"null")


def mods(port, root, side, tokens, pipes):
    """The mod in the idle session, against one twin."""
    with open(os.path.join(root, side, "mod-token"), encoding="utf-8") as fh:
        token = fh.read().strip()
    out = []
    usage = {"context": {"percent": 31.5, "window": 200000}, "cost": {"usd": 2.5},
             "model": "Opus 5.5", "rateLimits": [
                 {"kind": "five_hour", "percentUsed": 85, "resetsAt": "2100-01-01T00:00:00.000Z"},
                 {"kind": "seven_day", "percentUsed": 20, "resetsAt": "2100-01-03T12:00:00+02:00"},
                 {"kind": "odd", "percentUsed": "x"}]}
    beat = {"session_id": IDLE, "cwd": os.path.join(root, "work"), "busy": False,
            "version": "2.1.300", "mod_version": "0.3.0", "interactive": True,
            "surfaces": ["terminal"], "usage": usage}

    def b(extra=None, tok=token):
        got = post_mod(port, "/api/mod/beat", dict(beat, **(extra or {})), tok)
        out.append(("beat", got))
        return got[1]

    b(tok=None)
    b(tok="x" * 43)
    out.append(("raw", post_mod(port, "/api/mod/beat", None, token, raw=b"[1]")))
    out.append(("no sid", post_mod(port, "/api/mod/beat", {"busy": True}, token)))
    b()
    row = next(r for r in get(port, "/api/sessions")["sessions"] if r["sessionId"] == IDLE)
    out.append(("row", {k: row.get(k) for k in ("mod", "mod_pending", "status", "dispatchable")}))
    # The mod is live: prompts go to it, not the pipe.
    out.append(("queue", post_window(port, "/api/queue", {"session_id": IDLE, "prompt": "typed as me"},
                                     tokens[port])))
    out.append(("queue", post(port, "/api/queue", {"session_id": IDLE, "prompt": "framed one"})))
    out.append(("pipe", pipes[IDLE].take(wait=0.5)))
    row = next(r for r in get(port, "/api/sessions")["sessions"] if r["sessionId"] == IDLE)
    out.append(("row", {k: row.get(k) for k in ("mod", "mod_pending", "status")}))
    given = b().get("deliver") or []
    b()  # claimed: not handed out again
    if len(given) == 2:
        b({"acks": [{"id": given[0]["id"], "outcome": "accepted"},
                    {"id": given[1]["id"], "outcome": "dropped", "reason": "engine said no"},
                    {"id": "nope", "outcome": "submitted"}, "junk"]})
        b({"busy": True, "acks": [{"id": given[0]["id"], "outcome": "submitted"}]})
    for tool, inp in (("Bash", {"command": "git status"}), ("Bash", {"command": "rm -rf /"}),
                      ("Read", {"file_path": os.path.join(root, "work", "a.py")}), ("Bash", "not a dict")):
        out.append(("check", post_mod(port, "/api/mod/check",
                                      {"tool_name": tool, "tool_input": inp, "cwd": os.path.join(root, "work"),
                                       "session_id": IDLE, "agent_id": None}, token)))
    u = get(port, "/api/usage")
    out.append(("usage", {k: u.get(k) for k in ("limits", "plan")}))
    return out, len(given)


def parks(port):
    """Parked permission requests against one twin: what each step answered."""
    out = []

    def park(body):
        box = {}
        t = threading.Thread(target=lambda: box.update(r=post(port, "/permission/park", body)))
        t.start()
        return t, box

    def waiting(n):
        end = time.time() + 10
        while time.time() < end:
            got = get(port, "/api/pending")["pending"]
            if len(got) >= n:
                return got
            time.sleep(0.05)
        return get(port, "/api/pending")["pending"]

    def decide(body):
        out.append(("decide", body.get("verdict"), post(port, "/api/decide", body)))

    bash = {"session_id": IDLE, "cwd": "C:\\w", "tool_name": "Bash", "_reason": "not on the list",
            "tool_input": {"command": "rm -rf build", "description": "clean"}, "wait": 10}
    t, box = park(bash)
    item = waiting(1)[0]
    out.append(("pending", scrub(item)))
    decide({"id": item["id"], "verdict": "maybe"})
    decide({"id": "nope", "verdict": "allow"})
    decide({"id": item["id"], "verdict": "allow", "who": "deck"})
    decide({"id": item["id"], "verdict": "deny"})
    t.join()
    out.append(("park", box["r"]))

    question = {"session_id": IDLE, "tool_name": "AskUserQuestion", "wait": 1, "wait_question": 10,
                "tool_input": {"questions": [
                    {"question": "Which database?", "header": "DB", "multiSelect": False,
                     "options": [{"label": "Postgres", "description": "relational"}, {"label": ""}, "x"]},
                    {"question": "Also?", "multiSelect": True, "options": [{"label": "Cache"}]}]}}
    t, box = park(question)
    item = waiting(1)[0]
    out.append(("pending", scrub(item)))
    decide({"id": item["id"], "verdict": "allow"})
    decide({"id": item["id"], "verdict": "allow", "answers": {"Which database?": "Postgres"}})
    time.sleep(1.5)  # past `wait`: wait_question holds it
    decide({"id": item["id"], "verdict": "allow",
            "answers": {"Which database?": " Postgres ", "Also?": "Cache", "extra": "x"}})
    t.join()
    out.append(("park", box["r"]))

    # Nobody answers: a silent ask, and it leaves the list.
    out.append(("expired", post(port, "/permission/park", {"session_id": IDLE, "tool_name": "Edit",
                                                          "tool_input": {"file_path": "a.py"}, "wait": 0.3})))
    out.append(("after", get(port, "/api/pending")))
    # Pass: released without a decision.
    t, box = park({"session_id": IDLE, "tool_name": "WebFetch", "tool_input": {"url": "https://x"}, "wait": 10})
    item = waiting(1)[0]
    decide({"id": item["id"], "verdict": "pass"})
    t.join()
    out.append(("pass", box["r"], get(port, "/api/pending")))
    # A background session has no terminal: wait_background, not wait.
    t, box = park({"session_id": BG, "tool_name": "Bash", "tool_input": {"command": "make"},
                   "wait": 0.2, "wait_background": 10})
    item = waiting(1)[0]
    time.sleep(1)
    decide({"id": item["id"], "verdict": "deny"})
    t.join()
    out.append(("background", box["r"]))
    return out


def main():
    if os.name != "nt":
        print("inbox_twin: named pipes only (Windows); skipped")
        return 0
    root = tempfile.mkdtemp(prefix="imp-inbox-twin-")
    # The policy, with auto-approval on so /api/mod/check decides.
    live_policy = os.path.join(os.path.dirname(REPO), "imperatorium", "policy.json")
    try:
        with open(live_policy, encoding="utf-8") as fh:
            policy = json.load(fh)
    except (OSError, ValueError):
        policy = {}
    policy["decision_format"] = "object"
    with open(os.path.join(root, "policy.json"), "w", encoding="utf-8") as fh:
        json.dump(policy, fh)
    tag = uuid.uuid4().hex[:8]
    pipes = {sid: Pipe("imp-twin-%s-%s" % (tag, n))
             for n, sid in (("idle", IDLE), ("busy", BUSY), ("waits", WAITS), ("late", LATE))}
    gone = "\\\\.\\pipe\\imp-twin-%s-nobody" % tag
    record(root, IDLE, "idle", pipes[IDLE].path, "twin-idle")
    record(root, BUSY, "busy", pipes[BUSY].path, "twin-busy")
    record(root, WAITS, "idle", pipes[WAITS].path, "twin-waits")
    record(root, GONE, "idle", gone, "twin-gone")
    record(root, LATE, "idle", pipes[LATE].path, "twin-late")
    record(root, BG, "busy", gone, "twin-bg", kind="background")
    procs = start(root)
    failures, sent = [], 0

    def both(path, body, window=False):
        """The same POST to each twin in turn, and what each delivered."""
        nonlocal sent
        out = []
        for port in (PY_PORT, RS_PORT):
            if window:
                code, got = post_window(port, path, body, tokens[port])
            else:
                code, got = post(port, path, body)
            delivered = {sid: p.take(wait=1.0) for sid, p in pipes.items()}
            out.append((code, got, {k: v for k, v in delivered.items() if v}))
        sent += 1
        (ca, a, da), (cb, b, db) = out
        if (ca, scrub(a)) != (cb, scrub(b)):
            failures.append(("POST %s %r" % (path, body), "%r vs %r" % ((ca, a), (cb, b))))
        if da != db:
            failures.append(("delivered for %s %r" % (path, body), "%r vs %r" % (da, db)))
        return out

    try:
        tokens = {port: window_token(port) for port in (PY_PORT, RS_PORT)}
        for sid in (IDLE, BUSY, GONE, WAITS):
            sock = gone if sid == GONE else pipes[sid].path
            both("/hook", {"hook_event_name": "SessionStart", "session_id": sid,
                           "cwd": os.path.join(root, "work"), "source": "startup",
                           "messaging_socket": sock, "messaging_token": "tok-" + sid[:4]})
        both("/hook", {"hook_event_name": "Notification", "session_id": WAITS,
                       "notification_type": "permission_prompt", "message": "Claude needs permission"})
        rows = get(PY_PORT, "/api/sessions")["sessions"]
        seen = {r["sessionId"]: (r.get("status"), r.get("dispatchable"), r.get("attention")) for r in rows}
        print("sessions: %r" % seen)
        if len(seen) != 6:
            failures.append(("sessions", "expected the 6 made-up sessions, got %r" % seen))

        # Idle and reachable: delivered in the request.
        out = both("/api/queue", {"session_id": IDLE, "prompt": "run the tests é \"quoted\"\nline two"})
        delivered = out[0][2].get(IDLE) or []
        print("idle: answered %r; %d bytes delivered" % (out[0][1].get("waiting"), sum(map(len, delivered))))
        if len(delivered) != 1 or b'"type": "auth", "token": "tok-aaaa"' not in delivered[0]:
            failures.append(("idle delivery", "expected one auth+user write, got %r" % delivered))
        # From the window: marked as yours (the mod's path would use it).
        both("/api/queue", {"session_id": IDLE, "prompt": "typed in the window"}, window=True)
        # Busy: held, two of them, then one per Stop.
        both("/api/queue", {"session_id": BUSY, "prompt": "first after this turn"})
        both("/api/queue", {"session_id": BUSY, "prompt": "second after that"})
        # Waiting on you: held even though idle.
        both("/api/queue", {"session_id": WAITS, "prompt": "after you answer"})
        both("/api/queue", {"session_id": WAITS, "prompt": "and from the window"}, window=True)
        # Its pipe is gone: kept, with the error, at the front.
        both("/api/queue", {"session_id": GONE, "prompt": "nobody is listening"})
        # A turn ends: one queued prompt goes (Stop delivers whatever the
        # record says; it stays "busy" so the sweep keeps off the second).
        out = both("/hook", {"hook_event_name": "Stop", "session_id": BUSY,
                             "cwd": os.path.join(root, "work"), "last_assistant_message": "done"})
        print("stop: answered %r" % (scrub(out[0][1]),))
        if not out[0][2].get(BUSY):
            failures.append(("Stop", "the first queued prompt was not delivered"))

        # No token yet: held. Its SessionStart brings one, and the sweep
        # (every 15 s) delivers - once from each twin.
        both("/api/queue", {"session_id": LATE, "prompt": "when you can hear me"})
        late = {"hook_event_name": "SessionStart", "session_id": LATE,
                "cwd": os.path.join(root, "work"), "source": "startup",
                "messaging_socket": pipes[LATE].path, "messaging_token": "tok-late"}
        # Straight to each, not both(): a sweep may fire at any moment now,
        # and both() would count its delivery as the hook's.
        answers = [post(port, "/hook", late) for port in (PY_PORT, RS_PORT)]
        if answers[0] != answers[1]:
            failures.append(("POST /hook late", "%r vs %r" % tuple(answers)))
        swept, end = [], time.time() + 40
        while len(swept) < 2 and time.time() < end:
            swept += pipes[LATE].take(wait=1.0)
        print("sweep: %d deliveries to the late session" % len(swept))
        if len(swept) != 2 or swept[0] != swept[1]:
            failures.append(("sweep", "expected the same prompt from each twin, got %r" % swept))
        time.sleep(1)

        # Parked permission requests.
        got = [parks(port) for port in (PY_PORT, RS_PORT)]
        print("parks: %d steps; %s" % (len(got[0]), [s[0] + ("=" + str(s[1]) if s[0] == "decide" else "")
                                                    for s in got[0]]))
        steps = {step[0]: step for step in got[0]}
        want = {"park": (200, {"verdict": "allow", "decided_by": "window",
                               "answers": {"Which database?": "Postgres", "Also?": "Cache"}}),
                "expired": (200, {"verdict": None, "decided_by": None}),
                "background": (200, {"verdict": "deny", "decided_by": "window"})}
        for name, expect in want.items():
            if steps[name][1] != expect:
                failures.append(("parks: " + name, "%r, expected %r" % (steps[name][1], expect)))
        if got[0][5][1] != (200, {"verdict": "allow", "decided_by": "deck"}):
            failures.append(("parks: bash", repr(got[0][5])))
        diff = first_difference(scrub(got[0]), scrub(got[1]), "parks")
        if diff:
            failures.append(("parks", diff))

        # The mod.
        got, given = [], []
        for side, port in (("py", PY_PORT), ("rs", RS_PORT)):
            o, g = mods(port, root, side, tokens, pipes)
            got.append(o)
            given.append(g)
        print("mod: %d steps; %d prompts handed to the mod; checks %s" % (
            len(got[0]), given[0], [s[1][1].get("verdict") for s in got[0] if s[0] == "check"]))
        print("mod: status lines %r" % sorted({st[1][1].get("status") for st in got[0]
                                                if st[0] == "beat" and st[1][0] == 200}, key=str))
        print("mod: limits from its usage %r" % sorted((got[0][-1][1].get("limits") or {})))
        if given[0] != 2:
            failures.append(("mod", "expected both prompts handed to the mod, got %d" % given[0]))
        diff = first_difference(scrub(got[0]), scrub(got[1]), "mod")
        if diff:
            failures.append(("mod", diff))
        lines = []
        for side in ("py", "rs"):
            try:
                with open(os.path.join(root, side, "decisions.jsonl"), encoding="utf-8") as fh:
                    lines.append([scrub(json.loads(x)) for x in fh])
            except OSError:
                lines.append(None)
        if lines[0] != lines[1] or not lines[0]:
            failures.append(("decisions.jsonl", "%r vs %r" % tuple(lines)))

        # What both kept: the queues (the gone session's prompt, the busy
        # session's second, the waiting session's) and the events.
        states = []
        for side in ("py", "rs"):
            with open(os.path.join(root, side, "state.json"), encoding="utf-8") as fh:
                state = json.load(fh)
            states.append(scrub({k: state.get(k) for k in ("queue", "events", "sessions")}))
        diff = first_difference(states[0], states[1], "state")
        if diff:
            failures.append(("state.json", diff))
        q = states[0]["queue"] or {}
        print("left queued: %s" % {k[:8]: [i["prompt"] for i in v] for k, v in q.items()})
        if not any(i.get("as_user") for i in q.get(WAITS, [])):
            failures.append(("as_user", "the window's prompt was not marked as the person's own"))
        qa, qb = scrub(get(PY_PORT, "/api/queue")), scrub(get(RS_PORT, "/api/queue"))
        diff = first_difference(qa, qb, "/api/queue")
        if diff:
            failures.append(("/api/queue", diff))
    finally:
        for p in procs:
            p.terminate()
        for p in procs:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()
        shutil.rmtree(root, ignore_errors=True)
    print("inbox_twin: %d POSTs to each" % sent)
    for what, diff in failures:
        print("DIFF %s\n     %s" % (what, diff))
    print("inbox_twin: %s" % ("identical" if not failures else "%d differences" % len(failures)))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
