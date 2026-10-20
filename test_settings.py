"""The Settings window: what it may change in config.json, and how.

Run: python test_settings.py
"""
import json
import os
import subprocess
import threading
import unittest
import urllib.error
import urllib.request

from ccontrol import dispatcher, settings, ui
from test_dispatcher import Base
from test_ui import NODE

CONFIG = {
    "_comment": "This machine's settings.",
    "roots": [],
    "background": {"_comment": "Options for claude --bg.", "permission_mode": "acceptEdits"},
    "notify": {"ntfy": {"enabled": False, "topic": "", "delay": 60, "token": "s3cret"},
               "speak": {"enabled": False, "delay": 5}},
    "budget": {"thresholds": [80, 95]},
    "remote": {"enabled": True, "hosts": [{"name": "devbox", "claude": "x"},
                                          {"name": "pi.local", "label": "pi", "tunnel": True}]},
}


class Checks(unittest.TestCase):
    def field(self, key):
        return settings.BY_KEY[key]

    def test_numbers_are_checked_against_their_range(self):
        f = self.field("notify.ntfy.delay")
        self.assertEqual(settings.check(f, "30"), 30)
        for bad in (-1, 90000, "soon", 1.5, True):
            with self.assertRaises(settings.Invalid):
                settings.check(f, bad)

    def test_thresholds_are_sorted_and_deduplicated(self):
        f = self.field("budget.thresholds")
        self.assertEqual(settings.check(f, "95, 50,50"), [50, 95])
        with self.assertRaises(settings.Invalid):
            settings.check(f, "120")

    def test_the_form_cannot_choose_a_bypass_mode(self):
        f = self.field("background.permission_mode")
        with self.assertRaises(settings.Invalid):
            settings.check(f, "bypassPermissions")
        # ...but one already in config.json is kept when saved unchanged.
        self.assertEqual(settings.check(f, "bypassPermissions", current="bypassPermissions"),
                         "bypassPermissions")

    def test_text_fields_refuse_what_they_should(self):
        with self.assertRaises(settings.Invalid):
            settings.check(self.field("notify.ntfy.server"), "ftp://x")
        with self.assertRaises(settings.Invalid):
            settings.check(self.field("notify.ntfy.topic"), "has spaces")
        with self.assertRaises(settings.Invalid):
            settings.check(self.field("background.model"), "opus --dangerously-skip-permissions")
        with self.assertRaises(settings.Invalid):
            settings.check(self.field("palette.hotkey"), ["ctrl alt; j"])
        with self.assertRaises(settings.Invalid):
            settings.check(self.field("roots"), ["Z:/no/such/folder/here"])

    def test_the_form_shows_values_and_defaults_but_never_the_token(self):
        d = settings.describe(CONFIG)
        by = {f["key"]: f for f in d["fields"]}
        self.assertEqual(by["background.permission_mode"]["value"], "acceptEdits")
        self.assertEqual(by["notify.ntfy.server"]["value"], "https://ntfy.sh")  # a default
        self.assertFalse(by["voice.enabled"]["value"])   # no voice block: off
        self.assertNotIn("s3cret", json.dumps(d))
        self.assertEqual([h["name"] for h in d["hosts"]], ["devbox", "pi.local"])
        locked = settings.describe({"background": {"permission_mode": "bypassPermissions"}})
        f = next(f for f in locked["fields"] if f["key"] == "background.permission_mode")
        self.assertTrue(f["locked"])
        self.assertIn("bypassPermissions", f["choices"])

    def test_apply_keeps_comments_and_drops_a_blank_background_option(self):
        new = settings.apply(CONFIG, {"background.permission_mode": "",
                                      "voice.max_seconds": 60})
        self.assertNotIn("permission_mode", new["background"])
        self.assertEqual(new["background"]["_comment"], "Options for claude --bg.")
        self.assertEqual(new["voice"], {"max_seconds": 60})
        self.assertEqual(new["notify"]["ntfy"]["token"], "s3cret")
        self.assertEqual(CONFIG["background"]["permission_mode"], "acceptEdits")  # not mutated

    def test_one_bad_value_saves_nothing(self):
        with self.assertRaises(settings.Invalid) as got:
            settings.apply(CONFIG, {"notify.ntfy.delay": 5, "notify.speak.rate": 99,
                                    "nonsense": 1})
        self.assertEqual(set(got.exception.args[0]), {"notify.speak.rate", "nonsense"})

    def test_restarts_name_each_process_once(self):
        self.assertEqual(settings.restarts(["voice.model", "remote.enabled", "palette.hotkey",
                                            "notify.ntfy.delay"]), ["dispatcher", "hotkey"])
        self.assertEqual(settings.restarts(["budget.thresholds"]), [])


class Saving(Base):
    def setUp(self):
        super().setUp()
        self.path = os.path.join(self.dir, "config.json")
        with open(self.path, "w", encoding="utf-8") as fh:
            json.dump(CONFIG, fh)
        self.control.config_path = self.path

    def on_disk(self):
        with open(self.path, encoding="utf-8") as fh:
            return json.load(fh)

    def test_a_save_writes_the_file_and_keeps_the_old_one(self):
        got = self.control.save_settings({"voice.model": "small", "notify.ntfy.delay": 30})
        self.assertTrue(got["ok"], got)
        disk = self.on_disk()
        self.assertEqual(disk["voice"]["model"], "small")
        self.assertEqual(disk["_comment"], "This machine's settings.")
        with open(self.path + ".bak", encoding="utf-8") as fh:
            self.assertEqual(json.load(fh), CONFIG)
        self.assertEqual(got["restart"], ["dispatcher"])
        self.assertEqual(self.control.settings_view()["waiting"], ["dispatcher"])

    def test_live_settings_apply_at_once(self):
        got = self.control.save_settings({"budget.thresholds": [50, 90],
                                          "background.model": "opus",
                                          "notify.speak.enabled": True,
                                          "notify.ntfy.delay": 15})
        self.assertEqual(got["restart"], [])
        self.assertEqual(self.control.usage.thresholds, (50.0, 90.0))
        self.assertEqual(self.control.background_options["model"], "opus")
        self.assertTrue(self.control.notifier.enabled("speak"))
        self.assertEqual(self.control.notifier.cfg["ntfy"]["delay"], 15)
        # Kept where /api/notify keeps it, which wins at start.
        self.assertTrue(self.control.store.prefs()["notify"]["speak"])

    def test_a_toggle_made_through_api_notify_shows_as_it_is(self):
        self.control.notifier.set_enabled("speak", True)
        view = self.control.settings_view()
        f = next(f for f in view["fields"] if f["key"] == "notify.speak.enabled")
        self.assertTrue(f["value"])
        # Saving something else does not switch it back off.
        self.control.save_settings({"notify.speak.delay": 9})
        self.assertTrue(self.control.notifier.enabled("speak"))

    def test_the_form_carries_the_last_notification_error(self):
        self.assertIsNone(self.control.settings_view()["notify_error"])
        self.control.notifier.last_error = "ntfy: HTTP 403"
        self.assertEqual(self.control.settings_view()["notify_error"], "ntfy: HTTP 403")

    def test_errors_come_back_by_field_and_the_file_is_untouched(self):
        got = self.control.save_settings({"notify.ntfy.delay": -5})
        self.assertFalse(got["ok"])
        self.assertIn("notify.ntfy.delay", got["errors"])
        self.assertEqual(self.on_disk(), CONFIG)
        self.assertFalse(os.path.exists(self.path + ".bak"))

    def test_a_broken_file_is_not_overwritten(self):
        with open(self.path, "w", encoding="utf-8") as fh:
            fh.write("{ not json")
        self.assertFalse(self.control.save_settings({"notify.ntfy.delay": 5})["ok"])
        with open(self.path, encoding="utf-8") as fh:
            self.assertEqual(fh.read(), "{ not json")

    def test_without_a_config_file_there_is_no_window(self):
        self.control.config_path = None
        self.assertFalse(self.control.settings_view()["ok"])
        self.assertFalse(self.control.save_settings({"notify.ntfy.delay": 5})["ok"])


class Routes(Saving):
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

    def call(self, path, body=None, origin=None):
        headers = {"Content-Type": "application/json"}
        if origin:
            headers["Origin"] = origin
        req = urllib.request.Request("http://127.0.0.1:%d%s" % (self.port, path), headers=headers,
                                     data=None if body is None else json.dumps(body).encode())
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                return resp.status, resp.read().decode("utf-8")
        except urllib.error.HTTPError as exc:
            return exc.code, exc.read().decode("utf-8")

    def test_the_page_and_the_form_are_served(self):
        status, page = self.call("/settings")
        self.assertEqual(status, 200)
        self.assertIn("<title>Imperatorium Settings</title>", page)
        status, body = self.call("/api/settings")
        self.assertTrue(json.loads(body)["ok"])

    def test_the_windows_own_page_may_save_and_a_foreign_one_may_not(self):
        own = "http://127.0.0.1:%d" % self.port
        status, body = self.call("/api/settings", {"changes": {"notify.ntfy.delay": 20}}, own)
        self.assertTrue(json.loads(body)["ok"])
        status, _ = self.call("/api/settings", {"changes": {"notify.ntfy.delay": 1}},
                              "https://evil.example")
        self.assertEqual(status, 403)
        self.assertEqual(self.on_disk()["notify"]["ntfy"]["delay"], 20)

    def test_a_remote_hosts_tunnel_cannot_reach_it(self):
        dispatcher.HookOnlyHandler.control = self.control
        srv = dispatcher.Server(("127.0.0.1", 0), dispatcher.HookOnlyHandler)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        try:
            req = urllib.request.Request(
                "http://127.0.0.1:%d/api/settings" % srv.server_address[1],
                data=json.dumps({"changes": {"notify.ntfy.delay": 1}}).encode(),
                headers={"Content-Type": "application/json"})
            with self.assertRaises(urllib.error.HTTPError):
                urllib.request.urlopen(req, timeout=5)
        finally:
            srv.shutdown()
            srv.server_close()
        self.assertEqual(self.on_disk()["notify"]["ntfy"]["delay"], 60)


class Button(unittest.TestCase):
    def test_the_gear_sits_left_of_the_matrix_button(self):
        shell = ui.render_shell()
        self.assertLess(shell.index("id=setbtn"), shell.index("id=matrixbtn"))
        self.assertIn('aria-label="Settings"', shell)
        self.assertIn('window.open("/settings", "imperatorium-settings"', ui.SCRIPT)

    @unittest.skipUnless(NODE, "node is not installed")
    def test_the_settings_script_parses(self):
        out = subprocess.run([NODE, "-e", "new Function(require('fs').readFileSync(0, 'utf8'))"],
                             input=ui.SETTINGS_SCRIPT, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        self.assertEqual(out.returncode, 0, out.stderr)

    def test_the_settings_css_has_no_control_characters(self):
        self.assertFalse([c for c in ui.SETTINGS_CSS if ord(c) < 32 and c not in "\n\t"])
        self.assertIn('content:" \\2022"', ui.SETTINGS_CSS)


if __name__ == "__main__":
    unittest.main()
