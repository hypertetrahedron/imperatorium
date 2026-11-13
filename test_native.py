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


if __name__ == "__main__":
    unittest.main()
