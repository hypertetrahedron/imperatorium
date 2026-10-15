"""The status panel's numbers, from transcript entries shaped like the real ones."""
import json
import os
import shutil
import tempfile
import unittest

from ccontrol import sessionstat

T0 = "2026-10-14T10:00:00.000Z"
AT0 = 1791972000.0  # T0 as Unix seconds


def ts(sec):
    from datetime import datetime, timezone
    return datetime.fromtimestamp(AT0 + sec, timezone.utc).isoformat().replace("+00:00", "Z")


def call(cid, name, inp, sec=0, mid="m1", usage=None, side=False):
    return {"type": "assistant", "timestamp": ts(sec), "isSidechain": side,
            "message": {"id": mid, "model": "claude-opus-5-5", "usage": usage or {},
                        "content": [{"type": "tool_use", "id": cid, "name": name, "input": inp}]}}


def result(cid, sec=1, tur=None, text="ok"):
    return {"type": "user", "timestamp": ts(sec), "toolUseResult": tur,
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": cid, "content": text}]}}


def note(task_id, status, sec=100, queued=False):
    text = ("<task-notification>\n<task-id>%s</task-id>\n<status>%s</status>\n"
            "<summary>done</summary>\n</task-notification>" % (task_id, status))
    if queued:
        return {"type": "attachment", "timestamp": ts(sec),
                "attachment": {"type": "queued_command", "prompt": text}}
    return {"type": "user", "timestamp": ts(sec), "message": {"role": "user", "content": text}}


class Background(unittest.TestCase):
    def run_(self, entries, now=AT0 + 200):
        return sessionstat.summarise(entries, now)

    def test_a_backgrounded_command_runs_until_its_notification(self):
        e = [call("c1", "Bash", {"command": "make", "description": "build it"}),
             result("c1", tur={"backgroundTaskId": "b1"})]
        got = self.run_(e)
        self.assertEqual([(s["id"], s["label"]) for s in got["shells"]], [("b1", "build it")])
        self.assertEqual(self.run_(e + [note("b1", "completed")])["shells"], [])

    def test_a_notification_that_lands_mid_turn_counts(self):
        e = [call("c1", "Bash", {"command": "x"}), result("c1", tur={"backgroundTaskId": "b1"}),
             note("b1", "failed", queued=True)]
        self.assertEqual(self.run_(e)["shells"], [])

    def test_a_monitor_ends_at_its_timeout_or_on_task_stop(self):
        e = [call("c1", "Monitor", {"description": "watch log"}),
             result("c1", tur={"taskId": "mon", "timeoutMs": 300000, "persistent": False})]
        got = self.run_(e)
        self.assertEqual(got["monitors"][0]["expires"], AT0 + 1 + 300)
        self.assertEqual(self.run_(e, now=AT0 + 400)["monitors"], [])
        stop = [call("c2", "TaskStop", {"task_id": "mon"}, sec=50, mid="m2"),
                result("c2", sec=51, tur={"task_id": "mon", "message": "stopped"})]
        self.assertEqual(self.run_(e + stop)["monitors"], [])

    def test_a_persistent_monitor_has_no_end(self):
        e = [call("c1", "Monitor", {}), result("c1", tur={"taskId": "m", "timeoutMs": 1000,
                                                          "persistent": True})]
        self.assertEqual(len(self.run_(e, now=AT0 + 99999)["monitors"]), 1)

    def test_a_monitor_event_is_not_its_end(self):
        e = [call("c1", "Monitor", {}), result("c1", tur={"taskId": "m", "timeoutMs": 10**7}),
             {"type": "user", "timestamp": ts(5), "message": {"role": "user", "content":
              "<task-notification>\n<task-id>m</task-id>\n<summary>Monitor event</summary>\n"
              "<event>line</event>\n</task-notification>"}}]
        self.assertEqual(len(self.run_(e)["monitors"]), 1)

    def test_background_and_foreground_subagents(self):
        e = [call("a1", "Agent", {"description": "research"}),
             result("a1", tur={"status": "async_launched", "agentId": "ag1",
                               "description": "research", "resolvedModel": "sonnet"}),
             call("a2", "Agent", {"description": "inline"}, sec=2, mid="m2")]
        got = self.run_(e)
        self.assertEqual(sorted((a["label"], bool(a.get("foreground"))) for a in got["agents"]),
                         [("inline", True), ("research", False)])
        self.assertEqual(self.run_(e + [note("ag1", "completed")])["agents"][0]["label"], "inline")


class Tokens(unittest.TestCase):
    U1 = {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 900,
          "cache_creation_input_tokens": 90,
          "cache_creation": {"ephemeral_1h_input_tokens": 90, "ephemeral_5m_input_tokens": 0}}
    U2 = {"input_tokens": 2, "output_tokens": 7, "cache_read_input_tokens": 990,
          "cache_creation_input_tokens": 8, "cache_creation": {}}

    def entries(self):
        return [
            {"type": "permission-mode", "permissionMode": "auto"},
            {"type": "user", "timestamp": ts(0), "promptId": "p1", "version": "2.1.296",
             "gitBranch": "main", "message": {"role": "user", "content": "hi"}},
            call("c1", "Read", {}, sec=1, mid="m1", usage=self.U1),
            call("c2", "Read", {}, sec=1, mid="m1", usage=self.U1),   # same message, again
            result("c1", sec=2), result("c2", sec=2),
            call("c3", "Read", {}, sec=60, mid="m2", usage=self.U2),
            result("c3", sec=61),
            call("s1", "Read", {}, sec=70, mid="side", usage=self.U1, side=True),
            {"type": "system", "subtype": "turn_duration", "durationMs": 61000},
            {"type": "system", "subtype": "compact_boundary"},
        ]

    def test_usage_is_counted_once_per_message(self):
        got = sessionstat.summarise(self.entries(), AT0 + 100)
        self.assertEqual(got["tokens"], {"input": 22, "output": 17,
                                         "cache_read": 2790, "cache_write": 188})

    def test_cache_lifetime_and_context_come_from_the_main_chain(self):
        got = sessionstat.summarise(self.entries(), AT0 + 100)
        # The 1h write from m1 still sets the lifetime; the clock runs from m2,
        # not from the subagent's sidechain message after it.
        self.assertEqual(got["cache"]["ttl"], 3600.0)
        self.assertEqual(got["cache"]["expires_at"], AT0 + 60 + 3600)
        self.assertTrue(got["cache"]["warm"])
        self.assertEqual(got["context_tokens"], 1000)
        self.assertAlmostEqual(got["cache"]["hit"], 0.99)
        self.assertFalse(sessionstat.summarise(self.entries(), AT0 + 5000)["cache"]["warm"])

    def test_the_rest(self):
        got = sessionstat.summarise(self.entries(), AT0 + 100)
        self.assertEqual((got["prompts"], got["turns"], got["last_turn_ms"], got["compactions"]),
                         (1, 1, 61000, 1))
        self.assertEqual((got["permission_mode"], got["version"], got["branch"], got["model_id"]),
                         ("auto", "2.1.296", "main", "claude-opus-5-5"))
        self.assertEqual(got["started"], AT0)


class Folding(unittest.TestCase):
    def test_feeding_in_pieces_gives_the_same_answer(self):
        e = (Tokens().entries()
             + [call("c1", "Bash", {"command": "make"}, sec=80, mid="m3"),
                result("c1", sec=81, tur={"backgroundTaskId": "b1"}),
                call("c2", "Agent", {"description": "inline"}, sec=82, mid="m4"),
                call("c3", "Monitor", {}, sec=83, mid="m5"),
                result("c3", sec=84, tur={"taskId": "mon", "timeoutMs": 10**6}),
                note("b1", "completed", sec=90)])
        whole = sessionstat.summarise(e, AT0 + 100)
        for cut in range(len(e) + 1):
            fold = sessionstat.Fold().feed(e[:cut]).feed(e[cut:])
            self.assertEqual(fold.summary(AT0 + 100), whole, cut)

    def test_answered_calls_are_not_kept(self):
        fold = sessionstat.Fold().feed([call("c1", "Read", {}), result("c1")])
        self.assertEqual(fold.calls, {})


class ReaderTests(unittest.TestCase):
    SID = "0f0f0f0f-0000-4000-8000-000000000001"

    def setUp(self):
        self.dir = tempfile.mkdtemp()
        os.makedirs(os.path.join(self.dir, "projects", "Y--x"))
        self.path = os.path.join(self.dir, "projects", "Y--x", self.SID + ".jsonl")
        self.env = os.environ.get("CLAUDE_CONFIG_DIR")
        os.environ["CLAUDE_CONFIG_DIR"] = self.dir

    def tearDown(self):
        if self.env is None:
            os.environ.pop("CLAUDE_CONFIG_DIR", None)
        else:
            os.environ["CLAUDE_CONFIG_DIR"] = self.env
        shutil.rmtree(self.dir, ignore_errors=True)

    def write(self, text, mode="a"):
        with open(self.path, mode, encoding="utf-8", newline="\n") as fh:
            fh.write(text)

    def test_appends_are_read_and_a_half_written_line_waits(self):
        r = sessionstat.Reader()
        line = lambda pid: json.dumps({"type": "user", "promptId": pid,
                                       "message": {"content": "x"}}) + "\n"
        self.write(line("p1"), "w")
        self.assertEqual(r.read(self.SID, AT0)["prompts"], 1)
        half = line("p2")
        self.write(half[:10])
        self.assertEqual(r.read(self.SID, AT0)["prompts"], 1)
        self.write(half[10:])
        self.assertEqual(r.read(self.SID, AT0)["prompts"], 2)
        self.write(line("p9"), "w")  # rewritten shorter: start again
        self.assertEqual(r.read(self.SID, AT0)["prompts"], 1)

    def test_unknown_or_malformed_id(self):
        r = sessionstat.Reader()
        self.assertIsNone(r.read("0f0f0f0f-0000-4000-8000-00000000dead", AT0))
        self.assertIsNone(r.read("../../etc/passwd", AT0))


class StatuslineExtras(unittest.TestCase):
    def test_durations_and_lines_are_kept_when_sent(self):
        from ccontrol import usage
        got = usage.reading({"cost": {"total_cost_usd": 1.5, "total_duration_ms": 9000,
                                      "total_api_duration_ms": 4000,
                                      "total_lines_added": 12, "total_lines_removed": 3}})
        self.assertEqual((got["duration_ms"], got["api_duration_ms"], got["lines_added"],
                          got["lines_removed"]), (9000, 4000, 12, 3))
        self.assertIsNone(usage.reading({})["lines_added"])


if __name__ == "__main__":
    unittest.main()
