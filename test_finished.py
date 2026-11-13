"""Finished background sessions: never "waiting for you", always removable,
optionally hidden after a while.

`claude agents --json` goes on listing a finished `claude --bg` job as
`state: done` until it is opened in Claude Code's agent view. On 2026-11-12 a
17-day-old probe ("Reply with the single word OK") sat in the window flagged
"waiting for you" by an idle_prompt from before it finished, with no ✕.

Run: python test_finished.py
"""
import json
import os
import time
import unittest

from imperatorium import dispatcher, registry, settings, ui
from test_dispatcher import Base

NOW = 1_800_000_000.0


def job(**extra):
    row = {"sessionId": "04f327fc-a375-48b3-b69b-3142826d07c4", "id": "04f327fc",
           "kind": "background", "state": "done", "name": "reply-with-the-single",
           "cwd": "C:/w/imperatorium", "startedAt": int((NOW - 17 * 86400) * 1000),
           "key": "04f327fc-a375-48b3-b69b-3142826d07c4"}
    row.update(extra)
    return row


class Finished(unittest.TestCase):
    def test_only_a_completed_background_job_without_a_process(self):
        self.assertTrue(registry.finished(job()))
        self.assertFalse(registry.finished(job(state="working")))
        self.assertFalse(registry.finished(job(state="blocked")))   # waits on you
        self.assertFalse(registry.finished(job(state="failed")))    # needs you too
        self.assertFalse(registry.finished(job(pid=4242)))          # resumed
        self.assertFalse(registry.finished(job(kind="interactive")))

    def test_when_it_last_did_anything(self):
        self.assertEqual(dispatcher.finished_at(job(last_event_at=NOW - 60)), NOW - 60)
        self.assertEqual(dispatcher.finished_at(job()), NOW - 17 * 86400)
        self.assertEqual(dispatcher.finished_at({"kind": "background"}), 0)

    def test_the_setting_is_read_safely(self):
        self.assertEqual(dispatcher.hide_finished_hours({}), 0)
        self.assertEqual(dispatcher.hide_finished_hours({"list": {"hide_finished_hours": 24}}), 24)
        for bad in (-1, "24", True, None, [24]):
            self.assertEqual(dispatcher.hide_finished_hours({"list": {"hide_finished_hours": bad}}),
                             0, bad)


class Attention(Base):
    def test_a_stale_notification_is_cleared_for_a_finished_job(self):
        key = job()["key"]
        self.control.store.raise_attention(key, "waiting for you", dispatcher.BY_NOTIFICATION,
                                           last_notification="idle_prompt")
        extra = self.control.store.sessions()[key]
        row = job()
        self.control._overlay(row, extra, key)
        self.assertFalse(row.get("attention"))
        self.assertNotIn("attention_since", self.control.store.sessions()[key])

    def test_even_a_failure_flag_goes_once_the_job_is_done(self):
        key = job()["key"]
        self.control.store.raise_attention(key, "API error", dispatcher.BY_FAILURE)
        row = job()
        self.control._overlay(row, self.control.store.sessions()[key], key)
        self.assertFalse(row.get("attention"))

    def test_a_blocked_job_still_asks(self):
        key = job()["key"]
        self.control.store.raise_attention(key, "waiting for you", dispatcher.BY_NOTIFICATION)
        row = job(state="blocked")
        self.control._overlay(row, self.control.store.sessions()[key], key)
        self.assertTrue(row.get("attention"))


class Hiding(Base):
    def rows(self):
        return [job(last_event_at=NOW - 30 * 3600),                    # finished, old
                job(sessionId="b", key="b", last_event_at=NOW - 3600),  # finished, recent
                job(sessionId="c", key="c", state="working", last_event_at=NOW - 99 * 3600),
                {"sessionId": "d", "key": "d", "kind": "interactive", "pid": 1, "status": "idle",
                 "last_event_at": NOW - 99 * 3600}]

    def test_off_by_default(self):
        self.assertEqual(self.control.hide_finished_hours, 0)
        self.assertEqual(len(self.control._unhidden(self.rows(), now=NOW)), 4)

    def test_only_old_finished_jobs_are_hidden(self):
        self.control.hide_finished_hours = 24
        kept = [r["key"] for r in self.control._unhidden(self.rows(), now=NOW)]
        self.assertEqual(kept, ["b", "c", "d"])

    def test_it_is_a_setting_applied_at_once(self):
        field = next(f for f in settings.FIELDS if f["key"] == "list.hide_finished_hours")
        self.assertEqual(field["applies"], settings.LIVE)
        self.assertEqual(settings.check(field, "24"), 24)
        with self.assertRaises(ValueError):
            settings.check(field, "-1")
        path = os.path.join(self.dir, "config.json")
        with open(path, "w", encoding="utf-8") as fh:
            json.dump({"background": {"model": "opus"}}, fh)
        self.control.config_path = path
        got = self.control.save_settings({"list.hide_finished_hours": 48})
        self.assertTrue(got["ok"], got)
        self.assertEqual(got["restart"], [])
        self.assertEqual(self.control.hide_finished_hours, 48)
        with open(path, encoding="utf-8") as fh:
            disk = json.load(fh)
        self.assertEqual(disk["list"]["hide_finished_hours"], 48)
        # Kept out of the block that becomes `claude --bg` options.
        self.assertEqual(disk["background"], {"model": "opus"})


class Window(unittest.TestCase):
    def test_a_tile_with_no_process_can_be_taken_off_the_list(self):
        s = ui.SCRIPT
        a = s.index("function tile(s, short)")
        body = s[a:s.index("const spend", a)]
        # Dismiss no longer depends on the tile being archived.
        self.assertNotIn("archived ? `<span data-arch=\"${esc(pkey(s))}\" title=\"unarchive\">↩</span>${",
                         body)
        self.assertIn('data-dismiss=', body)
        self.assertIn('s.state === "working" || RUNNING[s.status] ? ""', body)

    def test_a_finished_job_reads_finished(self):
        self.assertIn('if (s.state === "done") return ["d-done", "finished"];', ui.SCRIPT)


if __name__ == "__main__":
    unittest.main()
