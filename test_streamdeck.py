"""The Stream Deck surface's pure logic - layout, labelling, state mapping.

Rendering and USB are not tested here; those need the device. Run this with
the venv interpreter, which has pillow and streamdeck:

    .venv\\Scripts\\python.exe test_streamdeck.py

It skips itself under an interpreter without those, so the plain
`python test_*.py` sweep stays green.
"""
import os
import unittest

try:
    import streamdeck_surface as sd
except Exception as exc:  # pragma: no cover - depends on the interpreter
    sd = None
    REASON = "streamdeck/pillow unavailable (%s)" % type(exc).__name__


class FakeDeck:
    def __init__(self, keys=15):
        self._keys = keys

    def key_count(self):
        return self._keys


def row(cwd, **kw):
    base = {"sessionId": "s-" + cwd, "cwd": cwd, "status": "idle"}
    base.update(kw)
    return base


@unittest.skipIf(sd is None, "surface module not importable")
class Labels(unittest.TestCase):
    def test_hyphens_become_word_breaks(self):
        self.assertEqual(sd.wrap("laser-ledger"), ["laser", "ledger"])

    def test_long_single_word_is_truncated_with_an_ellipsis(self):
        lines = sd.wrap("bookoferrantpages")
        self.assertEqual(len(lines), 1)
        self.assertTrue(lines[0].endswith("\u2026"))
        self.assertLessEqual(len(lines[0]), 9)

    def test_never_more_than_three_lines(self):
        self.assertLessEqual(len(sd.wrap("a b c d e f g h i j k")), 3)

    def test_empty_label_still_returns_a_line(self):
        self.assertEqual(len(sd.wrap("")), 1)


@unittest.skipIf(sd is None, "surface module not importable")
class States(unittest.TestCase):
    def test_no_session_is_none(self):
        self.assertEqual(sd.state_for(None), "none")

    def test_attention_outranks_busy(self):
        self.assertEqual(
            sd.state_for({"status": "busy", "attention": True}), "blocked"
        )

    def test_busy_and_working_both_read_as_working(self):
        self.assertEqual(sd.state_for({"status": "busy"}), "working")
        self.assertEqual(sd.state_for({"state": "working"}), "working")

    def test_idle_is_idle(self):
        self.assertEqual(sd.state_for({"status": "idle"}), "idle")

    def test_idle_waiting_on_a_background_task_is_working(self):
        self.assertEqual(sd.state_for({"status": "idle", "background_tasks": 1}),
                         "working")

    def test_finished_background_job_is_not_shown_as_live(self):
        self.assertEqual(sd.state_for({"state": "done"}), "none")


@unittest.skipIf(sd is None, "surface module not importable")
class Layout(unittest.TestCase):
    def setUp(self):
        self.by_path = {
            os.path.normcase(os.path.abspath(p)): n
            for p, n in [
                ("Y:/projects-software/laser-ledger", "laser-ledger"),
                ("Y:/projects-software/gem-trip", "gem-trip"),
                ("Y:/projects-software/amarantha", "amarantha"),
            ]
        }

    def names(self, slots):
        return [sl["project"] for sl in slots]

    def test_pinned_keys_keep_their_position(self):
        surface = sd.Surface(FakeDeck(), pinned=["laser-ledger", "gem-trip"])
        slots = surface.layout([row("Y:/projects-software/amarantha")], self.by_path)
        self.assertEqual(self.names(slots), ["laser-ledger", "gem-trip", "amarantha"])
        self.assertIsNone(slots[0]["row"])

    def test_unpinned_blocked_project_sorts_ahead_of_other_extras(self):
        surface = sd.Surface(FakeDeck(), pinned=[])
        slots = surface.layout(
            [row("Y:/projects-software/amarantha"),
             row("Y:/projects-software/gem-trip", attention=True)],
            self.by_path)
        self.assertEqual(slots[0]["project"], "gem-trip")

    def test_order_is_capped_at_the_key_count(self):
        surface = sd.Surface(FakeDeck(keys=2), pinned=["a", "b", "c", "d"])
        self.assertEqual(len(surface.layout([], self.by_path)), 2)

    def test_every_session_of_a_project_gets_a_key(self):
        surface = sd.Surface(FakeDeck(), pinned=[])
        slots = surface.layout(
            [row("Y:/projects-software/gem-trip", sessionId="late", startedAt=2),
             row("Y:/projects-software/gem-trip", sessionId="early", startedAt=1)],
            self.by_path)
        self.assertEqual([sl["row"]["sessionId"] for sl in slots], ["early", "late"])
        self.assertEqual([sl["count"] for sl in slots], ["1/2", "2/2"])

    def test_a_lone_session_has_no_counter(self):
        surface = sd.Surface(FakeDeck(), pinned=[])
        [slot] = surface.layout([row("Y:/projects-software/gem-trip")], self.by_path)
        self.assertIsNone(slot["count"])

    def test_pinned_project_expands_in_place(self):
        surface = sd.Surface(FakeDeck(), pinned=["gem-trip", "laser-ledger"])
        slots = surface.layout(
            [row("Y:/projects-software/gem-trip", sessionId="a"),
             row("Y:/projects-software/gem-trip", sessionId="b")], self.by_path)
        self.assertEqual(self.names(slots), ["gem-trip", "gem-trip", "laser-ledger"])

    def test_open_sessions_beat_pinned_projects_with_none(self):
        surface = sd.Surface(FakeDeck(keys=3), pinned=["laser-ledger", "amarantha"])
        slots = surface.layout(
            [row("Y:/projects-software/gem-trip", sessionId="a"),
             row("Y:/projects-software/gem-trip", sessionId="b")], self.by_path)
        self.assertEqual(self.names(slots), ["laser-ledger", "gem-trip", "gem-trip"])
        self.assertTrue(all(sl["row"] for sl in slots[1:]))

    def test_finished_and_remote_sessions_get_no_key(self):
        surface = sd.Surface(FakeDeck(), pinned=[])
        slots = surface.layout(
            [row("Y:/projects-software/gem-trip", status=None, state="done"),
             row("Y:/projects-software/amarantha", remote=True)], self.by_path)
        self.assertEqual(slots, [])

    def test_sessions_outside_the_catalogue_are_named_by_folder(self):
        surface = sd.Surface(FakeDeck(), pinned=[])
        [slot] = surface.layout([row("C:/somewhere/else")], self.by_path)
        self.assertEqual(slot["label"], "else")

    def test_a_press_on_an_empty_slot_does_nothing(self):
        surface = sd.Surface(FakeDeck(), pinned=[])
        surface.slots = []
        surface.on_press(None, 7, True)  # must not raise or call the API


@unittest.skipIf(sd is None, "surface module not importable")
class Flash(unittest.TestCase):
    def setUp(self):
        self.s = sd.Surface(FakeDeck(), pinned=[])

    def test_first_sight_does_not_flash(self):
        self.assertEqual(self.s.flash_state("a", "idle", 0.0), "idle")
        self.assertFalse(self.s.flashing())

    def test_a_change_alternates_new_and_old_then_settles(self):
        self.s.flash_state("a", "working", 0.0)
        seen = [self.s.flash_state("a", "idle", t) for t in (10.0, 10.3, 10.6, 11.1, 14.9)]
        self.assertEqual(seen, ["idle", "idle", "working", "idle", "working"])
        self.assertTrue(self.s.flashing())
        self.assertEqual(self.s.flash_state("a", "idle", 15.0), "idle")
        self.assertFalse(self.s.flashing())

    def test_a_change_mid_flash_restarts_from_the_shown_state(self):
        self.s.flash_state("a", "working", 0.0)
        self.s.flash_state("a", "idle", 1.0)
        self.assertEqual(self.s.flash_state("a", "blocked", 2.0), "blocked")
        self.assertEqual(self.s.flash_state("a", "blocked", 2.6), "idle")


@unittest.skipIf(sd is None, "surface module not importable")
class Decisions(unittest.TestCase):
    PENDING = [{"id": "a1", "tool": "Bash", "cwd": "Y:/p/gem-trip"},
               {"id": "b2", "tool": "Edit", "cwd": "Y:/p/amarantha"}]

    def surface(self):
        return sd.Surface(FakeDeck(15), [])

    def test_no_pending_leaves_every_key_to_projects(self):
        s = self.surface()
        s.set_decision([])
        self.assertEqual(s.project_keys(), 15)
        self.assertIsNone(s.decision_slot(14))

    def test_pending_takes_the_last_three_keys(self):
        s = self.surface()
        s.set_decision(self.PENDING, now=100.0)
        self.assertEqual(s.project_keys(), 12)
        self.assertEqual([s.decision_slot(k) for k in (11, 12, 13, 14)],
                         [None, "info", "allow", "deny"])
        self.assertEqual(s.decision["id"], "a1")  # oldest first

    def test_a_press_before_the_key_is_armed_does_nothing(self):
        s = self.surface()
        s.set_decision(self.PENDING, now=100.0)
        self.assertIsNone(s.decide_press(13, now=100.2))
        self.assertEqual(s.decide_press(13, now=101.5),
                         {"id": "a1", "verdict": "allow", "who": "streamdeck"})
        self.assertEqual(s.decide_press(14, now=101.5)["verdict"], "deny")

    def test_the_info_key_never_decides(self):
        s = self.surface()
        s.set_decision(self.PENDING, now=100.0)
        self.assertIsNone(s.decide_press(12, now=200.0))

    def test_a_new_request_rearms_the_keys(self):
        # The next request replacing the answered one must not inherit the
        # old arm time, or a double press would answer both.
        s = self.surface()
        s.set_decision(self.PENDING, now=100.0)
        s.set_decision(self.PENDING[1:], now=105.0)
        self.assertIsNone(s.decide_press(13, now=105.3))
        self.assertEqual(s.decide_press(13, now=106.5)["id"], "b2")

    def test_the_same_request_keeps_its_arm_time(self):
        s = self.surface()
        s.set_decision(self.PENDING, now=100.0)
        s.set_decision(self.PENDING, now=103.0)
        self.assertIsNotNone(s.decide_press(13, now=103.1))

    def test_layout_shrinks_while_a_decision_shows(self):
        s = sd.Surface(FakeDeck(15), ["p%d" % i for i in range(15)])
        s.set_decision(self.PENDING)
        self.assertEqual(len(s.layout([], {})), 12)



@unittest.skipIf(sd is None, "surface module not importable")
class DesktopAppGlyph(unittest.TestCase):
    def test_glyph_marks_the_top_left_corner_only(self):
        from PIL import Image, ImageDraw
        image = Image.new("RGB", (72, 72), "black")
        sd.draw_app_icon(ImageDraw.Draw(image), "white")
        self.assertEqual(image.getpixel((5, 5)), (255, 255, 255))
        self.assertEqual(image.getpixel((60, 60)), (0, 0, 0))


if __name__ == "__main__":
    if sd is None:
        print("skipped:", REASON)
    unittest.main(verbosity=2)
