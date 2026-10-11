"""The impctl command line: that help covers every command, arguments reach the
scripts they are handed to, sessions are found by the names people use, and
an uninstall is never one careless keystroke away.

Nothing here starts, stops or talks to a real service: the dispatcher API and
subprocess calls are replaced.

Run: python test_impctl.py
"""
import contextlib
import io
import unittest

import impctl as cli

ROWS = [
    {"sessionId": "9cbf54ae-0000", "name": "bookoferrantpages-d0", "cwd": "C:/b",
     "window_title": "AI has a pickaxe problem"},
    {"sessionId": "35692dd4-0000", "name": "central-control-a3", "cwd": "Y:/cc"},
    {"sessionId": "35aa0000-0000", "name": "claude-lens-33", "cwd": "C:/l"},
]


def fake_api(history=()):
    def api(path, body=None, timeout=15):
        if path == "/api/sessions":
            return {"sessions": ROWS}
        if path.startswith("/api/history"):
            return {"results": list(history)}
        raise AssertionError("unexpected call %s" % path)
    return api


class Api(unittest.TestCase):
    def setUp(self):
        self._api = cli.api

    def tearDown(self):
        cli.api = self._api


class Help(unittest.TestCase):
    def test_every_command_is_listed_in_help_and_every_listed_one_exists(self):
        _ap, cmds = cli.build()
        listed = [n for _title, names in cli.SECTIONS for n in names]
        self.assertEqual(sorted(listed), sorted(cmds))
        self.assertEqual(len(listed), len(set(listed)))

    def test_help_prints_each_command_with_its_parameters(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(cli.main(["help"]), 0)
        text = out.getvalue()
        self.assertIn("python impctl.py send [-s SESSION] [-p PROJECT]", text)
        self.assertIn("python impctl.py restart [{all,dispatcher,streamdeck,hotkey}]", text)
        self.assertNotIn(" rest ...", text)

    def test_no_arguments_prints_help_too(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(cli.main([]), 0)
        self.assertIn("Service", out.getvalue())

    def test_an_unknown_help_topic_is_an_error_not_a_traceback(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            self.assertEqual(cli.main(["help", "bogus"]), 1)
        self.assertIn("no command", err.getvalue())


class Passthrough(unittest.TestCase):
    def setUp(self):
        self._call = cli.subprocess.call
        self.calls = []
        cli.subprocess.call = lambda cmd, cwd=None: self.calls.append(cmd) or 0

    def tearDown(self):
        cli.subprocess.call = self._call

    def test_leading_flags_reach_the_script(self):
        cli.main(["hooks", "--dry-run", "--remote", "pi"])
        self.assertEqual(self.calls[0][-3:], ["--dry-run", "--remote", "pi"])
        self.assertTrue(self.calls[0][1].endswith("install.py"))

    def test_remote_keeps_its_subcommand_and_flags(self):
        cli.main(["remote", "add", "me@host", "--label", "box"])
        self.assertEqual(self.calls[0][2:], ["remote", "add", "me@host", "--label", "box"])

    def test_unknown_flags_on_other_commands_are_still_refused(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            cli.main(["sessions", "--bogus"])


class Uninstall(unittest.TestCase):
    def setUp(self):
        self._call, self._confirmed = cli.subprocess.call, cli.confirmed
        self.calls = []
        cli.subprocess.call = lambda cmd, cwd=None: self.calls.append(cmd) or 0

    def tearDown(self):
        cli.subprocess.call, cli.confirmed = self._call, self._confirmed

    def test_a_no_runs_nothing(self):
        cli.confirmed = lambda what, rest: False
        self.assertEqual(cli.main(["uninstall"]), 1)
        self.assertEqual(cli.main(["service", "uninstall"]), 1)
        self.assertEqual(self.calls, [])

    def test_yes_skips_the_question(self):
        self.assertTrue(cli.confirmed("x", ["--yes"]))
        self.assertTrue(cli.confirmed("x", ["-y"]))

    def test_install_does_not_ask(self):
        cli.confirmed = lambda what, rest: self.fail("install asked")
        cli.main(["service", "install"])
        self.assertEqual(self.calls[0][2:], ["service", "install"])


class FindSession(Api):
    def test_by_exact_name(self):
        cli.api = fake_api()
        self.assertEqual(cli.find_session("claude-lens-33")["cwd"], "C:/l")

    def test_by_id_prefix(self):
        cli.api = fake_api()
        self.assertEqual(cli.find_session("3569")["name"], "central-control-a3")

    def test_by_window_title(self):
        cli.api = fake_api()
        self.assertEqual(cli.find_session("pickaxe")["name"], "bookoferrantpages-d0")

    def test_ambiguous_names_both(self):
        cli.api = fake_api()
        with self.assertRaises(cli.Fail) as got:
            cli.find_session("35")
        self.assertIn("central-control-a3", str(got.exception))

    def test_resume_falls_back_to_a_past_session_by_id_prefix(self):
        cli.api = fake_api([{"session_id": "42e2913a-1111", "cwd": "Y:/old"}])
        row = cli.find_any_session("42e2")
        self.assertEqual((row["sessionId"], row["cwd"]), ("42e2913a-1111", "Y:/old"))

    def test_a_past_session_that_does_not_match_the_prefix_is_not_taken(self):
        cli.api = fake_api([{"session_id": "42e2913a-1111", "cwd": "Y:/old"}])
        with self.assertRaises(cli.Fail):
            cli.find_any_session("zzzz")


class Send(Api):
    def test_to_a_session_injects_by_its_full_id(self):
        sent = []

        def api(path, body=None, timeout=15):
            if path == "/api/dispatch":
                sent.append(body)
                return {"ok": True, "action": "inject", "session": body["session_id"]}
            return fake_api()(path, body)
        cli.api = api
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(cli.main(["send", "-s", "claude-lens", "hello", "there"]), 0)
        self.assertEqual(sent[0], {"prompt": "hello there", "mode": "auto",
                                   "session_id": "35aa0000-0000"})

    def test_needs_exactly_one_target(self):
        cli.api = fake_api()
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(cli.main(["send", "hi"]), 1)
            self.assertEqual(cli.main(["send", "-s", "x", "-p", "y", "hi"]), 1)

    def test_a_refusal_exits_non_zero(self):
        cli.api = lambda path, body=None, timeout=15: {"ok": False, "error": "nope"}
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(cli.main(["send", "-p", "x", "hi"]), 1)
        self.assertIn("refused: nope", out.getvalue())


class Down(Api):
    def test_a_dead_dispatcher_says_how_to_start_it(self):
        def api(path, body=None, timeout=15):
            raise cli.Fail("the dispatcher is not responding - start it with "
                           "`python impctl.py start`")
        cli.api = api
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            self.assertEqual(cli.main(["sessions"]), 1)
        self.assertIn("impctl.py start", err.getvalue())


if __name__ == "__main__":
    unittest.main(verbosity=2)
