"""The native port's wiring on the Python side: hooks/install.py --native.

The port's behaviour is checked by rust/conformance (run_all.py), which
needs a built binary; this file needs nothing but Python.

Run: python test_native.py
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "hooks"))
import install  # noqa: E402

BIN = os.path.join("C:", os.sep, "Users", "x", "AppData", "Local", "Programs",
                   "imperatorium", "imperatorium.exe")


class NativeHooks(unittest.TestCase):
    def test_every_hook_runs_the_binary(self):
        settings, _ = install.apply({}, native=BIN)
        for event, blocks in settings["hooks"].items():
            hook = blocks[0]["hooks"][0]
            self.assertEqual(hook["command"], BIN, event)
            self.assertEqual(hook["statusMessage"], install.MARK)
        report = settings["hooks"]["SessionStart"][0]["hooks"][0]
        self.assertEqual(report["args"], ["hook"])
        self.assertTrue(report["async"])
        permission = settings["hooks"]["PermissionRequest"][0]["hooks"][0]
        self.assertEqual(permission["args"][:2], ["permission", "--home"])
        # The same policy.json the Python hook reads.
        self.assertEqual(os.path.normcase(permission["args"][2]),
                         os.path.normcase(os.path.dirname(install.HERE)))
        self.assertNotIn("async", permission)

    def test_switching_replaces_the_python_hooks(self):
        python, _ = install.apply({})
        native, changes = install.apply(python, native=BIN)
        self.assertTrue(all(c.startswith("replace") for c in changes), changes)
        for blocks in native["hooks"].values():
            self.assertEqual(len(blocks), 1)
        back, _ = install.apply(native)
        self.assertEqual(back["hooks"]["Stop"][0]["hooks"][0]["args"], [install.HOOK_SCRIPT])

    def test_uninstall_finds_native_hooks(self):
        settings, _ = install.apply({"hooks": {"Stop": [{"hooks": [{"command": "other"}]}]}},
                                    native=BIN)
        cleaned, _ = install.apply(settings, uninstall=True)
        self.assertEqual(cleaned["hooks"], {"Stop": [{"hooks": [{"command": "other"}]}]})

    def test_native_statusline_is_recognised_as_ours(self):
        settings = {}
        install.apply_statusline(settings, native=BIN)
        self.assertEqual(settings["statusLine"]["command"], '"%s" statusline' % BIN)
        self.assertTrue(install.is_our_statusline(settings["statusLine"]))
        self.assertEqual(install.apply_statusline(settings, uninstall=True), ["remove statusLine"])
        self.assertFalse(install.is_our_statusline({"command": "my-statusline.sh"}))
        self.assertFalse(install.is_our_statusline({"command": "imperatorium-ish thing"}))


class NativeReader(unittest.TestCase):
    """conversation.py and history.py on the binary, with it stubbed."""

    def setUp(self):
        from imperatorium import conversation, native
        self.conv, self.native = conversation, native
        self.saved = (native.available, native.session_chain, native.session_titles,
                      native.session_info, native.session_list)
        native.available = lambda: True

    def tearDown(self):
        n = self.native
        (n.available, n.session_chain, n.session_titles, n.session_info,
         n.session_list) = self.saved

    def test_the_chain_is_classified_as_before(self):
        self.native.session_chain = lambda sid: [
            {"type": "user", "uuid": "1", "sessionId": "s",
             "message": {"role": "user", "content": "hello"}},
            {"type": "assistant", "uuid": "2", "sessionId": "s",
             "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}]}},
            {"type": "user", "uuid": "3", "isMeta": True, "origin": {"kind": "peer"},
             "message": {"role": "user", "content": "from the window"}},
            {"type": "attachment", "uuid": "4", "attachment": {
                "type": "queued_command", "prompt": "queued one"}},
            {"type": "user", "uuid": "5", "isMeta": True,
             "message": {"role": "user", "content": "hidden meta"}},
            {"type": "user", "uuid": "6", "isCompactSummary": True,
             "message": {"role": "user", "content": "summary"}},
        ]
        msgs = self.conv._native_messages("s")
        self.assertEqual([m.uuid for m in msgs], ["1", "2", "3", "4", "6"])
        self.assertEqual(msgs[2].via, "inbox")
        self.assertEqual(msgs[4].system, self.conv.NOTE_SUMMARY)
        items = self.conv.read("s")
        self.assertTrue(items)

    def test_a_failing_binary_is_an_error_not_an_empty_chat(self):
        def boom(sid):
            raise self.native.Unavailable("not UTF-8")
        self.native.session_chain = boom
        with self.assertRaises(RuntimeError):
            self.conv.read("s")

    def test_titles_take_one_call(self):
        calls = []
        self.native.session_titles = lambda ids: calls.append(ids) or {"a": "Alpha"}
        self.assertEqual(self.conv.titles(["a", "b", ""]), {"a": "Alpha", "b": ""})
        self.assertEqual(calls, [["a", "b"]])

    def test_history_lists_through_the_binary(self):
        from imperatorium import history
        self.native.session_list = lambda: [{
            "session_id": "x", "summary": "Sum", "custom_title": None, "first_prompt": "p",
            "cwd": "C:/w", "git_branch": "main", "last_modified": 2000, "created_at": 1000,
            "file_size": 10, "tag": None}]
        rows = [history.info_dict(i) for i in history.list_sessions()]
        self.assertEqual(rows[0]["title"], "Sum")
        self.assertEqual(rows[0]["modified"], 2.0)

    def test_the_binary_is_looked_for_where_it_is_installed(self):
        from imperatorium import native
        found = native.candidates({"IMPERATORIUM_BIN": "X", "LOCALAPPDATA": "L"})
        self.assertEqual(found[0], "X")
        if sys.platform == "win32":
            self.assertEqual(found[1], os.path.join("L", "Programs", "imperatorium",
                                                    "imperatorium.exe"))


class EmbeddedPages(unittest.TestCase):
    """The binary embeds the settings schema, written from settings.py by
    rust/assets/export_ui.py. A change to settings.py or ui.SHORTCUTS must be
    exported, or the native dispatcher would show yesterday's settings. (The
    pages themselves are compiled from imperatorium/web/ directly.)"""

    def test_the_exported_schema_is_current(self):
        import subprocess
        here = os.path.dirname(os.path.abspath(__file__))
        done = subprocess.run([sys.executable, os.path.join(here, "rust", "assets", "export_ui.py"),
                               "--check"], capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)


if __name__ == "__main__":
    unittest.main()
