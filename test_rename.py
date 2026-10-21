"""The rename from ccontrol / Central Control (2026-10-20): what an install
from before it left behind is still found, moved or replaced.

Run: python test_rename.py
"""
import os
import shutil
import tempfile
import unittest
from unittest import mock

import setup_machine as sm
from hooks import cc_permission
from hooks import install as hook_install
from imperatorium import dispatcher, policy, usage


class StateFolder(unittest.TestCase):
    def setUp(self):
        self.base = tempfile.mkdtemp()
        self.env = mock.patch.dict(os.environ, {"LOCALAPPDATA": self.base})
        self.env.start()

    def tearDown(self):
        self.env.stop()
        shutil.rmtree(self.base, ignore_errors=True)

    def old(self, name="state.json"):
        os.makedirs(os.path.join(self.base, "ccontrol"), exist_ok=True)
        with open(os.path.join(self.base, "ccontrol", name), "w") as fh:
            fh.write("{}")

    def test_finding_the_old_folder_does_not_move_it(self):
        # An import (every test run) computes the path; only serve() moves.
        self.old()
        self.assertEqual(dispatcher.default_state_path(),
                         os.path.join(self.base, "ccontrol", "state.json"))
        self.assertTrue(os.path.isdir(os.path.join(self.base, "ccontrol")))

    def test_the_old_folder_is_moved_on_first_start(self):
        self.old()
        self.old("decisions.jsonl")
        dispatcher.migrate_state_dir()
        got = dispatcher.default_state_path()
        self.assertEqual(got, os.path.join(self.base, "imperatorium", "state.json"))
        self.assertTrue(os.path.exists(got))
        self.assertTrue(os.path.exists(os.path.join(self.base, "imperatorium", "decisions.jsonl")))
        self.assertFalse(os.path.exists(os.path.join(self.base, "ccontrol")))

    def test_a_folder_that_cannot_move_goes_on_being_used(self):
        self.old()
        with mock.patch.object(dispatcher.os, "rename", side_effect=OSError("in use")):
            dispatcher.migrate_state_dir()
        got = dispatcher.default_state_path()
        self.assertEqual(got, os.path.join(self.base, "ccontrol", "state.json"))
        # ... and the permission hook writes beside it, not somewhere new.
        self.assertEqual(cc_permission.state_dir(), os.path.join(self.base, "ccontrol"))

    def test_a_new_install_uses_the_new_name(self):
        self.assertEqual(dispatcher.default_state_path(),
                         os.path.join(self.base, "imperatorium", "state.json"))
        self.assertEqual(cc_permission.state_dir(), os.path.join(self.base, "imperatorium"))

    def test_once_moved_the_old_one_is_ignored(self):
        os.makedirs(os.path.join(self.base, "imperatorium"))
        self.old()
        self.assertEqual(dispatcher.default_state_path(),
                         os.path.join(self.base, "imperatorium", "state.json"))
        self.assertEqual(cc_permission.state_dir(), os.path.join(self.base, "imperatorium"))


class Hooks(unittest.TestCase):
    def test_an_entry_stamped_before_the_rename_is_still_ours(self):
        # On a remote host only the stamp says so: the script path differs.
        block = {"hooks": [{"type": "command", "command": "python3",
                            "args": ["/home/les/.claude/ccontrol/cc_hook.py"],
                            "statusMessage": "ccontrol"}]}
        self.assertTrue(hook_install.is_ours(block))

    def test_anyone_elses_is_not(self):
        self.assertFalse(hook_install.is_ours(
            {"hooks": [{"type": "command", "command": "x", "statusMessage": "other"}]}))

    def test_a_statusline_from_before_the_rename_still_reports(self):
        self.assertEqual(usage.provider_of({"ccontrol_provider": "bedrock"}), "bedrock")
        self.assertEqual(usage.provider_of({"imperatorium_provider": "vertex"}), "vertex")
        self.assertIn(dispatcher.LEGACY_STATUSLINE, ("ccontrol_statusline",))


class Policy(unittest.TestCase):
    def test_the_policy_code_is_protected_under_either_name(self):
        for path in ("imperatorium/policy.py", "imperatorium\\policy.py", "ccontrol/policy.py"):
            self.assertTrue(policy.is_self_protected("edit %s" % path), path)


class Units(unittest.TestCase):
    def test_the_old_unit_names_are_known_so_install_can_remove_them(self):
        names = [n for _k, _p, n in sm.unit_names("ccontrol")]
        new = [n for _k, _p, n in sm.unit_names()]
        if sm.MACOS:
            self.assertIn("local.ccontrol.dispatcher", names)
            self.assertIn("local.imperatorium.dispatcher", new)
        else:
            self.assertIn("ccontrol-dispatcher.service", names)
            self.assertIn("imperatorium-dispatcher.service", new)

    def test_a_host_from_before_the_rename_is_noticed(self):
        got = sm.parse_probe("home=/home/les\nlegacy=ok\n")
        self.assertEqual(got.get("legacy"), "ok")


class Tasks(unittest.TestCase):
    def test_install_and_uninstall_remove_the_old_task_names(self):
        here = os.path.dirname(os.path.abspath(__file__))
        with open(os.path.join(here, "service", "tasks.ps1"), encoding="utf-8") as fh:
            text = fh.read()
        for name in ("CentralControlDispatcher", "CentralControlStreamDeck", "CentralControlHotkey"):
            self.assertIn("'%s'" % name, text)
        self.assertEqual(text.count("+ $LegacyTasks"), 2)   # install and uninstall
        self.assertIn("(imperatorium|ccontrol)\\.dispatcher", text)


if __name__ == "__main__":
    unittest.main()
