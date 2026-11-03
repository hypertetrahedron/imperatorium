"""The dispatcher's side of the Imperatorium mod: modlink's state, the two
endpoints, and which prompts speak as the person."""
import json
import os
import tempfile
import threading
import unittest
import urllib.error
import urllib.request

from imperatorium import dispatcher, modlink
from test_dispatcher import CWD, SID, Base


class Clock:
    def __init__(self):
        self.t = 1000.0

    def __call__(self):
        return self.t


class LinkTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.clock = Clock()
        self.link = modlink.ModLink(self.dir, clock=self.clock)

    def test_the_token_is_made_once_and_kept(self):
        again = modlink.ModLink(self.dir)
        self.assertEqual(again.token, self.link.token)
        self.assertGreaterEqual(len(self.link.token), 32)
        self.assertTrue(self.link.authentic(self.link.token))
        self.assertFalse(self.link.authentic("nope"))
        self.assertFalse(self.link.authentic(None))

    def test_a_beat_makes_a_mod_live_until_it_stops(self):
        self.assertFalse(self.link.live("s"))
        self.link.beat({"session_id": "s", "busy": True})
        self.assertTrue(self.link.live("s"))
        self.assertTrue(self.link.state("s")["busy"])
        self.clock.t += modlink.LIVE_FOR + 1
        self.assertFalse(self.link.live("s"))
        self.assertIsNone(self.link.state("s"))

    def test_a_prompt_is_handed_out_once_accepted(self):
        pid = self.link.offer("s", "do it", as_user=True)
        got = self.link.beat({"session_id": "s"})
        self.assertEqual(got, [{"id": pid, "text": "do it", "as_user": True}])
        # Claimed: not handed out again on the next beat...
        self.assertEqual(self.link.beat({"session_id": "s"}), [])
        # ...unless the mod never accepted it (it reloaded mid-beat).
        self.clock.t += modlink.CLAIM_FOR + 1
        self.assertEqual(len(self.link.beat({"session_id": "s"})), 1)
        # Accepted: pending, never handed out again, however long it waits.
        self.link.beat({"session_id": "s", "acks": [{"id": pid, "outcome": "accepted"}]})
        self.clock.t += 10 * modlink.CLAIM_FOR
        self.assertEqual(self.link.beat({"session_id": "s"}), [])
        self.assertEqual(len(self.link.pending("s")), 1)
        # Its turn started: settled.
        self.link.beat({"session_id": "s", "acks": [{"id": pid, "outcome": "submitted"}]})
        self.assertEqual(self.link.pending("s"), [])
        self.assertEqual(self.link.settled("s")[0]["outcome"], "submitted")

    def test_a_dropped_prompt_says_why(self):
        pid = self.link.offer("s", "x", as_user=False)
        self.link.beat({"session_id": "s"})
        self.link.beat({"session_id": "s", "acks": [
            {"id": pid, "outcome": "dropped",
             "reason": "the session was stopped and nobody has sent input since"}]})
        self.assertEqual(self.link.pending("s"), [])
        self.assertIn("stopped", self.link.settled("s")[0]["reason"])

    def test_usage_becomes_a_statusline_payload(self):
        got = modlink.statusline_payload({
            "context": {"tokens": 5000, "window": 200000, "percent": 2.5},
            "rateLimits": [{"kind": "five_hour", "percentUsed": 40,
                            "resetsAt": "2026-11-03T20:00:00Z"},
                           {"kind": "seven_day", "percentUsed": "junk"}],
            "cost": {"usd": 1.5}, "model": "Opus 5.5"})
        self.assertEqual(got["context_window"], {"used_percentage": 2.5,
                                                 "context_window_size": 200000})
        self.assertEqual(got["cost"], {"total_cost_usd": 1.5})
        self.assertEqual(got["model"], {"display_name": "Opus 5.5"})
        self.assertEqual(got["rate_limits"]["five_hour"]["used_percentage"], 40)
        self.assertAlmostEqual(got["rate_limits"]["five_hour"]["resets_at"], 1793736000.0)
        self.assertNotIn("seven_day", got["rate_limits"])
        self.assertEqual(modlink.statusline_payload(None)["rate_limits"], {})


class ControlTests(Base):
    """Delivery through the mod, in the dispatcher."""

    def beat(self, **extra):
        return self.control.mod_beat(dict({"session_id": SID, "cwd": CWD}, **extra))

    def test_a_live_mod_takes_the_prompt_instead_of_the_socket(self):
        self.live()
        self.beat()
        out = self.control.dispatch(prompt="run the tests", session_id=SID, as_user=True)
        self.assertTrue(out["ok"], out)
        self.assertEqual(self.sent, [])  # nothing through the inbox socket
        got = self.beat()["deliver"]
        self.assertEqual(got[0]["text"], "run the tests")
        self.assertTrue(got[0]["as_user"])

    def test_only_the_window_speaks_as_the_person(self):
        self.live()
        self.beat()
        self.control.dispatch(prompt="from a script", session_id=SID)
        got = self.beat()["deliver"][0]
        self.assertFalse(got["as_user"])
        # Framed as someone else's, as a peer message is.
        self.assertIn("from a script", got["text"])
        self.assertNotEqual(got["text"], "from a script")

    def test_without_a_mod_the_socket_is_used_as_before(self):
        self.live()
        self.start_hook()
        out = self.control.dispatch(prompt="hello", session_id=SID, as_user=True)
        self.assertTrue(out["ok"], out)
        self.assertEqual(len(self.sent), 1)

    def test_a_live_mod_makes_a_session_reachable_and_says_busy(self):
        self.live(status="idle")
        self.beat(busy=True)
        self.control.invalidate()
        row = next(r for r in self.control.sessions()["sessions"] if r["sessionId"] == SID)
        self.assertTrue(row["dispatchable"])
        self.assertEqual(row["status"], "busy")
        self.assertTrue(row["mod"]["busy"])

    def test_the_queue_remembers_who_queued(self):
        self.live(status="busy")
        self.beat(busy=True)
        self.control.enqueue(SID, "after this", as_user=True)
        self.assertTrue(self.control.store.queued(SID)[0]["as_user"])
        self.beat(busy=False)
        self.control.invalidate()
        self.control.drain(SID)
        got = self.beat()["deliver"]
        self.assertEqual([(g["text"], g["as_user"]) for g in got], [("after this", True)])

    def test_usage_from_a_beat_reaches_the_meters(self):
        self.live()
        self.beat(usage={"rateLimits": [{"kind": "five_hour", "percentUsed": 33}],
                         "context": {"percent": 10, "window": 200000}})
        self.assertEqual(self.control.usage.limits["five_hour"]["used"], 33)

    def test_the_status_line_names_who_is_waiting(self):
        self.agent_rows = [
            {"sessionId": SID, "cwd": CWD, "kind": "interactive", "name": "ll-1", "status": "idle"},
            {"sessionId": "other", "cwd": CWD, "kind": "interactive", "name": "api", "status": "idle"},
        ]
        self.control.on_hook({"hook_event_name": "Notification", "session_id": "other",
                              "cwd": CWD, "notification_type": "permission_prompt"})
        self.control.invalidate()
        line = self.beat()["status"]
        self.assertIn("1 waiting on you: api", line or "")

    def test_the_policy_is_asked_only_once_auto_approval_is_on(self):
        path = os.path.join(self.dir, "policy.json")
        os.environ["IMPERATORIUM_POLICY"] = path
        self.addCleanup(os.environ.pop, "IMPERATORIUM_POLICY", None)
        req = {"session_id": SID, "cwd": CWD, "tool_name": "Bash",
               "tool_input": {"command": "git status"}}
        with open(path, "w", encoding="utf-8") as fh:
            json.dump({"enabled": True, "decision_format": "string"}, fh)
        self.assertEqual(self.control.mod_check(req)["verdict"], "ask")
        with open(path, "w", encoding="utf-8") as fh:
            json.dump({"enabled": True, "decision_format": "object"}, fh)
        self.assertEqual(self.control.mod_check(req)["verdict"], "allow")
        # Logged with the hook's verdicts, marked as the mod's.
        self.assertEqual(self.control.decisions()[0]["via"], "mod")
        # The policy's own refusals stand: it never approves its own controls.
        bad = dict(req, tool_input={"command": "rm policy.json"})
        self.assertNotEqual(self.control.mod_check(bad)["verdict"], "allow")


class RouteTests(Base):
    def setUp(self):
        super().setUp()
        dispatcher.Handler.control = self.control
        self.srv = dispatcher.Server(("127.0.0.1", 0), dispatcher.Handler)
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()
        self.port = self.srv.server_address[1]

    def tearDown(self):
        self.srv.shutdown()
        self.srv.server_close()
        super().tearDown()

    def call(self, path, body, headers=None):
        h = {"Content-Type": "application/json"}
        h.update(headers or {})
        req = urllib.request.Request("http://127.0.0.1:%d%s" % (self.port, path),
                                     headers=h, data=json.dumps(body).encode())
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                return resp.status, json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            return exc.code, json.loads(exc.read().decode("utf-8"))

    def test_a_beat_needs_the_token(self):
        status, _ = self.call("/api/mod/beat", {"session_id": SID})
        self.assertEqual(status, 403)
        status, _ = self.call("/api/mod/beat", {"session_id": SID},
                              {"X-Imperatorium-Mod": "wrong"})
        self.assertEqual(status, 403)
        status, body = self.call("/api/mod/beat", {"session_id": SID},
                                 {"X-Imperatorium-Mod": self.control.modlink.token})
        self.assertEqual(status, 200)
        self.assertTrue(body["ok"])
        self.assertTrue(self.control.modlink.live(SID))

    def test_a_check_needs_the_token(self):
        status, _ = self.call("/api/mod/check", {"tool_name": "Bash"})
        self.assertEqual(status, 403)

    def test_a_dispatch_without_the_window_token_is_framed(self):
        self.live()
        self.control.mod_beat({"session_id": SID})
        status, body = self.call("/api/dispatch", {"session_id": SID, "prompt": "hi"})
        self.assertTrue(body["ok"], body)
        self.assertFalse(self.control.modlink.pending(SID)[0]["as_user"])

    def test_a_dispatch_from_the_window_speaks_as_the_person(self):
        self.live()
        self.control.mod_beat({"session_id": SID})
        own = "http://127.0.0.1:%d" % self.port
        status, body = self.call("/api/dispatch", {"session_id": SID, "prompt": "hi"},
                                 {"Origin": own,
                                  "X-Imperatorium-Window": self.control.window_token})
        self.assertTrue(body["ok"], body)
        self.assertTrue(self.control.modlink.pending(SID)[0]["as_user"])



class InstallTests(unittest.TestCase):
    """hooks/install.py --mod: the mod's folder in CLAUDE_CODE_PLUGIN_DIRS."""

    def setUp(self):
        import sys
        sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "hooks"))
        import install
        self.install = install

    def test_added_once_beside_what_is_there(self):
        other = os.path.join("C:" + os.sep, "mods", "other") if os.name == "nt" else "/opt/mods/other"
        settings = {"env": {"CLAUDE_CODE_PLUGIN_DIRS": other, "FOO": "1"}}
        changes = self.install.apply_mod(settings, mod_dir="/x/mod/imperatorium")
        self.assertEqual(len(changes), 1)
        self.assertEqual(settings["env"]["CLAUDE_CODE_PLUGIN_DIRS"],
                         other + os.pathsep + "/x/mod/imperatorium")
        self.assertEqual(settings["env"]["FOO"], "1")
        self.assertTrue(self.install.has_mod(settings, "/x/mod/imperatorium"))
        self.assertEqual(self.install.apply_mod(settings, mod_dir="/x/mod/imperatorium"), [])

    def test_uninstall_removes_only_ours(self):
        settings = {"env": {"CLAUDE_CODE_PLUGIN_DIRS": "/x/mod/imperatorium"}}
        self.install.apply_mod(settings, uninstall=True, mod_dir="/x/mod/imperatorium")
        self.assertNotIn("env", settings)
        settings = {"env": {"CLAUDE_CODE_PLUGIN_DIRS": "/a" + os.pathsep + "/x/mod/imperatorium"}}
        self.install.apply_mod(settings, uninstall=True, mod_dir="/x/mod/imperatorium")
        self.assertEqual(settings["env"]["CLAUDE_CODE_PLUGIN_DIRS"], "/a")

    def test_a_strange_env_is_left_alone(self):
        settings = {"env": "nope"}
        self.assertTrue(self.install.apply_mod(settings)[0].startswith("SKIP"))
        self.assertEqual(settings["env"], "nope")


if __name__ == "__main__":
    unittest.main()
