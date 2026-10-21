"""Slash commands typed at a session's prompt: what may be typed, and when."""
import unittest
from unittest import mock

from imperatorium import closing, slash

SID = "11111111-1111-1111-1111-111111111111"


def row(**kw):
    out = {"sessionId": SID, "pid": 4242, "status": "idle"}
    out.update(kw)
    return out


class Parse(unittest.TestCase):
    def test_a_command_and_its_arguments(self):
        self.assertEqual(slash.parse("/model opus"), ("model", "opus"))
        self.assertEqual(slash.parse("  /Compact  keep the API notes "),
                         ("compact", "keep the API notes"))

    def test_text_that_only_mentions_a_command_is_a_prompt(self):
        for text in ("run /compact please", "", "/", "/ compact", "//comment",
                     None):
            self.assertIsNone(slash.parse(text), text)


class Refusal(unittest.TestCase):
    def test_ordinary_commands_are_typed(self):
        for text in ("/compact", "/clear", "/context", "/cost",
                     "/model opus", "/resume abc123", "/code-review high"):
            self.assertIsNone(slash.refusal(text), text)

    def test_a_menu_the_window_cannot_drive_is_refused(self):
        for text in ("/config", "/permissions", "/mcp", "/model", "/rewind"):
            self.assertIn("opens a menu", slash.refusal(text), text)

    def test_a_bare_menu_command_is_told_to_take_an_argument(self):
        self.assertIn("/model <name>", slash.refusal("/model"))

    def test_exit_points_at_close(self):
        self.assertIn("Close", slash.refusal("/exit"))

    def test_a_second_line_would_submit_the_first(self):
        self.assertIn("one line", slash.refusal("/compact\nthen delete it all"))


@mock.patch.object(closing, "WINDOWS", True)
class Blocked(unittest.TestCase):
    def test_an_idle_local_session_takes_it(self):
        self.assertIsNone(slash.blocked(row()))

    def test_never_into_a_turn_or_a_prompt_waiting_on_you(self):
        for status in ("busy", "shell", "waiting", None):
            self.assertIsNotNone(slash.blocked(row(status=status)), status)

    def test_never_while_a_permission_decision_is_parked_for_it(self):
        # Enter would answer the dialog the hook is holding open.
        self.assertIn("permission", slash.blocked(row(), [{"session_id": SID}]))
        self.assertIsNone(slash.blocked(row(), [{"session_id": "other"}]))

    def test_a_remote_session_has_no_console_here(self):
        self.assertIn("pi", slash.blocked(row(remote=True, host="pi")))

    def test_a_session_without_a_process(self):
        self.assertIsNotNone(slash.blocked(row(pid=None)))

    def test_windows_only(self):
        with mock.patch.object(closing, "WINDOWS", False):
            self.assertIn("Windows", slash.blocked(row()))


class Typing(unittest.TestCase):
    def test_clears_the_draft_then_types_the_line_and_enter(self):
        typed = []
        ok = slash.type_command(4242, "  /compact  ",
                                typer=lambda pid, *c: typed.append((pid, c)) or True)
        self.assertTrue(ok)
        self.assertEqual(typed, [(4242, (closing.CTRL_C, "/compact", closing.ENTER))])


class CleanTyped(unittest.TestCase):
    def test_lines_become_one(self):
        self.assertEqual(slash.clean_typed("spend up to\r\n$40\n\n\tthen stop"),
                         ("spend up to $40 then stop", True))

    def test_plain_text_is_unchanged(self):
        self.assertEqual(slash.clean_typed("  i approve  "), ("i approve", False))

    def test_control_and_invisible_characters_go(self):
        clean, changed = slash.clean_typed("ok\x1b[31m​ go\x03")
        self.assertEqual(clean, "ok[31m go")
        self.assertTrue(changed)

    def test_emoji_stay(self):
        self.assertEqual(slash.clean_typed("ship it \U0001f680")[0], "ship it \U0001f680")

    def test_a_leading_shell_or_memory_key_is_dropped(self):
        self.assertEqual(slash.clean_typed("!rm -rf build")[0], "rm -rf build")
        self.assertEqual(slash.clean_typed("# Heading\ntext")[0], "Heading text")

    def test_a_command_is_kept_as_a_command(self):
        self.assertEqual(slash.clean_typed("/compact\nkeep notes")[0], "/compact keep notes")


class TypedRefusal(unittest.TestCase):
    def test_empty_and_too_long(self):
        self.assertIn("nothing", slash.typed_refusal(""))
        self.assertIn("too long", slash.typed_refusal("x" * (slash.TYPED_MAX + 1)))
        self.assertIsNone(slash.typed_refusal("x" * slash.TYPED_MAX))

    def test_a_command_gets_the_command_rules(self):
        self.assertIn("menu", slash.typed_refusal("/config"))


if __name__ == "__main__":
    unittest.main()
