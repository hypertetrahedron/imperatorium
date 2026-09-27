"""setup_machine.py's pure pieces: versions, generated settings, the hidapi check.

The install itself was run end to end into a temporary copy of the repository
with CLAUDE_CONFIG_DIR pointed at a scratch folder (2026-09-25). These tests
pin down the decisions it makes along the way.

Run: python test_setup.py
"""
import io
import json
import os
import shutil
import tempfile
import unittest
import zipfile

import setup_machine as sm

HERE = os.path.dirname(os.path.abspath(__file__))


def example(name):
    with open(os.path.join(HERE, name), encoding="utf-8") as fh:
        return json.load(fh)


class Versions(unittest.TestCase):
    def test_reads_claude_code_version_output(self):
        self.assertEqual(sm.parse_version("2.1.283 (Claude Code)"), (2, 1, 283))

    def test_nothing_to_read_is_none(self):
        self.assertIsNone(sm.parse_version(""))
        self.assertIsNone(sm.parse_version("command not found"))

    def test_minimum_is_compared_numerically(self):
        # 2.1.1000 is newer than 2.1.234, which a string compare gets wrong.
        self.assertTrue(sm.parse_version("2.1.1000") >= (2, 1, 234))
        self.assertFalse(sm.parse_version("2.1.99") >= (2, 1, 234))


class Settings(unittest.TestCase):
    def test_config_takes_the_chosen_roots_and_voice(self):
        cfg = sm.build_config(example("config.example.json"), ["D:/code"], voice=True)
        self.assertEqual(cfg["roots"], ["D:/code"])
        self.assertTrue(cfg["voice"]["enabled"])

    def test_config_example_carries_nothing_from_this_machine(self):
        text = json.dumps(example("config.example.json"))
        for personal in ("projects-software", "developmenthost", "raspberrypi", "Les"):
            self.assertNotIn(personal, text)

    def test_policy_example_carries_no_project_paths(self):
        self.assertEqual(example("policy.example.json")["projects"], {})

    def test_auto_approval_is_opt_in(self):
        base = example("policy.example.json")
        self.assertFalse(base["enabled"])
        self.assertFalse(sm.build_policy(base, auto_approve=False)["enabled"])
        self.assertTrue(sm.build_policy(base, auto_approve=True)["enabled"])

    def test_answers_from_the_window_work_without_auto_approval(self):
        # The reply format and parking are independent of `enabled`: with
        # auto-approval off every request is `ask`, which is what parks.
        pol = sm.build_policy(example("policy.example.json"), auto_approve=False)
        self.assertEqual(pol["decision_format"], "object")
        self.assertTrue(pol["park"]["enabled"])

    def test_generated_settings_do_not_alias_the_example(self):
        base = example("config.example.json")
        sm.build_config(base, ["X:/"], voice=True)
        self.assertEqual(base["roots"], [])


class Hidapi(unittest.TestCase):
    def fake_zip(self, dll=b"not the real dll"):
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as zf:
            zf.writestr(sm.HIDAPI_MEMBER, dll)
        return buf.getvalue()

    def test_an_archive_with_the_wrong_hash_is_refused(self):
        with self.assertRaises(ValueError):
            sm.extract_hidapi(self.fake_zip())

    def test_a_matching_archive_with_a_wrong_dll_is_refused(self):
        data = self.fake_zip()
        orig = sm.HIDAPI_ZIP_SHA256
        sm.HIDAPI_ZIP_SHA256 = __import__("hashlib").sha256(data).hexdigest()
        try:
            with self.assertRaises(ValueError):
                sm.extract_hidapi(data)
        finally:
            sm.HIDAPI_ZIP_SHA256 = orig


class Choices(unittest.TestCase):
    def test_yes_takes_every_default_without_reading_input(self):
        self.assertTrue(sm.ask("q", True, assume_yes=True))
        self.assertFalse(sm.ask("q", False, assume_yes=True))
        self.assertEqual(sm.ask_text("q", "D:/code", assume_yes=True), "D:/code")

    def test_default_root_is_the_checkouts_parent(self):
        self.assertEqual(sm.default_roots(), [os.path.dirname(HERE)])

    def test_a_checkout_in_home_prefers_a_code_folder(self):
        # The bootstrap scripts clone into ~/imperatorium; every folder in
        # home is not a project, so a conventional code folder wins.
        home = tempfile.mkdtemp()
        try:
            here = os.path.join(home, "imperatorium")
            self.assertEqual(sm.default_roots(here, home), [home])
            os.makedirs(os.path.join(home, "src"))
            self.assertEqual(sm.default_roots(here, home), [os.path.join(home, "src")])
        finally:
            shutil.rmtree(home)


class Ntfy(unittest.TestCase):
    def test_topics_are_unguessable_and_distinct(self):
        a, b = sm.ntfy_topic(), sm.ntfy_topic()
        self.assertNotEqual(a, b)
        self.assertGreaterEqual(len(a), len("imperatorium-") + 16)

    def test_choosing_push_switches_it_on_with_the_topic(self):
        cfg = sm.build_config(example("config.example.json"), ["X:/"], False, ntfy="t-123")
        self.assertEqual(cfg["notify"]["ntfy"]["topic"], "t-123")
        self.assertTrue(cfg["notify"]["ntfy"]["enabled"])

    def test_push_stays_off_by_default(self):
        cfg = sm.build_config(example("config.example.json"), ["X:/"], False)
        self.assertFalse(cfg["notify"]["ntfy"]["enabled"])


class Service(unittest.TestCase):
    def test_systemd_unit_restarts_and_quotes_paths_with_spaces(self):
        unit = sm.systemd_unit("/opt/my py/python", ["-m", "ccontrol.dispatcher"],
                               "the dispatcher", workdir="/srv/imp", path="/usr/bin:/bin")
        self.assertIn('ExecStart="/opt/my py/python" "-m" "ccontrol.dispatcher"', unit)
        self.assertIn("WorkingDirectory=/srv/imp", unit)
        self.assertIn("Restart=always", unit)
        self.assertIn('Environment="PATH=/usr/bin:/bin"', unit)
        self.assertIn("WantedBy=default.target", unit)

    def test_launchd_plist_is_valid_xml_and_keeps_alive(self):
        import plistlib
        body = sm.launchd_plist("local.ccontrol.dispatcher", "/usr/bin/python3",
                                ["-m", "ccontrol.dispatcher"], workdir="/Users/a & b/imp",
                                path="/usr/bin", log="/tmp/x.log")
        plist = plistlib.loads(body.encode("utf-8"))
        self.assertEqual(plist["ProgramArguments"], ["/usr/bin/python3", "-m", "ccontrol.dispatcher"])
        self.assertEqual(plist["WorkingDirectory"], "/Users/a & b/imp")
        self.assertTrue(plist["KeepAlive"])
        self.assertTrue(plist["RunAtLoad"])
        self.assertEqual(plist["EnvironmentVariables"]["PATH"], "/usr/bin")


class Remote(unittest.TestCase):
    PROBED = ("home=/home/pi\n"
              "claude=/home/pi/.local/bin/claude\n"
              "version=2.1.282 (Claude Code)\n"
              "python=3.11.2\n"
              "sdk=ok\n"
              "hooks=ok\n")

    def test_probe_output_is_parsed(self):
        facts = sm.parse_probe(self.PROBED)
        self.assertEqual(facts["home"], "/home/pi")
        self.assertEqual(facts["claude"], "/home/pi/.local/bin/claude")
        self.assertEqual(facts["version"], (2, 1, 282))
        self.assertEqual(facts["python"], (3, 11, 2))
        self.assertTrue(facts["sdk"])
        self.assertTrue(facts["hooks"])
        self.assertNotIn("hookscript", facts)

    def test_a_bare_host_probes_as_missing_everything(self):
        facts = sm.parse_probe("home=/home/x\n")
        self.assertIsNone(facts["version"])
        self.assertIsNone(facts["python"])
        self.assertFalse(facts.get("claude"))

    def test_the_probe_is_one_posix_script(self):
        # It goes over ssh as a single command line and must not depend on
        # PATH, which a non-interactive session on Ubuntu does not set up.
        self.assertIn("$HOME/.local/bin/claude", sm.PROBE)
        self.assertIn("claude_agent_sdk", sm.PROBE)

    def test_claude_is_recorded_relative_to_home(self):
        self.assertEqual(sm.home_relative("/home/pi/.local/bin/claude", "/home/pi"),
                         "$HOME/.local/bin/claude")
        self.assertEqual(sm.home_relative("/usr/local/bin/claude", "/home/pi"),
                         "/usr/local/bin/claude")
        # /home/pip is not inside /home/pi.
        self.assertEqual(sm.home_relative("/home/pip/claude", "/home/pi"), "/home/pip/claude")

    def test_host_entry(self):
        self.assertEqual(sm.host_entry("pi.local", "$HOME/.local/bin/claude", "pi"),
                         {"name": "pi.local", "claude": "$HOME/.local/bin/claude",
                          "label": "pi", "tunnel": True})
        self.assertNotIn("tunnel", sm.host_entry("h", None, tunnel=False))
        self.assertNotIn("label", sm.host_entry("h", None, label="h"))

    def test_adding_a_host_enables_remote_and_appends(self):
        cfg, how = sm.upsert_host(example("config.example.json"),
                                  sm.host_entry("devbox", "$HOME/.local/bin/claude"))
        self.assertEqual(how, "added")
        self.assertTrue(cfg["remote"]["enabled"])
        self.assertEqual([h["name"] for h in cfg["remote"]["hosts"]], ["devbox"])

    def test_re_adding_a_host_replaces_it_and_keeps_its_own_keys(self):
        base = {"remote": {"enabled": True, "hosts": [
            "plain",
            {"name": "pi", "claude": "/old", "label": "raspberry", "reader_python": "/x/python",
             "tunnel": {"local_port": 8801}}]}}
        cfg, how = sm.upsert_host(base, sm.host_entry("pi", "$HOME/.local/bin/claude"))
        self.assertEqual(how, "replaced")
        pi = cfg["remote"]["hosts"][1]
        self.assertEqual(pi["claude"], "$HOME/.local/bin/claude")
        self.assertEqual(pi["label"], "raspberry")
        self.assertEqual(pi["reader_python"], "/x/python")
        self.assertEqual(cfg["remote"]["hosts"][0], "plain")
        self.assertEqual(base["remote"]["hosts"][1]["claude"], "/old")  # not aliased

    def test_re_adding_without_a_tunnel_drops_it(self):
        base = {"remote": {"hosts": [{"name": "pi", "tunnel": True}]}}
        cfg, _ = sm.upsert_host(base, sm.host_entry("pi", None, tunnel=False))
        self.assertNotIn("tunnel", cfg["remote"]["hosts"][0])

    def test_removing_a_host(self):
        base = {"remote": {"hosts": ["a", {"name": "b"}, {"name": "c"}]}}
        cfg, changed = sm.remove_host(base, "b")
        self.assertTrue(changed)
        self.assertEqual(cfg["remote"]["hosts"], ["a", {"name": "c"}])
        self.assertFalse(sm.remove_host(cfg, "zzz")[1])
        self.assertTrue(sm.remove_host(cfg, "a")[1])

    def test_config_written_by_onboarding_is_read_by_the_dispatcher(self):
        from ccontrol import remote
        cfg, _ = sm.upsert_host(example("config.example.json"),
                                sm.host_entry("pi.local", "$HOME/.local/bin/claude", "pi"))
        hosts = remote.load_hosts(cfg)
        self.assertEqual(len(hosts), 1)
        self.assertEqual(hosts[0]["label"], "pi")
        self.assertEqual(hosts[0]["tunnel"]["local_port"], remote.FIRST_TUNNEL_PORT)


class CommandLine(unittest.TestCase):
    def test_bad_remote_usage_is_refused_without_touching_anything(self):
        self.assertEqual(sm.main(["remote", "bogus"]), 2)
        self.assertEqual(sm.main(["remote", "add"]), 2)
        self.assertEqual(sm.main(["service", "bogus"]), 2)


if __name__ == "__main__":
    unittest.main()
