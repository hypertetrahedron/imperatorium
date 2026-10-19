"""Quick phrases and the shortcut list in the Settings window."""
import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest

from ccontrol import dispatcher, settings, ui
from ccontrol.store import Store

NODE = shutil.which("node")
KEY, PHRASES = settings.BY_KEY["quick.key"], settings.BY_KEY["quick.phrases"]


class KeySetting(unittest.TestCase):
    def test_a_combination_is_stored_in_the_pages_order(self):
        self.assertEqual(settings.check(KEY, "Shift + Ctrl + Q"), "ctrl+shift+q")
        self.assertEqual(settings.check(KEY, "alt+space"), "alt+space")

    def test_shift_alone_or_no_modifier_is_refused(self):
        for v in ("q", "shift+q", "ctrl", "ctrl+alt", "ctrl+"):
            with self.assertRaises(settings.Invalid, msg=v):
                settings.check(KEY, v)

    def test_a_key_the_prompt_box_already_uses_is_refused(self):
        for v in ("ctrl+enter", "alt+enter", "ctrl+k", "alt+3"):
            with self.assertRaises(settings.Invalid, msg=v) as cm:
                settings.check(KEY, v)
            self.assertIn("already", str(cm.exception))


class PhrasesSetting(unittest.TestCase):
    def test_one_per_line_and_commas_are_text(self):
        got = settings.check(PHRASES, "continue, as recommended\n\n  implement as recommended \n")
        self.assertEqual(got, ["continue, as recommended", "implement as recommended"])

    def test_a_list_from_the_form(self):
        self.assertEqual(settings.check(PHRASES, ["a, b", " ", "c"]), ["a, b", "c"])

    def test_limits(self):
        with self.assertRaises(settings.Invalid):
            settings.check(PHRASES, ["x"] * 51)
        with self.assertRaises(settings.Invalid):
            settings.check(PHRASES, ["x" * 1001])

    def test_defaults_include_the_common_ones(self):
        self.assertIn("implement as recommended", PHRASES["default"])
        self.assertEqual(KEY["default"], "ctrl+space")


class Shortcuts(unittest.TestCase):
    def test_a_key_named_by_a_setting_is_a_real_setting(self):
        for _where, keys, _what in ui.SHORTCUTS:
            if re.match(r"^[a-z]+\.[a-z_]+$", keys):
                self.assertIn(keys, settings.BY_KEY)

    def test_every_reserved_key_is_on_the_list(self):
        # The list is what people read; a key the setting refuses should be
        # one they can find there.
        listed = " ".join(k.lower() for _w, k, _t in ui.SHORTCUTS)
        for combo in ("ctrl+enter", "alt+enter", "ctrl+k", "ctrl+f", "alt+n"):
            self.assertIn(combo, listed)
        self.assertIn("alt+up", listed)


class Live(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.path = os.path.join(self.dir, "config.json")
        with open(self.path, "w", encoding="utf-8") as fh:
            json.dump({"quick": {"key": "alt+q", "phrases": ["one"]}}, fh)
        with open(self.path, encoding="utf-8") as fh:
            cfg = json.load(fh)
        self.control = dispatcher.Control(Store(os.path.join(self.dir, "s.json")),
                                          roots=[], config=cfg)
        self.control.config_path = self.path

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def test_the_page_gets_the_configured_phrases(self):
        self.assertEqual(self.control.quick, {"key": "alt+q", "phrases": ["one"]})

    def test_a_saved_change_applies_at_once(self):
        got = self.control.save_settings({"quick.phrases": "two\nthree, four",
                                          "quick.key": "ctrl+shift+space"})
        self.assertTrue(got["ok"], got)
        self.assertEqual(self.control.quick, {"key": "ctrl+shift+space",
                                              "phrases": ["two", "three, four"]})

    def test_the_settings_window_gets_the_shortcut_list(self):
        view = self.control.settings_view()
        self.assertEqual(len(view["shortcuts"]), len(ui.SHORTCUTS))
        self.assertIn("quick", [s["id"] for s in view["sections"]])


@unittest.skipUnless(NODE, "node not installed")
class ComboOf(unittest.TestCase):
    def combos(self, events):
        s = ui.SHARED
        js = (s[s.index("function comboOf("):s.index("// \"ctrl+space\" as people")] +
              "process.stdout.write(JSON.stringify(%s.map(comboOf)));" % json.dumps(events))
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return json.loads(out.stdout)

    def test_matches_what_the_setting_stores(self):
        got = self.combos([
            {"code": "Space", "ctrlKey": True},
            {"code": "KeyQ", "ctrlKey": True, "shiftKey": True},
            {"code": "Digit1", "ctrlKey": True},
            {"code": "Period", "metaKey": True},
            {"code": "ControlLeft", "ctrlKey": True},
        ])
        self.assertEqual(got, ["ctrl+space", "ctrl+shift+q", "ctrl+1", "win+period", ""])
        for combo in got[:4]:
            self.assertEqual(settings.check(KEY, combo), combo)


if __name__ == "__main__":
    unittest.main()
