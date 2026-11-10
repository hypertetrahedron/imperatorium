"""Installing the Imperatorium mods and layout into a herdr-app home.

Every test uses a temporary home; nothing touches ~/.config/herdr-app.

Run: python test_herdrsetup.py
"""
import json
import os
import shutil
import tempfile
import unittest

import setup_machine
from imperatorium import herdrsetup

REPO = os.path.dirname(os.path.abspath(__file__))


class Install(unittest.TestCase):
    def setUp(self):
        self.home = tempfile.mkdtemp()
        self.said = []

    def tearDown(self):
        shutil.rmtree(self.home, ignore_errors=True)

    def install(self, **kw):
        return herdrsetup.install(REPO, self.home, say=self.said.append, **kw)

    def read(self, rel):
        with open(os.path.join(self.home, rel), encoding="utf-8") as fh:
            return json.load(fh)

    def test_mods_layout_config_and_telemetry(self):
        self.install(url="http://127.0.0.1:9999/", token_file="C:\\x\\herdr-token")
        for name in ("imperatorium-chat", "imperatorium-sessions", "imperatorium-matrix"):
            self.assertTrue(os.path.isfile(os.path.join(self.home, "mods", name, "main.js")), name)
        self.assertTrue(os.path.isfile(os.path.join(self.home, "layouts", "imperatorium.json")))
        self.assertEqual(self.read("imperatorium.json"),
                         {"url": "http://127.0.0.1:9999", "token_file": "C:/x/herdr-token"})
        self.assertIs(self.read("settings.json")["telemetry"], False)
        st = herdrsetup.status(REPO, self.home)
        self.assertEqual(set(st["mods"].values()), {"current"})
        self.assertTrue(st["telemetry_off"])

    def test_other_settings_are_kept_and_backed_up(self):
        with open(os.path.join(self.home, "settings.json"), "w", encoding="utf-8") as fh:
            json.dump({"zoom": 1.2, "telemetry": True}, fh)
        self.install()
        self.assertEqual(self.read("settings.json"), {"zoom": 1.2, "telemetry": False})
        self.assertEqual(self.read("settings.json.bak"), {"zoom": 1.2, "telemetry": True})

    def test_a_broken_settings_file_is_not_overwritten(self):
        with open(os.path.join(self.home, "settings.json"), "w", encoding="utf-8") as fh:
            fh.write("{not json")
        with self.assertRaises(ValueError):
            self.install()
        with open(os.path.join(self.home, "settings.json"), encoding="utf-8") as fh:
            self.assertEqual(fh.read(), "{not json")

    def test_a_stale_copy_is_reported_and_replaced(self):
        self.install()
        p = os.path.join(self.home, "mods", "imperatorium-chat", "main.js")
        with open(p, "a", encoding="utf-8") as fh:
            fh.write("// edited\n")
        self.assertEqual(herdrsetup.status(REPO, self.home)["mods"]["imperatorium-chat"], "stale")
        self.install()
        self.assertEqual(herdrsetup.status(REPO, self.home)["mods"]["imperatorium-chat"], "current")

    def test_uninstall_removes_only_ours(self):
        os.makedirs(os.path.join(self.home, "mods", "agents"))
        self.install()
        herdrsetup.uninstall(self.home, say=self.said.append)
        self.assertEqual(os.listdir(os.path.join(self.home, "mods")), ["agents"])
        self.assertFalse(os.path.exists(os.path.join(self.home, "imperatorium.json")))
        self.assertIs(self.read("settings.json")["telemetry"], False, "stays off")

    def test_the_layout_names_only_mods_that_exist(self):
        layout = self.read_layout()
        shipped = {"workspace-chrome", "spaces", "tabs", "pane", "files", "changes",
                   "workspace-status"}
        ours = set(herdrsetup.our_mods(REPO))
        for mod in self.mods_in(layout):
            self.assertIn(mod, shipped | ours)

    def read_layout(self):
        with open(os.path.join(REPO, "herdr", "layouts", "imperatorium.json"), encoding="utf-8") as fh:
            return json.load(fh)

    def mods_in(self, node):
        out = []
        content = node.get("content") or {}
        if content.get("mod"):
            out.append(content["mod"])
        header = ((content.get("pane") or {}).get("header") or {}).get("content") or {}
        if header.get("mod"):
            out.append(header["mod"])
        for ch in node.get("children") or []:
            out += self.mods_in(ch)
        return out


class Command(unittest.TestCase):
    def test_setup_machine_herdr_install_and_status(self):
        home = tempfile.mkdtemp()
        try:
            self.assertEqual(setup_machine.main(["herdr", "install", "--home", home,
                                                 "--token-file", "C:/t"]), 0)
            self.assertEqual(setup_machine.main(["herdr", "status", "--home", home]), 0)
            self.assertEqual(setup_machine.main(["herdr", "uninstall", "--home", home]), 0)
            self.assertEqual(setup_machine.main(["herdr", "status", "--home", home]), 1)
        finally:
            shutil.rmtree(home, ignore_errors=True)

    def test_the_token_path_matches_the_dispatcher(self):
        from imperatorium import dispatcher
        self.assertEqual(os.path.normcase(herdrsetup.token_path()),
                         os.path.normcase(dispatcher.herdr_token_path(dispatcher.default_state_path())))


if __name__ == "__main__":
    unittest.main()
