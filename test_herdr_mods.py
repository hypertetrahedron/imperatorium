"""The herdr-app mods' pure modules, run under node when it is installed.

The mods themselves need herdr-app to load (`herdr-app check <mod>`); what
can be tested without it lives in plain modules such as rows.js.

Run: python test_herdr_mods.py
"""
import os
import shutil
import subprocess
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TESTS = os.path.join(HERE, "herdr", "tests")


@unittest.skipUnless(shutil.which("node"), "node is not installed")
class ModModules(unittest.TestCase):
    def test_each_node_test_passes(self):
        found = sorted(f for f in os.listdir(TESTS) if f.endswith(".test.mjs"))
        self.assertTrue(found)
        for name in found:
            got = subprocess.run(["node", os.path.join(TESTS, name)], capture_output=True,
                                 text=True, timeout=60)
            self.assertEqual(got.returncode, 0, name + ":\n" + got.stdout + got.stderr)


if __name__ == "__main__":
    unittest.main()
