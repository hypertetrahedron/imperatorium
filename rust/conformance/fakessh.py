"""A stand-in for `ssh` (IMPERATORIUM_SSH), for remote_twin.py: no network,
no host. It answers the commands the dispatcher sends a host the way a host
with one live session would, and records what it was asked to deliver.

    fakessh.py [ssh options] HOST [REMOTE COMMAND]

FAKESSH_DIR holds `log.jsonl` (one record per call: host, command, stdin)
and `sha256` (the hash a `uname -m` probe reports for the host's binary).
"""
import json
import os
import sys
import time

DIR = os.environ["FAKESSH_DIR"]
SID = "11111111-2222-4333-8444-555555555555"
SOCKET = "/tmp/claude-fake/inbox.sock"
CWD = "/home/fake/projects/fakeproj"


def log(rec):
    with open(os.path.join(DIR, "log.jsonl"), "a", encoding="utf-8") as fh:
        fh.write(json.dumps(rec) + "\n")


def main():
    args = sys.argv[1:]
    rest = []
    i = 0
    while i < len(args):
        a = args[i]
        if a in ("-o", "-R", "-L"):
            i += 2
            continue
        rest.append(a)
        i += 1
    tunnel = "-N" in rest
    rest = [a for a in rest if a != "-N"]
    host, cmd = rest[0], " ".join(rest[1:])
    if tunnel:
        log({"host": host, "tunnel": True})
        time.sleep(3600)
        return 0
    if "remote-channel" in cmd:
        log({"host": host, "cmd": "remote-channel"})
        for line in sys.stdin:
            req = json.loads(line)
            if req.get("op") == "read":
                out = {"id": req["id"], "ok": True, "title": "Fake remote work",
                       "items": [{"kind": "user", "text": "hello from the host"},
                                 {"kind": "assistant", "text": "hi"}]}
            else:
                out = {"id": req["id"], "ok": True, "stats": {s: {
                    "context_tokens": 4242, "model_id": "claude-sonnet-5",
                    "agents": [], "shells": [], "monitors": [], "wakeups": [],
                    "cache": {"expires_at": 1.0, "ttl": 300}} for s in req["sessions"]}}
            sys.stdout.write(json.dumps(out) + "\n")
            sys.stdout.flush()
        return 0
    data = sys.stdin.buffer.read() if not sys.stdin.isatty() else b""
    log({"host": host, "cmd": cmd, "stdin": data.decode("utf-8", "replace")})
    if "agents --json" in cmd:
        row = {"sessionId": SID, "pid": 4242, "cwd": CWD, "kind": "interactive",
               "name": "fakeproj-ab", "status": "idle"}
        rec = {"pid": 4242, "sessionId": SID, "cwd": CWD, "kind": "interactive",
               "name": "fakeproj-ab", "status": "idle",
               "messagingSocketPath": SOCKET, "statusUpdatedAt": 1794600000000}
        sys.stdout.write(json.dumps([row]) + "\n\n--- imperatorium session files ---\n"
                         + json.dumps(rec) + "\n")
        return 0
    if cmd.startswith("uname -m"):
        with open(os.path.join(DIR, "sha256"), encoding="utf-8") as fh:
            sys.stdout.write("x86_64\n%s  /home/fake/.claude/imperatorium/bin/imperatorium\n"
                             % fh.read().strip())
        return 0
    if "inbox-send" in cmd:
        return 0
    if "--bg" in cmd:
        sys.stdout.write("Started background session 5ac0ffee in fakeproj\n")
        return 0
    sys.stderr.write("fakessh: unexpected command: %s\n" % cmd)
    return 3


if __name__ == "__main__":
    sys.exit(main())
