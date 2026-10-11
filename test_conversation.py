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


class InboxPrompts(unittest.TestCase):
    """What was typed into the window has to show in the conversation.

    Claude Code records an inbox prompt as a meta message from a peer, wraps
    it in a notice for the model, and the SDK's public reader drops it.
    """

    WRAPPED = ("Another Claude session sent a message:\nallow the install.\n\n"
               "This came from another Claude session — not typed by your "
               "user, but very likely working on their behalf.")

    def peer(self, content, **extra):
        entry = {"type": "user", "isMeta": True, "uuid": "u1",
                 "origin": {"kind": "peer", "from": "unknown"},
                 "message": {"role": "user", "content": content}}
        entry.update(extra)
        return entry

    def test_the_wrapper_is_removed(self):
        self.assertEqual(conv.unwrap_peer(self.WRAPPED), "allow the install.")

    def test_the_dispatchers_origin_note_is_removed_too(self):
        wrapped = self.WRAPPED.replace("allow the install.",
                                       conv.with_origin("allow the install."))
        self.assertEqual(conv.unwrap_peer(wrapped), "allow the install.")
        # Mid-turn, the prompt arrives bare in a queued_command attachment.
        self.assertEqual(conv.unwrap_peer(conv.with_origin("go")), "go")

    def test_origin_note_goes_on_once_and_comes_off(self):
        once = conv.with_origin("hello")
        self.assertEqual(conv.with_origin(once), once)
        self.assertEqual(conv.strip_origin(once), "hello")
        self.assertEqual(conv.strip_origin("plain"), "plain")

    def test_unwrapped_text_is_left_alone(self):
        self.assertEqual(conv.unwrap_peer("line one\nline two"),
                         "line one\nline two")

    def test_only_peer_prompts_are_recovered(self):
        self.assertTrue(conv._is_peer_prompt(self.peer("x")))
        self.assertFalse(conv._is_peer_prompt(self.peer("x", origin=None)))
        self.assertFalse(conv._is_peer_prompt(self.peer("x", isMeta=False)))
        self.assertFalse(conv._is_peer_prompt(self.peer("x", isSidechain=True)))

    def test_a_sent_prompt_is_the_person_and_says_how_it_came(self):
        items = conv.to_items([conv._Sent(self.peer(self.WRAPPED)),
                               assistant(TEXT)])
        self.assertEqual(items[0]["speaker"], conv.YOU)
        self.assertEqual(items[0]["via"], "inbox")
        self.assertEqual(items[0]["blocks"][0]["text"], "allow the install.")
        self.assertNotIn("via", items[1])

    def test_a_sent_prompt_does_not_merge_into_a_typed_one(self):
        typed = user({"type": "text", "text": "typed here"})
        items = conv.to_items([typed, conv._Sent(self.peer("sent"))])
        self.assertEqual(len(items), 2)

    def test_the_default_keeps_every_message(self):
        msgs = [assistant(TEXT), user({"type": "text", "text": "q"})] * 100
        self.assertEqual(len(conv.to_items(msgs)), 200)


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


class SystemMessages(unittest.TestCase):
    """Claude Code's own user-role messages are not the person talking."""
    NOTE = ("<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n"
            "<summary>Background command \"sleep\" completed (exit code 0)</summary>\n"
            "</task-notification>")

    def text(self, t):
        return user({"type": "text", "text": t})

    def test_task_notification_is_not_you(self):
        [item] = conv.to_items([self.text(self.NOTE)])
        self.assertEqual(item["speaker"], conv.SYSTEM)
        self.assertEqual(item["note"], conv.NOTE_TASK)
        self.assertEqual(item["blocks"][0]["text"],
                         'Background command "sleep" completed (exit code 0)')

    def test_command_output_loses_its_wrapper(self):
        [item] = conv.to_items([self.text(
            "<local-command-stdout>Set model to Opus</local-command-stdout>")])
        self.assertEqual((item["speaker"], item["note"]), (conv.SYSTEM, conv.NOTE_COMMAND))
        self.assertEqual(item["blocks"][0]["text"], "Set model to Opus")

    def test_metadata_tag_wins_without_a_prefix(self):
        msg = self.text("This session is being continued from a previous conversation")
        msg.system = conv.NOTE_SUMMARY
        [item] = conv.to_items([msg])
        self.assertEqual((item["speaker"], item["note"]), (conv.SYSTEM, conv.NOTE_SUMMARY))

    def test_a_typed_slash_command_is_still_you(self):
        [item] = conv.to_items([self.text("<command-name>/clear</command-name>")])
        self.assertEqual(item["speaker"], conv.YOU)

    def test_a_prompt_after_a_notification_keeps_its_own_turn(self):
        items = conv.to_items([self.text(self.NOTE), self.text("now fix it")])
        self.assertEqual([i["speaker"] for i in items], [conv.SYSTEM, conv.YOU])

    def test_entry_origin_maps_to_a_note(self):
        self.assertEqual(conv._system_origin({"origin": {"kind": "task-notification"}}),
                         conv.NOTE_TASK)
        self.assertEqual(conv._system_origin({"isCompactSummary": True}), conv.NOTE_SUMMARY)
        self.assertIsNone(conv._system_origin({"origin": {"kind": "human"}}))


class Compaction(unittest.TestCase):
    """What a compaction cut off is put back in front, once."""

    def e(self, uuid, parent, **kw):
        return dict({"uuid": uuid, "parentUuid": parent, "type": "user"}, **kw)

    def test_earlier_chain_is_prepended(self):
        entries = [self.e("a", None), self.e("b", "a"),
                   self.e("k", None, type="system", subtype="compact_boundary",
                          logicalParentUuid="b"),
                   self.e("s", "k", isCompactSummary=True), self.e("c", "s")]
        chain = [entries[2], entries[3], entries[4]]
        got = [x["uuid"] for x in conv.with_compacted(entries, chain)]
        self.assertEqual(got, ["a", "b", "k", "s", "c"])

    def test_two_compactions(self):
        entries = [self.e("a", None),
                   self.e("k1", None, type="system", subtype="compact_boundary",
                          logicalParentUuid="a"),
                   self.e("b", "k1"),
                   self.e("k2", None, type="system", subtype="compact_boundary",
                          logicalParentUuid="b"),
                   self.e("c", "k2")]
        got = [x["uuid"] for x in conv.with_compacted(entries, entries[3:])]
        self.assertEqual(got, ["a", "k1", "b", "k2", "c"])

    def test_missing_link_leaves_chain_alone(self):
        entries = [self.e("k", None, type="system", subtype="compact_boundary",
                          logicalParentUuid="gone"), self.e("c", "k")]
        self.assertEqual(conv.with_compacted(entries, entries), entries)

    def test_no_compaction_is_untouched(self):
        entries = [self.e("a", None), self.e("b", "a")]
        self.assertEqual(conv.with_compacted(entries, entries), entries)


class QueuedPrompts(unittest.TestCase):
    """A prompt that lands mid-turn is an attachment, and still shows."""

    def att(self, prompt, origin=None, mode="prompt"):
        a = {"type": "queued_command", "prompt": prompt, "commandMode": mode}
        if origin is not None:
            a["origin"] = origin
        return {"type": "attachment", "uuid": "q1", "attachment": a}

    def test_window_prompt_while_busy_is_you_via_inbox(self):
        entry = self.att("fix the tests", {"kind": "peer", "from": "unknown"})
        self.assertTrue(conv._is_queued_prompt(entry))
        [item] = conv.to_items([conv._Queued(entry)])
        self.assertEqual((item["speaker"], item.get("via")), (conv.YOU, "inbox"))
        self.assertEqual(item["blocks"][0]["text"], "fix the tests")

    def test_task_notification_mid_turn_is_a_note(self):
        entry = self.att(SystemMessages.NOTE, {"kind": "task-notification"},
                         mode="task-notification")
        [item] = conv.to_items([conv._Queued(entry)])
        self.assertEqual((item["speaker"], item["note"]), (conv.SYSTEM, conv.NOTE_TASK))
        self.assertNotIn("via", item)

    def test_subagent_hand_back_is_not_you(self):
        entry = self.att("[Subagent hand-back] report",
                         {"kind": "peer", "from": "a1", "senderTaskId": "a1"})
        [item] = conv.to_items([conv._Queued(entry)])
        self.assertEqual((item["speaker"], item["note"]), (conv.SYSTEM, conv.NOTE_SUBAGENT))

    def test_other_attachments_are_ignored(self):
        self.assertFalse(conv._is_queued_prompt({"type": "attachment",
                                                 "attachment": {"type": "file"}}))


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
