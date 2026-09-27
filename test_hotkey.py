"""The global hotkey: parsing, candidate selection, and finding the window.

What survived the palette. Registering a hotkey and raising a window need
Windows and a live window, so those are exercised by running it, not here;
these are the parts that were wrong before - an off-by-one in the function
keys binds the wrong key and looks exactly like "the hotkey does nothing".

Run: python test_hotkey.py
"""
import json
import os
import shutil
import tempfile
import unittest

import hotkey


class Parsing(unittest.TestCase):
    def test_modifiers_combine(self):
        mods, vk = hotkey.parse_hotkey("ctrl+alt+space")
        self.assertEqual(mods, hotkey.MOD_CONTROL | hotkey.MOD_ALT)
        self.assertEqual(vk, 0x20)

    def test_function_keys(self):
        # VK_F1 is 0x70, so F12 must be 0x7B.
        self.assertEqual(hotkey.parse_hotkey("ctrl+shift+f12")[1], 0x7B)
        self.assertEqual(hotkey.parse_hotkey("f1")[1], 0x70)

    def test_letters_become_virtual_keys(self):
        self.assertEqual(hotkey.parse_hotkey("ctrl+alt+j")[1], ord("J"))

    def test_backquote_by_name_or_symbol(self):
        self.assertEqual(hotkey.parse_hotkey("ctrl+alt+backquote")[1], 0xC0)
        self.assertEqual(hotkey.parse_hotkey("ctrl+alt+`")[1], 0xC0)

    def test_win_modifier(self):
        self.assertEqual(hotkey.parse_hotkey("win+k")[0], hotkey.MOD_WIN)

    def test_nonsense_is_refused_rather_than_silently_bound(self):
        for bad in ("", "ctrl+alt", "ctrl+alt+semicolon", None):
            with self.assertRaises(ValueError):
                hotkey.parse_hotkey(bad)

    def test_every_default_candidate_parses(self):
        for spec in hotkey.DEFAULT_HOTKEYS:
            hotkey.parse_hotkey(spec)


class Candidates(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.path = os.path.join(self.dir, "config.json")
        self._orig = hotkey.CONFIG
        hotkey.CONFIG = self.path

    def tearDown(self):
        hotkey.CONFIG = self._orig
        shutil.rmtree(self.dir, ignore_errors=True)

    def write(self, value):
        with open(self.path, "w", encoding="utf-8") as fh:
            json.dump({"palette": {"hotkey": value}}, fh)

    def test_an_explicit_override_wins(self):
        self.write("ctrl+alt+j")
        self.assertEqual(hotkey.candidates("win+k"), ["win+k"])

    def test_a_single_string_becomes_one_candidate(self):
        self.write("ctrl+alt+j")
        self.assertEqual(hotkey.candidates(), ["ctrl+alt+j"])

    def test_a_list_is_tried_in_order(self):
        # Conflicts are the norm, so config may name several.
        self.write(["win+k", "ctrl+alt+j"])
        self.assertEqual(hotkey.candidates(), ["win+k", "ctrl+alt+j"])

    def test_missing_or_broken_config_falls_back_to_defaults(self):
        self.assertEqual(hotkey.candidates(), hotkey.DEFAULT_HOTKEYS)
        with open(self.path, "w", encoding="utf-8") as fh:
            fh.write("{not json")
        self.assertEqual(hotkey.candidates(), hotkey.DEFAULT_HOTKEYS)

    def test_an_empty_list_falls_back_rather_than_binding_nothing(self):
        self.write([])
        self.assertEqual(hotkey.candidates(), hotkey.DEFAULT_HOTKEYS)


class WindowLookup(unittest.TestCase):
    @unittest.skipUnless(os.name == "nt", "window lookup is Windows-only")
    def test_find_window_returns_a_handle_or_none(self):
        found = hotkey.find_window()
        self.assertTrue(found is None or isinstance(found, int))

    def test_the_title_it_looks_for_matches_the_page(self):
        # If these drift apart the hotkey silently opens a second window
        # every time instead of raising the one already there.
        from ccontrol import ui

        self.assertIn(hotkey.WINDOW_TITLE, ui.render_shell())


if __name__ == "__main__":
    unittest.main(verbosity=2)
