"""The chat matrix: what a tile keeps of a conversation, and what it costs.

Run: python test_matrix.py
"""
import os
import time
import unittest
from datetime import datetime

from imperatorium import dispatcher, matrix
from imperatorium.store import Store
from test_dispatcher import Base, SID, write_transcript


def you(text):
    return {"speaker": "you", "role": "user", "blocks": [{"kind": "text", "text": text}]}


def claude(*blocks):
    return {"speaker": "claude", "role": "assistant", "blocks": list(blocks)}


def text(t):
    return {"kind": "text", "text": t}


def tool(cid, name="Bash", summary="git status"):
    return {"kind": "tool", "id": cid, "name": name, "summary": summary}


def result(cid):
    return {"kind": "result", "id": cid, "text": "ok", "truncated": False, "is_error": False}


def thinking(t):
    return {"kind": "thinking", "text": t}


class Focus(unittest.TestCase):
    """The sequence from the request, one step at a time: a reply, two tool
    calls, a thought, a second reply."""

    def steps(self):
        blocks = [text("starting jira item"), tool("a", "Read", "ticket.md"), result("a"),
                  tool("b", "Bash", "npm test"), result("b"), thinking("the failure is in auth"),
                  text("fixed and tested")]
        return [[you("do the jira item"), claude(*blocks[:n])] for n in range(1, len(blocks) + 1)]

    def test_each_step_shows_the_replies_and_only_the_step_in_flight(self):
        seen = [matrix.focus(items) for items in self.steps()]
        replies = [[r["text"] for r in f["replies"]] for f in seen]
        current = [f["current"] and (f["current"].get("name") or f["current"]["kind"])
                   for f in seen]
        self.assertEqual(current, [None, "Read", None, "Bash", None, "thinking", None])
        self.assertEqual(replies[0], ["starting jira item"])
        self.assertTrue(all(r == ["starting jira item"] for r in replies[:6]))
        self.assertEqual(replies[-1], ["starting jira item", "fixed and tested"])

    def test_a_tool_shows_its_name_and_one_line(self):
        f = matrix.focus([claude(tool("a", "Bash", "git status\n--porcelain"))])
        self.assertEqual(f["current"], {"kind": "tool", "name": "Bash", "summary": "git status"})

    def test_the_prompt_and_results_are_not_shown(self):
        f = matrix.focus([you("hello"), claude(tool("a"), result("a"))])
        self.assertEqual(f, {"replies": [], "current": None})

    def test_a_thought_is_not_live_once_the_turn_is_over(self):
        items = [claude(text("done"), thinking("hmm"))]
        self.assertEqual(matrix.focus(items, live=True)["current"]["kind"], "thinking")
        self.assertIsNone(matrix.focus(items, live=False)["current"])

    def test_nothing_is_in_flight_in_a_session_that_is_not(self):
        # The dispatcher passes live=True while a session waits on you, so a
        # call held for permission still shows.
        items = [claude(tool("a", "Bash", "rm -rf build"))]
        self.assertEqual(matrix.focus(items, live=True)["current"]["name"], "Bash")
        self.assertIsNone(matrix.focus(items, live=False)["current"])

    def test_a_reply_or_a_prompt_closes_calls_that_never_got_a_result(self):
        f = matrix.focus([claude(tool("a", "WebFetch"), tool("b"), result("b"), text("found it"))])
        self.assertIsNone(f["current"])
        f = matrix.focus([claude(tool("a", "WebFetch")), you("next"), claude(tool("c", "Read"))])
        self.assertEqual(f["current"], {"kind": "tool", "name": "Read", "summary": "git status"})

    def test_parallel_calls_show_the_newest_and_count_the_rest(self):
        f = matrix.focus([claude(tool("a", "Read"), tool("b", "Grep"), tool("c", "Glob"),
                                 result("c"))])
        self.assertEqual(f["current"]["name"], "Grep")
        self.assertEqual(f["current"]["more"], 1)

    def test_replies_carry_their_turn(self):
        f = matrix.focus([you("1"), claude(text("a"), text("b")), you("2"), claude(text("c"))])
        self.assertEqual([r["turn"] for r in f["replies"]], [1, 1, 2])

    def test_only_the_newest_replies_are_kept_and_long_ones_cut(self):
        items = [claude(*[text(str(i)) for i in range(30)]),
                 claude(text("head " + "x" * 5000 + " the conclusion"))]
        f = matrix.focus(items, keep=5)
        self.assertEqual([r["text"][:2] for r in f["replies"][:4]], ["26", "27", "28", "29"])
        last = f["replies"][-1]["text"]
        self.assertEqual(len(last), matrix.REPLY_CHARS + 1)
        # Seen live: a 2369-char reply lost its last paragraph, so the tile
        # looked stuck on an earlier line. The end is what the tile shows.
        self.assertTrue(last.startswith("…"))
        self.assertTrue(last.endswith(" the conclusion"))

    def test_a_result_that_came_back_with_text_still_ends_its_call(self):
        # Seen live: Edit results arrive with a reminder attached, so the
        # message reads as the person's - and every Edit looked unfinished.
        carried = {"speaker": "you", "role": "user",
                   "blocks": [result("a"), {"kind": "text", "text": "<system-reminder>"}]}
        f = matrix.focus([you("go"), claude(text("editing"), tool("a", "Edit")), carried,
                          claude(text("done"))])
        self.assertIsNone(f["current"])
        self.assertEqual([r["turn"] for r in f["replies"]], [1, 1])

    def test_system_notes_are_not_replies(self):
        note = {"speaker": "system", "role": "user", "note": "task",
                "blocks": [{"kind": "text", "text": "background task done"}]}
        self.assertEqual(matrix.focus([note])["replies"], [])


class Active(unittest.TestCase):
    def test_running_sessions_get_a_tile_and_ended_ones_do_not(self):
        self.assertTrue(matrix.active({"status": "idle"}))
        self.assertFalse(matrix.active({"status": None}))
        self.assertFalse(matrix.active({"status": "idle", "state": "done"}))
        self.assertTrue(matrix.active({"status": "busy", "state": "working"}))


class Spend(unittest.TestCase):
    def test_totals_and_each_session_most_costly_first(self):
        got = matrix.spend([
            {"name": "a", "cost_usd": 1.5, "tokens": {"input": 10, "output": 5}},
            {"name": "b", "cost_usd": 4.0, "tokens": {"cache_read": 100}},
            {"name": "c", "cost_usd": None, "tokens": None},
        ], {"five_hour": {"used": 40}})
        self.assertAlmostEqual(got["cost_usd"], 5.5)
        self.assertEqual((got["costed"], got["sessions"]), (2, 3))
        self.assertEqual(got["tokens"], {"input": 10, "output": 5, "cache_read": 100,
                                         "cache_write": 0})
        self.assertEqual(got["total_tokens"], 115)
        self.assertEqual([e["name"] for e in got["each"]], ["b", "a", "c"])
        self.assertEqual(got["limits"]["five_hour"]["used"], 40)


class Spend(Base):
    """Global and current-session spend (peer request 2026-10-18): both from
    the transcripts, so a restarted dispatcher has them at once."""

    def setUp(self):
        super().setUp()
        self.now = time.time()
        # Before the five-hour window, inside the week; inside both; weeks old.
        write_transcript(self.dir, SID, [("old", 0, 1000, self.now - 4.5 * 3600),
                                         ("new", 0, 2000, self.now - 3600),
                                         ("ancient", 0, 4000, self.now - 20 * 86400)])

    def window(self, got, name):
        return next(w for w in got["global"]["windows"] if w["name"] == name)

    def test_a_subscription_counts_its_windows_and_open_sessions_their_whole_life(self):
        self.live("busy")
        # The five-hour window resets in an hour, so it began four hours ago.
        self.control.usage.ingest(SID, {"rate_limits": {"five_hour": {
            "used_percentage": 30, "resets_at": self.now + 3600}}}, now=self.now)
        got = self.control.spend(self.now)
        five, week = self.window(got, "five_hour"), self.window(got, "seven_day")
        self.assertEqual(five["tokens"]["output"], 2000)
        self.assertAlmostEqual(five["since"], self.now - 4 * 3600)
        self.assertEqual(five["resets_at"], self.now + 3600)
        # No seven-day reading: the last seven days, its reset unknown.
        self.assertEqual(week["tokens"]["output"], 3000)
        self.assertIsNone(week["resets_at"])
        cur = got["current"]
        self.assertEqual(cur["sessions"], 1)
        self.assertEqual(cur["tokens"]["output"], 7000)
        self.assertAlmostEqual(cur["cost_usd"], 7000 * 15 / 1e6)
        self.assertEqual(cur["each"][0]["sessionId"], SID)
        self.assertTrue(got["complete"])
        # Split by model for the model mix, in the windows and the open sessions.
        self.assertEqual(five["models"]["claude-sonnet-4-5"]["output"], 2000)
        self.assertEqual(cur["models"]["claude-sonnet-4-5"]["output"], 7000)
        self.assertEqual(self.control.matrix()["spend"]["models"]["claude-sonnet-4-5"]["output"], 7000)

    def test_a_window_waiting_after_its_reset_counts_from_the_reset(self):
        self.control.usage.ingest(SID, {"rate_limits": {"five_hour": {
            "used_percentage": 50, "resets_at": self.now - 2 * 3600}}}, now=self.now - 3 * 3600)
        five = self.window(self.control.spend(self.now), "five_hour")
        self.assertEqual(five["since"], self.now - 2 * 3600)
        self.assertIsNone(five["resets_at"])
        self.assertEqual(five["tokens"]["output"], 2000)   # "new", an hour ago

    def test_a_closed_session_counts_globally_but_not_as_current(self):
        got = self.control.spend(self.now)
        self.assertEqual(self.window(got, "seven_day")["tokens"]["output"], 3000)
        self.assertEqual(got["current"]["sessions"], 0)
        self.assertEqual(got["current"]["total_tokens"], 0)

    def test_an_account_without_windows_counts_the_month(self):
        self.control.subscribed = lambda: False
        got = self.control.spend(self.now)
        self.assertEqual([w["name"] for w in got["global"]["windows"]], ["month"])
        start = datetime.fromtimestamp(self.now).replace(day=1, hour=0, minute=0,
                                                         second=0, microsecond=0)
        self.assertEqual(self.window(got, "month")["since"], start.timestamp())

    def test_the_meters_come_back_after_a_restart(self):
        self.control.usage.ingest(SID, {"rate_limits": {
            "five_hour": {"used_percentage": 30, "resets_at": self.now + 3600},
            "seven_day": {"used_percentage": 85, "resets_at": self.now - 60}}}, now=self.now - 120)
        again = dispatcher.Control(Store(os.path.join(self.dir, "state.json")), roots=[])
        limits = again.usage.summary(self.now)["limits"]
        self.assertEqual(limits["five_hour"]["used"], 30)
        # It reset while we were away: shown at 0%, waiting for a reading.
        self.assertEqual((limits["seven_day"]["used"], limits["seven_day"]["waiting"]), (0.0, True))
        # A threshold already crossed is not announced again.
        self.assertEqual(again.usage.ingest(SID, {"rate_limits": {"five_hour": {
            "used_percentage": 81, "resets_at": self.now + 3600}}}, now=self.now)[0]["threshold"], 80.0)
        self.assertEqual(again.usage.ingest(SID, {"rate_limits": {"five_hour": {
            "used_percentage": 82, "resets_at": self.now + 3600}}}, now=self.now), [])

    def test_usage_carries_the_spend(self):
        got = self.control.usage_summary()
        self.assertIn("windows", got["spend"]["global"])
        self.assertIn("current", got["spend"])


class Method(Base):
    def test_a_running_session_becomes_a_tile(self):
        self.live("busy")
        items = [you("go"), claude(text("on it"), tool("a", "Bash", "make"))]
        self.control.convs.conversation = lambda sid: (items, "sig")
        write_transcript(self.dir, SID, [("m1", 3, 4)])
        got = self.control.matrix()
        self.assertEqual(len(got["tiles"]), 1)
        tile = got["tiles"][0]
        self.assertEqual(tile["sessionId"], SID)
        self.assertEqual([r["text"] for r in tile["replies"]], ["on it"])
        self.assertEqual(tile["current"]["name"], "Bash")
        self.assertEqual(got["spend"]["total_tokens"], 7)
        self.assertEqual(tile["tokens"]["output"], 4)
        self.assertGreater(tile["cost_usd"], 0)
        self.assertIn("windows", got["spend"]["global"])
        # The background-work counts ride along for the "waiting on" line.
        self.assertIn("alive", tile["work"])

    def test_an_unreadable_transcript_still_gets_its_tile(self):
        self.live("idle")

        def boom(sid):
            raise OSError("gone")
        self.control.convs.conversation = boom
        tile = self.control.matrix()["tiles"][0]
        self.assertEqual(tile["replies"], [])
        self.assertIn("gone", tile["error"])

    def test_no_sessions_no_tiles(self):
        got = self.control.matrix()
        self.assertEqual(got["tiles"], [])
        self.assertEqual(got["spend"]["sessions"], 0)


if __name__ == "__main__":
    unittest.main()
