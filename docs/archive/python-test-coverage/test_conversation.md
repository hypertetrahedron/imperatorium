# test_conversation.py

Summary: 53 tests - 50 COVERED, 3 MOOT

Also replayed whole-corpus against Python by tests/corpus.rs (golden items_compare); the rows below are the specific behaviours test_conversation.py named.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Shapes.test_text_survives | COVERED | src/conversation.rs `conversation::py_ported::text_survives` |
| Shapes.test_a_tool_call_is_summarised_to_one_line | COVERED | src/conversation.rs `conversation::py_ported::a_tool_call_is_summarised_to_one_line` |
| Shapes.test_tool_results_carry_their_error_flag | COVERED | src/conversation.rs `conversation::py_ported::tool_results_carry_their_error_flag` |
| Shapes.test_long_results_are_cut_and_say_so | COVERED | src/conversation.rs `conversation::py_ported::long_results_are_cut_and_say_so` |
| Shapes.test_thinking_is_kept_but_marked | COVERED | src/conversation.rs `conversation::py_ported::thinking_is_kept_but_marked` |
| Shapes.test_a_result_given_as_blocks_is_flattened | COVERED | src/conversation.rs `conversation::py_ported::a_result_given_as_blocks_is_flattened` |
| Shapes.test_an_image_result_is_named_not_dumped | COVERED | src/conversation.rs `conversation::py_ported::an_image_result_is_named_not_dumped` |
| Shapes.test_string_content_is_accepted | COVERED | src/conversation.rs `conversation::py_ported::string_content_is_accepted` |
| Questions.test_ask_user_question_carries_its_choices | COVERED | src/conversation.rs `conversation::py_ported::ask_user_question_carries_its_choices` |
| Questions.test_other_tools_do_not | COVERED | src/conversation.rs `conversation::py_ported::other_tools_do_not_carry_questions` |
| Filtering.test_subagent_messages_are_hidden_by_default | COVERED | src/conversation.rs `conversation::py_ported::subagent_messages_are_hidden` (the binary drops isSidechain entries in `messages`; the SDK's parent_tool_use_id / include_subagents switch has no counterpart) |
| Filtering.test_empty_turns_are_dropped | COVERED | src/conversation.rs `conversation::py_ported::empty_turns_are_dropped` |
| Filtering.test_unknown_block_types_are_skipped_not_fatal | COVERED | src/conversation.rs `conversation::py_ported::unknown_block_types_are_skipped_not_fatal` |
| Filtering.test_malformed_entries_do_not_raise | COVERED | src/conversation.rs `conversation::py_ported::malformed_entries_do_not_raise` |
| Filtering.test_limit_keeps_the_most_recent | COVERED | src/conversation.rs `conversation::py_ported::limit_keeps_the_most_recent` |
| Filtering.test_no_limit_returns_everything | COVERED | src/conversation.rs `conversation::py_ported::no_limit_returns_everything` |
| InboxPrompts.test_the_wrapper_is_removed | COVERED | src/conversation.rs `conversation::py_ported::the_wrapper_is_removed` |
| InboxPrompts.test_the_dispatchers_origin_note_is_removed_too | COVERED | src/conversation.rs `conversation::py_ported::the_dispatchers_origin_note_is_removed_too` |
| InboxPrompts.test_origin_note_goes_on_once_and_comes_off | COVERED | src/conversation.rs `conversation::py_ported::origin_note_goes_on_once_and_comes_off` |
| InboxPrompts.test_unwrapped_text_is_left_alone | COVERED | src/conversation.rs `conversation::py_ported::unwrapped_text_is_left_alone` |
| InboxPrompts.test_only_peer_prompts_are_recovered | COVERED | src/conversation.rs `conversation::py_ported::only_peer_prompts_are_recovered` |
| InboxPrompts.test_a_sent_prompt_is_the_person_and_says_how_it_came | COVERED | src/conversation.rs `conversation::py_ported::a_sent_prompt_is_the_person_and_says_how_it_came` |
| InboxPrompts.test_a_sent_prompt_does_not_merge_into_a_typed_one | COVERED | src/conversation.rs `conversation::py_ported::a_sent_prompt_does_not_merge_into_a_typed_one` |
| InboxPrompts.test_the_default_keeps_every_message | COVERED | src/conversation.rs `conversation::py_ported::the_default_keeps_every_message` |
| Attribution.test_a_tool_result_is_not_the_person | COVERED | src/conversation.rs `conversation::py_ported::a_tool_result_is_not_the_person` |
| Attribution.test_typed_text_is_the_person | COVERED | src/conversation.rs `conversation::py_ported::typed_text_is_the_person` |
| Attribution.test_assistant_is_claude | COVERED | src/conversation.rs `conversation::py_ported::assistant_is_claude` |
| Attribution.test_a_result_joins_the_call_that_produced_it | COVERED | src/conversation.rs `conversation::py_ported::a_result_joins_the_call_that_produced_it` |
| Attribution.test_one_turn_is_one_item | COVERED | src/conversation.rs `conversation::py_ported::one_turn_is_one_item` |
| Attribution.test_the_person_still_gets_their_own_turn | COVERED | src/conversation.rs `conversation::py_ported::the_person_still_gets_their_own_turn` |
| Attribution.test_an_orphan_result_keeps_its_own_header | COVERED | src/conversation.rs `conversation::py_ported::an_orphan_result_keeps_its_own_header` |
| SystemMessages.test_task_notification_is_not_you | COVERED | src/conversation.rs `conversation::py_ported::task_notification_is_not_you` |
| SystemMessages.test_command_output_loses_its_wrapper | COVERED | src/conversation.rs `conversation::py_ported::command_output_loses_its_wrapper` |
| SystemMessages.test_metadata_tag_wins_without_a_prefix | COVERED | src/conversation.rs `conversation::py_ported::metadata_tag_wins_without_a_prefix` |
| SystemMessages.test_a_typed_slash_command_is_still_you | COVERED | src/conversation.rs `conversation::py_ported::a_typed_slash_command_is_still_you` |
| SystemMessages.test_a_prompt_after_a_notification_keeps_its_own_turn | COVERED | src/conversation.rs `conversation::py_ported::a_prompt_after_a_notification_keeps_its_own_turn` |
| SystemMessages.test_entry_origin_maps_to_a_note | COVERED | src/conversation.rs `conversation::py_ported::entry_origin_maps_to_a_note` |
| Compaction.test_earlier_chain_is_prepended | COVERED | src/conversation.rs `conversation::py_ported::earlier_chain_is_prepended` (calls `sessions::with_compacted`; sessions.rs `compactions_are_put_back` covers the one-boundary case too) |
| Compaction.test_two_compactions | COVERED | src/conversation.rs `conversation::py_ported::two_compactions` (calls `sessions::with_compacted`) |
| Compaction.test_missing_link_leaves_chain_alone | COVERED | src/conversation.rs `conversation::py_ported::missing_link_leaves_chain_alone` (calls `sessions::with_compacted`) |
| Compaction.test_no_compaction_is_untouched | COVERED | src/conversation.rs `conversation::py_ported::no_compaction_is_untouched` (calls `sessions::with_compacted`) |
| QueuedPrompts.test_window_prompt_while_busy_is_you_via_inbox | COVERED | src/conversation.rs `conversation::py_ported::window_prompt_while_busy_is_you_via_inbox` |
| QueuedPrompts.test_task_notification_mid_turn_is_a_note | COVERED | src/conversation.rs `conversation::py_ported::task_notification_mid_turn_is_a_note` |
| QueuedPrompts.test_subagent_hand_back_is_not_you | COVERED | src/conversation.rs `conversation::py_ported::subagent_hand_back_is_not_you` |
| QueuedPrompts.test_other_attachments_are_ignored | COVERED | src/conversation.rs `conversation::py_ported::other_attachments_are_ignored` |
| Preview.test_the_last_thing_claude_said | MOOT | not in binary: `last_assistant_text` has no Rust counterpart and nothing in Python called it (grep: defined in conversation.py, used only by this test) |
| Preview.test_tool_only_conversation_has_no_preview | MOOT | not in binary: `last_assistant_text` has no Rust counterpart and nothing in Python called it (grep: defined in conversation.py, used only by this test) |
| Preview.test_a_users_words_are_not_mistaken_for_claudes | MOOT | not in binary: `last_assistant_text` has no Rust counterpart and nothing in Python called it (grep: defined in conversation.py, used only by this test) |
| Summaries.test_the_most_useful_field_is_chosen | COVERED | src/conversation.rs `conversation::py_ported::the_most_useful_field_is_chosen` |
| Summaries.test_an_unrecognised_input_shows_keys_not_a_json_wall | COVERED | src/conversation.rs `conversation::py_ported::an_unrecognised_input_shows_keys_not_a_json_wall` |
| Summaries.test_summaries_are_bounded | COVERED | src/conversation.rs `conversation::py_ported::summaries_are_bounded` |
| EditDiff.test_edit_and_multiedit | COVERED | src/conversation.rs `conversation::py_ported::edit_and_multiedit` |
| EditDiff.test_cut_short | COVERED | src/conversation.rs `conversation::py_ported::edit_diff_is_cut_short` (also `long_diffs_are_cut_on_a_line`) |
