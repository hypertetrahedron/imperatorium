"""The dispatcher's routing and attention bookkeeping.

`claude agents --json`, the inbox pipe and the OS URL handler are all stubbed:
these tests are about what the dispatcher decides, not about the machine it
happens to be running on.

Run: python test_dispatcher.py
"""
import os
import shutil
import tempfile
import time
import unittest

from ccontrol import (background, conversation, deeplink, dispatcher, inbox,
                      projects, registry, remote, winfocus)
from ccontrol.store import Store

SID = "11111111-1111-1111-1111-111111111111"
CWD = os.path.join("Y:", os.sep, "projects-software", "laser-ledger")


class Base(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.control = dispatcher.Control(
            Store(os.path.join(self.dir, "state.json")), roots=[]
        )
        self.agent_rows = []
        self.files = {}
        self.sent = []
        self.launched = []

        self._orig = (
            registry.agents,
            registry.session_files,
            inbox.send,
            deeplink.launch,
            projects.discover,
            background.start,
        )
        registry.agents = lambda *a, **k: list(self.agent_rows)
        registry.session_files = lambda *a, **k: dict(self.files)
        inbox.send = lambda sock, tok, content, *a, **k: self.sent.append(
            (sock, tok, content)
        )
        deeplink.launch = lambda path, prompt="": self.launched.append(
            (path, prompt)
        ) or ("claude-cli://open?cwd=" + path)
        projects.discover = lambda roots=None, extra=None: [
            {"name": "laser-ledger", "path": CWD, "source": "root"},
            {"name": "gem-trip", "path": "Y:/projects-software/gem-trip",
             "source": "root"},
        ]

    def tearDown(self):
        (
            registry.agents,
            registry.session_files,
            inbox.send,
            deeplink.launch,
            projects.discover,
            background.start,
        ) = self._orig
        shutil.rmtree(self.dir, ignore_errors=True)

    def live(self, status="idle", sid=SID, cwd=CWD):
        self.agent_rows = [
            {"sessionId": sid, "cwd": cwd, "kind": "interactive",
             "name": "ll-1", "status": status}
        ]
        self.files = {sid: {"sessionId": sid, "cwd": cwd, "kind": "interactive",
                            "messagingSocketPath": "\\\\.\\pipe\\x", "name": "ll-1"}}

    def start_hook(self, sid=SID, cwd=CWD, token="tok", socket=None):
        # Default to whatever socket that session is advertising, because a
        # token is only honoured while it still pairs with that socket.
        if socket is None:
            socket = (self.files.get(sid) or {}).get(
                "messagingSocketPath", "\\\\.\\pipe\\x"
            )
        return self.control.on_hook({
            "hook_event_name": "SessionStart",
            "session_id": sid,
            "cwd": cwd,
            "session_name": "ll-1",
            "messaging_socket": socket,
            "messaging_token": token,
        })

    def rebind(self, sid=SID, socket="\\\\.\\pipe\\REBOUND"):
        """Simulate a resume: same session id, brand new inbox."""
        self.files[sid]["messagingSocketPath"] = socket
        self.control.invalidate()
        return socket


class HookIngest(Base):
    def test_session_start_records_the_token(self):
        self.start_hook()
        self.assertEqual(self.control.store.tokens(), {SID: "tok"})

    def test_a_session_is_dispatchable_only_with_socket_and_token(self):
        self.live()
        rows = {r["sessionId"]: r for r in self.control.sessions()["sessions"]}
        self.assertFalse(rows[SID]["dispatchable"])  # socket, but no token yet
        self.start_hook()
        self.control.invalidate()
        rows = {r["sessionId"]: r for r in self.control.sessions()["sessions"]}
        self.assertTrue(rows[SID]["dispatchable"])

    def test_a_rebound_socket_invalidates_the_stored_token(self):
        # A resumed session keeps its id but binds a NEW inbox. Pairing the
        # fresh socket with the old token produces a session that reports as
        # dispatchable and then fails at the auth line - observed live.
        self.live()
        self.start_hook()
        self.rebind()
        row = {r["sessionId"]: r for r in self.control.sessions()["sessions"]}[SID]
        self.assertFalse(row["dispatchable"])
        self.assertTrue(row.get("stale_credential"))
        self.assertIsNone(row.get("token"))

    def test_the_next_session_start_repairs_a_stale_credential(self):
        self.live()
        self.start_hook()
        socket = self.rebind()
        self.start_hook(token="tok2", socket=socket)
        self.control.invalidate()
        row = {r["sessionId"]: r for r in self.control.sessions()["sessions"]}[SID]
        self.assertTrue(row["dispatchable"])
        self.assertEqual(row["token"], "tok2")

    def test_a_stop_event_can_register_a_session_start_never_saw(self):
        # SessionStart fires once. A session already running when the
        # dispatcher comes up would otherwise stay unreachable forever, so
        # every event carries the credential and any of them can repair it.
        self.live()
        self.control.on_hook({
            "hook_event_name": "Stop", "session_id": SID, "cwd": CWD,
            "messaging_token": "late",
            "messaging_socket": self.files[SID]["messagingSocketPath"],
            "last_assistant_message": "done",
        })
        self.control.invalidate()
        row = {r["sessionId"]: r for r in self.control.sessions()["sessions"]}[SID]
        self.assertTrue(row["dispatchable"])
        self.assertEqual(row["token"], "late")

    def test_a_notification_also_carries_the_credential(self):
        self.live()
        self.control.on_hook({
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt",
            "session_id": SID, "cwd": CWD,
            "messaging_token": "tok",
            "messaging_socket": self.files[SID]["messagingSocketPath"],
        })
        self.control.invalidate()
        row = {r["sessionId"]: r for r in self.control.sessions()["sessions"]}[SID]
        self.assertTrue(row["dispatchable"])
        self.assertTrue(row["attention"])

    def test_permission_prompt_raises_attention(self):
        self.live()
        self.control.on_hook({
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt",
            "session_id": SID, "cwd": CWD,
        })
        row = self.control.sessions()["sessions"][0]
        self.assertTrue(row["attention"])
        self.assertIn("attention_since", row)

    def test_stop_clears_attention(self):
        self.live()
        self.control.on_hook({
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt",
            "session_id": SID, "cwd": CWD,
        })
        self.control.on_hook({
            "hook_event_name": "Stop", "session_id": SID, "cwd": CWD,
            "last_assistant_message": "done",
        })
        self.assertFalse(self.control.sessions()["sessions"][0].get("attention"))

    def test_session_end_forgets_the_token(self):
        self.start_hook()
        self.control.on_hook({"hook_event_name": "SessionEnd", "session_id": SID})
        self.assertEqual(self.control.store.tokens(), {})

    def test_uninteresting_events_are_dropped(self):
        before = len(self.control.store.events(999))
        out = self.control.on_hook({"hook_event_name": "UserPromptSubmit",
                                    "session_id": SID})
        self.assertTrue(out["ok"])
        self.assertEqual(out.get("ignored"), "UserPromptSubmit")
        self.assertEqual(len(self.control.store.events(999)), before)

    def test_tool_events_go_to_memory_not_the_store(self):
        # One per tool call per session: the store rewrites its whole file
        # on every write, so these must never reach it.
        self.start_hook()
        before = len(self.control.store.events(999))
        with open(self.control.store.path, "rb") as fh:
            snapshot = fh.read()
        out = self.control.on_hook({"hook_event_name": "PreToolUse",
                                    "session_id": SID, "tool_name": "Bash",
                                    "tool_input": {"command": "npm test"},
                                    "tool_use_id": "t1"})
        self.assertTrue(out["ok"])
        self.assertEqual(len(self.control.store.events(999)), before)
        with open(self.control.store.path, "rb") as fh:
            self.assertEqual(fh.read(), snapshot)
        self.assertEqual(self.control.live.current(SID)["summary"], "npm test")

    def test_an_event_without_a_session_id_is_dropped(self):
        self.assertTrue(self.control.on_hook({"hook_event_name": "Stop"})["ok"])

    def test_stop_failure_keeps_its_error(self):
        # The field is `error`. This asserted `error_type`, which Claude Code
        # has never sent, so every recorded failure carried an empty reason.
        self.control.on_hook({
            "hook_event_name": "StopFailure", "session_id": SID,
            "error": "rate_limit",
        })
        self.assertEqual(self.control.store.events(1)[0]["error"], "rate_limit")


class FailedTurns(Base):
    """A turn that died on an API error is exactly when a person is wanted.

    `StopFailure` used to sit in the clearing set beside a clean `Stop`, so
    hitting a 529 actively erased the "needs you" flag and the tile fell back
    to the live status - which is `busy` - and said "working". Seen on this
    machine: three 529s between 20:55 and 21:00 on session 649268a1, tile
    reading "working" throughout.
    """

    def fail(self, error="overloaded", detail=None, sid=SID):
        return self.control.on_hook({
            "hook_event_name": "StopFailure", "session_id": sid, "cwd": CWD,
            "error": error, "error_details": detail,
            "last_assistant_message": "API Error: 529 Overloaded.",
        })

    def test_an_overload_raises_attention_instead_of_clearing_it(self):
        self.live(status="busy")
        self.fail()
        row = self.control.sessions()["sessions"][0]
        self.assertTrue(row["attention"])
        self.assertEqual(row["attention_reason"], "API overloaded - wait and retry")
        self.assertEqual(row["attention_kind"], "failure")

    def test_the_retry_does_not_wipe_the_flag(self):
        # Claude Code retries a 529 by itself, so the session goes busy again
        # seconds later. That is not a person answering anything, and the
        # earlier staleness check must not treat it as one.
        self.live(status="busy")
        self.fail()
        self.files[SID]["status"] = "busy"
        self.files[SID]["statusUpdatedAt"] = (time.time() + 600) * 1000
        self.control.invalidate()
        row = self.control.sessions()["sessions"][0]
        self.assertTrue(row["attention"], "a retry is not a resolution")

    def test_a_permission_prompt_is_still_settled_by_working_again(self):
        # The other half of that rule must keep working.
        self.live(status="busy")
        self.control.on_hook({
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt",
            "session_id": SID, "cwd": CWD,
        })
        self.files[SID]["status"] = "busy"
        self.files[SID]["statusUpdatedAt"] = (time.time() + 600) * 1000
        self.control.invalidate()
        self.assertFalse(self.control.sessions()["sessions"][0].get("attention"))

    def test_finishing_cleanly_clears_a_failure(self):
        self.live(status="busy")
        self.fail()
        self.control.on_hook({
            "hook_event_name": "Stop", "session_id": SID, "cwd": CWD,
            "last_assistant_message": "done",
        })
        self.assertFalse(self.control.sessions()["sessions"][0].get("attention"))

    def test_the_quiet_failure_raises_nothing(self):
        self.live(status="busy")
        self.fail(error="max_output_tokens")
        row = self.control.sessions()["sessions"][0]
        self.assertFalse(row.get("attention"))
        self.assertEqual(row["last_failure"], "max_output_tokens")

    def test_the_reason_and_detail_are_kept_for_the_window(self):
        self.live(status="busy")
        self.fail(error="invalid_request", detail="prompt is too long")
        row = self.control.sessions()["sessions"][0]
        self.assertEqual(row["attention_reason"], "request too large - /compact or trim")
        self.assertEqual(row["last_failure"], "invalid_request")
        self.assertIn("too long", row["last_failure_detail"])

    def test_a_failure_still_records_the_credential(self):
        # Every event carries the token, and a failing session is one you may
        # most want to reach.
        self.live(status="busy")
        self.control.on_hook({
            "hook_event_name": "StopFailure", "session_id": SID, "cwd": CWD,
            "error": "overloaded",
            "messaging_token": "tok",
            "messaging_socket": self.files[SID]["messagingSocketPath"],
        })
        self.assertEqual(self.control.store.tokens()[SID], "tok")

    def test_an_unknown_error_code_still_raises_something(self):
        self.live(status="busy")
        self.fail(error="some_future_code")
        row = self.control.sessions()["sessions"][0]
        self.assertTrue(row["attention"])
        self.assertEqual(row["attention_reason"], "API error")


class FocusRouting(Base):
    """Raising the window a session is in, rather than any window.

    One `WindowsTerminal.exe` hosts every session here, so walking the
    process tree finds the terminal and then cannot tell its windows apart.
    It raised whichever came first, which was usually the wrong one.
    """

    def setUp(self):
        super().setUp()
        self.live()
        self.agent_rows[0]["pid"] = 4242
        self.files[SID]["pid"] = 4242
        self.control.invalidate()
        self.raised = []
        self._fw = (winfocus.terminal_for, winfocus.titled_windows,
                    winfocus.focus_hwnd, winfocus.focus_pid,
                    winfocus.owner_pid, conversation.title)
        winfocus.terminal_for = lambda pid: 71868
        winfocus.owner_pid = lambda hwnd: 71868
        winfocus.focus_hwnd = lambda hwnd: self.raised.append(hwnd) or True
        winfocus.focus_pid = lambda pid: self.raised.append(("pid", pid)) or True
        conversation.title = lambda sid: "Vercel OIDC token sufficiency"
        winfocus.titled_windows = lambda pid=None: [
            (11, "◑ Project tiles status not updating"),
            (22, "✳ Vercel OIDC token sufficiency"),
        ]

    def tearDown(self):
        (winfocus.terminal_for, winfocus.titled_windows, winfocus.focus_hwnd,
         winfocus.focus_pid, winfocus.owner_pid,
         conversation.title) = self._fw
        super().tearDown()

    def row(self):
        return self.control.sessions()["sessions"][0]

    def test_the_window_with_this_session_title_is_the_one_raised(self):
        out = self.control.focus_row(self.row())
        self.assertEqual(out["how"], "title")
        self.assertEqual(self.raised, [22], "raised the other window")

    def test_the_window_is_remembered_for_when_the_tab_is_behind(self):
        self.control.focus_row(self.row())
        self.assertEqual(
            self.control.store.sessions()[SID]["window_hwnd"], 22)

    def test_a_backgrounded_tab_falls_back_to_where_it_was_last_seen(self):
        # Its title is nowhere now, because its window is showing another
        # tab. A tab does not move between windows, so that window is still
        # the right one - it just cannot be brought to the front of itself.
        self.control.focus_row(self.row())
        self.raised[:] = []
        conversation.title = lambda sid: "Skills executed display in workflows"
        out = self.control.focus_row(self.row())
        self.assertEqual(out["how"], "remembered")
        self.assertEqual(self.raised, [22])
        self.assertIn("Skills executed", out["note"])

    def test_a_window_that_now_belongs_to_something_else_is_not_trusted(self):
        # Window handles are reused. Raising a stranger's window would be a
        # worse failure than admitting we cannot find it.
        self.control.focus_row(self.row())
        self.raised[:] = []
        conversation.title = lambda sid: "Skills executed display in workflows"
        winfocus.owner_pid = lambda hwnd: 999999
        out = self.control.focus_row(self.row())
        self.assertEqual(out["how"], "terminal")
        self.assertEqual(self.raised, [("pid", 4242)])

    def test_an_untitled_session_still_gets_its_terminal(self):
        conversation.title = lambda sid: ""
        out = self.control.focus_row(self.row())
        self.assertEqual(out["how"], "terminal")
        self.assertIn("note", out)

    def test_nothing_raised_at_all_is_reported_not_papered_over(self):
        conversation.title = lambda sid: ""
        winfocus.focus_pid = lambda pid: False
        self.assertIsNone(self.control.focus_row(self.row()))

    def test_a_failed_focus_never_launches_a_second_session(self):
        # A focus request that could not raise the window once opened a
        # brand new session on the same project instead.
        conversation.title = lambda sid: ""
        winfocus.focus_pid = lambda pid: False
        out = self.control.focus(session_id=SID)
        self.assertFalse(out["ok"])
        self.assertEqual(self.launched, [], "must not open a duplicate")


class Routing(Base):
    def test_injects_into_a_live_dispatchable_session(self):
        self.live()
        self.start_hook()
        self.control.invalidate()
        out = self.control.dispatch(text="project: laser ledger, do the thing")
        self.assertEqual(out["action"], "inject")
        self.assertEqual(self.sent[0][2], "do the thing")
        self.assertEqual(self.launched, [])

    def test_falls_back_to_a_deep_link_when_nothing_is_live(self):
        out = self.control.dispatch(text="project: laser ledger, do the thing")
        self.assertEqual(out["action"], "launch")
        self.assertEqual(self.launched, [(CWD, "do the thing")])

    def test_inject_mode_refuses_rather_than_launching(self):
        out = self.control.dispatch(
            text="project: laser ledger, do the thing", mode="inject"
        )
        self.assertFalse(out["ok"])
        self.assertEqual(self.launched, [])

    def test_launch_mode_never_injects(self):
        self.live()
        self.start_hook()
        self.control.invalidate()
        out = self.control.dispatch(
            text="project: laser ledger, do the thing", mode="launch"
        )
        self.assertEqual(out["action"], "launch")
        self.assertEqual(self.sent, [])

    def test_dry_run_touches_nothing(self):
        self.live()
        self.start_hook()
        out = self.control.dispatch(
            text="project: laser ledger, do the thing", mode="dry-run"
        )
        self.assertEqual(out["action"], "dry-run")
        self.assertEqual((self.sent, self.launched), ([], []))

    def test_an_unmatched_project_dispatches_nothing(self):
        out = self.control.dispatch(text="project: home assistant, why offline")
        self.assertFalse(out["ok"])
        self.assertEqual((self.sent, self.launched), ([], []))

    def test_a_failed_inject_falls_back_in_auto_mode(self):
        # A session can die between the snapshot and the write; auto mode
        # should still get the work in front of the user.
        self.live()
        self.start_hook()
        self.control.invalidate()

        def boom(*a, **k):
            raise inbox.InboxError("pipe is gone")

        inbox.send = boom
        out = self.control.dispatch(text="project: laser ledger, do the thing")
        self.assertEqual(out["action"], "launch")

    def test_a_stale_credential_is_never_injected_into(self):
        self.live()
        self.start_hook()
        self.rebind()
        out = self.control.dispatch(text="project: laser ledger, go")
        self.assertEqual(out["action"], "launch")
        self.assertEqual(self.sent, [])

    def test_background_mode_starts_a_session_and_never_opens_a_window(self):
        started = []
        background.start = lambda prompt, cwd, **kw: (
            started.append((prompt, cwd, kw)) or {"id": "abc12345", "command": [],
                                                  "output": ""}
        )
        out = self.control.dispatch(
            text="project: laser ledger, do the thing", mode="background"
        )
        self.assertEqual(out["action"], "background")
        self.assertEqual(out["bg_id"], "abc12345")
        self.assertEqual(started[0][:2], ("do the thing", CWD))
        self.assertEqual((self.sent, self.launched), ([], []))

    def test_background_mode_prefers_a_new_session_over_a_live_one(self):
        # "Go and do this" is not "put it in the window I am watching".
        self.live()
        self.start_hook()
        self.control.invalidate()
        background.start = lambda prompt, cwd, **kw: {"id": "abc12345",
                                                      "command": [], "output": ""}
        out = self.control.dispatch(
            text="project: laser ledger, do the thing", mode="background"
        )
        self.assertEqual(out["action"], "background")
        self.assertEqual(self.sent, [])

    def test_background_options_from_config_are_passed_through(self):
        seen = {}
        self.control.background_options = {"model": "sonnet",
                                           "permission_mode": "acceptEdits"}
        background.start = lambda prompt, cwd, **kw: (
            seen.update(kw) or {"id": "a1b2c3d4", "command": [], "output": ""}
        )
        self.control.dispatch(project="laser-ledger", prompt="go", mode="background")
        self.assertEqual(seen.get("model"), "sonnet")
        self.assertEqual(seen.get("permission_mode"), "acceptEdits")

    def test_a_failed_background_start_is_reported_not_swallowed(self):
        def boom(prompt, cwd, **kw):
            raise background.BackgroundError("claude is not on PATH")

        background.start = boom
        out = self.control.dispatch(project="laser-ledger", prompt="go",
                                    mode="background")
        self.assertFalse(out["ok"])
        self.assertIn("PATH", out["error"])
        self.assertEqual(self.launched, [])

    def test_missing_prompt_or_project_is_refused(self):
        self.assertFalse(self.control.dispatch(text="project: laser ledger,")["ok"])
        self.assertFalse(self.control.dispatch(text="just do something")["ok"])

    def test_idle_session_wins_over_busy_one(self):
        other = "22222222-2222-2222-2222-222222222222"
        self.agent_rows = [
            {"sessionId": SID, "cwd": CWD, "name": "busy-one", "status": "busy"},
            {"sessionId": other, "cwd": CWD, "name": "idle-one", "status": "idle"},
        ]
        self.files = {
            s: {"sessionId": s, "cwd": CWD, "messagingSocketPath": "\\\\.\\pipe\\%s" % s}
            for s in (SID, other)
        }
        self.start_hook(sid=SID, token="t1")
        self.start_hook(sid=other, token="t2")
        self.control.invalidate()
        out = self.control.dispatch(text="project: laser ledger, go")
        self.assertEqual(out["session"], other)

    def test_explicit_project_and_prompt_skip_parsing(self):
        out = self.control.dispatch(
            project="gem-trip", prompt="fix it", mode="dry-run"
        )
        self.assertEqual(out["project"]["name"], "gem-trip")



PI = {"name": "pi", "claude": "c", "label": "pi", "python": "python3",
      "tunnel": {"local_port": 0, "remote_port": 8792}}
RSID = "22222222-2222-2222-2222-222222222222"
RCWD = "/home/les/src/die-reader"
RSOCK = "/tmp/cc-msg-r.sock"


class RemoteBase(Base):
    def setUp(self):
        super().setUp()
        self.control.fleet = remote.Fleet([PI])
        self.remote_rows = []
        self.control.remote_rows = lambda: [dict(r) for r in self.remote_rows]
        self.injected = []
        self.started = []
        self._orig_remote = (remote.inject, remote.start_background)
        remote.inject = lambda host, sock, tok, content, **k: \
            self.injected.append((host["name"], sock, tok, content))
        remote.start_background = lambda host, cwd, prompt, opts=None: \
            self.started.append((host["name"], cwd, prompt)) or {
                "id": "bg1", "host": host["name"], "output": ""}
        projects.discover = lambda roots=None, extra=None: [
            {"name": "laser-ledger", "path": CWD, "source": "root"}]

    def tearDown(self):
        remote.inject, remote.start_background = self._orig_remote
        super().tearDown()

    def remote_live(self, status="idle", socket=RSOCK):
        self.remote_rows = [{
            "sessionId": RSID, "cwd": RCWD, "kind": "interactive", "pid": SIDPID,
            "status": status, "socket": socket, "host": "pi", "label": "pi",
            "remote": True, "dispatchable": False, "name": "die-1"}]

    def remote_hook(self, event="SessionStart", sid=RSID, token="rtok",
                    socket=RSOCK, **extra):
        payload = {"hook_event_name": event, "session_id": sid, "cwd": RCWD,
                   "messaging_socket": socket, "messaging_token": token}
        payload.update(extra)
        return self.control.on_hook(payload, host="pi")

    def row(self, sid=RSID):
        self.control.invalidate()
        return next(r for r in self.control.sessions()["sessions"]
                    if r["sessionId"] == sid)


SIDPID = 112015


class RemoteHooks(RemoteBase):
    def test_a_remote_session_becomes_dispatchable_once_its_hook_reports(self):
        self.remote_live()
        self.assertFalse(self.row()["dispatchable"])
        self.remote_hook()
        row = self.row()
        self.assertTrue(row["dispatchable"])
        self.assertEqual(row["token"], "rtok")

    def test_remote_records_are_kept_under_the_host(self):
        self.remote_hook()
        self.assertIn("pi/" + RSID, self.control.store.sessions())
        self.assertNotIn(RSID, self.control.store.sessions())

    def test_a_host_cannot_forget_a_local_session(self):
        # Anything on that host can post to its end of the tunnel. It must
        # not be able to make a session on this machine unreachable.
        self.live()
        self.start_hook()
        self.control.on_hook({"hook_event_name": "SessionEnd",
                              "session_id": SID}, host="pi")
        self.assertIn(SID, self.control.store.tokens())

    def test_a_host_cannot_repoint_a_local_session(self):
        self.live()
        self.start_hook()
        self.control.on_hook({"hook_event_name": "Stop", "session_id": SID,
                              "messaging_socket": "/tmp/evil",
                              "messaging_token": "evil"}, host="pi")
        self.control.invalidate()
        local = next(r for r in self.control.sessions()["sessions"]
                     if r["sessionId"] == SID and not r.get("remote"))
        self.assertEqual(local["token"], "tok")

    def test_a_remote_permission_prompt_raises_attention(self):
        self.remote_live(status="busy")
        self.remote_hook()
        self.remote_hook("Notification", notification_type="permission_prompt")
        row = self.row()
        self.assertTrue(row["attention"])
        self.assertTrue(row["attention_reason"])

    def test_a_remote_stop_clears_it(self):
        self.remote_live()
        self.remote_hook("Notification", notification_type="permission_prompt")
        self.remote_hook("Stop")
        self.assertFalse(self.row().get("attention_since"))

    def test_a_rebound_remote_socket_invalidates_the_token(self):
        self.remote_live(socket="/tmp/new.sock")
        self.remote_hook(socket="/tmp/old.sock")
        row = self.row()
        self.assertFalse(row["dispatchable"])
        self.assertTrue(row.get("stale_credential"))

    def test_remote_events_say_which_host(self):
        self.remote_hook()
        self.assertEqual(self.control.store.events(1)[0]["host"], "pi")


class RemoteRouting(RemoteBase):
    def test_a_prompt_to_a_remote_session_goes_over_ssh(self):
        self.remote_live()
        self.remote_hook()
        out = self.control.dispatch(prompt="go", session_id=RSID)
        self.assertTrue(out["ok"], out)
        self.assertEqual(self.injected, [("pi", RSOCK, "rtok", "go")])
        self.assertEqual(self.sent, [])

    def test_a_remote_session_without_a_token_is_refused(self):
        self.remote_live()
        out = self.control.dispatch(prompt="go", session_id=RSID)
        self.assertFalse(out["ok"])
        self.assertEqual(self.injected, [])

    def test_a_remote_failure_is_reported(self):
        self.remote_live()
        self.remote_hook()

        def boom(*a, **k):
            raise remote.RemoteError("Connection refused")
        remote.inject = boom
        out = self.control.dispatch(prompt="go", session_id=RSID)
        self.assertFalse(out["ok"])
        self.assertIn("Connection refused", out["error"])

    def test_a_remote_project_injects_into_its_live_session(self):
        self.remote_live()
        self.remote_hook()
        out = self.control.dispatch(project="pi/die-reader", prompt="go",
                                    confirm=True)
        self.assertEqual(out.get("action"), "inject", out)
        self.assertEqual(self.started, [])

    def test_a_remote_project_with_nothing_live_goes_to_the_background(self):
        self.remote_live()  # listed, but no hook has reported a token
        out = self.control.dispatch(project="pi/die-reader", prompt="go",
                                    confirm=True)
        self.assertEqual(out.get("action"), "remote", out)
        self.assertEqual(self.started, [("pi", RCWD, "go")])

    def test_inject_mode_to_a_remote_project_refuses_rather_than_starting(self):
        self.remote_live()
        out = self.control.dispatch(project="pi/die-reader", prompt="go",
                                    mode="inject", confirm=True)
        self.assertFalse(out["ok"])
        self.assertEqual(self.started, [])

    def test_background_mode_never_injects_remotely(self):
        self.remote_live()
        self.remote_hook()
        self.control.dispatch(project="pi/die-reader", prompt="go",
                              mode="background", confirm=True)
        self.assertEqual(self.injected, [])
        self.assertEqual(len(self.started), 1)

    def test_a_local_project_never_picks_a_remote_session(self):
        self.remote_live()
        self.remote_hook()
        self.remote_rows[0]["cwd"] = CWD
        out = self.control.dispatch(project="laser-ledger", prompt="go")
        self.assertEqual(out["action"], "launch")
        self.assertEqual(self.injected, [])

    def test_a_remote_session_is_never_focused_as_a_local_pid(self):
        self.remote_live()
        called = []
        orig = winfocus.focus_pid
        winfocus.focus_pid = lambda pid: called.append(pid) or True
        try:
            out = self.control.focus(session_id=RSID)
        finally:
            winfocus.focus_pid = orig
        self.assertFalse(out["ok"])
        self.assertEqual(called, [])
        self.assertEqual(self.launched, [])


class HookOnlyListener(RemoteBase):
    """The tunnel lands here, so this is what every process on the host sees."""

    def setUp(self):
        super().setUp()
        import threading
        dispatcher.HookOnlyHandler.control = self.control
        self.srv = dispatcher.Server(("127.0.0.1", 0), dispatcher.HookOnlyHandler)
        self.srv.ccontrol_host = "pi"
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()
        self.port = self.srv.server_address[1]

    def tearDown(self):
        self.srv.shutdown()
        self.srv.server_close()
        super().tearDown()

    def post(self, path, body):
        import json
        import urllib.error
        import urllib.request
        req = urllib.request.Request(
            "http://127.0.0.1:%d%s" % (self.port, path),
            data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                return resp.status
        except urllib.error.HTTPError as exc:
            return exc.code

    def test_hooks_are_accepted_and_tagged_with_the_listeners_host(self):
        self.assertEqual(self.post("/hook", {
            "hook_event_name": "SessionStart", "session_id": RSID,
            "messaging_socket": RSOCK, "messaging_token": "rtok"}), 200)
        self.assertIn("pi/" + RSID, self.control.store.tokens())

    def test_dispatch_is_not_reachable_from_the_host(self):
        self.live()
        self.start_hook()
        self.assertEqual(self.post("/api/dispatch", {
            "session_id": SID, "prompt": "rm -rf"}), 404)
        self.assertEqual(self.sent, [])

    def test_nothing_else_is_either(self):
        for path in ("/api/focus", "/api/decide", "/permission/park"):
            self.assertEqual(self.post(path, {}), 404, path)


class Origins(Base):
    """The window is a browser page, and browsers send Origin on every POST."""

    def setUp(self):
        super().setUp()
        import threading
        dispatcher.Handler.control = self.control
        self.srv = dispatcher.Server(("127.0.0.1", 0), dispatcher.Handler)
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()
        self.port = self.srv.server_address[1]

    def tearDown(self):
        self.srv.shutdown()
        self.srv.server_close()
        super().tearDown()

    def post(self, origin):
        import json
        import urllib.error
        import urllib.request
        headers = {"Content-Type": "application/json"}
        if origin:
            headers["Origin"] = origin % {"port": self.port}
        # /api/focus with nothing named answers without touching anything.
        req = urllib.request.Request(
            "http://127.0.0.1:%d/api/focus" % self.port,
            data=json.dumps({}).encode(), headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                return resp.status
        except urllib.error.HTTPError as exc:
            return exc.code

    def test_the_windows_own_origin_is_accepted(self):
        # The bug: every Send from the window was refused as cross-origin.
        self.assertEqual(self.post("http://127.0.0.1:%(port)d"), 200)
        self.assertEqual(self.post("http://localhost:%(port)d"), 200)

    def test_no_origin_is_accepted(self):
        self.assertEqual(self.post(None), 200)

    def test_a_foreign_page_is_refused(self):
        self.assertEqual(self.post("https://evil.example"), 403)

    def test_the_same_host_on_another_port_is_refused(self):
        # Any other local server - a dev server, another tool - is a
        # different origin, and a page there must not drive this.
        self.assertEqual(self.post("http://127.0.0.1:1"), 403)

    def test_an_opaque_origin_is_refused(self):
        self.assertEqual(self.post("null"), 403)


class RemoteConversation(RemoteBase):
    def setUp(self):
        super().setUp()
        self.reads = []
        self.made = []
        test = self

        class FakeReader:
            def __init__(self, host):
                test.made.append(host["name"])

            def read(self, sid, limit=60, cwd=None):
                test.reads.append((sid, limit, cwd))
                if test.fail:
                    raise remote.RemoteError(test.fail)
                return {"items": [{"speaker": "claude", "role": "assistant",
                                   "blocks": [{"kind": "text", "text": "4.2 MB"}]}],
                        "title": "Project size"}

        self.fail = None
        self._orig_reader = remote.Reader
        remote.Reader = FakeReader

    def tearDown(self):
        remote.Reader = self._orig_reader
        super().tearDown()

    def test_a_remote_session_is_read_on_its_own_host(self):
        self.remote_live()
        out = self.control.conversation(RSID)
        self.assertEqual(out["items"][0]["blocks"][0]["text"], "4.2 MB")
        self.assertEqual(out["host"], "pi")
        # Its folder is passed, so the SDK does not search every project.
        self.assertEqual(self.reads, [(RSID, 60, RCWD)])

    def test_one_reader_per_host_is_kept(self):
        self.remote_live()
        for _ in range(3):
            self.control.conversation(RSID)
        self.assertEqual(self.made, ["pi"])

    def test_a_failed_read_says_where_and_why(self):
        self.remote_live()
        self.fail = "no reply in 15s"
        out = self.control.conversation(RSID)
        self.assertEqual(out["items"], [])
        self.assertIn("pi", out["error"])
        self.assertIn("no reply", out["error"])

    def test_a_local_session_never_goes_to_a_host(self):
        self.live()
        orig = conversation.read
        conversation.read = lambda sid, limit=60: []
        try:
            self.control.conversation(SID)
        finally:
            conversation.read = orig
        self.assertEqual(self.reads, [])


if __name__ == "__main__":
    unittest.main(verbosity=2)
