#!/usr/bin/env python3
"""Writes the synthetic Claude Code transcript corpus beside this file.

Everything here is invented. The output is deterministic (fixed 2026
timestamps, derived ids, no wall clock) and written in binary mode, so the
bytes are exact. Layout: projects/<folder>/<session-uuid>.jsonl, plus
projects/<folder>/<session-uuid>/subagents/agent-*.jsonl. Point
CLAUDE_CONFIG_DIR at this folder and every reader finds it.

    python make_corpus.py
"""
import json
import os
import shutil
from datetime import datetime, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "projects")
BASE = 1767225600  # 2026-01-01T00:00:00Z

A = ("C--work-synthetic-alpha", "C:\\work\\synthetic-alpha")
B = ("C--work-synthetic-beta", "C:\\work\\synthetic-beta")
C = ("C--work-synthetic-gamma", "C:\\work\\synthetic-gamma")
KEEP = object()
ORIGIN_NOTE = (
    "[Delivered by Imperatorium, the session dispatcher on this machine - "
    "there is no sending session behind it. Reply here, in this session's own "
    "conversation, as you would to a typed prompt; do not use SendMessage to "
    "answer it.]")
OPUS, SONNET, HAIKU = "claude-opus-4-1", "claude-sonnet-5-5", "claude-haiku-4-5"
FILES = []  # (relative path, bytes)


def iso(sec, ms=0, style="z"):
    d = datetime.fromtimestamp(sec, timezone.utc)
    if style == "plain":
        return d.strftime("%Y-%m-%dT%H:%M:%SZ")
    if style == "micro":
        return d.strftime("%Y-%m-%dT%H:%M:%S") + ".%06dZ" % (ms * 1000 + 123)
    if style == "offset":
        return (datetime.fromtimestamp(sec + 3600, timezone.utc).strftime("%Y-%m-%dT%H:%M:%S")
                + ".%03d+01:00" % ms)
    return d.strftime("%Y-%m-%dT%H:%M:%S") + ".%03dZ" % ms


def U(i=10, o=20, cr=0, cw=0, tier="5m"):
    return {"input_tokens": i, "cache_creation_input_tokens": cw, "cache_read_input_tokens": cr,
            "output_tokens": o,
            "cache_creation": {"ephemeral_5m_input_tokens": cw if tier == "5m" else 0,
                               "ephemeral_1h_input_tokens": cw if tier == "1h" else 0},
            "service_tier": "standard"}


def TU(cid, name, inp):
    return {"type": "tool_use", "id": cid, "name": name, "input": inp}


def TX(text):
    return {"type": "text", "text": text}


def TH(text):
    return {"type": "thinking", "thinking": text, "signature": "c2lnbmF0dXJl"}


def TR(cid, content, err=False):
    b = {"type": "tool_result", "tool_use_id": cid, "content": content}
    if err:
        b["is_error"] = True
    return b


def note(tid, status, summary="A background task finished"):
    return ("<task-notification>\n<task-id>%s</task-id>\n<status>%s</status>\n<summary>%s</summary>\n"
            "</task-notification>" % (tid, status, summary))


def mate(name, body):
    if not isinstance(body, str):
        body = json.dumps(body)
    return ('Another Claude session sent a message:\n<teammate-message teammate_id="%s" color="blue">\n%s\n'
            '</teammate-message>\n\nThis came from another Claude session.' % (name, body))


class S:
    def __init__(self, no, proj, start, **opt):
        self.no, self.folder, self.cwd = no, proj[0], proj[1]
        self.clock, self.n, self.last = BASE + start, 0, None
        self.sid = "5e55%04x-0000-4000-8000-%012x" % (no, no)
        self.opt = opt
        self.branch = opt.get("branch", "main")
        self.eol = opt.get("eol", "\n")
        self.ascii = opt.get("ascii", False)
        self.spaced = opt.get("spaced", False)
        self.tsstyle = opt.get("ts", "z")
        self.fname = opt.get("fname", self.sid + ".jsonl")
        self.lines = []
        self.subs = []
        self.version = "2.1.0"

    # -- plumbing
    def ser(self, o):
        seps = (", ", ": ") if self.spaced else (",", ":")
        return json.dumps(o, ensure_ascii=self.ascii, separators=seps).encode("utf-8")

    def nid(self):
        self.n += 1
        return "%08x-%04x-4000-8000-%012x" % (self.no, self.n, self.n)

    def tick(self, step=7):
        self.clock += step
        return iso(self.clock, (self.n * 37) % 1000, self.tsstyle)

    def put(self, typ, body=None, parent=KEEP, uuid=None, advance=True, side=False, ts=None,
            nots=False, **top):
        u = uuid if uuid is not None else self.nid()
        p = self.last if parent is KEEP else parent
        e = {"parentUuid": p, "isSidechain": side, "userType": "external", "cwd": self.cwd,
             "sessionId": self.sid, "version": self.version, "gitBranch": self.branch, "type": typ}
        e.update(body or {})
        e.update(top)
        e["uuid"] = u
        if not nots:
            e["timestamp"] = ts or self.tick()
        self.lines.append(self.ser(e))
        if advance:
            self.last = u
        return u

    def raw(self, data):
        self.lines.append(data if isinstance(data, bytes) else data.encode("utf-8"))

    def meta(self, typ, **kw):
        o = {"type": typ}
        o.update(kw)
        o["sessionId"] = self.sid
        self.lines.append(self.ser(o))

    # -- entries
    def user(self, content, **kw):
        if isinstance(content, str) and "promptId" not in kw:
            kw["promptId"] = "prompt-%d-%d" % (self.no, self.n + 1)
        return self.put("user", {"message": {"role": "user", "content": content}}, **kw)

    def asst(self, blocks, mid=None, usage=True, model=SONNET, split=True, raw=None, **kw):
        if isinstance(blocks, dict):
            blocks = [blocks]
        if mid is None:
            mid = "msg_%02d%03d" % (self.no, self.n + 1)
        if usage is True:
            usage = U(100 + self.n, 20 + self.n, 50, 0)
        groups = [[b] for b in blocks] if split else [blocks]
        u = None
        for g in groups:
            m = {"type": "message", "role": "assistant", "model": model,
                 "content": g if raw is None else raw,
                 "stop_reason": "end_turn" if g is groups[-1] else None}
            if mid is not False:
                m = dict({"id": mid}, **m)
            if usage is not None:
                m["usage"] = usage
            u = self.put("assistant", {"message": m}, requestId="req_%d_%d" % (self.no, self.n + 1), **kw)
        return u

    def text(self, t, **kw):
        return self.asst([TX(t)], **kw)

    def result(self, cid, content, tur=None, err=False, **kw):
        if tur is not None:
            kw["toolUseResult"] = tur
        return self.put("user", {"message": {"role": "user", "content": [TR(cid, content, err)]}}, **kw)

    def call(self, cid, name, inp, content="ok", tur=None, err=False, **kw):
        self.asst([TU(cid, name, inp)], **kw)
        return self.result(cid, content, tur, err)

    def system(self, subtype, **kw):
        return self.put("system", {"subtype": subtype}, **kw)

    def attach(self, att, **kw):
        return self.put("attachment", {"attachment": att}, **kw)

    def data(self):
        eol = self.eol.encode()
        d = eol.join(self.lines) + (b"" if self.opt.get("trunc") else eol)
        post = self.opt.get("post")
        return post(d) if post else d

    def add(self):
        FILES.append(("projects/%s/%s" % (self.folder, self.fname), self.data()))
        for name, lines in self.subs:
            FILES.append(("projects/%s/%s/subagents/%s" % (self.folder, self.sid, name),
                          b"\n".join(lines) + b"\n"))


def turn(s, prompt, reply, **kw):
    s.user(prompt)
    s.text(reply, **kw)


# --------------------------------------------------------------------------
def s01():
    s = S(1, A, 60)
    s.meta("permission-mode", permissionMode="default")
    s.meta("file-history-snapshot", messageId="m0", snapshot={"trackedFileBackups": {}}, isSnapshotUpdate=False)
    s.user("<caveat>Caveats about local commands.</caveat>", isMeta=True)  # skipped as a first prompt
    s.user("Please add a retry to the fetch helper in client.py")
    s.asst([TH("The helper lives in client.py. I should read it first, then wrap the call.")])
    s.text("I'll start by reading the file.")
    s.call("toolu_r1", "Read", {"file_path": "C:\\work\\synthetic-alpha\\client.py"},
           "     1\tdef fetch(url):\n     2\t    return get(url)\n", {"type": "text", "file": {"filePath": "client.py"}})
    s.call("toolu_g1", "Grep", {"pattern": "def fetch", "path": "C:\\work\\synthetic-alpha"},
           [TX("client.py:1:def fetch(url):"), TX("tests.py:4:def fetch_twice():")])
    s.call("toolu_e1", "Edit", {"file_path": "client.py", "old_string": "return get(url)",
                                "new_string": "for _ in range(3):\n        try:\n            return get(url)\n        except IOError:\n            pass"},
           "The file client.py has been updated.", {"filePath": "client.py", "oldString": "x", "newString": "y"})
    s.call("toolu_w1", "Write", {"file_path": "retry_notes.txt", "content": "Retries: 3\n"}, "File created successfully")
    s.call("toolu_m1", "MultiEdit", {"file_path": "tests.py", "edits": [
        {"old_string": "a = 1", "new_string": "a = 2"}, {"old_string": "", "new_string": "b = 3"},
        {"old_string": "c = 4", "new_string": ""}, "not-an-edit"]}, "Applied 3 edits")
    s.call("toolu_t1", "TodoWrite", {"todos": [{"content": "Add retry", "status": "completed", "activeForm": "Adding retry"},
                                               {"content": "Run tests", "status": "in_progress", "activeForm": "Running tests"}]},
           "Todos have been modified successfully", {"oldTodos": [], "newTodos": []})
    s.call("toolu_b1", "Bash", {"command": "python -m unittest", "description": "Run the unit tests"},
           "Ran 3 tests in 0.01s\n\nOK", {"stdout": "Ran 3 tests", "stderr": "", "interrupted": False})
    s.asst([TX("Done: the helper now retries three times. Tests pass.")], usage=U(400, 60, 3000, 120))
    s.system("turn_duration", durationMs=4200)
    s.system("informational", content="Context at 12%", level="info")
    s.put("progress", {"data": {"type": "hook_progress", "hookEvent": "PostToolUse"}}, toolUseID="toolu_b1")
    s.user([TX("<system-reminder>Plan mode is off.</system-reminder>"), TX("Now document it in the README")])
    s.text("README updated.")
    s.meta("last-prompt", lastPrompt="Now document it in the README")
    s.add()


def s02():
    s = S(2, A, 120, spaced=True, ascii=True, ts="offset")
    s.meta("ai-title", aiTitle="Retry \"fetch\" helper \u2014 tidy \\ up")
    turn(s, "Rename the \"helper\" and add caf\u00e9 \U0001F600 support", "Done \u2713 \U0001F680")
    s.meta("custom-title", customTitle="Draft \u00e9tude")
    turn(s, "one more", "ok")
    s.meta("custom-title", customTitle="Retry & \"quotes\" \\ final \u65e5\u672c")
    s.raw('{"type":"tag","tag":"release-2026","sessionId":"%s"}' % s.sid)
    s.raw('{"type":"tag","tag":"final-tag","sessionId":"%s"}' % s.sid)
    s.meta("summary", summary="ignored because a custom title exists", leafUuid="x")
    s.add()


def s03():
    s = S(3, A, 180)
    s.user("Plan the migration of the ledger tables")
    s.asst([TU("toolu_a", "Bash", {"command": "ls"})])
    s.result("toolu_a", "ledger.sql")
    s.text("Plan drafted.")
    s.user("Now execute step one")
    a2 = s.text("Step one executed.")
    for k in (1, 2):
        s.put("system", {"subtype": "compact_boundary", "content": "Conversation compacted",
                         "logicalParentUuid": a2,
                         "compactMetadata": {"trigger": "auto", "preTokens": 150000 * k}}, parent=None)
        s.user("This session is being continued from a previous conversation. Summary %d: ledger tables migrated." % k,
               isCompactSummary=True, isVisibleInTranscriptOnly=True)
        s.text("Continuing after compaction %d." % k)
        s.user("Next task %d" % k)
        a2 = s.text("Task %d done." % k)
    s.add()


def s04():
    s = S(4, A, 240)
    turn(s, "hello from the terminal", "Hi there.")
    wrap = ("Another Claude session sent a message:\n" + ORIGIN_NOTE
            + "\n\nPlease run the tests\n\nThis came from another Claude session (id x).")
    s.user(wrap, isMeta=True, origin={"kind": "peer", "from": "5e550000-0000-4000-8000-00000000ffff"})
    s.text("Running the tests now.")
    s.user([TX("Another Claude session sent a message:\nsecond block"), TX("Check lint too")], isMeta=True,
           origin={"kind": "peer"})
    s.text("Lint checked.")
    s.user("Another Claude session sent a message:   \nPlain peer text without a tail", isMeta=True,
           origin={"kind": "peer"})
    s.user("This came from another Claude session only", isMeta=True, origin={"kind": "peer"})
    s.user("Subagent report: the audit is complete", isMeta=True, origin={"kind": "peer", "senderTaskId": "agent-7"})
    s.user("Hidden meta without origin", isMeta=True)
    s.user("Peer on a sidechain", isMeta=True, side=True, origin={"kind": "peer"})
    s.meta("queue-operation", operation="enqueue", content="also check lint", timestamp=iso(s.clock))
    s.attach({"type": "queued_command", "prompt": "also check lint", "commandMode": "prompt"})
    s.meta("queue-operation", operation="dequeue", timestamp=iso(s.clock))
    s.attach({"type": "queued_command", "prompt": note("bg9", "completed", "Build finished"),
              "origin": {"kind": "task-notification"}})
    s.attach({"type": "queued_command", "prompt": note("bg8", "failed", "Build failed"),
              "commandMode": "task-notification"})
    s.attach({"type": "queued_command", "prompt": [TX("list prompt"), {"type": "image", "source": {}}],
              "origin": {"kind": "peer", "senderTaskId": "agent-9"}})
    s.attach({"type": "queued_command", "prompt": ""})
    s.attach({"type": "queued_command", "prompt": "sidechain queued"}, side=True)
    s.attach({"type": "queued_command", "prompt": None})
    s.attach({"type": "edited_text_file", "filename": "a.py", "snippet": "x = 1"})
    s.attach({"type": "todo_reminder", "content": []})
    s.attach("not-a-dict")
    s.user(note("bg7", "completed", "Tests passed"), origin={"kind": "task-notification"})
    s.text("Noted.")
    s.user("[Delivered] " + ORIGIN_NOTE + "\n\nFrom the window", isMeta=True, origin={"kind": "peer"})
    s.user(ORIGIN_NOTE + "\n\nTyped in the window", isMeta=True, origin={"kind": "peer"})
    s.text("Replied to the window.")
    s.add()


def s05():
    s = S(5, A, 300)
    s.user("<local-command-stdout>Set model to opus</local-command-stdout>")
    s.user("<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args></command-args>")
    s.user("[Request interrupted by user for tool use]")
    s.user("<session-start-hook>hook output</session-start-hook>")
    s.user("<tick>12:00</tick>")
    s.user("<goal>ship it</goal>")
    s.user("<ide_opened_file>The user opened a.py</ide_opened_file>")
    s.user("<ide_selection>x = 1</ide_selection>")
    s.user("   \n  ")
    s.user([{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}},
            TX("  A real prompt\nspread over lines  ")])
    s.text("Understood.")
    s.user("<local-command-stdout>Compacted (ctrl+o to see full summary)</local-command-stdout>")
    s.user("  <local-command-stderr>warning: nothing to do</local-command-stderr>")
    s.user("<task-notification>\n<task-id>t1</task-id>\n<status>completed</status>\n"
           "<summary>  Linted 12 files  </summary>\n</task-notification>")
    s.user("<task-notification>no summary here</task-notification>")
    s.user("<bash-input>ls -la</bash-input>")
    s.user("<bash-stdout>total 0</bash-stdout><bash-stderr></bash-stderr>")
    s.user("Plain follow-up after the notes")
    s.text("Done.")
    s.add()


def s06():
    s = S(6, B, 360)
    s.user("Audit the settings module with helpers")
    a = s.text("Spawning a helper.")
    s.asst([TU("toolu_s1", "Task", {"description": "Audit settings", "prompt": "Read settings.py",
                                    "subagent_type": "general"})])
    s.put("user", {"message": {"role": "user", "content": "Read settings.py"}}, side=True, agentId="a1b2c3")
    s.put("assistant", {"message": {"id": "msg_side1", "type": "message", "role": "assistant", "model": HAIKU,
                                    "content": [TX("Reading the settings.")], "usage": U(30, 10, 0, 0)}},
          side=True, agentId="a1b2c3")
    s.put("user", {"message": {"role": "user", "content": "A teammate line"}}, teamName="crew", parent=a)
    s.put("assistant", {"message": {"id": "msg_team1", "type": "message", "role": "assistant", "model": HAIKU,
                                    "content": [TX("Team reply")]}}, teamName="crew", parent=a)
    s.result("toolu_s1", [TX("Audit found two issues")],
             {"status": "completed", "agentId": "a1b2c3", "totalTokens": 99}, parent=a)
    s.text("The helper found two issues.")
    s.put("assistant", {"message": {"id": "msg_side2", "type": "message", "role": "assistant", "model": HAIKU,
                                    "content": [TX("Sidechain leaf later in the file")], "usage": U(5, 5)}},
          side=True, parent=a, advance=False)
    for aid in ("a1b2c3", "d4e5f6"):
        ss = S(6, B, 400)
        ss.user("Subagent task %s" % aid, side=True, agentId=aid)
        ss.asst([TX("Subagent %s working" % aid), TU("toolu_x%s" % aid, "Read", {"file_path": "settings.py"})],
                side=True, agentId=aid, usage=U(20, 8, 0, 0), model=HAIKU)
        ss.result("toolu_x%s" % aid, "settings text", side=True, agentId=aid)
        ss.text("Subagent %s finished" % aid, side=True, agentId=aid)
        s.subs.append(("agent-%s.jsonl" % aid, ss.lines))
    s.add()
    t = S(29, B, 420)  # a session whose first line is a sidechain: not listed
    t.user("Sidechain-first session", side=True)
    t.text("Still sidechain", side=True)
    t.add()


def s07():
    s = S(7, B, 1900)
    s.user("Run the build and the watchers in the background")
    s.call("toolu_k1", "Bash", {"command": "make   build\n  --verbose", "run_in_background": True,
                                "description": "Build"}, "Started", {"backgroundTaskId": "bg1", "stdout": "", "stderr": ""})
    s.call("toolu_k2", "Bash", {"command": "tail -f server.log", "run_in_background": True},
           "Started", {"backgroundTaskId": "bg2"})
    s.call("toolu_k3", "PowerShell", {"command": "Get-Content app.log -Wait " + "x" * 120, "run_in_background": True},
           "Started", {"backgroundTaskId": "bg3"})
    s.call("toolu_k4", "Bash", {"prompt": "only a prompt label", "run_in_background": True}, "Started",
           {"backgroundTaskId": "bg4"})
    s.call("toolu_k5", "Bash", "a plain string input", "Started", {"backgroundTaskId": "bg5"})
    s.call("toolu_k6", "Bash", {"command": "sleep 99"}, "Started", {"backgroundTaskId": "bg6"})
    s.call("toolu_o1", "BashOutput", {"bash_id": "bg1"}, "building...", {"status": "running"})
    s.call("toolu_o2", "KillShell", {"shell_id": "bg2"}, "killed", {"message": "Successfully killed shell"})
    s.call("toolu_o3", "TaskOutput", {"task_id": "bg1"}, "output", {"retrieval_status": "success"})
    s.user(note("bg3", "completed", "Background command \"Get-Content\" completed"))
    s.user(note("bg4", "failed") + "\n" + note("bg5", "killed"))
    s.user(note("bg6", "running"))
    s.call("toolu_ts", "TaskStop", {"task_id": "bg1"}, "stopped", {"task_id": "bg1", "message": "Stopped"})
    s.asst([TU("toolu_fg1", "Bash", {"command": "npm test", "description": "Foreground test run"})])
    s.asst([TU("toolu_fg2", "PowerShell", {"command": "Get-ChildItem"})], mid="msg_fg2")
    s.asst([TU("toolu_fg3", "Read", {"file_path": "a"})], mid="msg_fg3")
    s.add()


def s08():
    s = S(8, B, 1950)
    s.user("Watch the logs")
    s.call("toolu_n1", "Monitor", {"description": "Tail errors", "command": "tail -f err.log", "timeout_ms": 300000},
           "Monitor started", {"taskId": "mon1", "timeoutMs": 300000, "persistent": False})
    s.call("toolu_n2", "Monitor", {"description": "Forever", "command": "watch"}, "started",
           {"taskId": "mon2", "timeoutMs": 60000, "persistent": True})
    s.call("toolu_n3", "Monitor", {"description": "Bool timeout"}, "started", {"taskId": "mon3", "timeoutMs": True})
    s.call("toolu_n4", "Monitor", {"description": "No timeout"}, "started", {"taskId": "mon4"})
    s.call("toolu_n5", "Monitor", {"description": "Stopped soon"}, "started", {"taskId": "mon5", "timeoutMs": 1800000})
    s.call("toolu_n6", "Monitor", {"description": "Ends by notice"}, "started", {"taskId": "mon6", "timeoutMs": 1800000})
    s.call("toolu_n7", "Monitor", {"description": "Short", "command": "x"}, "started", {"taskId": "mon7", "timeoutMs": 120000})
    s.call("toolu_n8", "Bash", {"command": "echo hi"}, "hi", {"taskId": "not-a-monitor"})
    s.call("toolu_n9", "TaskStop", {"task_id": "mon5"}, "ok", {"task_id": "mon5"})
    s.user(note("mon6", "completed", "Monitor ended"))
    s.user(note("mon7", "stopped"))
    s.add()


def s09():
    s = S(9, B, 2000)
    s.user("Spawn background and foreground agents")
    s.call("toolu_g1", "Task", {"description": "Index the repo", "prompt": "Index it", "subagent_type": "explore"},
           "Launched", {"status": "async_launched", "agentId": "ag-1", "description": "Indexing repo", "resolvedModel": HAIKU})
    s.call("toolu_g2", "Agent", {"prompt": "Summarise the diff in detail please"}, "Launched",
           {"status": "async_launched", "agentId": "ag-2", "resolvedModel": SONNET})
    s.call("toolu_g3", "Agent", {"description": "Foreground review", "prompt": "Review"}, [TX("Review text")],
           {"status": "completed", "agentId": "ag-3", "totalDurationMs": 1200})
    s.call("toolu_g4", "Task", {"description": "Failing helper"}, "Error: agent crashed", {"status": "failed"}, err=True)
    s.call("toolu_g5", "Agent", {"description": "No structured result"}, "plain string result")
    s.call("toolu_g6", "Agent", {"description": "Async w/o id"}, "x", {"status": "async_launched"})
    s.user(note("ag-1", "completed", "Indexed 40 files"))
    s.asst([TU("toolu_g7", "Task", {"description": "Still running foreground", "prompt": "Long job"})])
    s.add()


def s10():
    s = S(10, B, 2050)
    s.user("Start a team")
    for cid, name, aid in (("toolu_t1", "reviewer", "tm-1"), ("toolu_t2", "builder", "tm-2"),
                           ("toolu_t3", "tester", "tm-3"), ("toolu_t4", None, "tm-4"), ("toolu_t5", "ghost", "tm-5")):
        tur = {"status": "teammate_spawned", "agentId": aid, "description": "Teammate %s" % (name or aid),
               "resolvedModel": SONNET}
        if name:
            tur["name"] = name
        s.call(cid, "Agent" if aid != "tm-3" else "Task",
               {"name": name, "prompt": "Work as %s" % name, "team_name": "crew"}, "spawned", tur)
    s.user(mate("reviewer", {"type": "idle_notification", "from": "reviewer", "idleReason": "available"}))
    s.user(mate("builder", {"type": "idle_notification"}))
    s.call("toolu_sm", "SendMessage", {"to": "reviewer", "message": "Please review the diff"}, "sent")
    s.call("toolu_sm2", "SendMessage", {"to": "nobody", "message": "x"}, "sent")
    s.call("toolu_sm3", "SendMessage", "bare string", "sent")
    s.user(mate("reviewer", {"type": "idle_notification"}))
    s.user(mate("builder", {"type": "shutdown_approved", "requestId": "r1"}))
    s.user(mate("tm-4", {"type": "teammate_terminated"}))
    s.user(mate("reviewer", "this is not json"))
    s.user(mate("reviewer", [1, 2]))
    s.user(mate("stranger", {"type": "idle_notification"}))
    s.user(mate("tester", {"type": "message", "text": "progress"}))
    s.user([TX("prefix"), TX(mate("tester", {"type": "idle_notification"}))])
    s.attach({"type": "queued_command", "prompt": mate("ghost", {"type": "idle_notification"})})
    s.call("toolu_st", "TaskStop", {"task_id": "tester"}, "stopped", {"task_id": "tester"})
    s.call("toolu_st2", "TaskStop", {"task_id": "ghost-unknown"}, "stopped", {"task_id": "ghost-unknown"})
    s.add()


def s11():
    s = S(11, B, 2100)
    ms = (BASE + 2100) * 1000
    s.user("Schedule some wakeups")
    s.call("toolu_w1", "ScheduleWakeup", {"delaySeconds": 600, "reason": "check the deploy", "prompt": "/loop"},
           "Scheduled", {"scheduledFor": ms + 600_000})
    s.call("toolu_w2", "ScheduleWakeup", {"delaySeconds": 1800, "reason": "  re-check\n  the   deploy ", "prompt": "/loop"},
           "Scheduled", {"scheduledFor": ms + 1_800_000})
    s.call("toolu_c1", "CronCreate", {"cron": "*/5 * * * *", "prompt": "check inbox", "recurring": True},
           "Created", {"id": "c1", "recurring": True, "nextFireAt": ms + 300_000})
    s.call("toolu_c2", "CronCreate", {"cron": "30 3 * * *", "prompt": "one shot", "recurring": False},
           "Created", {"id": "c2", "recurring": False, "nextFireAt": ms + 900_000})
    s.call("toolu_c3", "CronCreate", {"cron": "0 0 * * *", "description": "past one"}, "Created",
           {"id": "c3", "recurring": False, "nextFireAt": 1000})
    s.call("toolu_c4", "CronCreate", {"cron": "bad"}, "Created", {"id": "c4", "recurring": False})
    s.call("toolu_c5", "CronCreate", {"cron": "* * * * *", "prompt": "recurring no next"}, "Created",
           {"id": "c5", "recurring": True})
    s.call("toolu_cl", "CronList", {}, "c1, c2, c3", {"jobs": []})
    s.call("toolu_cd", "CronDelete", {"id": "c1"}, "Deleted")
    s.call("toolu_cd2", "CronDelete", {"id": "zz"}, "Deleted")
    s.add()
    t = S(12, B, 2150)
    t.user("Stop and restart wakeups")
    t.call("toolu_a", "ScheduleWakeup", {"reason": "first"}, "Scheduled", {"scheduledFor": (BASE + 2500) * 1000})
    t.call("toolu_b", "ScheduleWakeup", {"reason": "stop"}, "Stopped", {"stopped": True})
    t.call("toolu_c", "ScheduleWakeup", {"reason": "again", "prompt": "p"}, "Scheduled",
           {"scheduledFor": (BASE + 2700) * 1000})
    t.call("toolu_d", "ScheduleWakeup", {"reason": "zero"}, "Scheduled", {"scheduledFor": 0})
    t.call("toolu_e", "ScheduleWakeup", {"prompt": "no reason, use the prompt label"}, "Scheduled",
           {"scheduledFor": (BASE + 2900) * 1000})
    t.add()


def s13():  # usage, cache, models (late and 1h cache)
    s = S(13, B, 3500)
    s.user("Count the tokens")
    s.asst([TH("thinking"), TX("first"), TU("toolu_u1", "Bash", {"command": "echo 1"})], mid="msg_dup",
           usage=U(1000, 200, 5000, 700, "5m"), model=OPUS)
    s.result("toolu_u1", "1")
    s.asst([TX("second, different model")], mid="msg_two", usage=U(50, 25, 0, 400, "1h"), model=HAIKU)
    s.asst([TX("no usage at all")], mid="msg_three", usage=None)
    s.asst([TX("empty usage")], mid="msg_four", usage={})
    s.asst([TX("odd usage types")], mid="msg_five",
           usage={"input_tokens": 1.5, "output_tokens": True, "cache_read_input_tokens": "7",
                  "cache_creation_input_tokens": None})
    s.asst([TX("no id one")], mid=False, usage=U(3, 3))
    s.asst([TX("no timestamp")], mid="msg_nots", usage=U(7, 7), nots=True)
    s.put("assistant", {"message": {"id": "msg_side", "role": "assistant", "model": SONNET, "content": [TX("sidechain")],
                                    "usage": U(9000, 100, 0, 0, "1h")}}, side=True, parent=s.last, advance=False)
    s.asst([TX("last main message")], mid="msg_last", usage=U(120, 40, 880, 0, "5m"), model=SONNET)
    s.add()


def s14():  # mcp, hooks, system subtypes
    s = S(14, C, 480)
    s.user("Check the hooks")
    s.attach({"type": "mcp_instructions_delta", "addedNames": ["github", "files", "github", 5], "removedNames": ["nothing"]})
    s.attach({"type": "mcp_instructions_delta", "addedNames": ["db"], "removedNames": ["files"]})
    s.attach({"type": "mcp_instructions_delta", "addedNames": None})
    s.attach({"type": "deferred_tools_delta", "addedNames": ["x"]})
    s.system("turn_duration", durationMs=1500)
    s.system("turn_duration", durationMs=2500.5)
    s.system("turn_duration", durationMs=True)
    s.system("turn_duration", durationMs="slow")
    s.system("turn_duration", durationMs=99, side=True)
    for k in range(12):
        errs = (["hook %d failed with\n   whitespace   inside" % k, {"z": k, "a": {"b": 1}}, "x" * 400]
                if k % 4 == 0 else ["plain error %d" % k])
        s.system("stop_hook_summary", hookCount=len(errs), hookErrors=errs, preventedContinuation=False)
    s.system("stop_hook_summary", hookErrors=None)
    s.system("api_error", error={"status": 529, "message": "overloaded"}, retryAttempt=1, maxRetries=10, level="error")
    s.system("local_command", content="<command-name>/cost</command-name>")
    s.system("bridge_status", content="connected")
    s.meta("permission-mode", permissionMode="acceptEdits")
    s.meta("permission-mode", permissionMode="")
    s.branch = "feature/hooks"
    s.version = "2.1.7"
    s.text("Hooks look fine.")
    s.add()


def s15():  # errors, interrupts, images, odd content
    s = S(15, C, 540)
    s.user("Try the risky things")
    s.asst([TX("API Error: 529 overloaded")], model="<synthetic>", usage=None, isApiErrorMessage=True, error="overloaded")
    s.call("toolu_e1", "Bash", {"command": "false"}, "Error: exit code 1", {"stdout": "", "stderr": "boom"}, err=True)
    s.call("toolu_e2", "Bash", {"command": "sleep 100"}, "[Request interrupted by user for tool use]", err=True)
    s.user("[Request interrupted by user]")
    s.user("[Request interrupted by user for tool use]")
    s.user([{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}},
            TX("What is in this screenshot?")])
    s.asst([TU("toolu_i1", "Read", {"file_path": "shot.png"})])
    s.result("toolu_i1", [{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBO"}},
                          TX("alt text"), "bare string", None, 42, {"type": "weird"}])
    s.call("toolu_i2", "Read", {"file_path": "n.txt"}, None)
    s.call("toolu_i3", "Read", {"file_path": "n.txt"}, {"a": 1})
    s.call("toolu_i4", "Read", {"file_path": "n.txt"}, 12345)
    s.call("toolu_i5", "Grep", "string input", "x")
    s.call("toolu_i6", "Grep", None, "x")
    s.call("toolu_i7", "Grep", {}, "x")
    s.call("toolu_i8", "Foo", {"zeta": 1, "beta": 2, "alpha": 3, "eta": 4, "gamma": 5, "delta": 6, "kappa": 7, "iota": 8}, "x")
    s.call("toolu_i9", "WebFetch", {"url": "https://example.invalid/x", "prompt": "p"}, "page")
    s.call("toolu_ia", "Task", {"description": "d", "prompt": "p"}, "x")
    s.call("toolu_q1", "AskUserQuestion", {"questions": [
        {"question": "Which database?", "header": "DB", "multiSelect": False,
         "options": [{"label": "Postgres", "description": "Relational"}, {"label": "", "description": "dropped"}, "bad"]},
        {"question": "Extras?", "multiSelect": True, "options": None}]}, "User answered",
        {"answers": {"Which database?": "Postgres"}})
    s.call("toolu_q2", "AskUserQuestion", {"questions": [{"question": "  "}]}, "x")
    s.call("toolu_q3", "AskUserQuestion", {"questions": ["not a dict"]}, "x")
    s.call("toolu_q4", "AskUserQuestion", {"questions": []}, "x")
    s.call("toolu_d1", "Edit", {"file_path": "a", "old_string": "", "new_string": ""}, "x")
    s.call("toolu_d2", "Edit", {"file_path": "a", "old_string": "only old"}, "x")
    s.call("toolu_d3", "Edit", {"file_path": "a", "new_string": "only new\nsecond"}, "x")
    s.call("toolu_d4", "Edit", "not a dict", "x")
    s.call("toolu_d5", "MultiEdit", {"file_path": "a", "edits": "nope"}, "x")
    s.asst([TX("x")], raw="assistant content as a string")
    s.asst([None, "str", 5, {"type": "server_tool_use", "name": "web_search"}, {"type": "redacted_thinking", "data": "x"},
            {"type": "text", "text": "   "}, {"type": "text", "text": None}, {"type": "text"},
            {"type": "thinking", "thinking": ""}, {"type": "tool_use"}, {"type": "tool_result"}, TX("kept")], split=False)
    s.put("user", {"message": {"role": "user", "content": None}})
    s.put("user", {"message": {"role": "user", "content": {"type": "text", "text": "dict content"}}})
    s.put("user", {})
    s.put("assistant", {"message": {"content": [TX("no role on this message")]}})
    s.put("user", {"message": {"role": "system", "content": [TX("role system")]}})
    s.put("assistant", {"message": {"role": "user", "content": [TX("assistant entry with user role")]}})
    s.put("user", {"message": {"role": "user", "content": [TR(None, "result without id")]}})
    s.put("user", {"message": {"role": "user", "content": [TX("later timestamp is junk")]}}, ts="yesterday-ish")
    s.text("Finished.")
    s.add()


def s16():  # unicode and escapes
    s = S(16, ("C--work-synthetic-\u00fc", "C:\\work\\synthetic-\u00fc"), 600)
    long_cjk = "\u65e5\u672c\u8a9e" * 66 + "\U0001F600" + "tail"  # code point 199 is astral
    s.user("Caf\u00e9 d\u00e9j\u00e0 vu na\u00efve \u2014 \u4f60\u597d\u4e16\u754c \u65e5\u672c\u8a9e\u306e\u30c6\u30b9\u30c8 "
           "\U0001F680\u2728 \U0001F469\u200d\U0001F4BB")
    s.text("Line one\u2028line two after LS and\u2029line three after PS, then NEL\u0085done, and \u00a0nbsp\u00a0")
    s.user("\u00a0\u3000\u2003Padded by exotic spaces\u2003\u3000\u00a0")
    s.text("\u3000\u3000wide indent and 50\u202f\u202fgap, ZWSP\u200b end")
    s.user(long_cjk)
    s.text("\U0001F600" * 50 + " emoji wall")
    s.call("toolu_u1", "Bash", {"command": "echo '\u00e9\u2028\u2029'"}, "out\u2028put\u2029!", {"stdout": "\u2028"})
    s.call("toolu_u2", "Write", {"file_path": "C:\\work\\synthetic-\u00fc\\\u6587\u4ef6.txt", "content": "\u00e9"}, "ok")
    s.meta("custom-title", customTitle="T\u00edtulo \u2028 con separador \U0001F600")
    s.text("Back to ASCII: quote \" backslash \\ tab\t slash / <tag> & ampersand")
    s.add()


def s17():  # CRLF throughout
    s = S(17, C, 660, eol="\r\n", ts="plain")
    s.meta("custom-title", customTitle="CRLF session")
    turn(s, "A prompt in a CRLF file", "A reply")
    s.call("toolu_c1", "Bash", {"command": "dir"}, "line1\nline2")
    s.raw('{"type":"tag","tag":"crlf","sessionId":"%s"}' % s.sid)
    s.add()


def s18():  # lone CR between records
    def post(d):
        parts = d.split(b"\n")
        out = parts[:3] + [parts[3] + b"\r" + parts[4]] + [parts[5] + b"\r\r"] + parts[6:]
        return b"\n".join(out)
    s = S(18, C, 700, ts="micro", post=post)
    turn(s, "Prompt before lone CR", "Reply one")
    s.user("Prompt two")
    s.text("Reply two")
    s.user("Prompt three")
    s.text("Reply three")
    s.add()


def s19():  # truncated last line
    s = S(19, C, 760, trunc=True)
    turn(s, "A prompt that is complete", "A complete reply")
    s.call("toolu_x1", "Bash", {"command": "echo partial"}, "partial")
    s.asst([TU("toolu_x2", "Bash", {"command": "sleep 1", "run_in_background": True})])
    s.result("toolu_x2", "Started", {"backgroundTaskId": "bgcut"})
    s.text("this last line is cut in half")
    s.lines[-1] = s.lines[-1][:len(s.lines[-1]) // 2]
    s.add()


def s20():  # empty
    s = S(20, A, 0)
    FILES.append(("projects/%s/%s" % (s.folder, s.fname), b""))


def s21():  # junk only
    s = S(21, B, 0)
    junk = [b"this is not json", b'{"type":"user","uuid":"abc"', b"[1,2,3]", b"42", b'"just a string"', b"null",
            b"true", b"{}", b'{"type":"user"}', b'{"type":"user","uuid":5}', b'{"type":"mystery","uuid":"x"}',
            b"", b"   ", b"\t",
            b'\xef\xbb\xbf{"type":"user","uuid":"bom","message":{"role":"user","content":"after a BOM"}}',
            b'{"type":"assistant","uuid":"zz","message":{"content":[]}} trailing junk', b'{"a":1}{"b":2}',
            b"{'single':'quotes'}"]
    FILES.append(("projects/%s/%s" % (s.folder, s.fname), b"\n".join(junk) + b"\n"))


def s22():  # big numbers within i64, floats
    s = S(22, C, 820)
    s.user("Big numbers")
    s.asst([TX("one")], mid="msg_b1", usage={"input_tokens": 9007199254740993, "output_tokens": 4611686018427387904,
                                              "cache_read_input_tokens": 10000000000000000.0,
                                              "cache_creation_input_tokens": 0.1,
                                              "cache_creation": {"ephemeral_5m_input_tokens": 0.1}})
    s.asst([TX("two")], mid="msg_b2", usage={"input_tokens": 1, "output_tokens": 0.2,
                                              "cache_read_input_tokens": 9007199254740993,
                                              "cache_creation_input_tokens": 1e16})
    s.system("turn_duration", durationMs=1e16)
    s.system("turn_duration", durationMs=9007199254740993)
    s.call("toolu_m1", "Monitor", {"description": "huge timeout"}, "x", {"taskId": "monbig", "timeoutMs": 9007199254740993})
    s.call("toolu_s1", "ScheduleWakeup", {"reason": "huge"}, "x", {"scheduledFor": 4000000000000.5})
    s.call("toolu_c1", "CronCreate", {"cron": "x"}, "x", {"id": "cbig", "recurring": False, "nextFireAt": 9007199254740993})
    s.asst([TX("three, last")], mid="msg_b3", usage={"input_tokens": 1e16, "output_tokens": 0.1,
                                                      "cache_read_input_tokens": 3, "cache_creation_input_tokens": 1})
    s.add()


def s23():  # numbers beyond i64 and u64
    s = S(23, C, 880)
    s.user("Numbers beyond 64 bits")
    s.asst([TX("one")], mid="msg_h1", usage={"input_tokens": 5, "output_tokens": 18446744073709551617,
                                              "cache_read_input_tokens": 123456789012345678901234567890,
                                              "cache_creation_input_tokens": 1})
    s.asst([TX("two")], mid="msg_h2", usage={"input_tokens": 1, "output_tokens": 1,
                                              "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0})
    s.system("turn_duration", durationMs=18446744073709551617)
    s.call("toolu_m1", "Monitor", {"description": "beyond u64"}, "x", {"taskId": "monhuge", "timeoutMs": 18446744073709551617})
    s.add()
    t = S(38, C, 900)  # token sums that overflow i64
    t.user("Overflowing sums")
    t.asst([TX("one")], mid="msg_o1", usage={"input_tokens": 9223372036854775807, "output_tokens": 1})
    t.asst([TX("two")], mid="msg_o2", usage={"input_tokens": 9223372036854775807, "output_tokens": 1})
    t.add()


def s24():  # branching, duplicate uuids, odd parents
    s = S(24, A, 940)
    r = s.user("Root prompt")
    a = s.text("Answer A")
    b1 = s.text("Branch one (earlier leaf)")
    b2 = s.text("Branch two (later leaf)", parent=a)
    s.put("progress", {"data": {"type": "bash_progress"}}, parent=b2, advance=False)
    s.put("system", {"subtype": "informational", "content": "terminal system leaf"}, parent=b2, advance=False)
    s.put("user", {"message": {"role": "user", "content": "Meta leaf later in the file"}}, parent=b2, isMeta=True,
          advance=False)
    s.put("assistant", {"message": {"role": "assistant", "content": [TX("Sidechain leaf later")]}}, parent=b2, side=True,
          advance=False)
    s.put("assistant", {"message": {"role": "assistant", "content": [TX("Team leaf later")]}}, parent=b2,
          teamName="crew", advance=False)
    s.put("user", {"message": {"role": "user", "content": "Duplicate of A's uuid, later in the file"}}, uuid=a, parent=r,
          advance=False)
    s.put("user", {"message": {"role": "user", "content": "Empty parent string"}}, parent="", advance=False)
    s.put("user", {"message": {"role": "user", "content": "Numeric parent"}}, parent=5, advance=False)
    s.put("user", {"message": {"role": "user", "content": "Self parent"}}, uuid="00000000-0000-4000-8000-0000000000aa",
          parent="00000000-0000-4000-8000-0000000000aa", advance=False)
    s.put("user", {"message": {"role": "user", "content": "Cycle x"}}, uuid="00000000-0000-4000-8000-0000000000b1",
          parent="00000000-0000-4000-8000-0000000000b2", advance=False)
    s.put("user", {"message": {"role": "user", "content": "Cycle y"}}, uuid="00000000-0000-4000-8000-0000000000b2",
          parent="00000000-0000-4000-8000-0000000000b1", advance=False)
    s.put("assistant", {"message": {"role": "assistant", "content": [TX("Final main leaf, branching from B1")]}},
          parent=b1)
    s.add()
    t = S(25, A, 970)  # orphan: parent points nowhere
    t.user("Rooted prompt")
    t.text("Rooted reply")
    t.user("Orphan prompt", parent="00000000-dead-beef-0000-000000000000")
    t.text("Orphan reply")
    t.add()
    u = S(26, A, 990)  # only sidechain/meta leaves: any leaf is picked
    u.user("Meta prompt", isMeta=True)
    u.text("Sidechain reply", side=True)
    u.text("Team reply", teamName="crew")
    u.add()


def s27():  # compaction links that dangle
    s = S(27, A, 1010)
    s.user("Before the dangling compaction")
    s.text("Reply")
    s.put("system", {"subtype": "compact_boundary", "logicalParentUuid": "00000000-dead-beef-0000-000000000001"},
          parent=None)
    s.user("Summary of nothing", isCompactSummary=True)
    s.text("After")
    s.add()
    t = S(28, A, 1030)  # boundary without any logical parent
    t.user("Before")
    t.text("Reply")
    t.put("system", {"subtype": "compact_boundary"}, parent=None)
    t.user("A summary with no link", isCompactSummary=True)
    t.text("After")
    t.add()
    u = S(30, A, 1050)  # a boundary whose logical parent is already in the chain
    x = u.user("Prompt")
    u.put("system", {"subtype": "compact_boundary", "logicalParentUuid": x}, parent=x)
    u.text("Reply after a self-referencing boundary")
    u.add()


def s31():  # summary-only, metadata-only, command-only
    s = S(31, A, 1070)
    s.raw('{"type":"summary","summary":"A summary line with no messages","leafUuid":"00000000-0000-4000-8000-0000000000ee"}')
    s.add()
    t = S(32, A, 1080)
    t.meta("permission-mode", permissionMode="default")
    t.meta("file-history-snapshot", messageId="x", snapshot={})
    t.add()
    u = S(33, A, 1090)
    u.meta("permission-mode", permissionMode="plan")
    u.user("<command-name>/compact</command-name>\n<command-message>compact</command-message>")
    u.asst([TX("Compacting")], usage=U(0, 0, 0, 0))
    u.asst([TX("Zero everything")], mid="msg_zero", usage={"input_tokens": 0, "output_tokens": 0})
    u.add()


def s34():  # big file: head and tail windows differ, long texts
    s = S(34, C, 1100)
    s.meta("custom-title", customTitle="Original title in the head")
    s.meta("ai-title", aiTitle="AI title in the head")
    s.user("First prompt that is far longer than two hundred characters " + "lorem ipsum dolor sit amet " * 12 + "end")
    s.text("Opening reply")
    for k in range(34):
        s.user("Filler prompt %d" % k)
        s.text(("Filler reply %d. " % k) + "The quick brown fox jumps over the lazy dog. " * 40)
        if k == 10:
            s.branch = "mid-branch"
    s.call("toolu_big", "Bash", {"command": "cat big.txt " + "y" * 700, "description": "A very long description " + "z" * 100},
           "R" * 5000, {"stdout": "R" * 5000})
    s.call("toolu_ed", "Edit", {"file_path": "big.py", "old_string": "\n".join("old line %d" % i for i in range(150)),
                                "new_string": "\n".join("new line %d" % i for i in range(150))}, "edited")
    s.text("An enormous reply. " + "Lorem ipsum. " * 1000)
    s.branch = "tail-branch"
    s.user("Last prompt of the session")
    s.text("Last reply")
    s.meta("custom-title", customTitle="Renamed title in the tail")
    s.raw('{"type":"tag","tag":"big-session","sessionId":"%s"}' % s.sid)
    s.meta("last-prompt", lastPrompt="Last prompt of the session")
    s.add()


def s35():  # invalid UTF-8 and a multibyte char cut at the end
    s = S(35, C, 1120, trunc=True)
    turn(s, "Prompt with a bad byte", "Reply")
    s.lines[-1] = s.lines[-1].replace(b"Reply", b"Re\xffply")
    s.raw(b'{"type":"user","uuid":"cut","message":{"content":"caf\xc3')
    s.add()


def s37():  # a message that is a string: the Python stats fold raises on it
    s = S(37, C, 1140)
    s.user("A prompt before the odd entry")
    s.put("user", {"message": "message is a string"})
    s.text("Reply after it")
    s.add()


def s36():  # duplicates across project folders; things that are not sessions
    real = S(3, ("C--work-synthetic-zeta", "C:\\work\\synthetic-zeta"), 200)  # session 3's id, another folder
    real.user("Same session id in a second folder")
    real.text("A different transcript")
    real.add()
    FILES.append(("projects/C--work-synthetic-aaa-first/%s.jsonl" % ("5e55%04x-0000-4000-8000-%012x" % (3, 3)), b""))
    d = S(4, ("C--work-synthetic-zeta", "C:\\work\\synthetic-zeta"), 260)  # session 4's id, a later folder
    d.user("Second copy of session four")
    d.text("Copy reply")
    d.add()
    FILES.append(("projects/C--work-synthetic-alpha/notes.jsonl",
                  b'{"type":"user","uuid":"n","message":{"content":"not a session"}}\n'))
    FILES.append(("projects/C--work-synthetic-alpha/not-a-uuid-0000-0000-000000000000.jsonl", b'{"type":"user"}\n'))
    FILES.append(("projects/stray.txt", b"not a project folder\n"))


def main():
    for fn in (s01, s02, s03, s04, s05, s06, s07, s08, s09, s10, s11, s13, s14, s15, s16, s17, s18, s19, s20, s21,
               s22, s23, s24, s27, s31, s34, s35, s36, s37):
        fn()
    if os.path.isdir(ROOT):
        shutil.rmtree(ROOT)
    for rel, data in FILES:
        path = os.path.join(HERE, *rel.split("/"))
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as fh:
            fh.write(data)
    print("wrote %d files, %d bytes" % (len(FILES), sum(len(d) for _, d in FILES)))


if __name__ == "__main__":
    main()
