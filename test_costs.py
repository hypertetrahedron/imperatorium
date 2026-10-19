"""Tests for ccontrol.costs: the estimated cost history from transcripts."""
import json
import os
import shutil
import tempfile
import unittest
from datetime import datetime, timedelta, timezone

from ccontrol import costs

NOW = datetime(2026, 10, 15, 12, 0, 0).timestamp()   # local noon
M = 1_000_000


def stamp(days_ago=0, hour=12):
    """A UTC ISO timestamp for local `hour` on the day `days_ago` before NOW."""
    local = datetime.fromtimestamp(NOW).replace(hour=hour) - timedelta(days=days_ago)
    return local.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def day(days_ago=0):
    return (datetime.fromtimestamp(NOW) - timedelta(days=days_ago)).strftime("%Y-%m-%d")


def entry(mid="m1", model="claude-sonnet-4-5", cwd="C:\\work\\app", when=None, **usage):
    usage.setdefault("input_tokens", 0)
    usage.setdefault("output_tokens", 0)
    return {"type": "assistant", "timestamp": when or stamp(), "cwd": cwd,
            "message": {"id": mid, "model": model, "usage": usage}}


class Base(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.root, True)
        self.history = costs.History(root=self.root)

    def write(self, session, entries, folder="proj", sub=None, mode="w"):
        base = os.path.join(self.root, folder)
        if sub:
            base = os.path.join(base, session, "subagents")
            name = sub + ".jsonl"
        else:
            name = session + ".jsonl"
        os.makedirs(base, exist_ok=True)
        path = os.path.join(base, name)
        with open(path, mode, encoding="utf-8") as fh:
            for e in entries:
                fh.write(json.dumps(e) + "\n")
        os.utime(path, (NOW, NOW))
        return path

    def summary(self, **kw):
        kw.setdefault("now", NOW)
        return self.history.summary(**kw)

    def cost(self, **kw):
        return self.summary(**kw)["total"]


class Pricing(Base):
    def test_input_output(self):
        self.write("s1", [entry(input_tokens=M, output_tokens=M)])
        self.assertAlmostEqual(self.cost(), 3 + 15)

    def test_streamed_entries_count_once_and_last_wins(self):
        self.write("s1", [entry(output_tokens=10), entry(output_tokens=500),
                          entry(output_tokens=M)])
        out = self.summary()
        self.assertEqual(out["total_tokens"], M)
        self.assertAlmostEqual(out["total"], 15)

    def test_cache_write_5m_vs_1h(self):
        self.write("s1", [entry(cache_creation_input_tokens=2 * M,
                                cache_creation={"ephemeral_5m_input_tokens": M,
                                                "ephemeral_1h_input_tokens": M})])
        self.assertAlmostEqual(self.cost(), 3 * 1.25 + 3 * 2)

    def test_cache_write_without_split_is_5m(self):
        self.write("s1", [entry(cache_creation_input_tokens=M)])
        self.assertAlmostEqual(self.cost(), 3 * 1.25)

    def test_cache_read_multipliers(self):
        for model, expect in (("claude-opus-5-5", 4 * 0.05), ("claude-fable-5-1", 10 * 0.025),
                              ("claude-sonnet-5-5", 2 * 0.1), ("claude-opus-5", 5 * 0.1)):
            with self.subTest(model=model):
                self.history = costs.History(root=tempfile.mkdtemp())
                self.addCleanup(shutil.rmtree, self.history.root, True)
                self.root = self.history.root
                self.write("s1", [entry(model=model, cache_read_input_tokens=M)])
                self.assertAlmostEqual(self.cost(), expect)

    def test_dated_and_bracketed_model_ids(self):
        self.write("s1", [entry("a", model="claude-sonnet-4-5-20250929", input_tokens=M),
                          entry("b", model="claude-opus-5-5[1m]", input_tokens=M),
                          entry("c", model="claude-opus-4-1-20250805", input_tokens=M)])
        out = self.summary()
        names = {m["model"]: m["cost"] for m in out["models"]}
        self.assertAlmostEqual(names["claude-sonnet-4-5"], 3)
        self.assertAlmostEqual(names["claude-opus-5-5"], 4)
        self.assertAlmostEqual(names["claude-opus-4-1"], 15)
        self.assertEqual(out["unpriced_tokens"], 0)

    def test_family_fallback(self):
        self.write("s1", [entry(model="claude-opus-9-9", input_tokens=M)])
        out = self.summary()
        self.assertAlmostEqual(out["total"], 4)
        self.assertEqual(out["unpriced_tokens"], 0)

    def test_unknown_model_is_unpriced(self):
        self.write("s1", [entry(model="gpt-mystery", input_tokens=300, output_tokens=200)])
        out = self.summary()
        self.assertEqual(out["unpriced_tokens"], 500)
        self.assertEqual(out["total"], 0)
        self.assertEqual(out["total_tokens"], 500)

    def test_rewriting_a_cold_cache(self):
        self.assertAlmostEqual(costs.rewrite_cost("claude-opus-5-5[1m]", M, 3600), 8.0)
        self.assertAlmostEqual(costs.rewrite_cost("claude-opus-5-5", M, 300), 5.0)
        self.assertIsNone(costs.rewrite_cost("gpt-mystery", M, 300))
        self.assertIsNone(costs.rewrite_cost("claude-opus-5-5", None, 300))

    def test_synthetic_and_non_assistant_skipped(self):
        self.write("s1", [entry(model="<synthetic>", input_tokens=M),
                          {"type": "user", "timestamp": stamp(), "message": {"content": "hi"}}])
        self.assertEqual(self.summary()["total_tokens"], 0)


class Bucketing(Base):
    def test_local_date_and_zero_fill(self):
        self.write("s1", [entry("a", when=stamp(0), output_tokens=100),
                          entry("b", when=stamp(2), output_tokens=200)])
        out = self.summary(days=5)
        self.assertEqual([d["date"] for d in out["days"]], [day(i) for i in (4, 3, 2, 1, 0)])
        self.assertEqual([d["tokens"] for d in out["days"]], [0, 0, 200, 0, 100])
        self.assertEqual(out["window_days"], 5)

    def test_late_evening_stays_on_its_local_day(self):
        self.write("s1", [entry("a", when=stamp(1, hour=23), output_tokens=7)])
        out = self.summary(days=3)
        self.assertEqual({d["date"]: d["tokens"] for d in out["days"]}[day(1)], 7)

    def test_usage_outside_window_not_counted(self):
        self.write("s1", [entry("a", when=stamp(10), output_tokens=100),
                          entry("b", when=stamp(0), output_tokens=5)])
        out = self.summary(days=3)
        self.assertEqual(out["total_tokens"], 5)
        self.assertEqual(len(out["days"]), 3)

    def test_old_file_ignored(self):
        path = self.write("s1", [entry(when=stamp(0), output_tokens=100)])
        old = NOW - 10 * 86400
        os.utime(path, (old, old))
        out = self.summary(days=3)
        self.assertEqual(out["files"], 0)
        self.assertEqual(out["total_tokens"], 0)

    def test_old_file_dropped_from_memory(self):
        path = self.write("s1", [entry(output_tokens=100)])
        self.assertEqual(self.summary(days=3)["files"], 1)
        old = NOW - 10 * 86400
        os.utime(path, (old, old))
        self.assertEqual(self.summary(days=3)["files"], 0)


class Projects(Base):
    def test_worktree_cwd_maps_to_repo(self):
        self.write("s1", [entry("a", cwd="C:\\src\\repo\\.claude\\worktrees\\feat-x", output_tokens=1),
                          entry("b", cwd="/home/u/other/.claude/worktrees/y/sub", output_tokens=1),
                          entry("c", cwd="C:\\src\\plain", output_tokens=1)])
        names = sorted(p["project"] for p in self.summary()["projects"])
        self.assertEqual(names, ["other", "plain", "repo"])

    def test_sessions_counted_and_subagent_joins_parent(self):
        self.write("s1", [entry("a", output_tokens=M)])
        self.write("s1", [entry("b", output_tokens=M)], sub="agent-1")
        self.write("s2", [entry("c", output_tokens=M)])
        out = self.summary()
        row = out["projects"][0]
        self.assertEqual(row["project"], "app")
        self.assertEqual(row["sessions"], 2)
        self.assertEqual(out["files"], 3)
        self.assertEqual(out["total_tokens"], 3 * M)

    def test_same_message_in_session_and_subagent_counts_once(self):
        self.write("s1", [entry("dup", output_tokens=100)])
        self.write("s1", [entry("dup", output_tokens=100)], sub="agent-1")
        self.assertEqual(self.summary()["total_tokens"], 100)

    def test_sorted_by_cost(self):
        self.write("s1", [entry("a", cwd="C:\\cheap", output_tokens=1),
                          entry("b", cwd="C:\\dear", output_tokens=M)])
        self.assertEqual([p["project"] for p in self.summary()["projects"]], ["dear", "cheap"])


class Incremental(Base):
    def test_appended_lines_only_are_parsed(self):
        path = self.write("s1", [entry("a", output_tokens=100)])
        self.assertEqual(self.summary()["total_tokens"], 100)
        state = self.history._files[path]
        before = state.lines
        self.write("s1", [entry("b", output_tokens=50)], mode="a")
        self.assertEqual(self.summary()["total_tokens"], 150)
        self.assertEqual(state.lines - before, 1)

    def test_unchanged_file_not_reread(self):
        path = self.write("s1", [entry(output_tokens=100)])
        self.summary()
        state = self.history._files[path]
        lines = state.lines
        self.summary()
        self.assertEqual(state.lines, lines)

    def test_later_entry_replaces_earlier_across_reads(self):
        self.write("s1", [entry("a", output_tokens=10)])
        self.assertEqual(self.summary()["total_tokens"], 10)
        self.write("s1", [entry("a", output_tokens=90)], mode="a")
        self.assertEqual(self.summary()["total_tokens"], 90)

    def test_partial_line_waits_for_its_newline(self):
        path = self.write("s1", [entry("a", output_tokens=10)])
        self.summary()
        text = json.dumps(entry("b", output_tokens=20))
        with open(path, "a", encoding="utf-8") as fh:
            fh.write(text[:30])
        os.utime(path, (NOW + 1, NOW + 1))
        self.assertEqual(self.summary()["total_tokens"], 10)
        with open(path, "a", encoding="utf-8") as fh:
            fh.write(text[30:] + "\n")
        os.utime(path, (NOW + 2, NOW + 2))
        self.assertEqual(self.summary()["total_tokens"], 30)

    def test_shrunk_file_is_reread(self):
        self.write("s1", [entry("a", output_tokens=100), entry("b", output_tokens=100)])
        self.assertEqual(self.summary()["total_tokens"], 200)
        self.write("s1", [entry("z", output_tokens=7)])
        self.assertEqual(self.summary()["total_tokens"], 7)

    def test_replaced_larger_file_is_reread(self):
        self.write("s1", [entry("a", output_tokens=1)])
        self.assertEqual(self.summary()["total_tokens"], 1)
        self.write("s1", [entry("x", output_tokens=5, cwd="C:\\somewhere\\longer\\path"),
                          entry("y", output_tokens=5)])
        self.assertEqual(self.summary()["total_tokens"], 10)

    def test_budget_zero_is_incomplete(self):
        self.write("s1", [entry(output_tokens=100)])
        out = self.summary(budget=0)
        self.assertFalse(out["complete"])
        self.assertEqual(out["total_tokens"], 0)
        again = self.summary()
        self.assertTrue(again["complete"])
        self.assertEqual(again["total_tokens"], 100)

    def test_cached_files_count_when_budget_runs_out(self):
        self.write("s1", [entry("a", output_tokens=100)])
        self.summary()
        self.write("s2", [entry("b", output_tokens=40)])
        out = self.summary(budget=0)
        self.assertFalse(out["complete"])
        self.assertEqual(out["total_tokens"], 100)

    def test_missing_root_is_empty(self):
        out = costs.History(root=os.path.join(self.root, "nope")).summary(now=NOW)
        self.assertTrue(out["ok"])
        self.assertEqual(out["total"], 0)
        json.dumps(out)


A = "aaaaaaaa-0000-0000-0000-000000000001"
B = "bbbbbbbb-0000-0000-0000-000000000002"


class Windows(Base):
    """`totals` over a window of time and `sessions` over a session's life:
    the global and current-session spend (peer request 2026-10-18)."""

    def test_a_window_counts_only_messages_sent_inside_it(self):
        self.write(A, [entry("old", when=stamp(0, hour=5), output_tokens=1 * M),
                       entry("new", when=stamp(0, hour=11), output_tokens=2 * M)])
        self.write(B, [entry("b1", when=stamp(3), output_tokens=4 * M)], folder="other")
        five = self.history.totals(NOW - 5 * 3600, now=NOW)
        self.assertEqual(five["tokens"]["output"], 2 * M)
        self.assertAlmostEqual(five["cost_usd"], 30.0)  # 2M out at $15
        self.assertTrue(five["complete"])
        week = self.history.totals(NOW - 7 * 86400, now=NOW)
        self.assertEqual(week["total_tokens"], 7 * M)

    def test_streamed_entries_count_once_in_a_window(self):
        self.write(A, [entry("m", when=stamp(0, hour=11), output_tokens=10),
                       entry("m", when=stamp(0, hour=11), output_tokens=1 * M)])
        self.assertEqual(self.history.totals(NOW - 3600 * 2, now=NOW)["total_tokens"], 1 * M)

    def test_a_session_is_its_whole_life_with_its_subagents(self):
        self.write(A, [entry("a1", when=stamp(60), input_tokens=1 * M),
                       entry("a2", when=stamp(0), output_tokens=1 * M)])
        self.write(A, [entry("s1", model="claude-haiku-4-5", output_tokens=2 * M)], sub="agent-x")
        old = os.path.join(self.root, "proj", A + ".jsonl")
        os.utime(old, (NOW - 50 * 86400, NOW - 50 * 86400))  # untouched for weeks
        got, complete = self.history.sessions([A, B])
        self.assertTrue(complete)
        self.assertEqual(set(got), {A})          # B has no transcript
        self.assertEqual(got[A]["tokens"], {"input": 1 * M, "output": 3 * M,
                                            "cache_read": 0, "cache_write": 0})
        self.assertAlmostEqual(got[A]["cost_usd"], 3 + 15 + 10)
        # Pinned: a 30-day cost history does not drop the open session's file.
        self.summary(days=30)
        self.assertIn(old, self.history._files)
        self.assertEqual(self.history.sessions([A])[0][A]["total_tokens"], 4 * M)

    def test_each_answer_splits_by_model(self):
        # The model-mix chart (peer request 2026-10-18) stacks output by model.
        self.write(A, [entry("a1", when=stamp(0, hour=11), output_tokens=1 * M),
                       entry("a2", when=stamp(0, hour=11), model="claude-haiku-4-5",
                             input_tokens=5, output_tokens=2 * M),
                       entry("a3", when=stamp(0, hour=11), model="mystery-9", output_tokens=7)])
        five = self.history.totals(NOW - 5 * 3600, now=NOW)["models"]
        self.assertEqual(five["claude-sonnet-4-5"]["output"], 1 * M)
        self.assertAlmostEqual(five["claude-sonnet-4-5"]["cost_usd"], 15.0)
        self.assertEqual(five["claude-haiku-4-5"]["total_tokens"], 2 * M + 5)
        # No price known: counted under its own name, at no cost.
        unknown = [k for k in five if "mystery" in k]
        self.assertEqual(len(unknown), 1)
        self.assertFalse(unknown[0].startswith("?"))
        self.assertEqual(five[unknown[0]], {"output": 7, "total_tokens": 7, "cost_usd": 0.0})
        got, _ = self.history.sessions([A])
        self.assertEqual(got[A]["models"]["claude-haiku-4-5"]["output"], 2 * M)

    def test_answers_follow_the_files(self):
        path = self.write(A, [entry("a1", when=stamp(0, hour=11), output_tokens=1 * M)])
        self.assertEqual(self.history.totals(NOW - 3600 * 2, now=NOW)["total_tokens"], 1 * M)
        with open(path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(entry("a2", when=stamp(0, hour=11), output_tokens=1 * M)) + "\n")
        os.utime(path, (NOW + 1, NOW + 1))
        self.assertEqual(self.history.totals(NOW - 3600 * 2, now=NOW)["total_tokens"], 2 * M)

    def test_not_a_session_id_finds_nothing(self):
        self.assertEqual(costs.session_files("../etc", self.root), [])


if __name__ == "__main__":
    unittest.main()
