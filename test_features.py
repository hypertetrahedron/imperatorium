"""Tests for the newer surfaces: changes/worktrees, PR lookup, usage pace,
activity timelines, session history/resume, notifications, the queue, the
statusline line, and the dispatcher glue that ties them together.

Real temporary git repositories are used for changes.py/worktrees, because
that module is a thin, load-bearing wrapper over `git` itself - a fake would
just test the fake. Nothing here ever calls a real `gh` or `git push`: those
are always replaced with an injected `runner`. Nothing here ever launches a
real process for resume, or hits a real network for notify: `popen`,
`bg_start`, `send` and `say` are always injected.

Run: python test_features.py
     .venv\\Scripts\\python.exe test_features.py
"""
import json
import os
import shutil
import subprocess
import tempfile
import threading
import time
import types
import unittest
import urllib.error
import urllib.request

from ccontrol import activity, background, changes, conversation, dispatcher
from ccontrol import history, inbox, notify, prs, usage
from ccontrol import store as store_mod
from ccontrol.store import Store
from hooks import cc_statusline
from test_dispatcher import Base, CWD, SID

GIT = shutil.which("git")


# -- git fixtures -----------------------------------------------------------
def run_git(cwd, *args):
    done = subprocess.run(["git", "-C", cwd] + list(args),
                          capture_output=True, text=True)
    if done.returncode != 0:
        raise RuntimeError("git %r in %s failed: %s" % (args, cwd, done.stderr))
    return done.stdout


def write(path, text):
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(text)


def init_repo(path):
    run_git(path, "init", "-q", "-b", "main")
    run_git(path, "config", "user.email", "test@example.com")
    run_git(path, "config", "user.name", "Test")
    run_git(path, "config", "core.autocrlf", "false")
    write(os.path.join(path, "f.txt"), "one\n")
    run_git(path, "add", "f.txt")
    run_git(path, "commit", "-q", "-m", "init")


def add_agent_worktree(main, name="x", branch="feature", commit=True):
    """A worktree under .claude/worktrees/, the shape Claude Code makes."""
    rel = os.path.join(".claude", "worktrees", name)
    run_git(main, "worktree", "add", rel, "-b", branch, "-q")
    full = os.path.join(main, rel)
    if commit:
        with open(os.path.join(full, "f.txt"), "a", encoding="utf-8") as fh:
            fh.write("two\n")
        run_git(full, "add", "f.txt")
        run_git(full, "commit", "-q", "-m", "wt change")
    return full


@unittest.skipUnless(GIT, "git is not on PATH")
class ChangesRepo(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        init_repo(self.dir)

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)


class Changes(ChangesRepo):
    def test_clean_repo_has_no_files_or_diff(self):
        got = changes.changes(self.dir)
        self.assertTrue(got["ok"])
        self.assertTrue(got["repo"])
        self.assertEqual(got["files"], [])
        self.assertEqual(got["diff"], "")
        self.assertEqual(got["branch"], "main")

    def test_dirty_repo_reports_modified_and_untracked_files(self):
        write(os.path.join(self.dir, "f.txt"), "one\nmodified\n")
        write(os.path.join(self.dir, "new.txt"), "new\n")
        got = changes.changes(self.dir)
        self.assertTrue(got["ok"])
        paths = {f["path"] for f in got["files"]}
        self.assertEqual(paths, {"f.txt", "new.txt"})
        new = next(f for f in got["files"] if f["path"] == "new.txt")
        self.assertTrue(new["untracked"])
        modified = next(f for f in got["files"] if f["path"] == "f.txt")
        self.assertTrue(modified["unstaged"])
        self.assertIn("modified", got["diff"])

    def test_non_repo_reports_repo_false_without_error(self):
        other = tempfile.mkdtemp()
        try:
            got = changes.changes(other)
            self.assertTrue(got["ok"])
            self.assertFalse(got["repo"])
            self.assertEqual(got["files"], [])
        finally:
            shutil.rmtree(other, ignore_errors=True)

    def test_missing_directory_is_a_clean_error(self):
        got = changes.changes(os.path.join(self.dir, "nope-nope"))
        self.assertFalse(got["ok"])
        self.assertIn("error", got)


class Worktrees(ChangesRepo):
    def test_ahead_count_and_agent_flag(self):
        add_agent_worktree(self.dir, name="x", branch="feature")
        got = changes.worktrees(self.dir)
        self.assertTrue(got["ok"])
        self.assertEqual(got["base"], "main")
        self.assertEqual(len(got["worktrees"]), 1)
        wt = got["worktrees"][0]
        self.assertTrue(wt["agent"])
        self.assertEqual(wt["branch"], "feature")
        self.assertEqual(wt["ahead"], 1)
        self.assertEqual(wt["behind"], 0)
        self.assertFalse(wt["dirty"])
        self.assertEqual(wt["subject"], "wt change")

    def test_a_hand_made_worktree_is_not_flagged_as_agent(self):
        run_git(self.dir, "worktree", "add", "sibling", "-b", "other", "-q")
        got = changes.worktrees(self.dir)
        wt = next(w for w in got["worktrees"] if w["branch"] == "other")
        self.assertFalse(wt["agent"])

    def test_dirty_worktree_is_reported_dirty(self):
        full = add_agent_worktree(self.dir, name="y", branch="dirtyfeat",
                                  commit=False)
        write(os.path.join(full, "f.txt"), "one\nuncommitted\n")
        got = changes.worktrees(self.dir)
        wt = next(w for w in got["worktrees"] if w["branch"] == "dirtyfeat")
        self.assertTrue(wt["dirty"])

    def test_no_worktrees_on_a_plain_repo(self):
        got = changes.worktrees(self.dir)
        self.assertEqual(got["worktrees"], [])

    def test_not_a_repo(self):
        other = tempfile.mkdtemp()
        try:
            got = changes.worktrees(other)
            self.assertTrue(got["ok"])
            self.assertFalse(got["repo"])
        finally:
            shutil.rmtree(other, ignore_errors=True)


class Finish(ChangesRepo):
    def test_unknown_action_is_refused(self):
        out = changes.finish(self.dir, "wherever", "explode")
        self.assertFalse(out["ok"])
        self.assertIn("action must be one of", out["error"])

    def test_no_such_worktree(self):
        out = changes.finish(self.dir, os.path.join(self.dir, "nope"), "merge")
        self.assertFalse(out["ok"])
        self.assertIn("no such worktree", out["error"])

    def test_refuses_a_worktree_not_made_by_an_agent(self):
        run_git(self.dir, "worktree", "add", "sibling", "-b", "other", "-q")
        path = os.path.join(self.dir, "sibling")
        out = changes.finish(self.dir, path, "merge")
        self.assertFalse(out["ok"])
        self.assertIn(".claude", out["error"])

    def test_merge_refuses_when_the_main_checkout_is_dirty(self):
        wt = add_agent_worktree(self.dir, name="x", branch="feature")
        write(os.path.join(self.dir, "f.txt"), "one\nmain change\n")
        out = changes.finish(self.dir, wt, "merge")
        self.assertFalse(out["ok"])
        self.assertIn("main checkout has uncommitted changes", out["error"])

    def test_merge_refuses_when_the_worktree_is_dirty(self):
        wt = add_agent_worktree(self.dir, name="x", branch="feature")
        write(os.path.join(wt, "f.txt"), "one\ntwo\nuncommitted\n")
        out = changes.finish(self.dir, wt, "merge")
        self.assertFalse(out["ok"])
        self.assertIn("uncommitted changes", out["error"])

    def test_merge_succeeds_on_a_clean_ahead_worktree(self):
        wt = add_agent_worktree(self.dir, name="x", branch="feature")
        out = changes.finish(self.dir, wt, "merge")
        self.assertTrue(out["ok"], out)
        self.assertEqual(out["action"], "merge")
        merged = read_file(os.path.join(self.dir, "f.txt"))
        self.assertIn("two", merged)

    def test_merge_refuses_when_nothing_to_merge(self):
        wt = add_agent_worktree(self.dir, name="x", branch="feature", commit=False)
        out = changes.finish(self.dir, wt, "merge")
        self.assertFalse(out["ok"])
        self.assertIn("nothing to merge", out["error"])

    def test_discard_refuses_unmerged_work_without_force(self):
        wt = add_agent_worktree(self.dir, name="x", branch="feature")
        out = changes.finish(self.dir, wt, "discard")
        self.assertFalse(out["ok"])
        self.assertTrue(out.get("needs_force"))
        self.assertTrue(os.path.isdir(wt))

    def test_discard_succeeds_with_force(self):
        wt = add_agent_worktree(self.dir, name="x", branch="feature")
        out = changes.finish(self.dir, wt, "discard", force=True)
        self.assertTrue(out["ok"], out)
        self.assertEqual(out["action"], "discard")
        self.assertFalse(os.path.isdir(wt))
        branches = run_git(self.dir, "branch", "--list", "feature")
        self.assertEqual(branches.strip(), "")

    def test_discard_of_clean_merged_work_needs_no_force(self):
        wt = add_agent_worktree(self.dir, name="x", branch="feature", commit=False)
        out = changes.finish(self.dir, wt, "discard")
        self.assertTrue(out["ok"], out)
        self.assertFalse(os.path.isdir(wt))

    def test_pr_uses_the_injected_runner_never_git_push_or_gh(self):
        wt = add_agent_worktree(self.dir, name="x", branch="feature")
        calls = []

        def runner(cmd, cwd=None):
            calls.append((cmd, cwd))
            if cmd[0] == "git":
                return (0, "")
            return (0, "https://github.com/x/y/pull/7\n")

        out = changes.finish(self.dir, wt, "pr", runner=runner)
        self.assertTrue(out["ok"], out)
        self.assertEqual(out["url"], "https://github.com/x/y/pull/7")
        norm = lambda p: os.path.normcase(os.path.abspath(p))  # noqa: E731
        self.assertEqual(calls[0][0][:2], ["git", "-C"])
        self.assertEqual(norm(calls[0][0][2]), norm(wt))
        self.assertIn("push", calls[0][0])
        self.assertEqual(calls[1][0][0], "gh")
        self.assertEqual(norm(calls[1][1]), norm(wt))

    def test_pr_reports_a_push_failure_without_touching_gh(self):
        wt = add_agent_worktree(self.dir, name="x", branch="feature")
        calls = []

        def runner(cmd, cwd=None):
            calls.append(cmd[0])
            return (1, "remote: permission denied")

        out = changes.finish(self.dir, wt, "pr", runner=runner)
        self.assertFalse(out["ok"])
        self.assertIn("push failed", out["error"])
        self.assertNotIn("gh", calls)

    def test_a_merge_git_refuses_is_reported_and_changes_nothing(self):
        # A refusal that leaves no conflict markers (here: unrelated
        # histories) was once reported as a successful merge.
        run_git(self.dir, "worktree", "add", ".claude/worktrees/x", "-q", "--detach")
        wt = os.path.join(self.dir, ".claude", "worktrees", "x")
        run_git(wt, "checkout", "--orphan", "orphanfeat", "-q")
        run_git(wt, "rm", "-f", "-q", "f.txt")
        write(os.path.join(wt, "g.txt"), "two\n")
        run_git(wt, "add", "g.txt")
        run_git(wt, "commit", "-q", "-m", "orphan")
        before = run_git(self.dir, "log", "--oneline").strip()
        out = changes.finish(self.dir, wt, "merge")
        after = run_git(self.dir, "log", "--oneline").strip()
        self.assertFalse(out["ok"])
        self.assertIn("refused", out["error"])
        self.assertEqual(after, before)

    def test_pr_refuses_a_dirty_worktree_before_touching_the_network(self):
        wt = add_agent_worktree(self.dir, name="x", branch="feature")
        write(os.path.join(wt, "f.txt"), "one\ntwo\nuncommitted\n")
        called = []
        out = changes.finish(self.dir, wt, "pr",
                             runner=lambda cmd, cwd=None: called.append(1))
        self.assertFalse(out["ok"])
        self.assertEqual(called, [])


def read_file(path):
    with open(path, encoding="utf-8") as fh:
        return fh.read()


# -- prs.py -------------------------------------------------------------
class ChecksState(unittest.TestCase):
    def test_no_rollup_is_none(self):
        self.assertEqual(prs.checks_state(None), "none")
        self.assertEqual(prs.checks_state([]), "none")

    def test_any_failure_wins(self):
        rollup = [{"status": "COMPLETED", "conclusion": "SUCCESS"},
                  {"status": "COMPLETED", "conclusion": "FAILURE"}]
        self.assertEqual(prs.checks_state(rollup), "failing")

    def test_pending_beats_passing(self):
        rollup = [{"status": "COMPLETED", "conclusion": "SUCCESS"},
                  {"status": "IN_PROGRESS"}]
        self.assertEqual(prs.checks_state(rollup), "pending")

    def test_all_passing(self):
        rollup = [{"status": "COMPLETED", "conclusion": "SUCCESS"},
                  {"status": "COMPLETED", "conclusion": "NEUTRAL"}]
        self.assertEqual(prs.checks_state(rollup), "passing")

    def test_status_context_shape(self):
        rollup = [{"state": "SUCCESS"}]
        self.assertEqual(prs.checks_state(rollup), "passing")
        self.assertEqual(prs.checks_state([{"state": "FAILURE"}]), "failing")
        self.assertEqual(prs.checks_state([{"state": "PENDING"}]), "pending")

    def test_mixed_shapes_failure_still_wins(self):
        rollup = [{"state": "SUCCESS"},
                  {"status": "COMPLETED", "conclusion": "FAILURE"}]
        self.assertEqual(prs.checks_state(rollup), "failing")


class Summarise(unittest.TestCase):
    def test_maps_the_fields_the_badge_needs(self):
        raw = {"number": 3, "state": "OPEN", "url": "u", "title": "t",
               "isDraft": True, "reviewDecision": "APPROVED",
               "headRefName": "feature", "statusCheckRollup": None}
        got = prs.summarise(raw)
        self.assertEqual(got, {"number": 3, "state": "open", "draft": True,
                              "url": "u", "title": "t", "branch": "feature",
                              "review": "approved", "checks": "none"})


@unittest.skipUnless(GIT, "git is not on PATH")
class Tracker(ChangesRepo):
    def test_get_registers_interest_and_returns_none_before_a_fetch(self):
        tr = prs.Tracker(refresh=100.0)
        self.assertIsNone(tr.get(self.dir))

    def test_poll_once_caches_the_injected_fetch_never_calling_gh(self):
        pr = {"number": 1, "state": "open"}
        seen = []
        tr = prs.Tracker(refresh=100.0, fetch=lambda cwd: seen.append(cwd) or pr)
        tr.get(self.dir)
        tr.poll_once()
        self.assertEqual(tr.get(self.dir), pr)
        self.assertEqual(seen, [self.dir])
        # Not refetched while fresh.
        tr.poll_once()
        self.assertEqual(seen, [self.dir])

    def test_a_runtime_error_from_fetch_disables_the_tracker(self):
        tr = prs.Tracker(refresh=100.0,
                         fetch=lambda cwd: (_ for _ in ()).throw(
                             RuntimeError("gh is not installed")))
        tr.get(self.dir)
        tr.poll_once()
        self.assertIsNotNone(tr.disabled)

    def test_directories_nobody_asked_about_recently_are_dropped(self):
        tr = prs.Tracker(refresh=1.0)
        tr.get(self.dir)
        tr.poll_once(now=time.time() + 100000)
        with tr._lock:
            self.assertEqual(tr._wanted, {})


# -- usage.py -------------------------------------------------------------
class Reading(unittest.TestCase):
    def test_extracts_the_fields_the_meter_needs(self):
        got = usage.reading({
            "context_window": {"used_percentage": 12.5, "context_window_size": 200000},
            "cost": {"total_cost_usd": 1.23},
            "model": {"display_name": "Opus 5.5"},
            "rate_limits": {"five_hour": {"used_percentage": 40, "resets_at": 999},
                            "junk": {}},
        })
        self.assertEqual(got["context_pct"], 12.5)
        self.assertEqual(got["cost_usd"], 1.23)
        self.assertEqual(got["model"], "Opus 5.5")
        self.assertEqual(got["limits"], {"five_hour": {"used": 40.0, "resets_at": 999.0}})

    def test_missing_fields_are_none_not_a_crash(self):
        got = usage.reading({})
        self.assertIsNone(got["context_pct"])
        self.assertEqual(got["limits"], {})


class Pace(unittest.TestCase):
    def test_too_little_history_says_nothing(self):
        self.assertIsNone(usage.pace([(1000.0, 10.0)], 1000.0))
        self.assertIsNone(usage.pace([(1000.0, 10.0), (1030.0, 12.0)], 1030.0))

    def test_a_rising_pace_forecasts_when_it_hits_100(self):
        now = 10_000.0
        samples = [(now - 1200, 10.0), (now - 600, 20.0), (now, 30.0)]
        got = usage.pace(samples, now)
        self.assertIsNotNone(got)
        self.assertGreater(got["per_hour"], 0)
        self.assertIsNotNone(got["full_at"])
        self.assertGreater(got["full_at"], now)

    def test_before_reset_is_true_only_when_full_at_precedes_it(self):
        now = 10_000.0
        samples = [(now - 1200, 40.0), (now - 600, 55.0), (now, 70.0)]
        # full_at lands at now+1200 here; a reset far beyond that means the
        # window goes full before it resets, a reset sooner means it does not.
        got = usage.pace(samples, now, resets_at=now + 100000)
        self.assertTrue(got["before_reset"])
        got2 = usage.pace(samples, now, resets_at=now + 10)
        self.assertFalse(got2["before_reset"])

    def test_flat_or_falling_usage_never_predicts_full(self):
        now = 10_000.0
        samples = [(now - 1200, 50.0), (now - 600, 40.0), (now, 30.0)]
        got = usage.pace(samples, now)
        self.assertEqual(got["per_hour"], 0.0)
        self.assertIsNone(got["full_at"])
        self.assertFalse(got["before_reset"])

    def test_old_samples_outside_the_pace_span_are_ignored(self):
        now = 100_000.0
        samples = [(now - 100000, 0.0), (now - 600, 20.0), (now, 30.0)]
        got = usage.pace(samples, now)
        self.assertIsNotNone(got)  # only the last two count


class UsageTracking(unittest.TestCase):
    def test_a_threshold_crossing_fires_once(self):
        crossed = []
        u = usage.Usage(thresholds=(80.0, 95.0), on_crossing=crossed.append)
        payload = {"rate_limits": {"five_hour": {"used_percentage": 85,
                                                  "resets_at": 5000}}}
        got = u.ingest("s1", payload, now=1.0)
        self.assertEqual(len(got), 1)
        self.assertEqual(got[0]["threshold"], 80.0)
        self.assertEqual(len(crossed), 1)
        # Reporting the same percentage again fires nothing more.
        got2 = u.ingest("s1", payload, now=2.0)
        self.assertEqual(got2, [])
        self.assertEqual(len(crossed), 1)

    def test_jumping_straight_past_both_thresholds_reports_the_top_one(self):
        u = usage.Usage(thresholds=(80.0, 95.0))
        payload = {"rate_limits": {"five_hour": {"used_percentage": 97,
                                                  "resets_at": 5000}}}
        got = u.ingest("s1", payload, now=1.0)
        self.assertEqual(len(got), 1)
        self.assertEqual(got[0]["threshold"], 95.0)

    def test_a_new_reset_time_starts_a_fresh_history(self):
        # A reset time that only jitters a little (within `same_window`'s
        # slack) is the same window and must not wipe the pace history.
        u = usage.Usage()
        u.ingest("s1", {"rate_limits": {"five_hour": {"used_percentage": 10,
                                                       "resets_at": 100}}}, now=1.0)
        u.ingest("s1", {"rate_limits": {"five_hour": {"used_percentage": 15,
                                                       "resets_at": 150}}}, now=2.0)
        self.assertEqual(u._history["five_hour"], [(1.0, 10.0), (2.0, 15.0)])
        # A reset time that actually moved on starts a fresh history.
        u.ingest("s1", {"rate_limits": {"five_hour": {"used_percentage": 90,
                                                       "resets_at": 100000}}}, now=3.0)
        self.assertEqual(u._history["five_hour"], [(3.0, 90.0)])

    def test_summary_drops_windows_that_already_reset(self):
        u = usage.Usage()
        u.ingest("s1", {"rate_limits": {"five_hour": {"used_percentage": 10,
                                                       "resets_at": 50}}}, now=1.0)
        out = u.summary(now=1000.0)
        self.assertNotIn("five_hour", out["limits"])

    def test_forget_drops_a_sessions_reading(self):
        u = usage.Usage()
        u.ingest("s1", {"context_window": {"used_percentage": 5}}, now=1.0)
        self.assertIsNotNone(u.session("s1"))
        u.forget("s1")
        self.assertIsNone(u.session("s1"))


# -- activity.py ----------------------------------------------------------
def tool_item(name, summary, tid, speaker="claude"):
    return {"speaker": speaker, "blocks": [
        {"kind": "tool", "name": name, "summary": summary, "id": tid}]}


def result_item(tid, ok=True, text=""):
    return {"speaker": "tool", "blocks": [
        {"kind": "result", "id": tid, "is_error": not ok, "text": text}]}


def text_item(text, speaker="claude"):
    return {"speaker": speaker, "blocks": [{"kind": "text", "text": text}]}


class Timeline(unittest.TestCase):
    def test_pairs_tool_calls_with_their_results(self):
        items = [tool_item("Bash", "npm test", "t1"), result_item("t1", ok=False,
                 text="exit code 1: failed\nmore")]
        calls = activity.timeline(items)
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0]["tool"], "Bash")
        self.assertFalse(calls[0]["ok"])
        self.assertIn("exit code 1", calls[0]["error"])

    def test_a_call_still_running_has_ok_none(self):
        calls = activity.timeline([tool_item("Read", "f.py", "t2")])
        self.assertIsNone(calls[0]["ok"])


class Doing(unittest.TestCase):
    def test_a_running_tool_call_wins(self):
        items = [text_item("earlier"), tool_item("Bash", "npm test", "t1")]
        self.assertEqual(activity.doing(items), "Bash npm test")

    def test_claudes_last_text_after_a_finished_turn(self):
        items = [tool_item("Bash", "npm test", "t1"), result_item("t1"),
                 text_item("all done")]
        self.assertEqual(activity.doing(items), "all done")

    def test_a_pending_human_prompt_is_marked_as_asked(self):
        items = [text_item("do the thing", speaker="you")]
        self.assertEqual(activity.doing(items), "asked: do the thing")

    def test_empty_conversation_is_a_blank_line(self):
        self.assertEqual(activity.doing([]), "")


class Categorise(unittest.TestCase):
    def test_known_shapes(self):
        cases = {
            "the user rejected this": "rejected by user",
            "No such file or directory": "file not found",
            "found 3 matches for old_string": "edit did not match",
            "the file has not been read first": "file not read first",
            "file changed on disk since last read": "file changed on disk",
            "the command timed out": "timed out",
            "exit code 1": "command failed",
            "ECONNREFUSED talking to host": "network",
            "something entirely unrecognised": "other",
        }
        for text, want in cases.items():
            self.assertEqual(activity.categorise(text), want, text)


class Tally(unittest.TestCase):
    def test_counts_calls_and_errors_by_tool_and_category(self):
        calls = activity.timeline([
            tool_item("Bash", "a", "t1"), result_item("t1", ok=False,
                                                       text="exit code 1"),
            tool_item("Read", "b", "t2"), result_item("t2", ok=True),
        ])
        t = activity.tally(calls)
        self.assertEqual(t["calls"], 2)
        self.assertEqual(t["errors"], 1)
        self.assertEqual(t["by_tool"]["Bash"], [1, 1])
        self.assertEqual(t["by_tool"]["Read"], [1, 0])
        self.assertEqual(t["by_category"], {"command failed": 1})

    def test_merge_tallies_adds_them_up(self):
        a = activity.tally(activity.timeline(
            [tool_item("Bash", "a", "t1"), result_item("t1", ok=False, text="x")]))
        b = activity.tally(activity.timeline(
            [tool_item("Bash", "b", "t2"), result_item("t2", ok=True)]))
        merged = activity.merge_tallies([a, b])
        self.assertEqual(merged["calls"], 2)
        self.assertEqual(merged["by_tool"]["Bash"], [2, 1])


class Live(unittest.TestCase):
    def test_pre_then_post_pairs_into_one_finished_call(self):
        live = activity.Live()
        live.ingest("k", {"hook_event_name": "PreToolUse", "tool_name": "Bash",
                          "tool_input": {"command": "npm test"},
                          "tool_use_id": "t1"}, now=100.0)
        live.ingest("k", {"hook_event_name": "PostToolUse", "tool_use_id": "t1"},
                   now=105.0)
        events = live.events("k")
        self.assertEqual(len(events), 1)
        self.assertTrue(events[0]["ok"])
        self.assertIsNone(live.current("k", now=110.0))

    def test_a_running_call_is_current_until_it_ends(self):
        live = activity.Live()
        live.ingest("k", {"hook_event_name": "PreToolUse", "tool_name": "Bash",
                          "tool_input": {"command": "npm test"},
                          "tool_use_id": "t1"}, now=100.0)
        cur = live.current("k", now=105.0)
        self.assertEqual(cur["tool"], "Bash")
        self.assertEqual(cur["since"], 100.0)

    def test_a_call_stuck_for_ten_minutes_is_not_reported_as_current(self):
        live = activity.Live()
        live.ingest("k", {"hook_event_name": "PreToolUse", "tool_name": "Bash",
                          "tool_input": {}, "tool_use_id": "t1"}, now=100.0)
        self.assertIsNone(live.current("k", now=100.0 + 601))

    def test_a_failure_event_is_recorded_not_ok(self):
        live = activity.Live()
        live.ingest("k", {"hook_event_name": "PreToolUse", "tool_name": "Bash",
                          "tool_input": {}, "tool_use_id": "t1"}, now=1.0)
        live.ingest("k", {"hook_event_name": "PostToolUseFailure",
                          "tool_use_id": "t1", "error": "boom\nmore"}, now=2.0)
        self.assertFalse(live.events("k")[0]["ok"])
        self.assertEqual(live.events("k")[0]["error"], "boom")

    def test_a_subagents_calls_are_not_recorded(self):
        live = activity.Live()
        got = live.ingest("k", {"hook_event_name": "PreToolUse", "tool_name": "Bash",
                                "tool_input": {}, "tool_use_id": "t1",
                                "agent_id": "sub1"}, now=1.0)
        self.assertTrue(got)
        self.assertEqual(live.events("k"), [])

    def test_a_non_tool_event_is_not_ingested(self):
        live = activity.Live()
        self.assertFalse(live.ingest("k", {"hook_event_name": "Stop"}))

    def test_forget_drops_a_sessions_events(self):
        live = activity.Live()
        live.ingest("k", {"hook_event_name": "PreToolUse", "tool_name": "Bash",
                          "tool_input": {}, "tool_use_id": "t1"}, now=1.0)
        live.forget("k")
        self.assertEqual(live.events("k"), [])


# -- history.py -----------------------------------------------------------
class Matches(unittest.TestCase):
    def test_every_word_must_appear_in_any_order(self):
        rec = {"title": "Raspberry Pi tunnel fix", "first_prompt": "", "cwd": ""}
        self.assertEqual(history.matches("tunnel pi", rec), "title")
        self.assertIsNone(history.matches("tunnel boat", rec))

    def test_a_prompt_only_match_is_reported_as_prompt(self):
        rec = {"title": "session 4", "first_prompt": "fix the leaky tunnel",
              "cwd": ""}
        self.assertEqual(history.matches("leaky", rec), "prompt")

    def test_a_body_text_only_match_needs_the_text_argument(self):
        rec = {"title": "session 4", "first_prompt": "", "cwd": ""}
        self.assertIsNone(history.matches("leaky", rec))
        self.assertEqual(history.matches("leaky", rec, text="a leaky pipe"), "text")

    def test_empty_query_matches_nothing(self):
        self.assertIsNone(history.matches("", {"title": "x"}))


class Snippet(unittest.TestCase):
    def test_returns_the_match_in_context(self):
        text = "a" * 60 + " the leaky tunnel here " + "b" * 60
        got = history.snippet(text, "leaky", width=20)
        self.assertIn("leaky", got)
        self.assertTrue(got.startswith("…"))
        self.assertTrue(got.endswith("…"))

    def test_no_match_is_empty(self):
        self.assertEqual(history.snippet("nothing here", "zzz"), "")


class FakeInfo:
    def __init__(self, session_id, title="", first_prompt="", cwd="",
                modified=0.0, size=0):
        self.session_id = session_id
        self.custom_title = title
        self.summary = None
        self.first_prompt = first_prompt
        self.cwd = cwd
        self.git_branch = None
        self.last_modified = modified * 1000.0
        self.created_at = modified * 1000.0
        self.file_size = size


class IndexTests(unittest.TestCase):
    def test_list_is_sorted_newest_first_and_cached(self):
        calls = []
        rows = [FakeInfo("a", modified=1.0), FakeInfo("b", modified=5.0)]
        idx = history.Index(lister=lambda: calls.append(1) or rows)
        got = idx.list()
        self.assertEqual([r["session_id"] for r in got], ["b", "a"])
        idx.list()
        self.assertEqual(len(calls), 1)  # cached within max_age

    def test_text_is_cached_by_size_and_rebuilt_when_it_changes(self):
        calls = []

        def reader(sid):
            calls.append(sid)
            return [text_item("hello world")]

        idx = history.Index(reader=reader)
        rec = {"session_id": "s1", "size": 10}
        self.assertIn("hello", idx.text(rec))
        idx.text(rec)
        self.assertEqual(calls, ["s1"])  # same size, no re-read
        idx.text({"session_id": "s1", "size": 20})
        self.assertEqual(calls, ["s1", "s1"])  # size changed, re-read

    def test_search_finds_a_title_match_without_reading_the_transcript(self):
        rows = [FakeInfo("s1", title="Raspberry Pi tunnel", modified=1.0)]
        reads = []
        idx = history.Index(lister=lambda: rows,
                            reader=lambda sid: reads.append(sid) or [])
        got = idx.search("tunnel")
        self.assertEqual(len(got["results"]), 1)
        self.assertEqual(got["results"][0]["where"], "title")
        self.assertEqual(reads, [])

    def test_search_falls_back_to_full_text(self):
        rows = [FakeInfo("s1", title="session 9", modified=1.0)]
        idx = history.Index(lister=lambda: rows,
                            reader=lambda sid: [text_item("a leaky pipe here")])
        got = idx.search("leaky")
        self.assertEqual(len(got["results"]), 1)
        self.assertEqual(got["results"][0]["where"], "text")
        self.assertIn("leaky", got["results"][0]["snippet"])

    def test_search_marks_incomplete_when_the_budget_runs_out(self):
        rows = [FakeInfo("s1", title="session 9", modified=1.0),
               FakeInfo("s2", title="session 10", modified=2.0)]
        idx = history.Index(lister=lambda: rows,
                            reader=lambda sid: [text_item("a leaky pipe")])
        got = idx.search("leaky", budget=-1.0)
        self.assertFalse(got["complete"])

    def test_empty_query_returns_the_plain_list(self):
        rows = [FakeInfo("s1", title="session 9", modified=1.0)]
        idx = history.Index(lister=lambda: rows, reader=lambda sid: [])
        got = idx.search("")
        self.assertTrue(got["complete"])
        self.assertEqual(got["results"][0]["where"], None)


class Resume(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self._which = history.shutil.which
        history.shutil.which = lambda name: "C:\\wt.exe"

    def tearDown(self):
        history.shutil.which = self._which
        shutil.rmtree(self.dir, ignore_errors=True)

    def test_requires_a_session_id(self):
        with self.assertRaises(history.ResumeError):
            history.resume(None, self.dir)

    def test_requires_a_real_directory(self):
        with self.assertRaises(history.ResumeError):
            history.resume("sid", os.path.join(self.dir, "gone"))

    def test_window_launches_the_terminal_command_through_the_injected_popen(self):
        calls = []
        got = history.resume("sid1", self.dir, where="window",
                             popen=lambda cmd, cwd: calls.append((cmd, cwd)))
        self.assertTrue(got["ok"])
        self.assertEqual(got["action"], "resume")
        cmd, cwd = calls[0]
        self.assertEqual(cwd, self.dir)
        self.assertIn("--resume", cmd)
        self.assertIn("sid1", cmd)
        self.assertNotIn("--fork-session", cmd)

    def test_fork_adds_fork_session_to_the_window_command(self):
        calls = []
        got = history.resume("sid1", self.dir, where="window", fork=True,
                             popen=lambda cmd, cwd: calls.append(cmd))
        self.assertEqual(got["action"], "fork")
        self.assertIn("--fork-session", calls[0])

    def test_background_uses_the_injected_bg_start_not_a_real_process(self):
        calls = []

        def bg_start(text, cwd, **opts):
            calls.append((text, cwd, opts))
            return {"id": "bg-1"}

        got = history.resume("sid1", self.dir, where="background", prompt="go",
                             bg_start=bg_start)
        self.assertTrue(got["ok"])
        self.assertEqual(got["id"], "bg-1")
        text, cwd, opts = calls[0]
        self.assertEqual(text, "go")
        self.assertIn("--resume", opts["extra"])
        self.assertIn("sid1", opts["extra"])

    def test_a_background_error_becomes_a_resume_error(self):
        def bg_start(text, cwd, **opts):
            raise background.BackgroundError("no slot")
        with self.assertRaises(history.ResumeError):
            history.resume("sid1", self.dir, where="background", bg_start=bg_start)

    def test_an_unknown_where_is_refused(self):
        with self.assertRaises(history.ResumeError):
            history.resume("sid1", self.dir, where="teleport")

    def test_window_needs_wt_on_path(self):
        history.shutil.which = lambda name: None
        with self.assertRaises(history.ResumeError):
            history.resume("sid1", self.dir, where="window", popen=lambda *a: None)


# -- notify.py --------------------------------------------------------------
class NtfyRequest(unittest.TestCase):
    def test_builds_a_request_against_the_configured_server_and_topic(self):
        req = notify.ntfy_request({"topic": "mytopic", "server": "https://ntfy.sh",
                                   "priority": "high"}, "Title here", "body")
        self.assertEqual(req.full_url, "https://ntfy.sh/mytopic")
        self.assertEqual(req.get_header("Title"), "Title here")
        self.assertEqual(req.get_header("Priority"), "high")
        self.assertIsNone(req.get_header("Authorization"))

    def test_a_token_adds_a_bearer_header(self):
        req = notify.ntfy_request({"topic": "t", "token": "abc"}, "T", "b")
        self.assertEqual(req.get_header("Authorization"), "Bearer abc")

    def test_missing_topic_refuses_to_build_a_request(self):
        with self.assertRaises(ValueError):
            notify.ntfy_request({"topic": ""}, "T", "b")


class SpeakCommand(unittest.TestCase):
    def test_quotes_are_escaped_for_powershell(self):
        cmd = notify.speak_command("it's here")
        script = cmd[-1]
        self.assertIn("it''s here", script)

    def test_hyphens_and_underscores_read_better_spoken(self):
        self.assertEqual(notify.spoken("gem-trip", "needs you"),
                         "gem trip needs you")
        self.assertEqual(notify.spoken("under_score", None),
                         "under score is waiting for you")


class NotifierTests(unittest.TestCase):
    def make(self, still_waiting=None, **cfg_over):
        cfg = {"notify": {"ntfy": {"enabled": True, "topic": "t"},
                          "speak": {"enabled": True}}}
        cfg["notify"].update(cfg_over)
        self.sent, self.said = [], []
        return notify.Notifier(cfg, still_waiting=still_waiting,
                               send=self.sent.append, say=self.said.append)

    def test_attention_schedules_every_enabled_channel_once(self):
        n = self.make()
        scheduled = []
        timer = lambda delay, fn: scheduled.append((delay, fn))
        n.attention("k1", "proj", "needs input", 100.0, timer=timer)
        self.assertEqual(len(scheduled), 2)
        # Scheduling again for the same (key, since) is a no-op.
        n.attention("k1", "proj", "needs input", 100.0, timer=timer)
        self.assertEqual(len(scheduled), 2)

    def test_firing_pushes_and_speaks_when_still_waiting(self):
        n = self.make(still_waiting=lambda key, since: True)
        scheduled = []
        n.attention("k1", "proj", "needs input", 100.0,
                   timer=lambda delay, fn: scheduled.append(fn))
        for fn in scheduled:
            fn()
        self.assertEqual(len(self.sent), 1)
        self.assertEqual(len(self.said), 1)
        self.assertEqual(n.sent, 1)

    def test_answered_before_the_delay_never_sends_anything(self):
        n = self.make(still_waiting=lambda key, since: False)
        scheduled = []
        n.attention("k1", "proj", "needs input", 100.0,
                   timer=lambda delay, fn: scheduled.append(fn))
        for fn in scheduled:
            fn()
        self.assertEqual(self.sent, [])
        self.assertEqual(self.said, [])

    def test_a_disabled_channel_is_never_scheduled(self):
        n = self.make()
        n.set_enabled("speak", False)
        scheduled = []
        n.attention("k1", "proj", "needs input", 100.0,
                   timer=lambda delay, fn: scheduled.append(fn))
        self.assertEqual(len(scheduled), 1)

    def test_a_send_failure_is_recorded_not_raised(self):
        n = notify.Notifier({"notify": {"ntfy": {"enabled": True, "topic": "t"}}},
                            still_waiting=lambda k, s: True,
                            send=lambda req: (_ for _ in ()).throw(OSError("down")))
        n.attention("k1", "proj", "reason", 1.0, timer=lambda delay, fn: fn())
        self.assertIsNotNone(n.last_error)

    def test_budget_fires_immediately_with_no_timer(self):
        n = self.make()
        text = n.budget({"window": "five_hour", "used": 81.4, "resets_at": None})
        self.assertIn("5-hour", text)
        self.assertEqual(len(self.sent), 1)
        self.assertEqual(len(self.said), 1)

    def test_status_reports_enabled_and_configured(self):
        n = self.make()
        got = n.status()
        self.assertTrue(got["ntfy"]["enabled"])
        self.assertTrue(got["ntfy"]["configured"])
        self.assertEqual(got["sent"], 0)

    def test_set_enabled_rejects_an_unknown_channel(self):
        n = self.make()
        with self.assertRaises(ValueError):
            n.set_enabled("carrier-pigeon", True)


# -- store.py: queue + prefs ------------------------------------------------
class QueueStore(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.store = Store(os.path.join(self.dir, "state.json"))

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def test_enqueue_then_take_is_fifo(self):
        self.store.enqueue("s1", "first")
        self.store.enqueue("s1", "second")
        self.assertEqual([i["prompt"] for i in self.store.queued("s1")],
                         ["first", "second"])
        first = self.store.take_queued("s1")
        self.assertEqual(first["prompt"], "first")
        self.assertEqual(len(self.store.queued("s1")), 1)

    def test_take_from_an_empty_queue_is_none(self):
        self.assertIsNone(self.store.take_queued("nobody"))

    def test_an_empty_queue_is_dropped_from_the_map(self):
        item = self.store.enqueue("s1", "x")
        self.store.take_queued("s1")
        self.assertEqual(self.store.queued(), {})

    def test_requeue_front_puts_it_back_first_with_the_error(self):
        self.store.enqueue("s1", "first")
        item = self.store.take_queued("s1")
        self.store.enqueue("s1", "second")
        self.store.requeue_front("s1", item, error="delivery failed")
        got = self.store.queued("s1")
        self.assertEqual(got[0]["prompt"], "first")
        self.assertEqual(got[0]["error"], "delivery failed")
        self.assertEqual(got[1]["prompt"], "second")

    def test_unqueue_removes_one_item_by_id(self):
        # Ticked forward explicitly so the two ids cannot collide - see
        # test_enqueue_ids_can_collide_within_the_same_clock_tick below for
        # what happens to this same behaviour when they do.
        import itertools
        ticks = itertools.count(1700000000.0, 1.0)
        orig_time = store_mod.time
        store_mod.time = types.SimpleNamespace(time=lambda: next(ticks),
                                               sleep=lambda *a: None)
        try:
            item = self.store.enqueue("s1", "x")
            self.store.enqueue("s1", "y")
        finally:
            store_mod.time = orig_time
        self.assertTrue(self.store.unqueue("s1", item["id"]))
        self.assertEqual(len(self.store.queued("s1")), 1)
        self.assertFalse(self.store.unqueue("s1", "no-such-id"))

    def test_enqueue_ids_are_unique_within_one_clock_tick(self):
        # Ids were the clock in microseconds, so two prompts queued in the
        # same tick shared one, and cancelling one cancelled both.
        orig_time = store_mod.time
        store_mod.time = types.SimpleNamespace(time=lambda: 1700000000.0,
                                               sleep=lambda *a: None)
        try:
            a = self.store.enqueue("s1", "x")
            b = self.store.enqueue("s1", "y")
        finally:
            store_mod.time = orig_time
        self.assertNotEqual(a["id"], b["id"])

    def test_queue_survives_a_reload(self):
        self.store.enqueue("s1", "persisted")
        reloaded = Store(self.store.path)
        self.assertEqual(reloaded.queued("s1")[0]["prompt"], "persisted")

    def test_prefs_round_trip_and_none_clears_a_key(self):
        self.store.set_prefs(pinned=["a", "b"], group="x")
        self.assertEqual(self.store.prefs(), {"pinned": ["a", "b"], "group": "x"})
        self.store.set_prefs(group=None)
        self.assertEqual(self.store.prefs(), {"pinned": ["a", "b"]})


# -- hooks/cc_statusline.py -------------------------------------------------
class StatuslineLine(unittest.TestCase):
    def test_full_payload(self):
        got = cc_statusline.line({
            "model": {"display_name": "Opus 5.5"},
            "context_window": {"used_percentage": 23.4},
            "rate_limits": {"five_hour": {"used_percentage": 41},
                            "seven_day": {"used_percentage": 12}},
        })
        self.assertEqual(got, "Opus 5.5 · ctx 23% · 5h 41% · 7d 12%")

    def test_missing_fields_are_skipped_not_blank(self):
        self.assertEqual(cc_statusline.line({}), "")
        got = cc_statusline.line({"model": {"display_name": "Sonnet"}})
        self.assertEqual(got, "Sonnet")

    def test_non_numeric_percentages_are_ignored(self):
        got = cc_statusline.line({"context_window": {"used_percentage": "n/a"}})
        self.assertEqual(got, "")


# -- dispatcher.py: queue, statusline ingest, activity/errors, resume,------
# -- finish_worktree guard, HTTP routes ------------------------------------
class Queue(Base):
    def test_idle_dispatchable_session_gets_immediate_delivery(self):
        self.live(status="idle")
        self.start_hook()
        out = self.control.enqueue(SID, "do the thing")
        self.assertTrue(out["ok"])
        self.assertEqual(len(self.sent), 1)
        self.assertEqual(self.sent[0][2], "do the thing")
        self.assertEqual(self.control.store.queued(SID), [])

    def test_a_busy_session_holds_the_prompt_until_a_stop_hook(self):
        self.live(status="busy")
        self.start_hook()
        self.control.enqueue(SID, "first")
        self.control.enqueue(SID, "second")
        self.assertEqual(self.sent, [])
        self.assertEqual(len(self.control.store.queued(SID)), 2)

        self.control.on_hook({"hook_event_name": "Stop", "session_id": SID,
                              "cwd": CWD, "last_assistant_message": "ok"})
        self.assertEqual(len(self.sent), 1)
        self.assertEqual(self.sent[0][2], "first")
        self.assertEqual(len(self.control.store.queued(SID)), 1)

        self.control.on_hook({"hook_event_name": "Stop", "session_id": SID,
                              "cwd": CWD, "last_assistant_message": "ok"})
        self.assertEqual(len(self.sent), 2)
        self.assertEqual(self.sent[1][2], "second")
        self.assertEqual(self.control.store.queued(SID), [])

        # A third Stop has nothing left to deliver.
        self.control.on_hook({"hook_event_name": "Stop", "session_id": SID,
                              "cwd": CWD, "last_assistant_message": "ok"})
        self.assertEqual(len(self.sent), 2)

    def test_a_failed_delivery_is_requeued_with_the_error(self):
        self.live(status="idle")
        self.start_hook()
        inbox.send = lambda *a, **k: (_ for _ in ()).throw(
            inbox.InboxError("pipe is gone"))
        out = self.control.enqueue(SID, "x")
        self.assertTrue(out["ok"])
        self.assertEqual(self.sent, [])
        q = self.control.store.queued(SID)
        self.assertEqual(len(q), 1)
        self.assertIn("pipe is gone", q[0]["error"])

    def test_sweep_queue_delivers_to_a_session_that_registered_late(self):
        # Queued before the session had a credential; sweep catches it.
        self.live(status="idle")
        self.control.store.enqueue(SID, "waiting")
        self.assertEqual(self.control.sweep_queue(), [])  # not dispatchable yet
        self.start_hook()
        self.control.invalidate()
        delivered = self.control.sweep_queue()
        self.assertEqual(len(delivered), 1)
        self.assertEqual(self.sent[0][2], "waiting")

    def test_enqueue_with_no_prompt_is_refused(self):
        out = self.control.enqueue(SID, "   ")
        self.assertFalse(out["ok"])

    def test_enqueue_for_an_unknown_session_is_refused(self):
        out = self.control.enqueue("nope", "hi")
        self.assertFalse(out["ok"])


class StatuslineIngest(Base):
    def test_a_statusline_event_updates_usage_not_the_event_log(self):
        before = len(self.control.store.events(999))
        out = self.control.on_hook({
            "hook_event_name": "ccontrol_statusline", "session_id": SID,
            "context_window": {"used_percentage": 55},
            "rate_limits": {"five_hour": {"used_percentage": 10, "resets_at": 1}},
        })
        self.assertTrue(out["ok"])
        got = self.control.usage.session(SID)
        self.assertEqual(got["context_pct"], 55)
        self.assertEqual(len(self.control.store.events(999)), before)


class ActivityMethod(Base):
    def test_prefers_hooks_over_the_conversation(self):
        self.live(status="busy")
        self.start_hook()
        self.control.live.ingest(SID, {"hook_event_name": "PreToolUse",
                                       "tool_name": "Bash",
                                       "tool_input": {"command": "npm test"},
                                       "tool_use_id": "t1"})
        out = self.control.activity(SID)
        self.assertEqual(out["source"], "hooks")
        self.assertEqual(out["timeline"][0]["tool"], "Bash")

    def test_falls_back_to_the_conversation_when_there_are_no_hook_events(self):
        self.live(status="idle")
        self.start_hook()
        orig = conversation.read
        conversation.read = lambda sid, limit=200: [
            tool_item("Read", "f.py", "t1"), result_item("t1", ok=False, text="ECONNREFUSED")]
        try:
            out = self.control.activity(SID)
        finally:
            conversation.read = orig
        self.assertEqual(out["source"], "conversation")
        self.assertEqual(out["tally"]["errors"], 1)
        self.assertEqual(out["tally"]["by_category"], {"network": 1})


class ErrorsMethod(Base):
    def test_aggregates_tool_failures_across_recent_sessions_by_project(self):
        rows = [{"session_id": "s1", "size": 5, "cwd": "Y:/p/proj-a",
                "modified": time.time()},
               {"session_id": "s2", "size": 5, "cwd": "Y:/p/proj-b",
                "modified": time.time()}]
        self.control.history.list = lambda max_age=30.0: rows
        reads = []

        def fake_read(sid, limit=0):
            reads.append(sid)
            if sid == "s1":
                return [tool_item("Bash", "x", "t1"),
                       result_item("t1", ok=False, text="exit code 1")]
            return [tool_item("Read", "y", "t2"), result_item("t2", ok=True)]

        orig = conversation.read
        conversation.read = fake_read
        try:
            got = self.control.errors(days=30)
            got2 = self.control.errors(days=30)  # should hit the cache
        finally:
            conversation.read = orig
        self.assertTrue(got["ok"])
        self.assertEqual(got["sessions"], 2)
        self.assertEqual(got["errors"], 1)
        self.assertEqual(got["by_project"]["proj-a"]["errors"], 1)
        self.assertEqual(got["by_project"]["proj-b"]["errors"], 0)
        self.assertEqual(got2, got)
        self.assertEqual(reads, ["s1", "s2"])  # not re-read for the cached call


class ResumeMethod(Base):
    def test_refuses_to_resume_a_still_running_session_into_a_window(self):
        self.live(status="idle")
        self.start_hook()
        out = self.control.resume(SID, where="window")
        self.assertFalse(out["ok"])
        self.assertIn("still running", out["error"])

    def test_forking_a_running_session_is_allowed(self):
        self.live(status="idle")
        self.start_hook()
        calls = []
        orig = dispatcher.history.resume
        dispatcher.history.resume = lambda *a, **k: calls.append((a, k)) or {
            "ok": True, "action": "fork", "command": "claude --resume x --fork-session"}
        try:
            out = self.control.resume(SID, where="window", fork=True)
        finally:
            dispatcher.history.resume = orig
        self.assertTrue(out["ok"])
        self.assertEqual(len(calls), 1)
        events = self.control.store.events(10)
        self.assertTrue(any(e.get("kind") == "fork" for e in events))

    def test_a_dead_session_resolves_its_cwd_from_history(self):
        self.control.history.list = lambda max_age=30.0: [
            {"session_id": "old-sid", "cwd": CWD, "modified": time.time(), "size": 1}]
        calls = []
        orig = dispatcher.history.resume
        dispatcher.history.resume = lambda sid, cwd, **k: calls.append((sid, cwd, k)) or {
            "ok": True, "action": "resume", "command": "claude --resume old-sid"}
        try:
            out = self.control.resume("old-sid")
        finally:
            dispatcher.history.resume = orig
        self.assertTrue(out["ok"])
        self.assertEqual(calls[0][1], CWD)

    def test_a_resume_error_is_reported_not_raised(self):
        orig = dispatcher.history.resume

        def boom(*a, **k):
            raise history.ResumeError("its directory is gone")
        dispatcher.history.resume = boom
        try:
            out = self.control.resume("whatever", cwd=self.dir)
        finally:
            dispatcher.history.resume = orig
        self.assertFalse(out["ok"])
        self.assertIn("gone", out["error"])


@unittest.skipUnless(GIT, "git is not on PATH")
class FinishWorktreeGuard(Base):
    def setUp(self):
        super().setUp()
        self.repo = tempfile.mkdtemp()
        init_repo(self.repo)

    def tearDown(self):
        shutil.rmtree(self.repo, ignore_errors=True)
        super().tearDown()

    def test_an_unknown_project_is_refused_before_touching_git(self):
        called = []
        orig = dispatcher.changes.finish
        dispatcher.changes.finish = lambda *a, **k: called.append(1)
        try:
            out = self.control.finish_worktree(self.repo, "x", "merge")
        finally:
            dispatcher.changes.finish = orig
        self.assertFalse(out["ok"])
        self.assertIn("not a known project", out["error"])
        self.assertEqual(called, [])

    def test_a_known_project_passes_through_and_logs_the_event(self):
        wt = add_agent_worktree(self.repo, name="x", branch="feature")
        self.live(cwd=self.repo)
        out = self.control.finish_worktree(self.repo, wt, "merge")
        self.assertTrue(out["ok"], out)
        events = self.control.store.events(10)
        self.assertTrue(any(e.get("kind") == "worktree_merge" for e in events))


class HttpRoutes(Base):
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

    def _get(self, path):
        with urllib.request.urlopen("http://127.0.0.1:%d%s" % (self.port, path),
                                    timeout=5) as resp:
            return resp.status, json.loads(resp.read().decode())

    def _post(self, path, body):
        req = urllib.request.Request(
            "http://127.0.0.1:%d%s" % (self.port, path),
            data=json.dumps(body).encode(),
            headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                return resp.status, json.loads(resp.read().decode())
        except urllib.error.HTTPError as exc:
            return exc.code, json.loads(exc.read().decode())

    def test_get_api_usage(self):
        status, body = self._get("/api/usage")
        self.assertEqual(status, 200)
        self.assertIn("limits", body)
        self.assertIn("thresholds", body)

    def test_post_queue_delivers_then_get_queue_is_empty(self):
        self.live(status="idle")
        self.start_hook()
        status, body = self._post("/api/queue", {"session_id": SID, "prompt": "hi"})
        self.assertEqual(status, 200)
        self.assertTrue(body["ok"])
        status, body = self._get("/api/queue")
        self.assertEqual(status, 200)
        self.assertEqual(body["queue"], {})

    def test_post_worktree_on_an_unknown_project_answers_200_not_ok(self):
        status, body = self._post("/api/worktree",
                                  {"project": "Z:/nope", "path": "x", "action": "merge"})
        self.assertEqual(status, 200)
        self.assertFalse(body["ok"])

    def test_post_resume_for_an_unknown_session_answers_200_not_ok(self):
        status, body = self._post("/api/resume", {"session_id": "nope-nope"})
        self.assertEqual(status, 200)
        self.assertFalse(body["ok"])

    def test_get_api_errors(self):
        status, body = self._get("/api/errors")
        self.assertEqual(status, 200)
        self.assertIn("calls", body)


class HookOutputs(unittest.TestCase):
    """The two hook scripts' pure pieces, imported from hooks/."""

    @classmethod
    def setUpClass(cls):
        import importlib
        import sys as _sys
        hooks = os.path.join(os.path.dirname(os.path.abspath(__file__)), "hooks")
        if hooks not in _sys.path:
            _sys.path.insert(0, hooks)
        cls.perm = importlib.import_module("cc_permission")
        cls.hook = importlib.import_module("cc_hook")

    def test_the_documented_reply_is_a_decision_object(self):
        out = self.perm.reply("allow", "safe read", "object")
        self.assertEqual(out, {"hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": {"behavior": "allow"}}})

    def test_a_deny_carries_its_message(self):
        out = self.perm.reply("deny", "touches policy.json", "object")
        self.assertEqual(out["hookSpecificOutput"]["decision"],
                         {"behavior": "deny", "message": "touches policy.json"})

    def test_legacy_stays_the_default_until_switched(self):
        # Switching the format also switches auto-approval on, so it must
        # never happen without policy.json saying so.
        out = self.perm.reply("allow", "x")
        self.assertEqual(out["hookSpecificOutput"]["decision"], "allow")

    def test_tool_payloads_lose_file_contents(self):
        out = self.hook.trim_tool_payload({
            "hook_event_name": "PreToolUse", "session_id": "s",
            "tool_name": "Write", "tool_use_id": "t",
            "tool_input": {"file_path": "a.py", "content": "x" * 100000}})
        self.assertEqual(out["tool_input"], {"file_path": "a.py"})
        self.assertLess(len(json.dumps(out)), 1000)

    def test_a_failure_keeps_a_short_reason(self):
        out = self.hook.trim_tool_payload({
            "hook_event_name": "PostToolUseFailure", "session_id": "s",
            "tool_name": "Bash", "tool_input": {"command": "make"},
            "error": "exit 2", "tool_response": "y" * 5000})
        self.assertEqual(out["error"], "exit 2")
        self.assertEqual(len(out["tool_response"]), 500)


class ParkWait(unittest.TestCase):
    """How long the dispatcher holds a parked permission request."""

    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.control = dispatcher.Control(Store(os.path.join(self.dir, "s.json")),
                                          roots=[])
        self.rows = []
        self.control.sessions = lambda max_age=2.0: {"sessions": self.rows}

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def test_a_background_session_gets_the_long_wait(self):
        self.rows = [{"sessionId": "bg", "kind": "background"}]
        self.assertEqual(self.control.park_wait(
            {"session_id": "bg", "wait": 20, "wait_background": 300}), 300)

    def test_an_interactive_session_gets_the_short_wait(self):
        # You may be at its terminal, and the prompt there is held back
        # while this waits.
        self.rows = [{"sessionId": "i", "kind": "interactive"}]
        self.assertEqual(self.control.park_wait(
            {"session_id": "i", "wait": 20, "wait_background": 300}), 20)

    def test_an_unknown_session_gets_the_short_wait(self):
        self.assertEqual(self.control.park_wait(
            {"session_id": "?", "wait": 20, "wait_background": 300}), 20)

    def test_an_older_hook_without_the_field_keeps_its_wait(self):
        self.rows = [{"sessionId": "bg", "kind": "background"}]
        self.assertEqual(self.control.park_wait({"session_id": "bg", "wait": 90}), 90)


if __name__ == "__main__":
    unittest.main(verbosity=2)
