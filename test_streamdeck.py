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

    def test_pinned_keys_keep_their_position(self):
        surface = sd.Surface(FakeDeck(), pinned=["laser-ledger", "gem-trip"])
        order, _ = surface.layout(
            [row("Y:/projects-software/amarantha")], self.by_path
        )
        self.assertEqual(order[:2], ["laser-ledger", "gem-trip"])
        self.assertIn("amarantha", order)

    def test_unpinned_blocked_project_sorts_ahead_of_other_extras(self):
        surface = sd.Surface(FakeDeck(), pinned=[])
        order, _ = surface.layout(
            [
                row("Y:/projects-software/amarantha"),
                row("Y:/projects-software/gem-trip", attention=True),
            ],
            self.by_path,
        )
        self.assertEqual(order[0], "gem-trip")

    def test_order_is_capped_at_the_key_count(self):
        surface = sd.Surface(FakeDeck(keys=2), pinned=["a", "b", "c", "d"])
        order, _ = surface.layout([], self.by_path)
        self.assertEqual(len(order), 2)

    def test_blocked_session_wins_when_two_share_a_project(self):
        surface = sd.Surface(FakeDeck(), pinned=[])
        _, live = surface.layout(
            [
                row("Y:/projects-software/gem-trip", sessionId="calm"),
                row("Y:/projects-software/gem-trip", sessionId="stuck", attention=True),
            ],
            self.by_path,
        )
        self.assertTrue(live["gem-trip"]["attention"])

    def test_sessions_outside_the_catalogue_are_ignored(self):
        surface = sd.Surface(FakeDeck(), pinned=[])
        order, live = surface.layout([row("C:/somewhere/else")], self.by_path)
        self.assertEqual((order, live), ([], {}))

    def test_a_press_on_an_empty_slot_does_nothing(self):
        surface = sd.Surface(FakeDeck(), pinned=[])
        surface.slots = []
        surface.on_press(None, 7, True)  # must not raise or call the API


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
        order, _ = s.layout([], {})
        self.assertEqual(len(order), 12)


if __name__ == "__main__":
    if sd is None:
        print("skipped:", REASON)
    unittest.main(verbosity=2)
