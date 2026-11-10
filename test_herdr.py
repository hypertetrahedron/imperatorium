"""herdr integration: the hc client, terminal pairing, typing through hc, and
the herdr-app mods' token.

No real hc is run: a fake runner records argv and answers as hc does.

Run: python test_herdr.py
"""
import base64
import json
import os
import subprocess
import unittest

from imperatorium import dispatcher, hcore, slash
import test_dispatcher as td

SID = td.SID


class FakeRun:
    """Stands in for subprocess.run: answers by subcommand."""

    def __init__(self, answers=None):
        self.calls = []
        self.answers = answers or {}

    def __call__(self, argv, **kw):
        self.calls.append((argv, kw))
        sub = argv[1]
        if sub == "server":
            sub = "server " + argv[2]
        got = self.answers.get(sub, {})
        if callable(got):
            got = got(argv)
        code = 0
        if isinstance(got, tuple):
            code, got = got
        out = got if isinstance(got, str) else json.dumps(got)
        return subprocess.CompletedProcess(argv, code, out.encode(), b"")


def b64(s):
    return base64.b64encode(s.encode()).decode()


class CleanEnv(unittest.TestCase):
    def test_claude_and_terminal_variables_are_dropped(self):
        env = {"PATH": "p", "CLAUDE_CODE_MESSAGING_TOKEN": "secret", "CLAUDECODE": "1",
               "claude_lower": "x", "HC_TERMINAL_ID": "t3", "HC_SCOPE": "s",
               "HC_SESSION": "keep", "AI_AGENT": "claude"}
        out = hcore.clean_env(env)
        self.assertEqual(out, {"PATH": "p", "HC_SESSION": "keep"})

    def test_a_named_core_is_selected_by_HC_SESSION(self):
        c = hcore.Core(exe="hc", session="imptest", env={"PATH": "p"})
        self.assertEqual(c.env()["HC_SESSION"], "imptest")
        self.assertNotIn("HC_SESSION", hcore.Core(exe="hc", env={"PATH": "p"}).env())


class Calls(unittest.TestCase):
    def core(self, **answers):
        self.run = FakeRun(answers)
        return hcore.Core(exe="hc", runner=self.run, env={"CLAUDE_CODE_X": "1", "PATH": "p"})

    def test_json_goes_before_a_trailing_argv(self):
        c = self.core(run={"address": "t7"})
        got = c.run(["claude", "--session-id", "abc"], cwd="C:/w",
                    labels={"herdr.tab": "tb1", "herdr.space": "sp1"})
        self.assertEqual(got["address"], "t7")
        argv, kw = self.run.calls[0]
        self.assertEqual(argv[:3], ["hc", "run", "--json"])
        self.assertEqual(argv[argv.index("--"):], ["--", "claude", "--session-id", "abc"])
        for flag in ("--bg", "--no-parent", "--keep"):
            self.assertIn(flag, argv)
        self.assertIn("herdr.space=sp1", argv)
        self.assertNotIn("CLAUDE_CODE_X", kw["env"], "terminals must not inherit it")

    def test_an_error_is_raised_with_hcs_message(self):
        c = self.core(send=(1, {"error": {"message": "no terminal t9"}}))
        with self.assertRaises(hcore.HcError) as cm:
            c.send("t9", "hi")
        self.assertIn("no terminal t9", str(cm.exception))

    def test_send_only_if_idle(self):
        c = self.core(send={"ok": True})
        c.send("t1", "hello", only_if="idle")
        argv = self.run.calls[0][0]
        self.assertEqual(argv[-4:], ["--if", "idle", "t1", "hello"])

    def test_not_installed(self):
        c = hcore.Core(exe=None, runner=FakeRun())
        c.exe = None
        with self.assertRaises(hcore.HcError):
            c.terminals()


class Ensure(unittest.TestCase):
    def test_a_down_core_is_started_and_checked(self):
        state = {"up": False}

        def status(argv):
            return {"pid": 5} if state["up"] else (1, {"error": "not running"})

        def start(argv):
            state["up"] = True
            return {}
        run = FakeRun({"server status": status, "server start": start,
                       "exec": {"code": 0, "stdout": b64("[]\n")}})
        c = hcore.Core(exe="hc", runner=run)
        self.assertEqual(c.ensure()["pid"], 5)
        subs = [a[0][1:3] for a in run.calls]
        self.assertIn(["server", "start"], subs)

    def test_a_core_carrying_claude_variables_is_refused(self):
        run = FakeRun({"server status": {"pid": 5},
                       "exec": {"code": 0, "stdout": b64('["CLAUDE_CODE_MESSAGING_TOKEN"]\n')}})
        with self.assertRaises(hcore.HcError) as cm:
            hcore.Core(exe="hc", runner=run).ensure()
        self.assertIn("CLAUDE_CODE_MESSAGING_TOKEN", str(cm.exception))


class FakeCore:
    def __init__(self):
        self.did = []

    def keys(self, address, *names):
        self.did.append(("keys", address) + names)

    def send(self, address, text, force=False, only_if=None):
        self.did.append(("send", address, text, only_if))


class Pairing(td.SlashCommands):
    """A session started in an hc terminal is known by that terminal."""

    def hc_hook(self, event="SessionStart", terminal="t4", core="imptest"):
        return self.control.on_hook({
            "hook_event_name": event, "session_id": SID, "cwd": "C:/w",
            "messaging_socket": "\\\\.\\pipe\\x", "messaging_token": "tok",
            "hc_terminal": terminal, "hc_session": core})

    def row(self):
        self.control.invalidate()
        return next(r for r in self.control.sessions()["sessions"] if r["sessionId"] == SID)

    def test_the_row_carries_its_terminal(self):
        self.live()
        self.hc_hook()
        self.assertEqual(self.row()["hc"], {"address": "t4", "core": "imptest"})

    def test_a_later_event_repairs_it(self):
        self.live()
        self.start_hook()
        self.assertFalse(self.row().get("hc"))
        self.hc_hook(event="Stop")
        self.assertEqual(self.row()["hc"]["address"], "t4")

    def test_resumed_outside_hc_it_is_forgotten(self):
        self.live()
        self.hc_hook()
        self.start_hook()
        self.assertFalse(self.row().get("hc"))

    def test_a_slash_command_is_typed_through_hc(self):
        self.live()
        self.agent_rows[0]["pid"] = 4242
        self.hc_hook()
        fake = FakeCore()
        self.control.hc_core = lambda name=None: fake
        out = self.control.dispatch(prompt="/compact", session_id=SID)
        self.assertEqual(out["action"], "command")
        self.assertEqual(fake.did, [("keys", "t4", "ctrl+c"),
                                    ("send", "t4", "/compact", "idle")])
        self.assertEqual(self.typed, [], "not through the console")

    def test_an_hc_failure_is_a_refusal_not_a_console_fallback(self):
        self.live()
        self.agent_rows[0]["pid"] = 4242
        self.hc_hook()

        class Broken(FakeCore):
            def send(self, *a, **k):
                raise hcore.HcError("blocked")
        self.control.hc_core = lambda name=None: Broken()
        out = self.control.dispatch(prompt="/compact", session_id=SID)
        self.assertFalse(out["ok"])
        self.assertEqual(self.typed, [])


class HerdrToken(td.Origins):
    def call(self, headers):
        import urllib.error
        import urllib.request
        h = {"Content-Type": "application/json"}
        h.update(headers)
        req = urllib.request.Request("http://127.0.0.1:%d/api/typed" % self.port,
                                     data=json.dumps({"session_id": SID, "prompt": "hi"}).encode(),
                                     headers=h)
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                return resp.status, json.loads(resp.read())
        except urllib.error.HTTPError as exc:
            return exc.code, json.loads(exc.read())

    def test_the_mods_token_counts_as_the_window(self):
        code, body = self.call({"X-Imperatorium-Herdr": self.control.herdr_token})
        self.assertEqual(code, 200)
        self.assertIn("Settings", body["error"])  # reached; Send As Typed is off

    def test_a_wrong_or_missing_token_is_refused(self):
        for h in ({}, {"X-Imperatorium-Herdr": "guessed"},
                  {"X-Imperatorium-Herdr": self.control.window_token}):
            self.assertEqual(self.call(h)[0], 403, h)

    def test_a_browser_page_cannot_use_it(self):
        code, _ = self.call({"X-Imperatorium-Herdr": self.control.herdr_token,
                             "Origin": "http://127.0.0.1:%d" % self.port})
        self.assertEqual(code, 403)

    def test_it_is_written_beside_the_state(self):
        state = os.path.join(self.dir, "state.json")
        dispatcher.write_herdr_token(self.control, state)
        with open(dispatcher.herdr_token_path(state), encoding="utf-8") as fh:
            self.assertEqual(fh.read(), self.control.herdr_token)


if __name__ == "__main__":
    unittest.main()
