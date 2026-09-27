"""Projects, deep links, the inbox frame, and the store.

Run: python test_core.py
"""
import json
import os
import shutil
import tempfile
import unittest

from ccontrol import background, deeplink, inbox, projects, registry, winfocus
from ccontrol.store import Store

CATALOGUE = [
    {"name": n, "path": "Y:/projects-software/" + n, "source": "root"}
    for n in (
        "laser-ledger",
        "bookoferrantpages",
        "gem-trip",
        "amarantha",
        "amarantha-story",
        "pulse-palette",
        "diceCounter",
        "elixir-forge",
        "fiducials",
        "rank-craft",
    )
]


class Utterances(unittest.TestCase):
    def test_prefix_forms(self):
        for text, name, prompt in [
            ("project: laser ledger, do a thing", "laser ledger", "do a thing"),
            ("project laser ledger: do a thing", "laser ledger", "do a thing"),
            ("Project: Gem Trip, fix it", "Gem Trip", "fix it"),
            ("projects: gem trip, fix it", "gem trip", "fix it"),
        ]:
            got_name, got_prompt = projects.parse_utterance(text)
            self.assertEqual((got_name, got_prompt), (name, prompt), text)

    def test_no_prefix_is_all_prompt(self):
        name, prompt = projects.parse_utterance("just do the thing")
        self.assertIsNone(name)
        self.assertEqual(prompt, "just do the thing")

    def test_prompt_may_contain_commas(self):
        _, prompt = projects.parse_utterance(
            "project: gem trip, fix the bug, then run the tests"
        )
        self.assertEqual(prompt, "fix the bug, then run the tests")

    def test_multiline_prompt_survives(self):
        # Dictated prompts arrive with newlines; DOTALL matters.
        _, prompt = projects.parse_utterance("project: gem trip, line one\nline two")
        self.assertEqual(prompt, "line one\nline two")


class Matching(unittest.TestCase):
    def certain(self, said, expect):
        res = projects.resolve(said, CATALOGUE)
        self.assertEqual(res["confidence"], "certain", "%s -> %s" % (said, res))
        self.assertEqual(res["match"]["name"], expect)

    def test_spoken_forms_of_real_projects(self):
        self.certain("laser ledger", "laser-ledger")
        self.certain("book of errant pages", "bookoferrantpages")
        self.certain("gem trip", "gem-trip")
        self.certain("dice counter", "diceCounter")
        self.certain("elixir forge", "elixir-forge")
        self.certain("rank craft", "rank-craft")

    def test_case_and_separators_do_not_matter(self):
        self.certain("LASER-LEDGER", "laser-ledger")
        self.certain("Laser_Ledger", "laser-ledger")

    def test_unknown_project_is_refused_not_guessed(self):
        # The point of the confidence gate: a prompt fired at the wrong repo
        # is worse than no dispatch at all.
        res = projects.resolve("home assistant", CATALOGUE)
        self.assertEqual(res["confidence"], "none")
        self.assertIsNone(res["match"])

    def test_near_tie_is_not_certain(self):
        # "amarantha" and "amarantha-story" are close enough that acting
        # silently would be a coin flip.
        res = projects.resolve("amarantha story", CATALOGUE)
        self.assertNotEqual(res["confidence"], "none")
        if res["confidence"] == "certain":
            self.assertEqual(res["match"]["name"], "amarantha-story")

    def test_empty_input_matches_nothing(self):
        self.assertEqual(projects.resolve("", CATALOGUE)["confidence"], "none")
        self.assertEqual(projects.resolve(None, CATALOGUE)["confidence"], "none")

    def test_candidates_are_ranked_and_bounded(self):
        res = projects.resolve("gem trip", CATALOGUE)
        scores = [c["score"] for c in res["candidates"]]
        self.assertLessEqual(len(scores), 5)
        self.assertEqual(scores, sorted(scores, reverse=True))


class DeepLinks(unittest.TestCase):
    def test_prompt_is_encoded_not_dropped(self):
        url = deeplink.build("Y:/projects-software/gem-trip", "fix a & b")
        self.assertIn("claude-cli://open?cwd=", url)
        self.assertIn("%26", url)

    def test_unc_paths_are_refused(self):
        # Deep links reject UNC outright; failing here names the reason.
        with self.assertRaises(deeplink.DeepLinkError):
            deeplink.build(r"\\NAS\Home\projects", "x")

    def test_overlong_prompt_is_refused(self):
        with self.assertRaises(deeplink.DeepLinkError):
            deeplink.build("Y:/x", "a" * (deeplink.MAX_Q + 1))

    def test_cwd_is_required(self):
        with self.assertRaises(deeplink.DeepLinkError):
            deeplink.build("", "x")


class InboxFrames(unittest.TestCase):
    def test_auth_line_comes_first(self):
        payload = inbox._frames("tok", "hello")
        lines = [json.loads(l) for l in payload.decode().strip().split("\n")]
        self.assertEqual(lines[0], {"type": "auth", "token": "tok"})
        self.assertEqual(
            lines[1], {"type": "user", "message": {"role": "user", "content": "hello"}}
        )

    def test_every_line_is_newline_terminated(self):
        # The receiver reads whole lines; a missing terminator hangs until the
        # 30s silent-connection deadline closes the connection.
        self.assertTrue(inbox._frames("tok", "hi").endswith(b"\n"))

    def test_unicode_is_not_escaped_away(self):
        payload = inbox._frames("t", "caf\u00e9 \u2014 done")
        self.assertIn("caf\u00e9", payload.decode("utf-8"))

    def test_empty_content_is_refused(self):
        for bad in ("", "   ", None, 5):
            with self.assertRaises(inbox.InboxError):
                inbox.send("\\\\.\\pipe\\x", "tok", bad)

    def test_oversized_content_is_refused_before_connecting(self):
        with self.assertRaises(inbox.InboxError):
            inbox.send("\\\\.\\pipe\\x", "tok", "a" * (inbox.MAX_CONTENT + 1))

    @unittest.skipUnless(inbox.WINDOWS, "token is only mandatory on native Windows")
    def test_windows_requires_a_token(self):
        with self.assertRaises(inbox.InboxError):
            inbox.send("\\\\.\\pipe\\x", None, "hello")


class BackgroundCommand(unittest.TestCase):
    def test_prompt_is_the_last_argument(self):
        # Options before the prompt, so a prompt starting with a dash is not
        # read as a flag.
        cmd = background.build_command("--not-a-flag", model="sonnet")
        self.assertEqual(cmd[-1], "--not-a-flag")
        self.assertEqual(cmd[:2], ["claude", "--bg"])

    def test_options_are_omitted_when_unset(self):
        # A name is always supplied, derived from the prompt; nothing else is.
        self.assertEqual(background.build_command("x"),
                         ["claude", "--bg", "--name", "x", "x"])
        for flag in ("--model", "--permission-mode", "--agent"):
            self.assertNotIn(flag, background.build_command("x"))

    def test_every_option_is_passed_through(self):
        cmd = background.build_command(
            "x", model="opus", permission_mode="plan", agent="Plan", name="n",
            extra=["--effort", "high"],
        )
        for flag in ("--model", "--permission-mode", "--agent", "--name", "--effort"):
            self.assertIn(flag, cmd)

    def test_a_short_name_is_derived_from_the_prompt(self):
        # Without one, Claude Code names the session after the whole prompt,
        # which is unreadable on the board and useless on a Stream Deck key.
        self.assertEqual(
            background.short_name("implement the new feature as recommended"),
            "implement-the-new-feature",
        )
        self.assertEqual(
            background.short_name("Reply with the single word: ok."),
            "reply-with-the-single",
        )

    def test_a_derived_name_is_a_usable_handle(self):
        name = background.short_name("fix the *Kohler* shower!! integration")
        self.assertRegex(name, r"^[a-z0-9-]+$")
        self.assertLessEqual(len(name), 28)

    def test_an_unnameable_prompt_yields_no_name_rather_than_junk(self):
        for prompt in ("   ", "!!!", "", None):
            self.assertIsNone(background.short_name(prompt))

    def test_an_explicit_name_beats_the_derived_one(self):
        cmd = background.build_command("do the thing", name="mine")
        self.assertIn("mine", cmd)
        self.assertNotIn("do-the-thing", cmd)

    def test_the_id_is_taken_from_the_end_of_the_output(self):
        # Preamble can contain hex-looking noise; the id is what it ends with.
        self.assertEqual(background.parse_id("started 8b879562"), "8b879562")
        self.assertEqual(background.parse_id("deadbeef then cafe1234"), "cafe1234")

    def test_no_id_is_reported_as_none_not_guessed(self):
        for text in ("", None, "no identifier printed"):
            self.assertIsNone(background.parse_id(text))

    def test_an_empty_prompt_is_refused_before_launching(self):
        for bad in ("", "   ", None):
            with self.assertRaises(background.BackgroundError):
                background.start(bad, os.getcwd())

    def test_a_missing_directory_is_refused_before_launching(self):
        with self.assertRaises(background.BackgroundError):
            background.start("do it", os.path.join(os.getcwd(), "no-such-dir"))

    def test_unknown_config_keys_do_not_break_a_dispatch(self):
        # config.json is hand-edited and carries "_comment" keys; those must
        # not reach build_command as keyword arguments.
        captured = {}

        def fake_run(cmd, **kwargs):
            captured["cmd"] = cmd
            raise OSError("stop here, the command is what matters")

        original = background.subprocess.run
        background.subprocess.run = fake_run
        try:
            with self.assertRaises(Exception):
                background.start("do it", os.getcwd(),
                                 _comment="ignore me", permission_mode="plan")
        finally:
            background.subprocess.run = original
        self.assertIn("--permission-mode", captured["cmd"])
        self.assertNotIn("_comment", " ".join(captured["cmd"]))


class StoreState(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.path = os.path.join(self.dir, "state.json")

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def test_tokens_survive_a_restart(self):
        # Losing a token means losing the ability to dispatch to that session
        # until it restarts, so this is the store's whole reason to exist.
        s = Store(self.path)
        s.register("sid", token="secret", cwd="C:/x", name="n")
        self.assertEqual(Store(self.path).tokens(), {"sid": "secret"})

    def test_register_merges_and_ignores_none(self):
        s = Store(self.path)
        s.register("sid", token="t", cwd="C:/x", name="first")
        s.register("sid", cwd=None, name="second")
        rec = s.sessions()["sid"]
        self.assertEqual(rec["cwd"], "C:/x")
        self.assertEqual(rec["name"], "second")

    def test_clear_attention_removes_the_key(self):
        s = Store(self.path)
        s.register("sid", attention_since=123.0)
        s.clear_attention("sid")
        self.assertNotIn("attention_since", s.sessions()["sid"])
        s.clear_attention("unknown-sid")  # must not raise

    def test_forget_drops_token_and_record(self):
        s = Store(self.path)
        s.register("sid", token="t")
        s.forget("sid")
        self.assertEqual(s.tokens(), {})
        self.assertEqual(s.sessions(), {})

    def test_events_are_capped_and_newest_first(self):
        s = Store(self.path)
        for i in range(520):
            s.add_event({"kind": "e%d" % i})
        events = s.events(1000)
        self.assertLessEqual(len(events), 400)
        self.assertEqual(events[0]["kind"], "e519")

    def test_corrupt_state_file_does_not_crash_startup(self):
        with open(self.path, "w", encoding="utf-8") as fh:
            fh.write("{not json")
        self.assertEqual(Store(self.path).tokens(), {})


class StaleAttention(unittest.TestCase):
    """The bug this fixes: a tile stuck on `permission_prompt`.

    Answering a permission prompt fires no hook. `Stop` is the only event
    that clears attention and it can be minutes away, so the recorded
    attention needs something live to contradict it. Claude Code stamps
    `statusUpdatedAt` on every status change, and answering flips the
    session back to `busy`.

    Measured on a real prompt here: notification at 20:26:48, `busy` at
    20:26:58, and the tile still said `permission_prompt` five minutes on.
    """

    PROMPTED = 1000.0

    def row(self, status, changed=None):
        row = {"sessionId": "sid", "status": status}
        if changed is not None:
            row["status_changed_at"] = changed
        return row

    def test_working_again_means_the_prompt_was_answered(self):
        self.assertTrue(registry.moved_on(
            self.row("busy", self.PROMPTED + 10), self.PROMPTED))

    def test_still_idle_is_still_waiting(self):
        # The status clock ticked, but not into anything that runs code.
        self.assertFalse(registry.moved_on(
            self.row("idle", self.PROMPTED + 10), self.PROMPTED))

    def test_a_status_older_than_the_prompt_proves_nothing(self):
        self.assertFalse(registry.moved_on(
            self.row("busy", self.PROMPTED - 10), self.PROMPTED))

    def test_the_same_instant_is_the_prompt_appearing_not_an_answer(self):
        # The hook and the status write race; a person is not that fast.
        self.assertFalse(registry.moved_on(
            self.row("busy", self.PROMPTED + 0.2), self.PROMPTED))

    def test_no_status_clock_leaves_the_hook_in_charge(self):
        # Background jobs have no session file, so nothing contradicts them.
        self.assertFalse(registry.moved_on(self.row("busy"), self.PROMPTED))

    def test_nothing_was_waiting_in_the_first_place(self):
        self.assertFalse(registry.moved_on(
            self.row("busy", self.PROMPTED + 10), None))

    def test_the_status_clock_is_carried_off_the_session_file(self):
        rows = registry.merge(
            [{"sessionId": "sid", "status": "busy"}],
            {"sid": {"sessionId": "sid", "statusUpdatedAt": 1790036818458}},
        )
        self.assertAlmostEqual(rows[0]["status_changed_at"], 1790036818.458)

    def test_a_missing_or_junk_stamp_is_simply_absent(self):
        for stamp in (None, "soon", True):
            rows = registry.merge(
                [{"sessionId": "sid", "status": "busy"}],
                {"sid": {"sessionId": "sid", "statusUpdatedAt": stamp}},
            )
            self.assertNotIn("status_changed_at", rows[0], repr(stamp))


class FailureCaptions(unittest.TestCase):
    """What a tile says when a turn dies on an API error.

    The wording is Claude Code's own, read out of the binary, so the window
    and the session agree rather than inventing a second vocabulary.
    """

    def test_an_overload_says_so(self):
        self.assertEqual(registry.failure_caption("overloaded"),
                         "API overloaded - wait and retry")

    def test_a_5xx_says_so(self):
        self.assertEqual(registry.failure_caption("server_error"),
                         "API unavailable - retry")

    def test_the_quiet_failure_stays_quiet(self):
        # Claude Code maps this one to no blocked state, so nor do we.
        self.assertIsNone(registry.failure_caption("max_output_tokens"))

    def test_an_oversize_request_gets_the_actionable_wording(self):
        self.assertEqual(
            registry.failure_caption("invalid_request", "prompt is too long"),
            "request too large - /compact or trim")

    def test_an_ordinary_rejection_does_not(self):
        self.assertEqual(
            registry.failure_caption("invalid_request", "bad field"),
            "invalid API request - see detail")

    def test_an_unknown_or_missing_error_still_says_something(self):
        for error in ("a_new_error_code", None, "unknown"):
            self.assertEqual(registry.failure_caption(error), "API error",
                             repr(error))

    def test_every_documented_error_is_covered(self):
        # The enum as the binary lists it; a new one must not crash a tile.
        for error in ("authentication_failed", "oauth_org_not_allowed",
                      "account_on_hold", "verification_required",
                      "billing_error", "rate_limit", "overloaded",
                      "invalid_request", "model_not_found", "server_error",
                      "unknown", "max_output_tokens",
                      "cloud_credential_error"):
            caption = registry.failure_caption(error)
            self.assertTrue(caption is None or caption.strip(), error)


class AttentionBookkeeping(unittest.TestCase):
    """A failed turn wants a human; a retry does not mean it stopped wanting one."""

    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.path = os.path.join(self.dir, "state.json")

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def test_repeated_failures_do_not_restart_the_clock(self):
        # Three 529s in four minutes is what this actually looked like. The
        # age on the tile has to mean "stuck since", not "failed again just
        # now", or a session stuck for ten minutes looks seconds old.
        s = Store(self.path)
        s.raise_attention("sid", "API overloaded - wait and retry", "failure")
        first = s.sessions()["sid"]["attention_since"]
        s.raise_attention("sid", "API unavailable - retry", "failure")
        rec = s.sessions()["sid"]
        self.assertEqual(rec["attention_since"], first)
        self.assertEqual(rec["attention_reason"], "API unavailable - retry")

    def test_clearing_drops_the_reason_with_the_flag(self):
        s = Store(self.path)
        s.raise_attention("sid", "API overloaded - wait and retry", "failure")
        s.clear_attention("sid")
        rec = s.sessions()["sid"]
        for key in ("attention_since", "attention_reason", "attention_kind"):
            self.assertNotIn(key, rec)

    def test_clearing_keeps_everything_else(self):
        s = Store(self.path)
        s.raise_attention("sid", "why", "failure", cwd="Y:/x")
        s.clear_attention("sid")
        self.assertEqual(s.sessions()["sid"]["cwd"], "Y:/x")

    def test_a_failure_is_not_settled_by_the_session_working_again(self):
        # `moved_on` is only ever consulted for notification attention. A
        # permission prompt is answered by a person, so work resuming proves
        # it; a 529 retry starting proves nothing, and clearing on it was the
        # bug being fixed.
        row = {"sessionId": "sid", "status": "busy",
               "status_changed_at": 2000.0, "status_at_change": "busy"}
        self.assertTrue(registry.moved_on(row, 1000.0))

    def test_a_running_command_counts_as_running(self):
        # `shell` in the session file, `busy` from `agents --json`: the same
        # session, both meaning it is executing.
        row = {"sessionId": "sid", "status": "busy",
               "status_changed_at": 2000.0, "status_at_change": "shell"}
        self.assertTrue(registry.moved_on(row, 1000.0))

    def test_sources_disagreeing_about_anything_else_clears_nothing(self):
        row = {"sessionId": "sid", "status": "busy",
               "status_changed_at": 2000.0, "status_at_change": "idle"}
        self.assertFalse(registry.moved_on(row, 1000.0))


class WindowTitles(unittest.TestCase):
    """Finding the window a session is actually in.

    One `WindowsTerminal.exe` hosts every session on this machine - four
    sessions, one pid, three windows - so the process tree identifies the
    terminal and cannot tell its windows apart. The title can: Claude Code
    puts the session's own summary in the title bar, and the SDK hands back
    the same string.
    """

    BUSY = "◑ Project tiles status not updating"      # glyph while working
    IDLE = "✳ Vercel OIDC token sufficiency"          # glyph while waiting

    def test_the_status_glyph_is_not_part_of_the_title(self):
        # The glyph changes as the session works, so a raw compare would
        # match while idle and stop matching the moment it got busy.
        self.assertEqual(winfocus.normalise_title(self.BUSY),
                         "project tiles status not updating")

    def test_the_same_title_either_way_round(self):
        self.assertEqual(winfocus.normalise_title(self.BUSY),
                         winfocus.normalise_title(
                             "Project tiles status not updating"))

    def test_an_empty_or_missing_title_normalises_to_nothing(self):
        for value in ("", None, "   ", "◑"):
            self.assertEqual(winfocus.normalise_title(value), "", repr(value))

    def test_the_right_window_is_picked(self):
        hwnd = winfocus.match_title("Project tiles status not updating",
                                    [(11, self.BUSY), (22, self.IDLE)])
        self.assertEqual(hwnd, 11)

    def test_a_background_tab_matches_nothing(self):
        # Its window is showing another tab, so its title is nowhere.
        self.assertIsNone(winfocus.match_title(
            "Skills executed display in workflows",
            [(11, self.BUSY), (22, self.IDLE)]))

    def test_an_elided_title_still_matches(self):
        # A narrow tab truncates what the terminal displays.
        self.assertEqual(winfocus.match_title(
            "Project tiles status not updating",
            [(11, "◑ Project tiles status")]), 11)

    def test_two_windows_claiming_the_same_title_pick_neither(self):
        # Focusing the wrong one is worse than focusing none: not guessing is
        # the entire point of this change.
        self.assertIsNone(winfocus.match_title(
            "Project tiles status not updating",
            [(11, self.BUSY), (22, self.BUSY)]))

    def test_an_ambiguous_prefix_picks_neither(self):
        self.assertIsNone(winfocus.match_title(
            "Project tiles status not updating",
            [(11, "◑ Project"), (22, "✳ Project tiles")]))

    def test_no_title_never_matches_anything(self):
        # An unsummarised session must not collide with a blank window title.
        self.assertIsNone(winfocus.match_title("", [(11, ""), (22, self.IDLE)]))

    def test_no_windows_is_not_an_error(self):
        self.assertIsNone(winfocus.match_title("anything", []))


if __name__ == "__main__":
    unittest.main(verbosity=2)
