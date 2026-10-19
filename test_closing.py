"""Ending a session: asked to exit first, killed only if it stays."""
import unittest
from unittest import mock

from ccontrol import closing

CLAUDE, SHELL, WT = 100, 50, 10


class Fake:
    """Processes that leave when the right thing is typed into them."""

    def __init__(self, live, leaves_on=None):
        self.live = set(live)
        self.leaves_on = leaves_on or {}
        self.typed, self.killed = [], []

    def alive(self, pid):
        return pid in self.live

    def typer(self, pid, *chunks):
        self.typed.append((pid, chunks))
        if self.leaves_on.get(pid) in chunks:
            self.live.discard(pid)
        return True

    def killer(self, pid):
        self.killed.append(pid)
        self.live.discard(pid)


def table(parent_exe="windowsterminal", shell_exe="pwsh"):
    return [(WT, 1, parent_exe, None), (SHELL, WT, shell_exe, None),
            (CLAUDE, SHELL, "claude", None)]


@mock.patch.object(closing, "WINDOWS", True)
class Close(unittest.TestCase):
    def run_close(self, fake, tbl=None):
        return closing.close(CLAUDE, wait=0.3, typer=fake.typer, killer=fake.killer,
                             alive=fake.alive, table=tbl if tbl is not None else [])

    def test_exit_is_typed_and_the_tab_closes(self):
        fake = Fake({CLAUDE}, {CLAUDE: "/exit"})
        got = self.run_close(fake)
        self.assertEqual(got, {"ok": True, "how": "exit", "tab_closed": True})
        self.assertEqual(fake.typed, [(CLAUDE, (closing.CTRL_C, "/exit", closing.ENTER))])
        self.assertEqual(fake.killed, [])

    def test_a_tab_shell_is_sent_exit_after_claude_leaves(self):
        fake = Fake({CLAUDE, SHELL}, {CLAUDE: "/exit", SHELL: "exit"})
        got = self.run_close(fake, table())
        self.assertTrue(got["tab_closed"])
        self.assertEqual(fake.typed[1], (SHELL, ("exit", closing.ENTER)))

    def test_a_shell_under_something_else_is_left_alone(self):
        fake = Fake({CLAUDE, SHELL}, {CLAUDE: "/exit"})
        self.run_close(fake, table(parent_exe="code"))
        self.assertEqual([pid for pid, _ in fake.typed], [CLAUDE])
        self.assertIn(SHELL, fake.live)

    def test_one_that_will_not_exit_is_killed(self):
        fake = Fake({CLAUDE})
        got = self.run_close(fake)
        self.assertEqual(got, {"ok": True, "how": "killed", "tab_closed": False})
        self.assertEqual(fake.killed, [CLAUDE])

    def test_one_already_gone_is_not_touched(self):
        fake = Fake(set())
        self.assertEqual(self.run_close(fake)["how"], "gone")
        self.assertEqual((fake.typed, fake.killed), ([], []))

    def test_no_pid_is_an_error(self):
        self.assertFalse(closing.close(None)["ok"])


class TabShell(unittest.TestCase):
    def test_shell_directly_under_the_terminal(self):
        self.assertEqual(closing.tab_shell(CLAUDE, table()), SHELL)

    def test_started_by_the_terminal_itself(self):
        tbl = [(WT, 1, "windowsterminal", None), (CLAUDE, WT, "claude", None)]
        self.assertIsNone(closing.tab_shell(CLAUDE, tbl))

    def test_not_a_shell_parent(self):
        self.assertIsNone(closing.tab_shell(CLAUDE, table(shell_exe="node")))


class Keys(unittest.TestCase):
    def test_control_keys_round_trip_by_name(self):
        names = {v: k for k, v in closing.KEYS.items()}
        for key in (closing.CTRL_C, closing.ENTER):
            self.assertEqual(names[closing.KEYS[key]], key)
            self.assertTrue(closing.KEYS[key].isprintable())


if __name__ == "__main__":
    unittest.main()
