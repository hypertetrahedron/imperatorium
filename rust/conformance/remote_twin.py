"""The binary's remote hosts, end to end against a fake ssh (fakessh.py):
rows from a host, its hooks through the tunnel's hook-only listener, the
credential they bring, injection, the queue drained by a remote Stop, a
remote conversation and Matrix tile, the host-qualified catalogue, and
background dispatch over there. Nothing leaves this machine.

Each expectation is what dispatcher.py does with the same inputs (its
remote paths: Control._remote_with_hooks, on_hook(host=), _deliver,
dispatch's remote branch, catalogue).
"""
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from twin import binary, require_free  # noqa: E402

PORT, HOOKS = 8816, 8817
SID = "11111111-2222-4333-8444-555555555555"
SOCKET = "/tmp/claude-fake/inbox.sock"
KEY = "fakehost/" + SID
PY = sys.executable


def call(port, path, body=None):
    req = urllib.request.Request("http://127.0.0.1:%d%s" % (port, path))
    if body is not None:
        req.data = json.dumps(body).encode()
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=30) as fh:
            return fh.status, json.loads(fh.read() or b"{}")
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read() or b"{}")


def main():
    failures = []

    def check(name, cond, got=None):
        print("%-58s %s" % (name, "ok" if cond else "FAIL"))
        if not cond:
            failures.append(name)
            if got is not None:
                print("    got: %s" % json.dumps(got)[:600])

    tmp = tempfile.mkdtemp(prefix="imp-remote-twin-")
    home, fake = os.path.join(tmp, "home"), os.path.join(tmp, "fake")
    builds = os.path.join(tmp, "builds", "x86_64-unknown-linux-musl")
    for d in (home, fake, builds):
        os.makedirs(d)
    # Any bytes will do for the "build": the fake host reports their hash.
    with open(os.path.join(builds, "imperatorium"), "wb") as fh:
        fh.write(b"not really an ELF")
    import hashlib
    with open(os.path.join(fake, "sha256"), "w") as fh:
        fh.write(hashlib.sha256(b"not really an ELF").hexdigest())
    with open(os.path.join(home, "config.json"), "w") as fh:
        json.dump({"remote": {"hosts": [{"name": "fakehost", "label": "Fake",
                                          "tunnel": {"local_port": HOOKS}}]},
                   "roots": [tmp]}, fh)
    shim = os.path.join(tmp, "ssh.cmd")
    with open(shim, "w") as fh:
        fh.write('@"%s" "%s" %%*\r\n' % (PY, os.path.join(HERE, "fakessh.py")))
    env = dict(os.environ, IMPERATORIUM_SSH=shim, FAKESSH_DIR=fake,
               IMPERATORIUM_REMOTE_BUILDS=os.path.join(tmp, "builds"))
    for k in [k for k in env if k.startswith("CLAUDE")]:
        del env[k]
    require_free(PORT, HOOKS)
    proc = subprocess.Popen([binary(), "serve", "--port", str(PORT), "--home", home,
                             "--state", os.path.join(tmp, "state.json")],
                            env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    log = os.path.join(fake, "log.jsonl")

    def calls():
        try:
            with open(log, encoding="utf-8") as fh:
                return [json.loads(l) for l in fh if l.strip()]
        except OSError:
            return []

    try:
        row = None
        for _ in range(60):
            time.sleep(0.5)
            try:
                rows = call(PORT, "/api/sessions")[1].get("sessions") or []
            except OSError:
                continue
            row = next((r for r in rows if r.get("remote")), None)
            if row:
                break
        check("a host's session is on the board", bool(row))
        if not row:
            return 1
        check("keyed by host, labelled, remote",
              row.get("key") == KEY and row.get("host") == "fakehost"
              and row.get("label") == "Fake" and row.get("remote") is True, row)
        check("socket from the host's session file", row.get("socket") == SOCKET, row)
        check("not dispatchable before its hooks bring a token",
              row.get("dispatchable") is False, row)
        check("work from the host's stats channel",
              (row.get("work") or {}).get("context_tokens") == 4242, row.get("work"))

        hosts = call(PORT, "/api/hosts")[1]
        check("/api/hosts lists it with its tunnel",
              hosts["hosts"][0]["name"] == "fakehost"
              and hosts["hosts"][0]["tunnel_status"]["local_port"] == HOOKS
              and hosts["status"]["fakehost"]["fails"] == 0, hosts)

        # The host's hooks, through the hook-only listener.
        code, body = call(HOOKS, "/api/dispatch", {"text": "x"})
        check("the hook listener refuses everything but /hook", code == 404, body)
        start = {"hook_event_name": "SessionStart", "session_id": SID, "cwd": "/home/fake/projects/fakeproj",
                 "messaging_socket": SOCKET, "messaging_token": "remote-tok",
                 "session_name": "fakeproj-ab"}
        check("SessionStart through the tunnel", call(HOOKS, "/hook", start)[1].get("ok") is True)
        time.sleep(2.5)
        rows = call(PORT, "/api/sessions")[1]["sessions"]
        row = next(r for r in rows if r.get("key") == KEY)
        check("the token makes it dispatchable", row.get("dispatchable") is True, row)
        check("the token never leaves in /api/sessions", "token" not in row, row)
        ev = call(PORT, "/api/events")[1]["events"][0]
        check("its event names the host",
              ev.get("kind") == "session_start" and ev.get("host") == "fakehost"
              and ev.get("session_id") == SID, ev)
        # A local hook with the same id must not touch the remote record.
        call(PORT, "/hook", {"hook_event_name": "SessionEnd", "session_id": SID})
        state = json.load(open(os.path.join(tmp, "state.json"), encoding="utf-8"))
        check("a local SessionEnd cannot forget the host's session",
              KEY in (state.get("sessions") or {}), list((state.get("sessions") or {}).keys()))

        # Injection by session.
        before = len(calls())
        r = call(PORT, "/api/dispatch", {"session_id": SID, "prompt": "do the thing",
                                         "confirm": True, "mode": "auto"})[1]
        check("dispatch to the remote session injects", r.get("ok") and r.get("action") == "inject"
              and r.get("host") == "fakehost", r)
        sent = [c for c in calls()[before:] if "inbox-send" in (c.get("cmd") or "")]
        frames = [json.loads(l) for l in (sent[0]["stdin"] if sent else "").splitlines() if l]
        check("inbox-send to the session's socket, quoted",
              bool(sent) and sent[0]["cmd"].endswith("inbox-send /tmp/claude-fake/inbox.sock"), sent)
        check("frames: the token first, then the prompt with its origin",
              len(frames) == 2 and frames[0] == {"type": "auth", "token": "remote-tok"}
              and frames[1]["message"]["content"].endswith("do the thing"), frames)
        check("the token is never on the command line",
              not any("remote-tok" in (c.get("cmd") or "") for c in calls()))
        ev = call(PORT, "/api/events")[1]["events"][0]
        check("dispatch_inject event names the host",
              ev.get("kind") == "dispatch_inject" and ev.get("host") == "fakehost", ev)

        # Queue, drained by the host's Stop.
        q = call(PORT, "/api/queue", {"session_id": SID, "prompt": "after this turn"})[1]
        row = next(r for r in call(PORT, "/api/sessions")[1]["sessions"] if r.get("key") == KEY)
        check("queued under the host-qualified key",
              q.get("ok") is not False and (row.get("queued") or 0) >= 0, q)
        before = len(calls())
        stop = call(HOOKS, "/hook", {"hook_event_name": "Stop", "session_id": SID,
                                     "cwd": "/home/fake/projects/fakeproj",
                                     "last_assistant_message": "done"})[1]
        later = [c for c in calls()[before:] if "inbox-send" in (c.get("cmd") or "")]
        check("a queued prompt goes out on the host's Stop (or was sent at once)",
              bool(later) or not q.get("waiting"), {"stop": stop, "queue": q})

        # Conversation and Matrix.
        conv = call(PORT, "/api/conversation?session=" + SID)[1]
        check("conversation read on the host",
              conv.get("remote") is True and conv.get("title") == "Fake remote work"
              and len(conv.get("items") or []) == 2 and conv.get("sig"), conv)
        again = call(PORT, "/api/conversation?session=%s&known=%s" % (SID, conv.get("sig")))[1]
        check("an unchanged remote conversation is answered by its sig",
              again.get("unchanged") is True, again)
        for path, want in (("/api/changes?session=" + SID, "changes are only read on this machine"),
                           ("/api/activity?session=" + SID, "only through tool hooks"),
                           ("/api/session_status?session=" + SID, "report only what their tile shows")):
            body = call(PORT, path)[1]
            check("refused as Python does: " + path.split("?")[0],
                  want in json.dumps(body), body)
        code, body = call(PORT, "/api/focus", {"session_id": SID})
        check("focus refuses a remote session", "no window here to raise" in json.dumps(body), body)

        # Catalogue and dispatch by project.
        projects = call(PORT, "/api/projects")[1].get("projects") or []
        proj = next((p for p in projects if p.get("host") == "fakehost"), None)
        check("the catalogue has the host-qualified project",
              bool(proj) and proj["name"] == "fakehost/fakeproj" and proj["source"] == "remote", proj)
        dry = call(PORT, "/api/dispatch", {"project": "fakehost/fakeproj", "prompt": "x",
                                           "mode": "dry-run"})[1]
        check("dry run resolves it on the host",
              dry.get("ok") and (dry.get("project") or {}).get("host") == "fakehost", dry)
        bg = call(PORT, "/api/dispatch", {"project": "fakehost/fakeproj", "path": "/home/fake/projects/fakeproj",
                                          "host": "fakehost", "prompt": "run it over there",
                                          "confirm": True, "mode": "background"})[1]
        check("background dispatch starts claude --bg on the host",
              bg.get("ok") and bg.get("action") == "remote" and bg.get("bg_id") == "5ac0ffee", bg)
        bgc = [c for c in calls() if "--bg" in (c.get("cmd") or "")]
        check("in the project folder, the claude path left to expand",
              bool(bgc) and bgc[-1]["cmd"].startswith("cd /home/fake/projects/fakeproj && $HOME/.local/bin/claude --bg"),
              bgc[-1:] )
        check("a tunnel was started (ssh -N -R to the host)", any(c.get("tunnel") for c in calls()))
        refresh = call(PORT, "/api/remote/refresh", {})[1]
        check("/api/remote/refresh", refresh == {"ok": True, "hosts": 1}, refresh)
        call(HOOKS, "/hook", {"hook_event_name": "SessionEnd", "session_id": SID})
        state = json.load(open(os.path.join(tmp, "state.json"), encoding="utf-8"))
        check("the host's SessionEnd forgets its record",
              KEY not in (state.get("sessions") or {}), list((state.get("sessions") or {}).keys()))
    finally:
        proc.kill()
        proc.wait()
    print("remote_twin: %s" % ("all passed" if not failures else "%d failed" % len(failures)))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
