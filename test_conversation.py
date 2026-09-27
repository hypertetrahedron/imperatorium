"""Turning session messages into something a window can render.

`to_items` is pure, so this needs no SDK, no session and no model. The live
path was verified separately against a session running in another process.

Run: python test_conversation.py
"""
import unittest

from ccontrol import conversation as conv


class Msg:
    """Stands in for the SDK's SessionMessage."""

    def __init__(self, message, parent_tool_use_id=None, parent_agent_id=None,
                 type="assistant"):
        self.message = message
        self.parent_tool_use_id = parent_tool_use_id
        self.parent_agent_id = parent_agent_id
        self.type = type


def assistant(*blocks):
    return Msg({"role": "assistant", "content": list(blocks)})


def user(*blocks):
    return Msg({"role": "user", "content": list(blocks)}, type="user")


TEXT = {"type": "text", "text": "Done. Tests pass."}
TOOL = {"type": "tool_use", "name": "Bash", "id": "t1",
        "input": {"command": "pytest -x"}}
RESULT = {"type": "tool_result", "tool_use_id": "t1", "content": "41 passed"}


class Shapes(unittest.TestCase):
    def test_text_survives(self):
        items = conv.to_items([assistant(TEXT)])
        self.assertEqual(items[0]["role"], "assistant")
        self.assertEqual(items[0]["blocks"][0], {"kind": "text",
                                                 "text": "Done. Tests pass."})

    def test_a_tool_call_is_summarised_to_one_line(self):
        block = conv.to_items([assistant(TOOL)])[0]["blocks"][0]
        self.assertEqual(block["kind"], "tool")
        self.assertEqual(block["name"], "Bash")
        self.assertEqual(block["summary"], "pytest -x")

    def test_tool_results_carry_their_error_flag(self):
        bad = dict(RESULT, is_error=True)
        block = conv.to_items([user(bad)])[0]["blocks"][0]
        self.assertTrue(block["is_error"])
        self.assertFalse(block["truncated"])

    def test_long_results_are_cut_and_say_so(self):
        big = {"type": "tool_result", "content": "x" * (conv.RESULT_CHARS + 500)}
        block = conv.to_items([user(big)])[0]["blocks"][0]
        self.assertTrue(block["truncated"])
        self.assertEqual(len(block["text"]), conv.RESULT_CHARS)

    def test_thinking_is_kept_but_marked(self):
        block = conv.to_items([assistant({"type": "thinking",
                                          "thinking": "hmm"})])[0]["blocks"][0]
        self.assertEqual(block["kind"], "thinking")

    def test_a_result_given_as_blocks_is_flattened(self):
        block = conv.to_items([user({"type": "tool_result", "content": [
            {"type": "text", "text": "one"}, {"type": "text", "text": "two"},
        ]})])[0]["blocks"][0]
        self.assertEqual(block["text"], "one\ntwo")

    def test_an_image_result_is_named_not_dumped(self):
        block = conv.to_items([user({"type": "tool_result", "content": [
            {"type": "image", "source": {"data": "AAAA" * 5000}},
        ]})])[0]["blocks"][0]
        self.assertEqual(block["text"], "[image]")

    def test_string_content_is_accepted(self):
        items = conv.to_items([Msg({"role": "assistant", "content": "plain"})])
        self.assertEqual(items[0]["blocks"][0]["text"], "plain")


class Filtering(unittest.TestCase):
    def test_subagent_messages_are_hidden_by_default(self):
        # A subagent's chatter is not what you are deciding about.
        msgs = [assistant(TEXT), Msg({"role": "assistant", "content": [TEXT]},
                                     parent_tool_use_id="t9")]
        # Counted in blocks, not items: same speaker either way, so the two
        # turns merge and only the content tells them apart.
        self.assertEqual(len(conv.to_items(msgs)[0]["blocks"]), 1)
        self.assertEqual(
            len(conv.to_items(msgs, include_subagents=True)[0]["blocks"]), 2)

    def test_empty_turns_are_dropped(self):
        # A turn whose only block was empty text would render as a blank row.
        msgs = [assistant({"type": "text", "text": "   "}), assistant(TEXT)]
        self.assertEqual(len(conv.to_items(msgs)), 1)

    def test_unknown_block_types_are_skipped_not_fatal(self):
        items = conv.to_items([assistant({"type": "future_block"}, TEXT)])
        self.assertEqual([b["kind"] for b in items[0]["blocks"]], ["text"])

    def test_malformed_entries_do_not_raise(self):
        msgs = [Msg(None), Msg({"role": "assistant"}), Msg({"content": "x"}),
                assistant(TEXT)]
        items = conv.to_items(msgs)
        self.assertEqual([b["text"] for b in items[0]["blocks"]],
                         ["x", TEXT["text"]])

    def test_limit_keeps_the_most_recent(self):
        msgs = [assistant({"type": "text", "text": str(i)}) for i in range(10)]
        items = conv.to_items(msgs, limit=3)
        # The limit still caps what is read off the wire; the three survivors
        # are one speaker's turn, so they render under one name.
        self.assertEqual(len(items), 1)
        self.assertEqual([b["text"] for b in items[0]["blocks"]], ["7", "8", "9"])

    def test_no_limit_returns_everything(self):
        msgs = [assistant(TEXT)] * 5
        self.assertEqual(len(conv.to_items(msgs, limit=0)[0]["blocks"]), 5)


class Attribution(unittest.TestCase):
    """The bug this fixes: the window called Claude's own output "USER".

    Every tool result travels in a message whose role is "user", because that
    is how a result is handed back to the model. Taking that role at face
    value mislabelled almost every turn in a tool-heavy session.
    """

    def test_a_tool_result_is_not_the_person(self):
        items = conv.to_items([user({"type": "tool_result", "content": "out"})])
        self.assertEqual(items[0]["speaker"], conv.TOOLS)
        self.assertEqual(items[0]["role"], "user", "the wire role is preserved")

    def test_typed_text_is_the_person(self):
        items = conv.to_items([Msg({"role": "user", "content": "do the thing"})])
        self.assertEqual(items[0]["speaker"], conv.YOU)

    def test_assistant_is_claude(self):
        self.assertEqual(conv.to_items([assistant(TEXT)])[0]["speaker"], conv.CLAUDE)

    def test_a_result_joins_the_call_that_produced_it(self):
        items = conv.to_items([assistant(TOOL),
                               user({"type": "tool_result", "content": "out"})])
        self.assertEqual(len(items), 1)
        self.assertEqual(items[0]["speaker"], conv.CLAUDE)
        self.assertEqual([b["kind"] for b in items[0]["blocks"]], ["tool", "result"])

    def test_one_turn_is_one_item(self):
        # How a real turn arrives: thinking, prose and each call separately.
        items = conv.to_items([
            assistant({"type": "thinking", "thinking": "hm"}),
            assistant(TEXT),
            assistant(TOOL),
            user({"type": "tool_result", "content": "out"}),
        ])
        self.assertEqual(len(items), 1)
        self.assertEqual(items[0]["speaker"], conv.CLAUDE)

    def test_the_person_still_gets_their_own_turn(self):
        items = conv.to_items([Msg({"role": "user", "content": "go"}),
                               assistant(TEXT)])
        self.assertEqual([i["speaker"] for i in items], [conv.YOU, conv.CLAUDE])

    def test_an_orphan_result_keeps_its_own_header(self):
        # The limit can cut a turn in half; the leftover must not be filed
        # under whoever happens to be above it.
        items = conv.to_items([Msg({"role": "user", "content": "go"}),
                               user({"type": "tool_result", "content": "out"})])
        self.assertEqual([i["speaker"] for i in items], [conv.YOU, conv.TOOLS])


class Preview(unittest.TestCase):
    def test_the_last_thing_claude_said(self):
        msgs = [assistant({"type": "text", "text": "first"}),
                assistant(TOOL),
                assistant({"type": "text", "text": "last"}),
                user(RESULT)]
        self.assertEqual(conv.last_assistant_text(conv.to_items(msgs)), "last")

    def test_tool_only_conversation_has_no_preview(self):
        self.assertEqual(conv.last_assistant_text(conv.to_items([assistant(TOOL)])), "")

    def test_a_users_words_are_not_mistaken_for_claudes(self):
        msgs = [assistant({"type": "text", "text": "mine"}),
                user({"type": "text", "text": "theirs"})]
        self.assertEqual(conv.last_assistant_text(conv.to_items(msgs)), "mine")


class Summaries(unittest.TestCase):
    def test_the_most_useful_field_is_chosen(self):
        for tool_input, expect in (
            ({"command": "ls"}, "ls"),
            ({"file_path": "a.py"}, "a.py"),
            ({"pattern": "TODO"}, "TODO"),
            ({"url": "https://x"}, "https://x"),
        ):
            block = conv.to_items([assistant(
                {"type": "tool_use", "name": "T", "input": tool_input}
            )])[0]["blocks"][0]
            self.assertEqual(block["summary"], expect)

    def test_an_unrecognised_input_shows_keys_not_a_json_wall(self):
        block = conv.to_items([assistant(
            {"type": "tool_use", "name": "T", "input": {"zeta": 1, "alpha": 2}}
        )])[0]["blocks"][0]
        self.assertEqual(block["summary"], "alpha, zeta")

    def test_summaries_are_bounded(self):
        block = conv.to_items([assistant(
            {"type": "tool_use", "name": "T", "input": {"command": "x" * 5000}}
        )])[0]["blocks"][0]
        self.assertLessEqual(len(block["summary"]), conv.INPUT_CHARS)


if __name__ == "__main__":
    unittest.main(verbosity=2)
