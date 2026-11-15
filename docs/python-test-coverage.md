# Python test coverage map

Every test in the Python suite that is going away, and where its behaviour is tested in Rust. Verdicts: COVERED (an existing Rust test, golden replay, or a twin replay on a sibling branch), NEW (was a PORT; now a new Rust test), MOOT (Python-only plumbing, or not in the binary), DIFFERS (binary behaves differently; see the row), PORT (still open: blocked, reason in the row). Out of scope: test_ui.py (ported to node by another agent), test_herdr*.py (dropped with herdr).

## Totals

| File | COVERED | NEW | MOOT | DIFFERS | PORT open | Total |
|---|---|---|---|---|---|---|
| test_closing.py | 10 | 0 | 0 | 0 | 0 | 10 |
| test_convcache.py | 6 | 0 | 0 | 2 | 0 | 8 |
| test_conversation.py | 50 | 0 | 3 | 0 | 0 | 53 |
| test_core.py | 100 | 0 | 0 | 0 | 0 | 100 |
| test_costs.py | 34 | 0 | 0 | 0 | 0 | 34 |
| test_dispatcher.py | 116 | 0 | 0 | 0 | 11 | 127 |
| test_features.py | 200 | 0 | 2 | 1 | 0 | 203 |
| test_finished.py | 11 | 0 | 0 | 0 | 0 | 11 |
| test_hotkey.py | 6 | 10 | 0 | 0 | 0 | 16 |
| test_impctl.py | 17 | 0 | 2 | 1 | 0 | 20 |
| test_matrix.py | 0 | 23 | 0 | 0 | 0 | 23 |
| test_modlink.py | 2 | 18 | 0 | 0 | 0 | 20 |
| test_native.py | 6 | 0 | 3 | 0 | 0 | 9 |
| test_policy.py | 44 | 0 | 0 | 0 | 0 | 44 |
| test_public.py | 4 | 0 | 0 | 0 | 0 | 4 |
| test_quick.py | 13 | 0 | 0 | 0 | 0 | 13 |
| test_remote.py | 4 | 53 | 5 | 0 | 0 | 62 |
| test_rename.py | 9 | 0 | 0 | 3 | 0 | 12 |
| test_sessionstat.py | 21 | 0 | 0 | 0 | 0 | 21 |
| test_settings.py | 23 | 0 | 0 | 0 | 0 | 23 |
| test_setup.py | 26 | 0 | 4 | 0 | 0 | 30 |
| test_slash.py | 21 | 0 | 0 | 0 | 1 | 22 |
| test_streamdeck.py | 33 | 0 | 0 | 0 | 0 | 33 |
| test_subproc.py | 5 | 0 | 3 | 3 | 0 | 11 |
| test_taskout.py | 16 | 0 | 0 | 0 | 0 | 16 |
| test_voice.py | 15 | 0 | 8 | 1 | 4 | 28 |
| **all** | 792 | 104 | 30 | 11 | 16 | 953 |

test_herdr.py, test_herdr_mods.py, test_herdrsetup.py: dropped with herdr. test_ui.py: ported to node elsewhere.

**Resolved since (2026-11-15):** of the 11 DIFFERS rows, 8 are fixed in the binary (conversation cache budget and single parse, dismissal on reappearance, uninstall asking first, run_timeout's job object x3, the CPU voice model) and 3 will not be (the pre-rename `ccontrol` state folder, `ccontrol-*` units and `CentralControl*` tasks: a clean break with the Python version). Each row says which. The counts above are as mapped.


## test_closing.py

10 tests (by name): COVERED 2, PORT-now-COVERED 8.

| Test | Verdict | Rust test or reason |
|---|---|---|
| `Close.test_exit_is_typed_and_the_tab_closes` | COVERED | new: dispatch_close.rs::exit_is_typed_and_the_tab_closes (fake console via POST /api/close; fakecli logs Ctrl+C then the line /exit) |
| `Close.test_a_tab_shell_is_sent_exit_after_claude_leaves` | COVERED | new: dispatch_close.rs::a_tab_shell_is_sent_exit_after_claude_leaves (real process tree: cmd.exe copies named windowsterminal / pwsh around a fake claude) |
| `Close.test_a_shell_under_something_else_is_left_alone` | COVERED | new: dispatch_close.rs::a_shell_under_something_else_is_left_alone (host copy named `code`) |
| `Close.test_one_that_will_not_exit_is_killed` | COVERED | new: dispatch_close.rs::one_that_will_not_exit_is_killed |
| `Close.test_one_already_gone_is_not_touched` | COVERED | new: dispatch_close.rs::one_already_gone_is_not_touched |
| `Close.test_no_pid_is_an_error` | COVERED | new: dispatch_close.rs::no_pid_is_an_error (via POST /api/close: a row without a process, an unknown session, no session) |
| `TabShell.test_shell_directly_under_the_terminal` | COVERED | closing.rs::tests::tab_shell_only_under_a_terminal (asserts Some(20) for claude under pwsh under windowsterminal) |
| `TabShell.test_started_by_the_terminal_itself` | COVERED | closing.rs::tests::tab_shell_only_under_a_terminal (its `direct` case asserts None) |
| `TabShell.test_not_a_shell_parent` | COVERED | new: closing.rs::tab_shell_more_tests::not_a_shell_parent |
| `Keys.test_control_keys_round_trip_by_name` | COVERED | new: dispatch_close.rs::control_keys_round_trip_by_name (Rust has no KEYS table; <ctrl-c>/<enter> are named inline and arrive as Ctrl+C and Enter in a fake console) |


## test_convcache.py

Summary: 8 tests - 0 COVERED already, 6 PORT now COVERED by new Rust tests, 0 MOOT, 2 DIFFERS

| Test | Verdict | Rust test or reason |
|---|---|---|
| CacheTests.test_an_unchanged_transcript_is_parsed_once | COVERED | new: tests/core_convcache.rs `an_unchanged_transcript_is_not_parsed_again (same-size rewrite with mtime restored still answers the old text)` |
| CacheTests.test_an_appended_transcript_is_parsed_again | COVERED | new: tests/core_convcache.rs `an_appended_transcript_is_parsed_again` |
| CacheTests.test_the_limit_applies_before_turns_merge | COVERED | new: tests/core_convcache.rs `the_limit_applies_before_turns_merge` |
| CacheTests.test_merging_does_not_change_the_cached_messages | COVERED | new: tests/core_convcache.rs `merging_does_not_change_the_cached_messages` |
| CacheTests.test_without_a_transcript_file_it_reads_through | COVERED | new: tests/core_convcache.rs `without_a_transcript_file_nothing_is_cached` |
| CacheTests.test_least_recently_used_sessions_are_dropped | COVERED | new: tests/core_convcache.rs `least_recently_used_sessions_are_dropped` |
| CacheTests.test_a_weight_budget_keeps_at_least_the_newest | DIFFERS | FIXED 2026-11-15 (`4920b38`): byte budget, setting chat.conversation_cache_mb; conv.rs `a_weight_budget_keeps_at_least_the_newest`, tests/conv_cache.rs. Was: the binary bounds the cache by count only (CONVS_KEPT = 32 in server/conv.rs); there is no byte-weight budget, so a huge transcript is kept like a small one. Nothing to test |
| CacheTests.test_concurrent_requests_share_one_parse | DIFFERS | FIXED 2026-11-15 (`4920b38`): per-session fill lock; tests/conv_cache.rs `polls_that_arrive_together_share_one_parse`. Was: the binary has no per-session fill lock: server/conv.rs `conversation` checks the cache, then parses outside any lock, so N simultaneous polls of one changed transcript parse N times. Not observable through the API (only the parse count would show it) |


## test_conversation.py

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


## test_core.py

Summary: 100 tests - 32 COVERED already, 68 PORT now COVERED by new Rust tests, 0 MOOT, 0 DIFFERS

| Test | Verdict | Rust test or reason |
|---|---|---|
| Utterances.test_prefix_forms | COVERED | pure_checks.rs `projects_check_answers_as_python_did` (golden projects_compare): the utterance cases (project:/projects/colon forms, case) |
| Utterances.test_no_prefix_is_all_prompt | COVERED | pure_checks.rs `projects_check_answers_as_python_did` (golden projects_compare): 'no prefix at all' |
| Utterances.test_prompt_may_contain_commas | COVERED | pure_checks.rs `projects_check_answers_as_python_did` (golden projects_compare): 'project gem trip, fix the build, then ship' |
| Utterances.test_multiline_prompt_survives | COVERED | pure_checks.rs `projects_check_answers_as_python_did` (golden projects_compare): 'project: a\nmulti, line\nprompt' |
| Matching.test_spoken_forms_of_real_projects | COVERED | new: projects::core_tests::spoken_forms_of_real_projects |
| Matching.test_case_and_separators_do_not_matter | COVERED | new: projects::core_tests::case_and_separators_do_not_matter |
| Matching.test_unknown_project_is_refused_not_guessed | COVERED | new: projects::core_tests::unknown_project_is_refused_not_guessed |
| Matching.test_near_tie_is_not_certain | COVERED | new: projects::core_tests::near_tie_is_not_certain |
| Matching.test_empty_input_matches_nothing | COVERED | new: projects::core_tests::empty_input_matches_nothing |
| Matching.test_candidates_are_ranked_and_bounded | COVERED | new: projects::core_tests::candidates_are_ranked_and_bounded |
| DeepLinks.test_prompt_is_encoded_not_dropped | COVERED | pure_checks.rs `projects_check_answers_as_python_did` (golden projects_compare): deeplink 'do the thing & more?' |
| DeepLinks.test_unc_paths_are_refused | COVERED | pure_checks.rs `projects_check_answers_as_python_did` (golden projects_compare): deeplink `\\server\share` |
| DeepLinks.test_overlong_prompt_is_refused | COVERED | pure_checks.rs `projects_check_answers_as_python_did` (golden projects_compare): deeplink with 5001 chars |
| DeepLinks.test_cwd_is_required | COVERED | pure_checks.rs `projects_check_answers_as_python_did` (golden projects_compare): deeplink with empty cwd |
| InboxFrames.test_auth_line_comes_first | COVERED | inbox::tests::frames_match_python (exact bytes, auth line first) |
| InboxFrames.test_every_line_is_newline_terminated | COVERED | inbox::tests::frames_match_python (expected text ends in newline) |
| InboxFrames.test_unicode_is_not_escaped_away | COVERED | inbox::tests::frames_match_python (the e-acute is literal in the expected bytes) |
| InboxFrames.test_empty_content_is_refused | COVERED | inbox::tests::refuses_what_python_refuses and new inbox::core_tests::empty_content_is_refused_whatever_the_blanks ("", blank, whitespace; None/5 cannot occur: content is &str) |
| InboxFrames.test_oversized_content_is_refused_before_connecting | COVERED | new: inbox::core_tests::oversized_content_is_refused_before_connecting |
| InboxFrames.test_windows_requires_a_token | COVERED | inbox::tests::refuses_what_python_refuses |
| InboxFrames.test_pipe_is_opened_write_only | COVERED | new: inbox::core_tests::pipe_is_opened_write_only (real named pipe that grants write only) |
| BackgroundCommand.test_prompt_is_the_last_argument | COVERED | new: background::core_tests::prompt_is_the_last_argument |
| BackgroundCommand.test_options_are_omitted_when_unset | COVERED | new: background::core_tests::options_are_omitted_when_unset |
| BackgroundCommand.test_every_option_is_passed_through | COVERED | new: background::core_tests::every_option_is_passed_through |
| BackgroundCommand.test_a_short_name_is_derived_from_the_prompt | COVERED | new: background::core_tests::a_short_name_is_derived_from_the_prompt |
| BackgroundCommand.test_a_derived_name_is_a_usable_handle | COVERED | new: background::core_tests::a_derived_name_is_a_usable_handle |
| BackgroundCommand.test_an_unnameable_prompt_yields_no_name_rather_than_junk | COVERED | new: background::core_tests::an_unnameable_prompt_yields_no_name |
| BackgroundCommand.test_an_explicit_name_beats_the_derived_one | COVERED | new: background::core_tests::an_explicit_name_beats_the_derived_one |
| BackgroundCommand.test_the_id_is_taken_from_the_end_of_the_output | COVERED | new: background::core_tests::the_id_is_taken_from_the_end_of_the_output |
| BackgroundCommand.test_no_id_is_reported_as_none_not_guessed | COVERED | new: background::core_tests::no_id_is_reported_as_none |
| BackgroundCommand.test_an_empty_prompt_is_refused_before_launching | COVERED | new: background::core_tests::an_empty_prompt_is_refused_before_launching |
| BackgroundCommand.test_a_missing_directory_is_refused_before_launching | COVERED | new: background::core_tests::a_missing_directory_is_refused_before_launching |
| BackgroundCommand.test_unknown_config_keys_do_not_break_a_dispatch | COVERED | new: background::core_tests::unknown_config_keys_do_not_reach_the_command |
| IdleStates.test_pending_work_is_waiting_on_tasks | COVERED | registry::reviewed_tests::idle_is_tasks_complete_or_ready |
| IdleStates.test_an_idle_prompt_while_tasks_run_is_not_shown | COVERED | new: registry::core_tests::an_idle_prompt_while_tasks_run_is_not_shown |
| IdleStates.test_a_finished_turn_with_nothing_pending_is_complete | COVERED | registry::reviewed_tests::idle_is_tasks_complete_or_ready |
| IdleStates.test_idle_prompt_is_complete | COVERED | new: registry::core_tests::idle_prompt_is_complete |
| IdleStates.test_blocked_and_failed_keep_their_attention | COVERED | new: registry::core_tests::blocked_and_failed_keep_their_attention (the blocked half is in idle_is_tasks_complete_or_ready; the failure half was untested) |
| IdleStates.test_never_given_work_is_ready | COVERED | registry::reviewed_tests::idle_is_tasks_complete_or_ready |
| IdleStates.test_working_and_finished_jobs_are_left_alone | COVERED | new: registry::core_tests::working_and_finished_jobs_are_left_alone |
| Reviewed.test_reviewed_after_the_wait_began_clears_it | COVERED | registry::reviewed_tests::reviewed_after_the_wait_began_clears_it |
| Reviewed.test_work_in_progress_is_never_reviewed | COVERED | registry::reviewed_tests::work_in_progress_is_never_reviewed |
| Reviewed.test_mark_reviewed_refuses_work_in_progress | COVERED | new: tests/core_reviewed.rs `reviewed_refuses_work_in_progress (POST /api/reviewed)` |
| Reviewed.test_store_keeps_when | COVERED | new: store::core_tests::reviewed_keeps_when |
| StoreState.test_tokens_survive_a_restart | COVERED | store::tests::register_merges_and_attention_starts_once (reopens the file and reads the token) |
| StoreState.test_register_merges_and_ignores_none | COVERED | new: store::core_tests::register_merges_and_ignores_none |
| StoreState.test_clear_attention_removes_the_key | COVERED | new: store::core_tests::clear_attention_removes_the_key_and_tolerates_strangers |
| StoreState.test_forget_drops_token_and_record | COVERED | new: store::core_tests::forget_drops_token_and_record |
| StoreState.test_events_are_capped_and_newest_first | COVERED | new: store::core_tests::events_are_capped |
| StoreState.test_corrupt_state_file_does_not_crash_startup | COVERED | new: store::core_tests::a_corrupt_state_file_does_not_crash_startup |
| StaleAttention.test_working_again_means_the_prompt_was_answered | COVERED | registry::tests::moved_on_needs_running_again_after_settling |
| StaleAttention.test_still_idle_is_still_waiting | COVERED | registry::tests::moved_on_needs_running_again_after_settling (idle row, later clock: false) |
| StaleAttention.test_a_status_older_than_the_prompt_proves_nothing | COVERED | new: registry::core_tests::moved_on_edges |
| StaleAttention.test_the_same_instant_is_the_prompt_appearing_not_an_answer | COVERED | new: registry::core_tests::moved_on_edges |
| StaleAttention.test_no_status_clock_leaves_the_hook_in_charge | COVERED | new: registry::core_tests::moved_on_edges |
| StaleAttention.test_nothing_was_waiting_in_the_first_place | COVERED | new: registry::core_tests::moved_on_edges |
| StaleAttention.test_the_status_clock_is_carried_off_the_session_file | COVERED | new: registry::core_tests::the_status_clock_is_carried_off_the_session_file |
| StaleAttention.test_a_missing_or_junk_stamp_is_simply_absent | COVERED | new: registry::core_tests::a_missing_or_junk_stamp_is_simply_absent |
| StaleAttention.test_a_desktop_app_session_is_flagged | COVERED | new: registry::core_tests::a_desktop_app_session_is_flagged |
| StaleAttention.test_a_vs_code_session_is_flagged | COVERED | new: registry::core_tests::a_vs_code_session_is_flagged |
| FailureCaptions.test_an_overload_says_so | COVERED | new: registry::core_tests::failure_captions |
| FailureCaptions.test_a_5xx_says_so | COVERED | new: registry::core_tests::failure_captions |
| FailureCaptions.test_the_quiet_failure_stays_quiet | COVERED | new: registry::core_tests::failure_captions |
| FailureCaptions.test_an_oversize_request_gets_the_actionable_wording | COVERED | new: registry::core_tests::failure_captions |
| FailureCaptions.test_an_ordinary_rejection_does_not | COVERED | new: registry::core_tests::failure_captions |
| FailureCaptions.test_an_unknown_or_missing_error_still_says_something | COVERED | new: registry::core_tests::failure_captions |
| FailureCaptions.test_every_documented_error_is_covered | COVERED | new: registry::core_tests::every_documented_error_has_a_caption |
| AttentionBookkeeping.test_repeated_failures_do_not_restart_the_clock | COVERED | new: store::core_tests::repeated_failures_do_not_restart_the_clock |
| AttentionBookkeeping.test_clearing_drops_the_reason_with_the_flag | COVERED | new: store::core_tests::clearing_drops_the_reason_with_the_flag |
| AttentionBookkeeping.test_clearing_keeps_everything_else | COVERED | new: store::core_tests::clearing_keeps_everything_else |
| AttentionBookkeeping.test_a_failure_is_not_settled_by_the_session_working_again | COVERED | new: registry::core_tests::moved_on_sources |
| AttentionBookkeeping.test_a_running_command_counts_as_running | COVERED | new: registry::core_tests::moved_on_sources |
| AttentionBookkeeping.test_sources_disagreeing_about_anything_else_clears_nothing | COVERED | new: registry::core_tests::moved_on_sources |
| WindowTitles.test_the_status_glyph_is_not_part_of_the_title | COVERED | winfocus::tests::titles_lose_their_glyph_and_case (and golden focus_compare) |
| WindowTitles.test_the_same_title_either_way_round | COVERED | new: winfocus::core_tests::the_same_title_either_way_round |
| WindowTitles.test_an_empty_or_missing_title_normalises_to_nothing | COVERED | new: winfocus::core_tests::an_empty_title_normalises_to_nothing |
| WindowTitles.test_the_right_window_is_picked | COVERED | new: winfocus::core_tests::the_right_window_is_picked |
| WindowTitles.test_a_background_tab_matches_nothing | COVERED | new: winfocus::core_tests::a_background_tab_matches_nothing |
| WindowTitles.test_an_elided_title_still_matches | COVERED | winfocus::tests::matching_refuses_to_guess ('Fix the build please' vs 'fix the build') |
| WindowTitles.test_two_windows_claiming_the_same_title_pick_neither | COVERED | winfocus::tests::matching_refuses_to_guess |
| WindowTitles.test_an_ambiguous_prefix_picks_neither | COVERED | winfocus::tests::matching_refuses_to_guess ('Fix' vs 'fix a'/'fix b') |
| WindowTitles.test_no_title_never_matches_anything | COVERED | winfocus::tests::matching_refuses_to_guess (empty title vs empty window) |
| WindowTitles.test_no_windows_is_not_an_error | COVERED | new: winfocus::core_tests::no_windows_is_not_an_error |
| BackgroundTasks.test_no_shells_means_nothing_waiting | COVERED | procs::tests::counts_the_topmost_shell_on_each_branch (pid with no children: empty) |
| BackgroundTasks.test_a_running_shell_counts | COVERED | new: procs::core_tests::the_python_table |
| BackgroundTasks.test_git_bash_reexec_is_one_task | COVERED | procs::tests::counts_the_topmost_shell_on_each_branch (shell inside a shell) |
| BackgroundTasks.test_powershell_behind_cmd_counts | COVERED | new: procs::core_tests::the_python_table |
| BackgroundTasks.test_a_brand_new_shell_is_a_hook_not_a_task | COVERED | procs::tests::counts_the_topmost_shell_on_each_branch (too young) |
| BackgroundTasks.test_two_tasks | COVERED | new: procs::core_tests::the_python_table |
| BackgroundTasks.test_exe_names_normalise | COVERED | procs::tests::exe_names |
| BackgroundTasks.test_annotate_only_touches_idle_rows | COVERED | new: procs::core_tests::annotate_marks_idle_rows_that_wait_on_a_shell (real child shell) |
| BackgroundTasks.test_annotate_counts_zero_and_skips_ended_rows | COVERED | new: procs::core_tests::annotate_counts_zero_and_skips_ended_rows |
| BackgroundTasks.test_live_table_reads | COVERED | new: procs::core_tests::the_live_table_reads |
| Worktrees.test_claude_worktree_path_on_either_platform | COVERED | new: registry::core_tests::worktree_paths_on_either_platform |
| Worktrees.test_plain_folder_is_its_own_project | COVERED | new: registry::core_tests::a_plain_folder_is_its_own_project |
| Worktrees.test_linked_worktree_elsewhere_read_from_its_git_file | COVERED | new: registry::core_tests::a_linked_worktree_is_read_from_its_git_file |
| Worktrees.test_default_worktree_name_becomes_project_and_worktree | COVERED | registry::tests::worktree_names |
| Worktrees.test_chosen_name_and_plain_sessions_keep_theirs | COVERED | new: registry::core_tests::chosen_names_and_plain_sessions_keep_theirs |
| Worktrees.test_short_name_drops_what_the_node_already_says | COVERED | registry::tests::worktree_names (worktree and plain-folder cases); the chosen-name case is in core_tests::chosen_names_and_plain_sessions_keep_theirs |
| Worktrees.test_merge_fills_the_fields | COVERED | new: registry::core_tests::merge_fills_the_fields |


## test_costs.py

34 tests: 34 COVERED
All ported to unit tests beside `History` (the spend views are in-process; the HTTP spend endpoints are covered elsewhere). Each ran green first time: no DIFFERS.

| Test | Verdict | Rust test or reason |
|---|---|---|
| test_input_output | COVERED | `costs::history_tests::input_output` |
| test_streamed_entries_count_once_and_last_wins | COVERED | `costs::history_tests::streamed_entries_count_once_and_last_wins` |
| test_cache_write_5m_vs_1h | COVERED | `costs::history_tests::cache_write_5m_vs_1h` |
| test_cache_write_without_split_is_5m | COVERED | `costs::history_tests::cache_write_without_split_is_5m` |
| test_cache_read_multipliers | COVERED | `costs::history_tests::cache_read_multipliers` |
| test_dated_and_bracketed_model_ids | COVERED | `costs::history_tests::dated_and_bracketed_model_ids` (also the existing `costs::tests::model_ids`, which checks `resolve` only) |
| test_family_fallback | COVERED | `costs::history_tests::family_fallback` |
| test_unknown_model_is_unpriced | COVERED | `costs::history_tests::unknown_model_is_unpriced` |
| test_rewriting_a_cold_cache | COVERED | `costs::history_tests::rewriting_a_cold_cache_prices` (also the existing `costs::tests::a_cold_cache`) |
| test_synthetic_and_non_assistant_skipped | COVERED | `costs::history_tests::synthetic_and_non_assistant_skipped` |
| test_local_date_and_zero_fill | COVERED | `costs::history_tests::local_date_and_zero_fill` |
| test_late_evening_stays_on_its_local_day | COVERED | `costs::history_tests::late_evening_stays_on_its_local_day` |
| test_usage_outside_window_not_counted | COVERED | `costs::history_tests::usage_outside_window_not_counted` |
| test_old_file_ignored | COVERED | `costs::history_tests::old_file_ignored` |
| test_old_file_dropped_from_memory | COVERED | `costs::history_tests::old_file_dropped_from_memory` |
| test_worktree_cwd_maps_to_repo | COVERED | `costs::history_tests::worktree_cwd_maps_to_repo` |
| test_sessions_counted_and_subagent_joins_parent | COVERED | `costs::history_tests::sessions_counted_and_subagent_joins_parent` |
| test_same_message_in_session_and_subagent_counts_once | COVERED | `costs::history_tests::same_message_in_session_and_subagent_counts_once` |
| test_sorted_by_cost | COVERED | `costs::history_tests::projects_sorted_by_cost` |
| test_appended_lines_only_are_parsed | COVERED | `costs::history_tests::appended_lines_only_are_parsed` |
| test_unchanged_file_not_reread | COVERED | `costs::history_tests::unchanged_file_not_reread` |
| test_later_entry_replaces_earlier_across_reads | COVERED | `costs::history_tests::later_entry_replaces_earlier_across_reads` |
| test_partial_line_waits_for_its_newline | COVERED | `costs::history_tests::partial_line_waits_for_its_newline` |
| test_shrunk_file_is_reread | COVERED | `costs::history_tests::shrunk_file_is_reread` |
| test_replaced_larger_file_is_reread | COVERED | `costs::history_tests::replaced_larger_file_is_reread` |
| test_budget_zero_is_incomplete | COVERED | `costs::history_tests::budget_zero_is_incomplete` |
| test_cached_files_count_when_budget_runs_out | COVERED | `costs::history_tests::cached_files_count_when_budget_runs_out` |
| test_missing_root_is_empty | COVERED | `costs::history_tests::missing_root_is_empty` |
| test_a_window_counts_only_messages_sent_inside_it | COVERED | `costs::history_tests::a_window_counts_only_messages_sent_inside_it` |
| test_streamed_entries_count_once_in_a_window | COVERED | `costs::history_tests::streamed_entries_count_once_in_a_window` |
| test_a_session_is_its_whole_life_with_its_subagents | COVERED | `costs::history_tests::a_session_is_its_whole_life_with_its_subagents` |
| test_each_answer_splits_by_model | COVERED | `costs::history_tests::each_answer_splits_by_model` |
| test_answers_follow_the_files | COVERED | `costs::history_tests::answers_follow_the_files` |
| test_not_a_session_id_finds_nothing | COVERED | `costs::history_tests::not_a_session_id_finds_nothing` |


## test_dispatcher.py

127 tests (by name): PORT 11, PORT-now-COVERED 116.

| Test | Verdict | Rust test or reason |
|---|---|---|
| `HookIngest.test_session_start_records_the_token` | COVERED | new: dispatch_hooks.rs::session_start_records_the_token |
| `HookIngest.test_a_session_is_dispatchable_only_with_socket_and_token` | COVERED | new: dispatch_hooks.rs::a_session_is_dispatchable_only_with_socket_and_token |
| `HookIngest.test_a_rebound_socket_invalidates_the_stored_token` | COVERED | new: dispatch_hooks.rs::a_rebound_socket_invalidates_the_stored_token |
| `HookIngest.test_the_next_session_start_repairs_a_stale_credential` | COVERED | new: dispatch_hooks.rs::the_next_session_start_repairs_a_stale_credential |
| `HookIngest.test_a_stop_event_can_register_a_session_start_never_saw` | COVERED | new: dispatch_hooks.rs::a_stop_event_can_register_a_session_start_never_saw |
| `HookIngest.test_a_notification_also_carries_the_credential` | COVERED | new: dispatch_hooks.rs::a_notification_also_carries_the_credential |
| `HookIngest.test_permission_prompt_raises_attention` | COVERED | new: dispatch_hooks.rs::permission_prompt_raises_attention |
| `HookIngest.test_stop_clears_attention` | COVERED | new: dispatch_hooks.rs::stop_clears_attention |
| `HookIngest.test_session_end_forgets_the_token` | COVERED | new: dispatch_hooks.rs::session_end_forgets_the_token |
| `HookIngest.test_uninteresting_events_are_dropped` | COVERED | new: dispatch_hooks.rs::uninteresting_events_are_dropped |
| `HookIngest.test_tool_events_go_to_memory_not_the_store` | COVERED | new: dispatch_hooks.rs::tool_events_go_to_memory_not_the_store |
| `HookIngest.test_an_event_without_a_session_id_is_dropped` | COVERED | new: dispatch_hooks.rs::an_event_without_a_session_id_is_dropped |
| `HookIngest.test_stop_failure_keeps_its_error` | COVERED | new: dispatch_hooks.rs::stop_failure_keeps_its_error |
| `IdleWhileRunning.test_idle_prompt_while_running_a_command_is_not_attention` | COVERED | new: dispatch_hooks.rs::idle_prompt_while_running_a_command_is_not_attention |
| `IdleWhileRunning.test_idle_prompt_with_a_background_shell_is_not_attention` | COVERED | new: registry.rs::finished_tests::an_idle_prompt_is_contradicted_by_a_background_shell (unit: still_running with background_tasks, which comes from the process table and cannot be put on an HTTP row) |
| `IdleWhileRunning.test_truly_idle_still_waits_for_you` | COVERED | new: dispatch_hooks.rs::truly_idle_still_waits_for_you |
| `IdleWhileRunning.test_it_comes_back_once_the_command_is_done` | COVERED | new: dispatch_hooks.rs::it_comes_back_once_the_command_is_done |
| `IdleWhileRunning.test_a_permission_prompt_still_raises_it` | COVERED | new: dispatch_hooks.rs::a_permission_prompt_still_raises_it |
| `RowLookup.test_a_few_seconds_old_snapshot_is_good_enough` | COVERED | new: dispatch_views.rs::a_few_seconds_old_snapshot_is_good_enough |
| `RowLookup.test_an_unknown_session_looks_in_a_fresh_snapshot` | COVERED | new: dispatch_views.rs::an_unknown_session_looks_in_a_fresh_snapshot |
| `RowLookup.test_an_old_snapshot_is_rebuilt` | COVERED | new: dispatch_views.rs::an_old_snapshot_is_rebuilt |
| `Dictation.test_no_voice_block_means_no_dictation_process` | COVERED | new: dispatch_views.rs::no_voice_block_means_no_dictation_process |
| `Dictation.test_a_voice_block_makes_a_worker_that_starts_nothing_without_preload` | COVERED | new: dispatch_views.rs::a_voice_block_makes_a_worker_that_starts_nothing_without_preload |
| `Dictation.test_disabled_voice_makes_no_worker` | COVERED | new: dispatch_views.rs::disabled_voice_makes_no_worker |
| `Awaited.test_unknown_session` | COVERED | new: dispatch_views.rs::unknown_session |
| `Awaited.test_a_live_session_lists_its_tasks` | COVERED | new: dispatch_views.rs::a_live_session_lists_its_tasks |
| `Awaited.test_an_ended_session_waits_on_nothing` | COVERED | new: dispatch_views.rs::an_ended_session_waits_on_nothing |
| `Awaited.test_status_does_not_carry_the_id_list` | COVERED | new: dispatch_views.rs::status_does_not_carry_the_id_list |
| `SessionStatus.test_unknown_session` | COVERED | new: dispatch_views.rs::session_status_for_an_unknown_session |
| `SessionStatus.test_a_live_session_keeps_its_background_work` | COVERED | new: dispatch_views.rs::a_live_session_keeps_its_background_work |
| `SessionStatus.test_an_ended_session_has_nothing_running` | COVERED | new: dispatch_views.rs::an_ended_session_has_nothing_running |
| `SessionStatus.test_every_local_row_carries_its_work_at_a_glance` | COVERED | new: dispatch_views.rs::every_local_row_carries_its_work_at_a_glance |
| `SessionStatus.test_an_ended_rows_work_has_no_counts` | COVERED | new: dispatch_views.rs::an_ended_rows_work_has_no_counts |
| `FailedTurns.test_an_overload_raises_attention_instead_of_clearing_it` | COVERED | new: dispatch_hooks.rs::an_overload_raises_attention_instead_of_clearing_it |
| `FailedTurns.test_the_retry_does_not_wipe_the_flag` | COVERED | new: dispatch_hooks.rs::the_retry_does_not_wipe_the_flag |
| `FailedTurns.test_a_permission_prompt_is_still_settled_by_working_again` | COVERED | new: dispatch_hooks.rs::a_permission_prompt_is_still_settled_by_working_again |
| `FailedTurns.test_finishing_cleanly_clears_a_failure` | COVERED | new: dispatch_hooks.rs::finishing_cleanly_clears_a_failure |
| `FailedTurns.test_the_quiet_failure_raises_nothing` | COVERED | new: dispatch_hooks.rs::the_quiet_failure_raises_nothing |
| `FailedTurns.test_the_reason_and_detail_are_kept_for_the_window` | COVERED | new: dispatch_hooks.rs::the_reason_and_detail_are_kept_for_the_window |
| `FailedTurns.test_a_failure_still_records_the_credential` | COVERED | new: dispatch_hooks.rs::a_failure_still_records_the_credential |
| `FailedTurns.test_an_unknown_error_code_still_raises_something` | COVERED | new: dispatch_hooks.rs::an_unknown_error_code_still_raises_something |
| `FocusRouting.test_the_window_with_this_session_title_is_the_one_raised` | PORT | BLOCKED: needs real terminal windows or a seam in winfocus (focus_row calls the Windows window APIs directly); only winfocus::tests::matching_refuses_to_guess covers the title match |
| `FocusRouting.test_the_window_is_remembered_for_when_the_tab_is_behind` | PORT | BLOCKED: as above (window_hwnd is recorded only after a real raise) |
| `FocusRouting.test_a_backgrounded_tab_falls_back_to_where_it_was_last_seen` | PORT | BLOCKED: as above |
| `FocusRouting.test_a_window_that_now_belongs_to_something_else_is_not_trusted` | PORT | BLOCKED: as above |
| `FocusRouting.test_an_untitled_session_still_gets_its_terminal` | PORT | BLOCKED: as above (would raise a real terminal) |
| `FocusRouting.test_nothing_raised_at_all_is_reported_not_papered_over` | COVERED | new: dispatch_send.rs::nothing_raised_at_all_is_reported_not_papered_over |
| `FocusRouting.test_a_failed_focus_never_launches_a_second_session` | COVERED | new: dispatch_send.rs::a_failed_focus_never_launches_a_second_session |
| `Routing.test_injects_into_a_live_dispatchable_session` | COVERED | new: dispatch_send.rs::injects_into_a_live_dispatchable_session |
| `Routing.test_a_dispatched_prompt_says_it_has_no_sender_to_answer` | COVERED | new: dispatch_send.rs::a_dispatched_prompt_says_it_has_no_sender_to_answer |
| `Routing.test_falls_back_to_a_deep_link_when_nothing_is_live` | PORT | BLOCKED, unsafe to run: auto/launch mode ends in deeplink::launch, which opens the OS URL handler (a real Claude window) and has no test seam; the checks before it are covered by dispatch_send.rs::inject_mode_refuses_rather_than_launching |
| `Routing.test_inject_mode_refuses_rather_than_launching` | COVERED | new: dispatch_send.rs::inject_mode_refuses_rather_than_launching |
| `Routing.test_launch_mode_never_injects` | PORT | BLOCKED, unsafe to run: auto/launch mode ends in deeplink::launch, which opens the OS URL handler (a real Claude window) and has no test seam; the checks before it are covered by dispatch_send.rs::dry_run_touches_nothing / background_mode_prefers_a_new_session_over_a_live_one |
| `Routing.test_dry_run_touches_nothing` | COVERED | new: dispatch_send.rs::dry_run_touches_nothing |
| `Routing.test_a_picked_path_wins_over_a_shared_name` | COVERED | new: dispatch_send.rs::a_picked_path_wins_over_a_shared_name (dry-run, asserting the project that would be launched) |
| `Routing.test_a_path_not_in_the_catalogue_dispatches_nothing` | COVERED | new: dispatch_send.rs::a_path_not_in_the_catalogue_dispatches_nothing (dry-run: the refusal comes before the mode) |
| `Routing.test_an_unmatched_project_dispatches_nothing` | COVERED | new: dispatch_send.rs::an_unmatched_project_dispatches_nothing |
| `Routing.test_a_failed_inject_falls_back_in_auto_mode` | COVERED | new: dispatch_send.rs::a_failed_inject_falls_back_in_auto_mode (inject-mode half only: a vanished pipe is reported; the fall-back to a window cannot be run safely) |
| `Routing.test_a_stale_credential_is_never_injected_into` | COVERED | new: dispatch_send.rs::a_stale_credential_is_never_injected_into (inject mode and session_id; auto would launch) |
| `Routing.test_background_mode_starts_a_session_and_never_opens_a_window` | COVERED | new: dispatch_send.rs::background_mode_starts_a_session_and_never_opens_a_window |
| `Routing.test_background_mode_prefers_a_new_session_over_a_live_one` | COVERED | new: dispatch_send.rs::background_mode_prefers_a_new_session_over_a_live_one |
| `Routing.test_background_options_from_config_are_passed_through` | COVERED | new: dispatch_send.rs::background_options_from_config_are_passed_through |
| `Routing.test_background_runs_in_a_worktree_of_the_chosen_sessions_branch` | COVERED | new: dispatch_send.rs::background_runs_in_a_worktree_of_the_chosen_sessions_branch |
| `Routing.test_a_worktree_whose_session_never_started_is_removed` | COVERED | new: dispatch_send.rs::a_worktree_whose_session_never_started_is_removed |
| `Routing.test_a_failed_background_start_is_reported_not_swallowed` | COVERED | new: dispatch_send.rs::a_failed_background_start_is_reported_not_swallowed |
| `Routing.test_missing_prompt_or_project_is_refused` | COVERED | new: dispatch_send.rs::missing_prompt_or_project_is_refused |
| `Routing.test_idle_session_wins_over_busy_one` | COVERED | new: dispatch_send.rs::idle_session_wins_over_busy_one |
| `Routing.test_explicit_project_and_prompt_skip_parsing` | COVERED | new: dispatch_send.rs::explicit_project_and_prompt_skip_parsing |
| `SlashCommands.test_the_chat_types_it_into_an_idle_session` | COVERED | new: dispatch_send.rs::the_chat_types_it_into_an_idle_session |
| `SlashCommands.test_a_busy_session_is_refused_not_typed_into` | COVERED | new: dispatch_send.rs::a_busy_session_is_refused_not_typed_into |
| `SlashCommands.test_a_parked_permission_prompt_is_never_answered_by_it` | COVERED | new: dispatch_send.rs::a_parked_permission_prompt_is_never_answered_by_it |
| `SlashCommands.test_a_menu_command_is_refused` | COVERED | new: dispatch_send.rs::a_menu_command_is_refused |
| `SlashCommands.test_by_project_name_it_runs_in_that_project_and_never_launches` | COVERED | new: dispatch_send.rs::by_project_name_it_runs_in_that_project_and_never_launches |
| `SlashCommands.test_queued_it_is_typed_when_the_turn_ends` | COVERED | new: dispatch_send.rs::queued_it_is_typed_when_the_turn_ends |
| `SlashCommands.test_what_could_never_be_typed_is_not_queued` | COVERED | new: dispatch_send.rs::what_could_never_be_typed_is_not_queued |
| `SlashCommands.test_an_ordinary_prompt_still_goes_to_the_inbox` | COVERED | new: dispatch_send.rs::an_ordinary_prompt_still_goes_to_the_inbox |
| `SendTyped.test_off_by_default` | COVERED | new: dispatch_send.rs::off_by_default |
| `SendTyped.test_typed_as_one_line_into_an_idle_session` | COVERED | new: dispatch_send.rs::typed_as_one_line_into_an_idle_session |
| `SendTyped.test_never_into_a_busy_session` | COVERED | new: dispatch_send.rs::never_into_a_busy_session |
| `SendTyped.test_a_command_goes_the_command_way` | COVERED | new: dispatch_send.rs::a_command_goes_the_command_way |
| `SendTyped.test_the_setting_applies_live` | COVERED | new: dispatch_send.rs::the_setting_applies_live |
| `RemoteHooks.test_a_remote_session_becomes_dispatchable_once_its_hook_reports` | COVERED | new: dispatch_remote.rs::a_remote_session_becomes_dispatchable_once_its_hook_reports |
| `RemoteHooks.test_remote_records_are_kept_under_the_host` | COVERED | new: dispatch_remote.rs::remote_records_are_kept_under_the_host |
| `RemoteHooks.test_a_host_cannot_forget_a_local_session` | COVERED | new: dispatch_remote.rs::a_host_cannot_forget_a_local_session |
| `RemoteHooks.test_a_host_cannot_repoint_a_local_session` | COVERED | new: dispatch_remote.rs::a_host_cannot_repoint_a_local_session |
| `RemoteHooks.test_a_remote_permission_prompt_raises_attention` | COVERED | new: dispatch_remote.rs::a_remote_permission_prompt_raises_attention |
| `RemoteHooks.test_a_remote_stop_clears_it` | COVERED | new: dispatch_remote.rs::a_remote_stop_clears_it |
| `RemoteHooks.test_a_rebound_remote_socket_invalidates_the_token` | COVERED | new: dispatch_remote.rs::a_rebound_remote_socket_invalidates_the_token |
| `RemoteHooks.test_remote_events_say_which_host` | COVERED | new: dispatch_remote.rs::remote_events_say_which_host |
| `RemoteWork.test_a_remote_row_carries_the_numbers_its_host_read` | PORT | PARTIAL, left open: needs a stub for the second ssh channel (`imperatorium remote-channel`, JSON lines) to hand over `stat`; dispatch_remote.rs::a_remote_row_carries_the_numbers_its_host_read only checks the work shape and that `stat` does not leak |
| `RemoteWork.test_without_them_the_work_is_empty_rather_than_local` | COVERED | new: dispatch_remote.rs::without_them_the_work_is_empty_rather_than_local |
| `RemoteWork.test_refresh_reaches_the_fleet` | COVERED | new: dispatch_remote.rs::refresh_reaches_the_fleet |
| `RemoteRouting.test_a_prompt_to_a_remote_session_goes_over_ssh` | COVERED | new: dispatch_remote.rs::a_prompt_to_a_remote_session_goes_over_ssh |
| `RemoteRouting.test_a_remote_session_without_a_token_is_refused` | COVERED | new: dispatch_remote.rs::a_remote_session_without_a_token_is_refused |
| `RemoteRouting.test_a_remote_failure_is_reported` | COVERED | new: dispatch_remote.rs::a_remote_failure_is_reported |
| `RemoteRouting.test_a_remote_project_injects_into_its_live_session` | COVERED | new: dispatch_remote.rs::a_remote_project_injects_into_its_live_session |
| `RemoteRouting.test_a_remote_project_with_nothing_live_goes_to_the_background` | COVERED | new: dispatch_remote.rs::a_remote_project_with_nothing_live_goes_to_the_background |
| `RemoteRouting.test_inject_mode_to_a_remote_project_refuses_rather_than_starting` | COVERED | new: dispatch_remote.rs::inject_mode_to_a_remote_project_refuses_rather_than_starting |
| `RemoteRouting.test_background_mode_never_injects_remotely` | COVERED | new: dispatch_remote.rs::background_mode_never_injects_remotely |
| `RemoteRouting.test_a_local_project_never_picks_a_remote_session` | COVERED | new: dispatch_remote.rs::a_local_project_never_picks_a_remote_session |
| `RemoteRouting.test_a_remote_session_is_never_focused_as_a_local_pid` | COVERED | new: dispatch_remote.rs::a_remote_session_is_never_focused_as_a_local_pid |
| `HookOnlyListener.test_hooks_are_accepted_and_tagged_with_the_listeners_host` | COVERED | new: dispatch_remote.rs::hooks_are_accepted_and_tagged_with_the_listeners_host |
| `HookOnlyListener.test_dispatch_is_not_reachable_from_the_host` | COVERED | new: dispatch_remote.rs::dispatch_is_not_reachable_from_the_host |
| `HookOnlyListener.test_nothing_else_is_either` | COVERED | new: dispatch_remote.rs::nothing_else_is_either |
| `Origins.test_the_windows_own_origin_is_accepted` | COVERED | new: dispatch_http.rs::the_windows_own_origin_is_accepted |
| `Origins.test_no_origin_is_accepted` | COVERED | new: dispatch_http.rs::no_origin_is_accepted |
| `Origins.test_a_foreign_page_is_refused` | COVERED | new: dispatch_http.rs::a_foreign_page_is_refused |
| `Origins.test_the_same_host_on_another_port_is_refused` | COVERED | new: dispatch_http.rs::the_same_host_on_another_port_is_refused |
| `Origins.test_an_opaque_origin_is_refused` | COVERED | new: dispatch_http.rs::an_opaque_origin_is_refused |
| `WindowOnly.test_a_plain_api_call_cannot_type` | COVERED | new: dispatch_http.rs::a_plain_api_call_cannot_type |
| `WindowOnly.test_the_window_reaches_it` | COVERED | new: dispatch_http.rs::the_window_reaches_it |
| `WindowOnly.test_only_the_window_turns_it_on` | COVERED | new: dispatch_http.rs::only_the_window_turns_it_on |
| `WindowOnly.test_the_page_carries_the_token` | COVERED | new: dispatch_http.rs::the_page_carries_the_token |
| `ConversationRoute.test_the_default_is_the_whole_conversation` | COVERED | new: dispatch_http.rs::the_default_is_the_whole_conversation |
| `ConversationRoute.test_an_unchanged_conversation_is_not_resent` | COVERED | new: dispatch_http.rs::an_unchanged_conversation_is_not_resent |
| `ConversationRoute.test_a_changed_conversation_is_sent_in_full` | COVERED | new: dispatch_http.rs::a_changed_conversation_is_sent_in_full |
| `ConversationRoute.test_blocks_sends_only_the_newest_blocks` | COVERED | new: dispatch_http.rs::blocks_sends_only_the_newest_blocks |
| `TailBlocks.test_tail_never_changes_the_cached_items` | COVERED | new: conversation.rs::tail_blocks_ported::tail_never_changes_the_cached_items |
| `TailBlocks.test_more_than_there_is_returns_everything` | COVERED | new: conversation.rs::tail_blocks_ported::more_than_there_is_returns_everything |
| `RemoteConversation.test_a_remote_session_is_read_on_its_own_host` | PORT | BLOCKED: needs a stub remote-channel (reader protocol); not built |
| `RemoteConversation.test_one_reader_per_host_is_kept` | PORT | BLOCKED: as above (and the reader object is Python plumbing; Rust keeps one channel per host) |
| `RemoteConversation.test_a_failed_read_says_where_and_why` | PORT | BLOCKED: as above |
| `RemoteConversation.test_a_local_session_never_goes_to_a_host` | COVERED | new: dispatch_remote.rs::a_local_session_never_goes_to_a_host |
| `EventStream.test_the_list_is_pushed_and_a_hook_pushes_the_change` | COVERED | new: dispatch_http.rs::the_list_is_pushed_and_a_hook_pushes_the_change |
| `EventStream.test_an_unchanged_list_is_not_resent` | COVERED | new: dispatch_http.rs::an_unchanged_list_is_not_resent |
| `CostRoute.test_costs_come_from_the_history_and_errors_are_answers` | COVERED | new: dispatch_views.rs::costs_come_from_the_history_and_errors_are_answers (window_days and clamping over a real transcript; the Python `disk gone` half has no Rust counterpart: summary() cannot fail) |
| `WorkContext.test_the_tile_row_carries_the_context_size` | COVERED | new: dispatch_views.rs::the_tile_row_carries_the_context_size |


## test_features.py: Python test coverage in Rust

203 tests (199 defined, 4 inherited by QuestionWait): COVERED 200, PORT (still to port) 0, MOOT 2, DIFFERS 1.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Changes.test_clean_repo_has_no_files_or_diff | COVERED | changes::features_tests::features_changes_of_a_clean_repo |
| Changes.test_dirty_repo_reports_modified_and_untracked_files | COVERED | changes::features_tests::features_changes_report_modified_and_untracked_files |
| Changes.test_non_repo_reports_repo_false_without_error | COVERED | changes::features_tests::features_changes_of_a_non_repo_and_a_missing_directory |
| Changes.test_missing_directory_is_a_clean_error | COVERED | changes::features_tests::features_changes_of_a_non_repo_and_a_missing_directory |
| Worktrees.test_ahead_count_and_agent_flag | COVERED | changes::features_tests::features_worktrees_ahead_count_agent_flag_and_dirty |
| Worktrees.test_a_hand_made_worktree_is_not_flagged_as_agent | COVERED | changes::features_tests::features_worktrees_ahead_count_agent_flag_and_dirty |
| Worktrees.test_dirty_worktree_is_reported_dirty | COVERED | changes::features_tests::features_worktrees_ahead_count_agent_flag_and_dirty |
| Worktrees.test_no_worktrees_on_a_plain_repo | COVERED | changes::features_tests::features_worktrees_ahead_count_agent_flag_and_dirty |
| Worktrees.test_not_a_repo | COVERED | changes::features_tests::features_worktrees_of_a_non_repo |
| Finish.test_unknown_action_is_refused | COVERED | worktree_twin::worktree_actions_answer_as_python_did, step "bad action" (branch python-out/twins-b) |
| Finish.test_no_such_worktree | COVERED | worktree_twin step "no such worktree" (branch python-out/twins-b) |
| Finish.test_refuses_a_worktree_not_made_by_an_agent | COVERED | worktree_twin step "made by hand" (branch python-out/twins-b) |
| Finish.test_merge_refuses_when_the_main_checkout_is_dirty | COVERED | worktree_twin step "merge, main dirty" (branch python-out/twins-b) |
| Finish.test_merge_refuses_when_the_worktree_is_dirty | COVERED | worktree_twin step "merge dirty" (branch python-out/twins-b) |
| Finish.test_merge_succeeds_on_a_clean_ahead_worktree | COVERED | worktree_twin step "merge" (branch python-out/twins-b) |
| Finish.test_merge_refuses_when_nothing_to_merge | COVERED | worktree_twin step "merge again" (branch python-out/twins-b) |
| Finish.test_discard_refuses_unmerged_work_without_force | COVERED | worktree_twin step "discard unmerged" (branch python-out/twins-b) |
| Finish.test_discard_succeeds_with_force | COVERED | worktree_twin step "discard unmerged, forced" and the "left" step (branch python-out/twins-b) |
| Finish.test_discard_of_clean_merged_work_needs_no_force | COVERED | worktree_twin step "discard clean" (branch python-out/twins-b) |
| Finish.test_pr_uses_the_injected_runner_never_git_push_or_gh | COVERED | worktree_twin step "worktree pr" (branch python-out/twins-b): the push really reaches a local origin ("pushed" step) and gh is then run in the worktree (its failure is reported). Not covered: taking the URL from gh's last output line, since gh cannot be stubbed (no runner seam, and a stand-in must be gh.exe) |
| Finish.test_pr_reports_a_push_failure_without_touching_gh | COVERED | changes::features_tests::features_pr_reports_a_push_failure_and_refuses_a_dirty_worktree_first |
| Finish.test_a_merge_git_refuses_is_reported_and_changes_nothing | COVERED | changes::features_tests::features_a_merge_git_refuses_is_reported_and_changes_nothing |
| Finish.test_pr_refuses_a_dirty_worktree_before_touching_the_network | COVERED | worktree_twin step "pr dirty" (branch python-out/twins-b) |
| MadeWorktrees.test_made_from_the_sources_branch_not_main | COVERED | changes::tests::made_worktrees_branch_from_the_source_and_merge_back_into_it |
| MadeWorktrees.test_names_do_not_collide | COVERED | same (second make gives worktree-fix-the-thing-2) |
| MadeWorktrees.test_a_detached_source_gives_its_commit_and_no_base | COVERED | changes::features_tests::features_made_worktree_edge_cases |
| MadeWorktrees.test_not_a_repo_is_none | COVERED | changes::features_tests::features_made_worktree_edge_cases |
| MadeWorktrees.test_merge_goes_into_its_base_where_that_is_checked_out | COVERED | same |
| MadeWorktrees.test_merge_refuses_a_dirty_base_checkout | COVERED | same |
| MadeWorktrees.test_merge_refuses_a_base_checked_out_nowhere | COVERED | changes::features_tests::features_merge_refuses_a_base_checked_out_nowhere |
| MadeWorktrees.test_diff_is_against_its_base | COVERED | same |
| MadeWorktrees.test_discard_forgets_the_base | COVERED | same |
| MadeWorktrees.test_drop_worktree_undoes_it | COVERED | changes::features_tests::features_drop_worktree_undoes_it_including_the_branch |
| ChecksState.test_no_rollup_is_none | COVERED | prs::tests::checks_fold_both_shapes |
| ChecksState.test_any_failure_wins | COVERED | prs::features_tests::features_checks_state_shapes |
| ChecksState.test_pending_beats_passing | COVERED | prs::tests::checks_fold_both_shapes |
| ChecksState.test_all_passing | COVERED | prs::features_tests::features_checks_state_shapes |
| ChecksState.test_status_context_shape | COVERED | prs::features_tests::features_checks_state_shapes |
| ChecksState.test_mixed_shapes_failure_still_wins | COVERED | prs::tests::checks_fold_both_shapes (IN_PROGRESS beside a state ERROR) |
| Summarise.test_maps_the_fields_the_badge_needs | COVERED | prs::features_tests::features_summarise_maps_the_badge_fields |
| Tracker.test_get_registers_interest_and_returns_none_before_a_fetch | COVERED | prs::features_tests::features_get_registers_interest_and_a_fresh_cache_is_not_refetched |
| Tracker.test_poll_once_caches_the_injected_fetch_never_calling_gh | COVERED | prs::features_tests::features_get_registers_interest_and_a_fresh_cache_is_not_refetched |
| Tracker.test_a_runtime_error_from_fetch_disables_the_tracker | MOOT | no fetch seam in the binary; gh missing is the only trigger and a test cannot make gh missing without touching PATH |
| Tracker.test_directories_nobody_asked_about_recently_are_dropped | COVERED | prs::features_tests::features_directories_nobody_asked_about_recently_are_dropped |
| Reading.test_extracts_the_fields_the_meter_needs | COVERED | usage::features_tests::features_reading_extracts_the_meter_fields_and_tolerates_none |
| Reading.test_missing_fields_are_none_not_a_crash | COVERED | usage::features_tests::features_reading_extracts_the_meter_fields_and_tolerates_none |
| Pace.test_too_little_history_says_nothing | COVERED | usage::tests::pace_needs_five_minutes_of_readings |
| Pace.test_a_rising_pace_forecasts_when_it_hits_100 | COVERED | same (per_hour 60, full_at 5400) |
| Pace.test_before_reset_is_true_only_when_full_at_precedes_it | COVERED | same (reset 3000 vs 6000) |
| Pace.test_flat_or_falling_usage_never_predicts_full | COVERED | usage::features_tests::features_pace_flat_falling_and_old_samples |
| Pace.test_old_samples_outside_the_pace_span_are_ignored | COVERED | usage::features_tests::features_pace_flat_falling_and_old_samples |
| UsageTracking.test_a_threshold_crossing_fires_once | COVERED | usage::features_tests::features_a_threshold_crossing_fires_once_and_the_top_one_wins |
| UsageTracking.test_jumping_straight_past_both_thresholds_reports_the_top_one | COVERED | usage::features_tests::features_a_threshold_crossing_fires_once_and_the_top_one_wins |
| UsageTracking.test_a_new_reset_time_starts_a_fresh_history | COVERED | usage::features_tests::features_a_new_reset_time_starts_a_fresh_history |
| UsageTracking.test_a_window_that_reset_waits_at_zero_for_a_new_reading | COVERED | usage::features_tests::features_a_window_that_reset_waits_at_zero_for_a_new_reading |
| UsageTracking.test_forget_drops_a_sessions_reading | COVERED | usage::features_tests::features_forget_drops_a_sessions_reading |
| Timeline.test_pairs_tool_calls_with_their_results | COVERED | activity::features_tests::features_timeline_pairs_calls_with_results_and_leaves_running_ones_open |
| Timeline.test_a_call_still_running_has_ok_none | COVERED | activity::features_tests::features_timeline_pairs_calls_with_results_and_leaves_running_ones_open |
| Doing.test_a_running_tool_call_wins | COVERED | activity::tests::the_newest_thing_wins |
| Doing.test_claudes_last_text_after_a_finished_turn | COVERED | activity::features_tests::features_doing_after_a_finished_turn_and_when_empty |
| Doing.test_a_pending_human_prompt_is_marked_as_asked | COVERED | activity::tests::the_newest_thing_wins |
| Doing.test_empty_conversation_is_a_blank_line | COVERED | activity::features_tests::features_doing_after_a_finished_turn_and_when_empty |
| Categorise.test_known_shapes | COVERED | activity::features_tests::features_categorise_known_shapes |
| Tally.test_counts_calls_and_errors_by_tool_and_category | COVERED | activity::features_tests::features_tally_counts_calls_and_errors_and_merges |
| Tally.test_merge_tallies_adds_them_up | COVERED | activity::features_tests::features_tally_counts_calls_and_errors_and_merges |
| Live.test_pre_then_post_pairs_into_one_finished_call | COVERED | live::features_tests::features_pre_then_post_pairs_into_one_finished_call |
| Live.test_a_running_call_is_current_until_it_ends | COVERED | live::features_tests::features_a_running_call_is_current_until_it_ends_or_goes_stale |
| Live.test_a_call_stuck_for_ten_minutes_is_not_reported_as_current | COVERED | live::features_tests::features_a_running_call_is_current_until_it_ends_or_goes_stale |
| Live.test_a_failure_event_is_recorded_not_ok | COVERED | live::features_tests::features_a_failure_event_is_recorded_not_ok |
| Live.test_a_subagents_calls_are_not_recorded | COVERED | live::features_tests::features_a_subagents_calls_a_non_tool_event_and_forget |
| Live.test_a_non_tool_event_is_not_ingested | COVERED | live::features_tests::features_a_subagents_calls_a_non_tool_event_and_forget |
| Live.test_forget_drops_a_sessions_events | COVERED | live::features_tests::features_a_subagents_calls_a_non_tool_event_and_forget |
| Matches.test_every_word_must_appear_in_any_order | COVERED | history::tests::every_word_somewhere |
| Matches.test_a_prompt_only_match_is_reported_as_prompt | COVERED | history::tests::every_word_somewhere |
| Matches.test_a_body_text_only_match_needs_the_text_argument | COVERED | history::features_tests::features_a_body_match_needs_the_text_and_an_empty_query_matches_nothing |
| Matches.test_empty_query_matches_nothing | COVERED | history::features_tests::features_a_body_match_needs_the_text_and_an_empty_query_matches_nothing |
| Snippet.test_returns_the_match_in_context | COVERED | history::tests::snippets_match_python |
| Snippet.test_no_match_is_empty | COVERED | history::tests::snippets_match_python |
| IndexTests.test_list_is_sorted_newest_first_and_cached | COVERED | tests/features_api.rs features_api::features_history_lists_newest_first_and_searches_title_then_text (GET /api/history over seeded transcripts) and history::features_tests::features_a_cached_listing_is_not_taken_again |
| IndexTests.test_text_is_cached_by_size_and_rebuilt_when_it_changes | COVERED | history::features_tests::features_the_text_is_cached_by_size_and_rebuilt_when_it_changes |
| IndexTests.test_search_finds_a_title_match_without_reading_the_transcript | COVERED | history::features_tests::features_search_title_match_reads_no_transcript |
| IndexTests.test_search_falls_back_to_full_text | COVERED | history::features_tests::features_search_falls_back_to_full_text |
| IndexTests.test_search_marks_incomplete_when_the_budget_runs_out | MOOT | the 8 s budget is a constant in history::Index::search (no parameter, unlike Python's budget=); running it out would take 8 s of real time and a corpus big enough to need it |
| IndexTests.test_empty_query_returns_the_plain_list | COVERED | history::features_tests::features_an_empty_query_returns_the_plain_list |
| Resume.test_requires_a_session_id | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (POST /api/resume, fakecli as wt and claude) |
| Resume.test_requires_a_real_directory | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (POST /api/resume, fakecli as wt and claude) |
| Resume.test_window_launches_the_terminal_command_through_the_injected_popen | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (POST /api/resume, fakecli as wt and claude) |
| Resume.test_fork_adds_fork_session_to_the_window_command | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (POST /api/resume, fakecli as wt and claude) |
| Resume.test_background_uses_the_injected_bg_start_not_a_real_process | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (POST /api/resume, fakecli as wt and claude) |
| Resume.test_a_background_error_becomes_a_resume_error | COVERED | tests/features_api.rs features_api::features_launching_needs_wt_and_claude_on_the_path |
| Resume.test_an_unknown_where_is_refused | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (POST /api/resume, fakecli as wt and claude) |
| Resume.test_new_session_opens_plain_claude_in_a_tab | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (POST /api/resume, fakecli as wt and claude) |
| Resume.test_launches_drop_the_calling_sessions_identity | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (fakecli logs which session variables the launch carried: none, though the dispatcher had CLAUDECODE and CLAUDE_PID set) |
| Resume.test_a_background_launch_drops_the_calling_sessions_identity_too | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (fakecli logs which session variables the launch carried: none, though the dispatcher had CLAUDECODE and CLAUDE_PID set) |
| Resume.test_new_session_gets_a_fresh_id_each_time_or_the_one_given | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (two POST /api/new give different ids; the binary always makes the id itself, there is no way to hand it one) |
| Resume.test_new_session_requires_a_real_directory | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (POST /api/resume, fakecli as wt and claude) |
| Resume.test_window_needs_wt_on_path | COVERED | tests/features_api.rs features_api::features_launching_needs_wt_and_claude_on_the_path |
| NtfyRequest.test_builds_a_request_against_the_configured_server_and_topic | COVERED | notify::tests::ntfy_request_matches_python |
| NtfyRequest.test_a_token_adds_a_bearer_header | COVERED | notify::tests::ntfy_request_matches_python |
| NtfyRequest.test_missing_topic_refuses_to_build_a_request | COVERED | notify::tests::ntfy_request_matches_python |
| SpeakCommand.test_quotes_are_escaped_for_powershell | COVERED | notify::tests::lines_match_python |
| SpeakCommand.test_hyphens_and_underscores_read_better_spoken | COVERED | notify::tests::lines_match_python |
| NotifierTests.test_attention_schedules_every_enabled_channel_once | COVERED | notify::features_tests::features_attention_schedules_a_channel_once_per_raise (ntfy only; the speak channel is never enabled in a test, it would run PowerShell speech) |
| NotifierTests.test_firing_pushes_and_speaks_when_still_waiting | COVERED | notify_twin::notifications_push_and_count_as_python_did (branch python-out/twins-a); the speak channel is never enabled in a test (it would run PowerShell speech) |
| NotifierTests.test_answered_before_the_delay_never_sends_anything | COVERED | notify_twin (session twin-answered; branch python-out/twins-a) |
| NotifierTests.test_a_disabled_channel_is_never_scheduled | COVERED | notify_twin (ntfy switched off, then a Notification; branch python-out/twins-a) |
| NotifierTests.test_a_send_failure_is_recorded_not_raised | COVERED | notify::features_tests::features_a_send_failure_is_recorded_not_raised |
| NotifierTests.test_budget_fires_immediately_with_no_timer | COVERED | notify_twin ("5-hour limit at 91%" push; branch python-out/twins-a) |
| NotifierTests.test_status_reports_enabled_and_configured | COVERED | notify_twin "get /api/notify" (branch python-out/twins-a) and twin.rs (branch python-out/twins-b) |
| NotifierTests.test_set_enabled_rejects_an_unknown_channel | COVERED | twin.rs "no such channel: nope" (branch python-out/twins-b) |
| QueueStore.test_enqueue_then_take_is_fifo | COVERED | store::features_tests::features_the_queue_is_fifo_and_an_empty_one_leaves_the_map |
| QueueStore.test_take_from_an_empty_queue_is_none | COVERED | store::features_tests::features_the_queue_is_fifo_and_an_empty_one_leaves_the_map |
| QueueStore.test_an_empty_queue_is_dropped_from_the_map | COVERED | store::features_tests::features_the_queue_is_fifo_and_an_empty_one_leaves_the_map |
| QueueStore.test_requeue_front_puts_it_back_first_with_the_error | COVERED | store::features_tests::features_requeue_front_puts_it_back_first_with_the_error |
| QueueStore.test_unqueue_removes_one_item_by_id | COVERED | store::features_tests::features_ids_are_unique_so_unqueue_removes_one_item_by_id |
| QueueStore.test_enqueue_ids_are_unique_within_one_clock_tick | COVERED | store::features_tests::features_ids_are_unique_so_unqueue_removes_one_item_by_id |
| QueueStore.test_queue_survives_a_reload | COVERED | store::features_tests::features_the_queue_survives_a_reload_and_prefs_round_trip |
| QueueStore.test_prefs_round_trip_and_none_clears_a_key | COVERED | store::features_tests::features_the_queue_survives_a_reload_and_prefs_round_trip |
| StatuslineLine.test_full_payload | COVERED | hooks::tests::statusline_text |
| StatuslineLine.test_missing_fields_are_skipped_not_blank | COVERED | hooks::tests::statusline_text |
| StatuslineLine.test_non_numeric_percentages_are_ignored | COVERED | hooks::tests::statusline_text |
| StatuslineProvider.test_cloud_flags_win_in_claude_codes_order | COVERED | hooks::features_tests::features_cloud_flags_win_in_claude_codes_order |
| StatuslineProvider.test_a_flag_set_to_zero_is_off | COVERED | hooks::tests::providers |
| StatuslineProvider.test_keys_and_gateways | COVERED | hooks::tests::providers |
| StatuslineProvider.test_the_claude_ai_login_is_none | COVERED | hooks::tests::providers |
| StatuslineProvider.test_the_line_names_the_provider_only_without_windows | COVERED | hooks::features_tests::features_the_line_names_the_provider_only_without_windows |
| PlanWithoutWindows.test_provider_comes_from_the_script_or_the_model_id | COVERED | usage::features_tests::features_provider_comes_from_the_script_or_the_model_id |
| PlanWithoutWindows.test_all_bedrock_sessions_name_bedrock_in_place_of_meters | COVERED | tests/features_api.rs features_api::features_plan_with_bedrock_sessions_and_beside_real_windows (an enterprise login with every session on Bedrock names Bedrock) and usage::features_tests::features_a_session_gone_quiet_stops_counting |
| PlanWithoutWindows.test_a_pro_or_max_login_before_its_first_reading_shows_nothing | COVERED | tests/features_api.rs features_api::features_plan_names_the_login_only_where_there_are_no_meters (pro/max login, enterprise, team, only the organization type read, broken or missing file); provider_of itself: usage::features_tests::features_provider_comes_from_the_script_or_the_model_id |
| PlanWithoutWindows.test_an_enterprise_login_names_the_plan | COVERED | tests/features_api.rs features_api::features_plan_names_the_login_only_where_there_are_no_meters, and features_api::features_plan_with_bedrock_sessions_and_beside_real_windows (Bedrock-only with an enterprise login) |
| PlanWithoutWindows.test_beside_real_windows_only_the_uncovered_providers_are_named | COVERED | tests/features_api.rs features_api::features_plan_with_bedrock_sessions_and_beside_real_windows |
| PlanWithoutWindows.test_a_session_gone_quiet_stops_counting | COVERED | usage::features_tests::features_a_session_gone_quiet_stops_counting |
| PlanWithoutWindows.test_account_plan_reads_only_the_organization_type | COVERED | tests/features_api.rs features_api::features_plan_names_the_login_only_where_there_are_no_meters (pro/max login, enterprise, team, only the organization type read, broken or missing file); provider_of itself: usage::features_tests::features_provider_comes_from_the_script_or_the_model_id |
| Queue.test_idle_dispatchable_session_gets_immediate_delivery | COVERED | tests/features_api.rs features_api::features_queue_delivers_holds_requeues_and_sweeps |
| Queue.test_a_busy_session_holds_the_prompt_until_a_stop_hook | COVERED | tests/features_api.rs features_api::features_queue_delivers_holds_requeues_and_sweeps |
| Queue.test_a_failed_delivery_is_requeued_with_the_error | COVERED | tests/features_api.rs features_api::features_queue_delivers_holds_requeues_and_sweeps |
| Queue.test_sweep_queue_delivers_to_a_session_that_registered_late | COVERED | tests/features_api.rs features_api::features_queue_delivers_holds_requeues_and_sweeps |
| Queue.test_enqueue_with_no_prompt_is_refused | COVERED | twin.rs "queue prompt '  '" -> no prompt (branch python-out/twins-b) |
| Queue.test_enqueue_for_an_unknown_session_is_refused | COVERED | twin.rs "queue no-such-session" (branch python-out/twins-b) |
| StatuslineIngest.test_a_statusline_event_updates_usage_not_the_event_log | COVERED | tests/features_api.rs features_api::features_a_statusline_event_updates_usage_not_the_event_log |
| ActivityMethod.test_prefers_hooks_over_the_conversation | COVERED | twin.rs "get /api/activity" session 3, source hooks (branch python-out/twins-b) |
| ActivityMethod.test_falls_back_to_the_conversation_when_there_are_no_hook_events | COVERED | tests/features_api.rs features_api::features_activity_falls_back_to_the_conversation_and_errors_aggregate |
| ErrorsMethod.test_aggregates_tool_failures_across_recent_sessions_by_project | COVERED | tests/features_api.rs features_api::features_activity_falls_back_to_the_conversation_and_errors_aggregate |
| IdleAfterStop.test_an_idle_session_a_minute_after_stop_is_waiting_for_you | COVERED | notify_twin idle push + idle_prompt event (branch python-out/twins-a) |
| IdleAfterStop.test_only_the_latest_turns_check_counts | COVERED | tests/features_api.rs features_api::features_an_idle_session_after_stop_is_waiting_for_you_once (a stale first check raises nothing; one event for two Stops) |
| IdleAfterStop.test_a_busy_session_or_one_with_background_work_is_not_waiting | COVERED | tests/features_api.rs features_api::features_an_idle_session_after_stop_is_waiting_for_you_once (the busy half; the background-work half would need an agent row from claude agents, which fakecli answers with []) |
| IdleAfterStop.test_a_session_that_scheduled_its_own_next_turn_is_not_waiting | COVERED | tests/features_api.rs features_api::features_a_session_that_scheduled_its_own_next_turn_is_not_waiting |
| IdleAfterStop.test_a_real_idle_prompt_first_is_not_announced_again | COVERED | tests/features_api.rs features_api::features_an_idle_session_after_stop_is_waiting_for_you_once (a real idle_prompt first, no second raise by the dispatcher) |
| ResumeMethod.test_refuses_to_resume_a_still_running_session_into_a_window | COVERED | tests/features_api.rs features_api::features_resume_of_running_and_dead_sessions_and_new_beside_one |
| ResumeMethod.test_forking_a_running_session_is_allowed | COVERED | tests/features_api.rs features_api::features_resume_of_running_and_dead_sessions_and_new_beside_one |
| ResumeMethod.test_a_dead_session_resolves_its_cwd_from_history | COVERED | tests/features_api.rs features_api::features_resume_of_running_and_dead_sessions_and_new_beside_one |
| ResumeMethod.test_new_session_uses_the_selected_sessions_cwd | COVERED | tests/features_api.rs features_api::features_resume_of_running_and_dead_sessions_and_new_beside_one |
| ResumeMethod.test_new_and_close_launches_first_then_closes_the_old_one | COVERED | session_twin::session_twin_answers_as_python_did, step 'post new and close the old' (branch python-out/twins-c) |
| ResumeMethod.test_close_session_ends_its_process | COVERED | session_twin steps 'post close' and 'post close, stubborn' (branch python-out/twins-c; fake console sessions, never a record with this process's pid) |
| ResumeMethod.test_close_session_without_a_process_is_refused | COVERED | tests/features_api.rs features_api::features_close_and_dismiss_refuse_what_has_no_process_or_still_runs |
| ResumeMethod.test_dismissing_a_session_with_no_process_takes_it_off_the_list | COVERED | tests/features_api.rs features_api::features_close_and_dismiss_refuse_what_has_no_process_or_still_runs (a hook-only session is dismissed; its tokens and archive entry go too; the dismissal survives in state.json) |
| ResumeMethod.test_a_running_session_is_not_dismissed | COVERED | tests/features_api.rs features_api::features_close_and_dismiss_refuse_what_has_no_process_or_still_runs |
| ResumeMethod.test_a_dismissed_session_comes_back_when_it_runs_again | DIFFERS | FIXED 2026-11-15 (`c8511f4`): the mark is cleared when the row is listed with a process; features_api.rs. Was: Python cleared the dismissal when the row came back with a process (assertNotIn(SID, store.dismissed()) right after listing); the binary lists the row at once (snapshot.rs ignores the mark for a row with a pid) but clears the mark at that session's next hook event (hook.rs store.undismiss). Tested as the binary does it: tests/features_api.rs features_api::features_close_and_dismiss_refuse_what_has_no_process_or_still_runs |
| ResumeMethod.test_a_dismissed_session_comes_back_on_session_start | COVERED | tests/features_api.rs features_api::features_close_and_dismiss_refuse_what_has_no_process_or_still_runs |
| ResumeMethod.test_new_and_close_without_a_process_launches_nothing | COVERED | tests/features_api.rs features_api::features_resume_of_running_and_dead_sessions_and_new_beside_one |
| ResumeMethod.test_a_resume_error_is_reported_not_raised | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (a gone folder comes back as ok false, not an error) |
| FinishWorktreeGuard.test_an_unknown_project_is_refused_before_touching_git | COVERED | worktree_twin step "unknown project" (branch python-out/twins-b) |
| FinishWorktreeGuard.test_a_known_project_passes_through_and_logs_the_event | COVERED | worktree_twin "merge" + "events" steps (branch python-out/twins-b) |
| HttpRoutes.test_get_api_usage | COVERED | twin.rs "get /api/usage" (branch python-out/twins-b) |
| HttpRoutes.test_post_queue_delivers_then_get_queue_is_empty | COVERED | tests/features_api.rs features_api::features_queue_delivers_holds_requeues_and_sweeps |
| HttpRoutes.test_post_worktree_on_an_unknown_project_answers_200_not_ok | COVERED | worktree_twin "unknown project" (branch python-out/twins-b) |
| HttpRoutes.test_post_resume_for_an_unknown_session_answers_200_not_ok | COVERED | tests/features_api.rs features_api::features_close_and_dismiss_refuse_what_has_no_process_or_still_runs (unknown session -> 200 with ok false) |
| HttpRoutes.test_post_close_for_an_unknown_session_answers_200_not_ok | COVERED | tests/features_api.rs features_api::features_close_and_dismiss_refuse_what_has_no_process_or_still_runs (unknown session -> 200 with ok false) |
| HttpRoutes.test_pages_and_health_name_the_same_dispatcher | COVERED | tests/features_api.rs features_api::features_pages_fonts_and_matrix_are_served |
| HttpRoutes.test_post_dismiss_for_an_unknown_session_answers_200_not_ok | COVERED | twin.rs "post /api/dismiss" (branch python-out/twins-b) |
| HttpRoutes.test_post_new_without_a_project_answers_200_not_ok | COVERED | tests/features_api.rs features_api::features_resume_and_fork_launch_through_wt_and_claude (POST /api/new with no project) |
| HttpRoutes.test_bundled_fonts_are_served_and_cached | COVERED | tests/features_api.rs features_api::features_pages_fonts_and_matrix_are_served |
| HttpRoutes.test_only_fonts_in_the_folder_are_served | COVERED | tests/features_api.rs features_api::features_pages_fonts_and_matrix_are_served |
| HttpRoutes.test_the_chat_matrix_page_and_its_data | COVERED | tests/features_api.rs features_api::features_pages_fonts_and_matrix_are_served |
| HttpRoutes.test_get_api_errors | COVERED | tests/features_api.rs features_api::features_activity_falls_back_to_the_conversation_and_errors_aggregate |
| HookOutputs.test_the_documented_reply_is_a_decision_object | COVERED | hooks::tests::replies |
| HookOutputs.test_a_deny_carries_its_message | COVERED | hooks::tests::replies |
| HookOutputs.test_legacy_stays_the_default_until_switched | COVERED | hooks::tests::replies |
| HookOutputs.test_tool_payloads_lose_file_contents | COVERED | hooks::tests::tool_payloads_lose_file_contents |
| HookOutputs.test_a_failure_keeps_a_short_reason | COVERED | hooks::tests::a_failure_keeps_a_short_reason |
| ParkWait.test_a_background_session_gets_the_long_wait | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |
| ParkWait.test_an_interactive_session_gets_the_short_wait | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |
| ParkWait.test_an_unknown_session_gets_the_short_wait | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |
| ParkWait.test_an_older_hook_without_the_field_keeps_its_wait | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |
| ParkedQuestions.test_the_item_carries_the_questions | COVERED | pending::features_tests::features_the_item_carries_the_questions |
| ParkedQuestions.test_answers_come_back_with_the_allow | COVERED | pending::features_tests::features_answers_come_back_with_the_allow |
| ParkedQuestions.test_a_bare_allow_is_refused_for_a_question | COVERED | pending::features_tests::features_a_bare_allow_is_refused_for_a_question |
| ParkedQuestions.test_pass_releases_it_to_the_terminal_at_once | COVERED | pending::features_tests::features_pass_releases_it_to_the_terminal_at_once |
| ParkedQuestions.test_a_malformed_question_cannot_be_allowed | COVERED | pending::features_tests::features_a_malformed_question_cannot_be_allowed |
| ParkedQuestions.test_other_tools_are_unchanged | COVERED | pending::features_tests::features_other_tools_are_unchanged |
| ParkedQuestions.test_the_hook_sends_the_answers_as_updated_input | COVERED | hooks::tests::replies (deny never carries input) and hooks_blackbox "test_questions" (allow with answers -> updatedInput; branch rust) |
| ParkedQuestions.test_the_chat_line_shows_the_question | COVERED | conversation::features_tests::features_the_chat_line_shows_the_question |
| QuestionHook.test_answered_in_the_window | COVERED | hooks_blackbox::hooks_behave_as_the_python_hooks_did (cases "test_questions") |
| QuestionHook.test_an_allow_without_answers_says_nothing | COVERED | same |
| QuestionHook.test_declined | COVERED | same |
| QuestionHook.test_the_legacy_format_never_parks_a_question | COVERED | same |
| QuestionWait.test_an_interactive_question_gets_the_question_wait | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |
| QuestionWait.test_a_background_question_keeps_the_long_wait | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |
| QuestionWait.test_other_tools_ignore_it | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |
| QuestionWait.test_a_background_session_gets_the_long_wait | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |
| QuestionWait.test_an_interactive_session_gets_the_short_wait | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |
| QuestionWait.test_an_unknown_session_gets_the_short_wait | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |
| QuestionWait.test_an_older_hook_without_the_field_keeps_its_wait | COVERED | tests/features_api.rs features_api::features_a_parked_request_is_held_as_long_as_its_kind_of_session_allows (POST /permission/park, holds at 0.02 time scale, elapsed time compared) |


## test_finished.py

11 tests (by name): PORT-now-COVERED 11.

| Test | Verdict | Rust test or reason |
|---|---|---|
| `Finished.test_only_a_completed_background_job_without_a_process` | COVERED | new: registry.rs::finished_tests::only_a_completed_background_job_without_a_process |
| `Finished.test_when_it_last_did_anything` | COVERED | new: dispatch_finished.rs::when_it_last_did_anything (through the list: a hook event now outranks a start 30 h ago; no time at all is hidden) |
| `Finished.test_the_setting_is_read_safely` | COVERED | new: dispatch_finished.rs::the_setting_is_read_safely (config values -1, "24", true, null, [24] never hide; 24 does) |
| `Attention.test_a_stale_notification_is_cleared_for_a_finished_job` | COVERED | new: dispatch_finished.rs::a_stale_notification_is_cleared_for_a_finished_job |
| `Attention.test_even_a_failure_flag_goes_once_the_job_is_done` | COVERED | new: dispatch_finished.rs::even_a_failure_flag_goes_once_the_job_is_done |
| `Attention.test_a_blocked_job_still_asks` | COVERED | new: dispatch_finished.rs::a_blocked_job_still_asks |
| `Hiding.test_off_by_default` | COVERED | new: dispatch_finished.rs::off_by_default |
| `Hiding.test_only_old_finished_jobs_are_hidden` | COVERED | new: dispatch_finished.rs::only_old_finished_jobs_are_hidden |
| `Hiding.test_it_is_a_setting_applied_at_once` | COVERED | new: dispatch_finished.rs::it_is_a_setting_applied_at_once |
| `Window.test_a_tile_with_no_process_can_be_taken_off_the_list` | COVERED | new: dispatch_finished.rs::a_tile_with_no_process_can_be_taken_off_the_list (fetches /ui/app.js; same string checks) |
| `Window.test_a_finished_job_reads_finished` | COVERED | new: dispatch_finished.rs::a_finished_job_reads_finished (fetches /ui/shared.js) |


## test_hotkey.py (16 tests)

Summary: 6 COVERED, 10 PORT-now-COVERED, 0 MOOT, 0 DIFFERS. The registration loop (`run`, broken-config file read in `main`) needs a real RegisterHotKey and is not exercised; `candidates(Null)` covers its fallback.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Parsing.test_modifiers_combine | PORT-now-COVERED | `hotkey::python_tests::modifiers_combine` |
| Parsing.test_function_keys | COVERED | `hotkey::tests::parses_like_python` (f12); f1 also in `python_tests::function_keys_count_from_vk_f1` |
| Parsing.test_letters_become_virtual_keys | COVERED | `parses_like_python` ("Win+K" -> 'K') |
| Parsing.test_backquote_by_name_or_symbol | PORT-now-COVERED | `hotkey::python_tests::backquote_by_name_or_symbol` |
| Parsing.test_win_modifier | COVERED | `parses_like_python` |
| Parsing.test_nonsense_is_refused_rather_than_silently_bound | PORT-now-COVERED | `hotkey::python_tests::nonsense_is_refused_rather_than_silently_bound` |
| Parsing.test_every_default_candidate_parses | PORT-now-COVERED | `hotkey::python_tests::every_default_candidate_parses` |
| Candidates.test_an_explicit_override_wins | COVERED | `candidate_order` (Some("x+y") -> 1 candidate; exact value PORTed too) |
| Candidates.test_a_single_string_becomes_one_candidate | COVERED | `candidate_order` |
| Candidates.test_a_list_is_tried_in_order | COVERED | `candidate_order` |
| Candidates.test_missing_or_broken_config_falls_back_to_defaults | PORT-now-COVERED | `hotkey::python_tests::missing_config_falls_back_to_defaults` (Null = no/unparseable file; the file read itself is in `main`) |
| Candidates.test_an_empty_list_falls_back_rather_than_binding_nothing | PORT-now-COVERED | `hotkey::python_tests::an_empty_list_falls_back_rather_than_binding_nothing` |
| WindowLookup.test_find_window_returns_a_handle_or_none | PORT-now-COVERED | `hotkey::python_tests::window_lookup_returns_what_it_found_or_nothing` |
| WindowLookup.test_only_the_app_window_matches | PORT-now-COVERED | `hotkey::python_tests::only_the_app_window_matches` |
| WindowLookup.test_the_waiting_count_in_the_title_still_matches | PORT-now-COVERED | `hotkey::python_tests::the_waiting_count_in_the_title_still_matches` |
| WindowLookup.test_the_title_it_looks_for_matches_the_page | PORT-now-COVERED | `hotkey::python_tests::the_title_it_looks_for_matches_the_page` (the shell page) |


## test_impctl.py (20 tests)

Summary: COVERED 17 (6 existing + 11 ported), MOOT 2, DIFFERS 1 (the sub-case that can be tested is covered). New tests: tests/impctl_cli.rs (12), cli.rs `impctl_cov`, helpers tests/common/setup.rs

The Rust CLI (src/cli.rs) is black-boxed here against a recording fake dispatcher (tests/common FakeServer) via IMPERATORIUM_URL.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Help.test_every_command_is_listed_in_help_and_every_listed_one_exists | COVERED | cli.rs `every_command_is_in_the_help`, `every_subcommand_of_main_is_listed` |
| Help.test_help_prints_each_command_with_its_parameters | COVERED | tests/impctl_cli.rs `impctl_help_prints_each_command_with_its_parameters` |
| Help.test_no_arguments_prints_help_too | COVERED | tests/impctl_cli.rs `impctl_no_arguments_print_the_help_too` |
| Help.test_an_unknown_help_topic_is_an_error_not_a_traceback | COVERED | tests/impctl_cli.rs `impctl_an_unknown_help_topic_is_an_error_not_a_crash` |
| Passthrough.test_leading_flags_reach_the_script | MOOT | Python impctl handed argv to install.py; the binary dispatches in-process (install flags: cli.rs `arguments_read_as_argparse_does`, install.rs `opts`) |
| Passthrough.test_remote_keeps_its_subcommand_and_flags | MOOT | same; `remote add ...` is handled in-binary (remote_cli.rs tests) |
| Passthrough.test_unknown_flags_on_other_commands_are_still_refused | COVERED | cli.rs `arguments_read_as_argparse_does`; black-box exit 2 in tests/impctl_cli.rs `impctl_unknown_flags_are_refused` |
| Uninstall.test_a_no_runs_nothing | DIFFERS | FIXED 2026-11-15 (`206df1d`): top-level uninstall asks first; impctl_cli.rs `uninstall_without_yes_or_a_terminal_removes_nothing`. Was: `service uninstall` without --yes and without a terminal refuses and removes nothing (exit 1): tests/impctl_cli.rs `impctl_service_uninstall_without_yes_or_a_terminal_removes_nothing`. But Python's `impctl uninstall` also asked; the binary's top-level `uninstall` (install.rs uninstall_main, main.rs) does NOT ask: it edits settings.json and removes the tasks at once. Not exercised (unsafe), so no ignored test |
| Uninstall.test_yes_skips_the_question | COVERED | cli.rs `impctl_cov::yes_skips_the_question` |
| Uninstall.test_install_does_not_ask | COVERED | tests/impctl_cli.rs `impctl_service_install_does_not_ask` |
| FindSession.test_by_exact_name | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` |
| FindSession.test_by_id_prefix | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` |
| FindSession.test_by_window_title | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` |
| FindSession.test_ambiguous_names_both | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` ('alph' lists both) |
| FindSession.test_resume_falls_back_to_a_past_session_by_id_prefix | COVERED | tests/impctl_cli.rs `impctl_resume_falls_back_to_a_past_session_by_id_prefix` |
| FindSession.test_a_past_session_that_does_not_match_the_prefix_is_not_taken | COVERED | tests/impctl_cli.rs `impctl_a_past_session_that_does_not_match_the_prefix_is_not_taken` |
| Send.test_to_a_session_injects_by_its_full_id | COVERED | tests/impctl_cli.rs `impctl_send_to_a_session_injects_by_its_full_id` |
| Send.test_needs_exactly_one_target | COVERED | tests/impctl_cli.rs `impctl_send_needs_exactly_one_target` |
| Send.test_a_refusal_exits_non_zero | COVERED | tests/impctl_cli.rs `impctl_a_refusal_exits_non_zero` |
| Down.test_a_dead_dispatcher_says_how_to_start_it | COVERED | tests/impctl_cli.rs `impctl_a_dead_dispatcher_says_how_to_start_it` |


## test_matrix.py (23 tests)

Summary: 0 COVERED, 23 PORT-now-COVERED, 0 MOOT, 0 DIFFERS (one partial: the tile `error` text cannot be induced, see its row). 14 unit tests in `matrix.rs`, 10 integration tests in `tests/matrix_api.rs`. The file defines two classes named `Spend`; the first is shadowed in Python and never ran, but is ported.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Focus.test_each_step_shows_the_replies_and_only_the_step_in_flight | PORT-now-COVERED | `matrix::tests::each_step_shows_the_replies_and_only_the_step_in_flight` |
| Focus.test_a_tool_shows_its_name_and_one_line | PORT-now-COVERED | `matrix::tests::a_tool_shows_its_name_and_one_line` |
| Focus.test_the_prompt_and_results_are_not_shown | PORT-now-COVERED | `matrix::tests::the_prompt_and_results_are_not_shown` |
| Focus.test_a_thought_is_not_live_once_the_turn_is_over | PORT-now-COVERED | `matrix::tests::a_thought_is_not_live_once_the_turn_is_over` |
| Focus.test_nothing_is_in_flight_in_a_session_that_is_not | PORT-now-COVERED | `matrix::tests::nothing_is_in_flight_in_a_session_that_is_not` |
| Focus.test_a_reply_or_a_prompt_closes_calls_that_never_got_a_result | PORT-now-COVERED | `matrix::tests::a_reply_or_a_prompt_closes_calls_that_never_got_a_result` |
| Focus.test_parallel_calls_show_the_newest_and_count_the_rest | PORT-now-COVERED | `matrix::tests::parallel_calls_show_the_newest_and_count_the_rest` |
| Focus.test_replies_carry_their_turn | PORT-now-COVERED | `matrix::tests::replies_carry_their_turn` |
| Focus.test_only_the_newest_replies_are_kept_and_long_ones_cut | PORT-now-COVERED | `matrix::tests::only_the_newest_replies_are_kept_and_long_ones_cut` (fixed 12 kept; Python parameterised `keep`) |
| Focus.test_a_result_that_came_back_with_text_still_ends_its_call | PORT-now-COVERED | `matrix::tests::a_result_that_came_back_with_text_still_ends_its_call` |
| Focus.test_system_notes_are_not_replies | PORT-now-COVERED | `matrix::tests::system_notes_are_not_replies` |
| Active.test_running_sessions_get_a_tile_and_ended_ones_do_not | PORT-now-COVERED | `matrix_api.rs::running_sessions_get_a_tile_and_ended_ones_do_not` (agents stub rows: running, state done, no process) |
| Spend(unit).test_totals_and_each_session_most_costly_first | PORT-now-COVERED | `matrix::tests::spend_totals_and_each_session_most_costly_first` |
| Spend.test_a_subscription_counts_its_windows_and_open_sessions_their_whole_life | PORT-now-COVERED | `matrix_api.rs::a_subscription_counts_its_windows_and_open_sessions_their_whole_life` |
| Spend.test_a_window_waiting_after_its_reset_counts_from_the_reset | PORT-now-COVERED | `matrix_api.rs::a_window_waiting_after_its_reset_counts_from_the_reset` |
| Spend.test_a_closed_session_counts_globally_but_not_as_current | PORT-now-COVERED | `matrix_api.rs::a_closed_session_counts_globally_but_not_as_current` |
| Spend.test_an_account_without_windows_counts_the_month | PORT-now-COVERED | `matrix_api.rs::an_account_without_windows_counts_the_month` (month start checked as local midnight within 32 days; exact value needs the local zone) |
| Spend.test_the_meters_come_back_after_a_restart | PORT-now-COVERED | `matrix::tests::the_meters_come_back_after_a_restart` |
| Spend.test_usage_carries_the_spend | PORT-now-COVERED | `matrix_api.rs::usage_carries_the_spend` |
| Method.test_a_running_session_becomes_a_tile | PORT-now-COVERED | `matrix_api.rs::a_running_session_becomes_a_tile` |
| Method.test_an_unreadable_transcript_still_gets_its_tile | PORT-now-COVERED | `matrix_api.rs::an_unreadable_transcript_still_gets_its_tile` (tile and empty replies only: the binary reads an unopenable transcript as none, so `error` is not set; it is set only when the reader returns Err, which no input induces) |
| Method.test_no_sessions_no_tiles | PORT-now-COVERED | `matrix_api.rs::no_sessions_no_tiles` |
| Method.test_the_status_tile_is_always_shown_unless_turned_off | PORT-now-COVERED | `matrix_api.rs::the_status_tile_is_always_shown_unless_turned_off` |


## test_modlink.py (20 tests)

Summary: 2 COVERED, 18 PORT-now-COVERED, 0 MOOT, 0 DIFFERS. 5 unit tests in `modlink.rs` (`python_tests`), 9 integration tests in `tests/modlink_dispatch.rs` (Rig on a temp home), 2 in `tests/modlink_install.rs`.

| Test | Verdict | Rust test or reason |
|---|---|---|
| LinkTests.test_the_token_is_made_once_and_kept | PORT-now-COVERED | `modlink::python_tests::the_token_is_made_once_and_kept` |
| LinkTests.test_a_beat_makes_a_mod_live_until_it_stops | PORT-now-COVERED | `modlink::python_tests::a_beat_makes_a_mod_live_until_it_stops` |
| LinkTests.test_a_prompt_is_handed_out_once_accepted | COVERED | `modlink::tests::a_prompt_is_handed_out_once_then_settles`; also `python_tests::a_prompt_is_handed_out_once_accepted` |
| LinkTests.test_a_dropped_prompt_says_why | PORT-now-COVERED | `modlink::python_tests::a_dropped_prompt_says_why` |
| LinkTests.test_usage_becomes_a_statusline_payload | PORT-now-COVERED | `modlink::python_tests::usage_becomes_a_statusline_payload` |
| ControlTests.test_a_live_mod_takes_the_prompt_instead_of_the_socket | PORT-now-COVERED | `modlink_dispatch.rs::a_live_mod_takes_the_prompt_instead_of_the_socket` |
| ControlTests.test_only_the_window_speaks_as_the_person | PORT-now-COVERED | `modlink_dispatch.rs::only_the_window_speaks_as_the_person` |
| ControlTests.test_without_a_mod_the_socket_is_used_as_before | PORT-now-COVERED | `modlink_dispatch.rs::without_a_mod_the_socket_is_used_as_before` |
| ControlTests.test_a_live_mod_makes_a_session_reachable_and_says_busy | PORT-now-COVERED | `modlink_dispatch.rs::a_live_mod_makes_a_session_reachable_and_says_busy` |
| ControlTests.test_the_queue_remembers_who_queued | PORT-now-COVERED | `modlink_dispatch.rs::the_queue_remembers_who_queued` |
| ControlTests.test_usage_from_a_beat_reaches_the_meters | PORT-now-COVERED | `modlink_dispatch.rs::usage_from_a_beat_reaches_the_meters` |
| ControlTests.test_the_status_line_names_who_is_waiting | PORT-now-COVERED | `modlink_dispatch.rs::the_status_line_names_who_is_waiting` |
| ControlTests.test_the_policy_is_asked_only_once_auto_approval_is_on | PORT-now-COVERED | `modlink_dispatch.rs::the_policy_is_asked_only_once_auto_approval_is_on` |
| RouteTests.test_a_beat_needs_the_token | PORT-now-COVERED | `modlink_dispatch.rs::a_beat_needs_the_token` |
| RouteTests.test_a_check_needs_the_token | PORT-now-COVERED | `modlink_dispatch.rs::a_check_needs_the_token` |
| RouteTests.test_a_dispatch_without_the_window_token_is_framed | PORT-now-COVERED | `modlink_dispatch.rs::only_the_window_speaks_as_the_person` (plain API caller framed) |
| RouteTests.test_a_dispatch_from_the_window_speaks_as_the_person | PORT-now-COVERED | `modlink_dispatch.rs::a_live_mod_takes_the_prompt_instead_of_the_socket` (as_user true from the window) and `a_dispatch_from_the_window_speaks_as_the_person_and_one_without_its_token_does_not` |
| InstallTests.test_added_once_beside_what_is_there | COVERED | `install::tests::mod_dirs_are_kept` and the install_compare golden ("plugin dirs" x `--mod`, once and twice) |
| InstallTests.test_uninstall_removes_only_ours | PORT-now-COVERED | `modlink_install.rs::uninstall_removes_only_ours` (CLI, temp settings) |
| InstallTests.test_a_strange_env_is_left_alone | PORT-now-COVERED | `modlink_install.rs::a_strange_env_is_left_alone` (CLI prints SKIP, env untouched) |


## test_native.py

Summary: 9 tests - 4 COVERED already, 2 PORT now COVERED by new Rust tests, 3 MOOT, 0 DIFFERS

| Test | Verdict | Rust test or reason |
|---|---|---|
| NativeHooks.test_every_hook_runs_the_binary | COVERED | install_compare.rs `install_hooks_writes_what_install_py_wrote` (golden install_compare: settings.json from `install.py --native`, byte for byte, every event, args, async, permission --home) + install::tests::hooks_replace_ours_and_keep_others |
| NativeHooks.test_switching_replaces_the_python_hooks | COVERED | install_compare.rs (start 'python install' re-installed natively: replaced, one block per event). The way back to Python hooks is MOOT: the binary has no Python mode |
| NativeHooks.test_uninstall_finds_native_hooks | COVERED | install_compare.rs (variant --uninstall on 'someone else's hooks' and after install) |
| NativeHooks.test_native_statusline_is_recognised_as_ours | COVERED | install_compare.rs (--statusline variants, 'foreign statusline' start) + install::tests::statusline_refuses_a_stranger |
| NativeReader.test_the_chain_is_classified_as_before | COVERED | new: conversation::core_tests::the_chain_is_classified |
| NativeReader.test_a_failing_binary_is_an_error_not_an_empty_chat | COVERED | new: tests/core_convcache.rs `an_unreadable_transcript_is_an_error_not_an_empty_chat` |
| NativeReader.test_titles_take_one_call | MOOT | Python wrapper plumbing (conversation.titles calling native.session_titles once). The binary has `sessions titles <id>...` itself: corpus.rs `synthetic_corpus_reads_as_python_reads_it` compares it |
| NativeReader.test_history_lists_through_the_binary | MOOT | Python history.py calling the binary; the binary's own `sessions list` is compared by corpus.rs (golden sessions_compare) |
| NativeReader.test_the_binary_is_looked_for_where_it_is_installed | MOOT | Python native.candidates() lookup of the binary; the binary does not look for itself |


## test_policy.py

Summary: 44 tests - 44 COVERED

Also: policy.rs unit tests (disabled_asks, safe_and_unsafe_shell, self_protection_denies, errors_ask) and the 86,197-case differential (tests/differential.rs) check the same rules, with Python's recorded answers.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Defaults.test_disabled_policy_answers_nothing | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| Defaults.test_no_policy_at_all_answers_nothing | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| Defaults.test_an_unknown_tool_is_asked_about | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| Defaults.test_a_malformed_request_never_raises | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| Defaults.test_project_can_opt_out_entirely | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyShell.test_the_common_safe_ones_are_allowed | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyShell.test_leading_env_assignment_is_tolerated | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyShell.test_chained_safe_commands_are_allowed | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyShell.test_one_bad_link_poisons_the_whole_chain | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyShell.test_a_safe_verb_must_start_the_segment | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyShell.test_command_substitution_is_never_allowed | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyShell.test_redirection_is_never_allowed | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyShell.test_unknown_commands_are_asked_not_guessed | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyVerbsWithWritingOptions.test_find_that_deletes_runs_or_writes | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyVerbsWithWritingOptions.test_git_branch_only_lists | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyVerbsWithWritingOptions.test_git_remote_only_lists | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyVerbsWithWritingOptions.test_output_to_a_file | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyVerbsWithWritingOptions.test_programs_run_by_a_reader | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadOnlyVerbsWithWritingOptions.test_formatting_is_an_edit | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| NeverAutoApproved.test_destructive_and_irreversible_commands | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| NeverAutoApproved.test_privilege_and_machine_level | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| NeverAutoApproved.test_installs_and_publishes | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| NeverAutoApproved.test_download_piped_into_a_shell | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| NeverAutoApproved.test_scheduled_tasks_and_registry | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| NeverAutoApproved.test_a_project_cannot_mark_these_safe | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| SelfProtection.test_editing_the_controls_is_refused | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| SelfProtection.test_disabling_permission_checks_is_refused | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| SelfProtection.test_reaching_the_controls_through_a_shell_is_refused | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| Secrets.test_secrets_are_asked_about_for_every_tool | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| Secrets.test_ssh_and_cloud_credential_directories | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| Secrets.test_a_command_naming_a_secret_is_asked_about | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| Secrets.test_a_secret_is_not_auto_approved_by_a_safe_verb | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| Secrets.test_is_sensitive_handles_both_separators_and_junk | COVERED | src/policy.rs `policy::py_ported::is_sensitive_handles_both_separators_and_junk` (a direct call, not a `decide` call, so the golden does not have it) |
| ReadsAndEdits.test_reads_inside_the_project_are_allowed | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadsAndEdits.test_reads_outside_the_project_are_asked_about | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadsAndEdits.test_reads_outside_can_be_opted_into | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadsAndEdits.test_pathless_read_tools_are_allowed | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadsAndEdits.test_edits_need_an_explicit_opt_in | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadsAndEdits.test_edits_outside_the_project_are_asked_about_even_when_opted_in | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadsAndEdits.test_a_multi_edit_is_only_as_safe_as_its_worst_path | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| ReadsAndEdits.test_traversal_does_not_escape_the_project | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| PerProject.test_the_most_specific_project_entry_wins | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| PerProject.test_a_project_can_add_its_own_safe_command | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |
| PerProject.test_settings_do_not_leak_between_projects | COVERED | tests/policy_tests.rs `test_policy_calls_answer_as_python_did` (golden policy_tests/calls.jsonl: every `decide` call this test made, exact verdict, reason and tier) |


## test_public.py

4 tests (by name): PORT-now-COVERED 4.

| Test | Verdict | Rust test or reason |
|---|---|---|
| `NoTokens.test_the_session_list_has_no_tokens` | COVERED | new: dispatch_public.rs::the_session_list_has_no_tokens |
| `NoTokens.test_the_stream_has_no_tokens` | COVERED | new: dispatch_public.rs::the_stream_has_no_tokens |
| `NoTokens.test_the_dispatcher_keeps_its_own_copy` | COVERED | new: dispatch_public.rs::the_dispatcher_keeps_its_own_copy |
| `NoTokens.test_public_snapshot_copies` | COVERED | new: dispatch_public.rs::public_snapshot_copies |


## test_quick.py

Summary: 13 tests - 5 COVERED already, 8 PORT now COVERED by new Rust tests, 0 MOOT, 0 DIFFERS

| Test | Verdict | Rust test or reason |
|---|---|---|
| KeySetting.test_a_combination_is_stored_in_the_pages_order | COVERED | pure_checks.rs `settings_check_answers_as_python_did` (golden settings_compare: 'Ctrl + Alt + J' -> ctrl+alt+j, 'win+ctrl+x' -> ctrl+win+x, alt+shift+x) + settings::quick_tests::a_combination_is_stored_in_the_pages_order |
| KeySetting.test_shift_alone_or_no_modifier_is_refused | COVERED | new: settings::quick_tests::shift_alone_or_no_modifier_is_refused |
| KeySetting.test_a_key_the_prompt_box_already_uses_is_refused | COVERED | pure_checks.rs `settings_check_answers_as_python_did` (golden: ctrl+enter, ctrl+k, alt+1, alt+9 'is already ...') + settings::quick_tests::reserved_keys_are_refused |
| PhrasesSetting.test_one_per_line_and_commas_are_text | COVERED | pure_checks.rs `settings_check_answers_as_python_did` (golden: 'a,b\nc' -> ['a,b','c']) |
| PhrasesSetting.test_a_list_from_the_form | COVERED | pure_checks.rs `settings_check_answers_as_python_did` (golden: [' a ','','b'] -> ['a','b']) |
| PhrasesSetting.test_limits | COVERED | new: settings::quick_tests::phrase_limits |
| PhrasesSetting.test_defaults_include_the_common_ones | COVERED | new: settings::quick_tests::defaults_include_the_common_ones |
| Shortcuts.test_a_key_named_by_a_setting_is_a_real_setting | COVERED | new: settings::quick_tests::a_key_named_by_a_setting_is_a_real_setting |
| Shortcuts.test_every_reserved_key_is_on_the_list | COVERED | new: settings::quick_tests::every_reserved_key_is_on_the_list |
| Live.test_the_page_gets_the_configured_phrases | COVERED | new: tests/core_quick.rs `the_page_gets_the_configured_phrases` |
| Live.test_a_saved_change_applies_at_once | COVERED | tests/settings_twin.rs `settings_saves_answer_and_write_as_python_did` (branch python-out/twins-a): save quick.key 'Shift + Ctrl + Q' and quick.phrases, then GET /api/quick as recorded |
| Live.test_the_settings_window_gets_the_shortcut_list | COVERED | new: tests/core_quick.rs `the_settings_window_gets_the_shortcut_list` |
| ComboOf.test_matches_what_the_setting_stores | COVERED | new: tests/core_quick.rs `combo_of_matches_what_the_setting_stores (node runs web/shared.js comboOf; skipped without node)` |


## test_remote.py (62 tests)

Summary: 4 COVERED, 53 PORT-now-COVERED, 5 MOOT, 0 DIFFERS. 53 tests in `remote::ssh_tests` (src/remote.rs) against a fake `ssh` (`examples/fakessh.rs`, IMPERATORIUM_SSH; scripted by rules.json, logs every call) and, for the channels, the real `imperatorium remote-channel` run as if on the host over temp transcripts. Nothing reaches a real host.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Config.test_a_bare_string_is_a_host | COVERED | `remote::tests::loads_hosts_like_python` ("b", default claude path) |
| Config.test_disabled_means_no_hosts | COVERED | `loads_hosts_like_python` (enabled:false) |
| Config.test_entries_without_a_name_are_skipped | COVERED | `loads_hosts_like_python` (label-only entry dropped) |
| Config.test_no_remote_block_is_not_an_error | PORT-now-COVERED | `remote::ssh_tests::no_remote_block_is_not_an_error` |
| Polling.test_claude_is_called_by_absolute_path | PORT-now-COVERED | `remote::ssh_tests::claude_is_called_by_absolute_path` |
| Polling.test_ssh_is_bounded_and_never_prompts | PORT-now-COVERED | `remote::ssh_tests::ssh_is_bounded_and_never_prompts` |
| Polling.test_rows_are_tagged_with_their_host | PORT-now-COVERED | `remote::ssh_tests::rows_are_tagged_with_their_host` |
| Polling.test_a_remote_session_is_never_marked_dispatchable_by_the_poll | PORT-now-COVERED | `remote::ssh_tests::a_remote_session_is_never_marked_dispatchable_by_the_poll` |
| Polling.test_a_missing_binary_is_reported_with_its_reason | PORT-now-COVERED | `remote::ssh_tests::a_missing_binary_is_reported_with_its_reason` |
| Polling.test_an_old_claude_without_json_is_reported | PORT-now-COVERED | `remote::ssh_tests::an_old_claude_without_json_is_reported` |
| Polling.test_non_json_output_is_refused | PORT-now-COVERED | `remote::ssh_tests::non_json_output_is_refused` |
| Polling.test_a_timeout_becomes_a_remote_error | PORT-now-COVERED | `remote::ssh_tests::a_timeout_becomes_an_error` (through `run`, whose bound `agents` uses) |
| Dispatch.test_the_interpreter_path_is_left_unquoted_so_HOME_expands | PORT-now-COVERED | `remote::ssh_tests::the_interpreter_path_is_left_unquoted_so_home_expands` |
| Dispatch.test_the_prompt_is_quoted_so_spaces_and_quotes_survive | PORT-now-COVERED | `remote::ssh_tests::the_prompt_is_quoted_so_spaces_and_quotes_survive` |
| Dispatch.test_the_working_directory_is_quoted | PORT-now-COVERED | `remote::ssh_tests::the_working_directory_is_quoted` |
| Dispatch.test_the_session_id_comes_back | PORT-now-COVERED | `remote::ssh_tests::the_session_id_comes_back` |
| Dispatch.test_an_empty_prompt_never_reaches_the_wire | PORT-now-COVERED | `remote::ssh_tests::an_empty_prompt_never_reaches_the_wire` |
| Dispatch.test_a_failure_is_reported_rather_than_claimed_as_success | PORT-now-COVERED | `remote::ssh_tests::a_failure_is_reported_rather_than_claimed_as_success` |
| Dispatch.test_a_start_with_no_id_is_not_silently_retried | PORT-now-COVERED | `remote::ssh_tests::a_start_with_no_id_is_not_silently_retried` |
| Backoff.test_a_failing_host_is_parked_and_skipped | PORT-now-COVERED | `remote::ssh_tests::a_failing_host_is_parked_and_skipped` (the skip is `due`, which the poll loop checks) |
| Backoff.test_the_delay_grows_with_each_failure | PORT-now-COVERED | `remote::ssh_tests::the_delay_grows_with_each_failure` |
| Backoff.test_a_parked_host_becomes_due_again_once_the_delay_passes | PORT-now-COVERED | `remote::ssh_tests::a_parked_host_becomes_due_again_once_the_delay_passes` |
| Backoff.test_the_delay_is_capped | PORT-now-COVERED | `remote::ssh_tests::the_delay_is_capped` |
| Backoff.test_success_clears_the_park | PORT-now-COVERED | `remote::ssh_tests::success_clears_the_park` |
| Backoff.test_one_dead_host_does_not_hide_a_working_one | PORT-now-COVERED | `remote::ssh_tests::one_dead_host_does_not_hide_a_working_one` |
| Backoff.test_poll_never_raises | PORT-now-COVERED | `remote::ssh_tests::poll_never_raises` |
| SessionFiles.test_status_and_inbox_come_from_the_hosts_session_files | PORT-now-COVERED | `remote::ssh_tests::status_and_inbox_come_from_the_hosts_session_files` |
| SessionFiles.test_a_session_file_without_a_live_process_is_not_a_session | PORT-now-COVERED | `remote::ssh_tests::a_session_file_without_a_live_process_is_not_a_session` |
| SessionFiles.test_the_same_call_reads_the_files | PORT-now-COVERED | `remote::ssh_tests::the_same_call_reads_the_files` |
| SessionFiles.test_no_session_files_is_not_an_error | PORT-now-COVERED | `remote::ssh_tests::no_session_files_is_not_an_error` |
| SessionFiles.test_a_torn_session_file_is_skipped | PORT-now-COVERED | `remote::ssh_tests::a_torn_session_file_is_skipped` |
| Inject.test_the_token_goes_on_stdin_never_the_command_line | PORT-now-COVERED | `remote::ssh_tests::the_token_goes_on_stdin_never_the_command_line` |
| Inject.test_the_socket_path_is_quoted | PORT-now-COVERED | `remote::ssh_tests::the_socket_path_is_quoted` |
| Inject.test_it_runs_on_the_hosts_python | MOOT | no host Python in the binary: the host runs `imperatorium inbox-send` (`old_python_fields_are_accepted_and_ignored` covers the config keys) |
| Inject.test_the_writer_actually_writes_to_a_socket | MOOT | the INJECT_SCRIPT is gone; `inbox-send` is the host side and Unix-only (Python skipped this test on Windows too) |
| Inject.test_a_windows_pipe_path_is_refused | PORT-now-COVERED | `remote::ssh_tests::a_windows_pipe_path_is_refused` |
| Inject.test_no_token_is_refused_before_the_wire | PORT-now-COVERED | `remote::ssh_tests::no_token_is_refused_before_the_wire` |
| Inject.test_an_empty_prompt_is_refused_before_the_wire | PORT-now-COVERED | `remote::ssh_tests::an_empty_prompt_is_refused_before_the_wire` |
| Inject.test_the_hosts_own_error_is_what_is_reported | PORT-now-COVERED | `remote::ssh_tests::the_hosts_own_error_is_what_is_reported` |
| Tunnels.test_tunnel_true_gets_a_port_of_its_own | PORT-now-COVERED | `remote::ssh_tests::tunnel_true_gets_a_port_of_its_own` |
| Tunnels.test_an_explicit_port_is_kept_and_not_reused | COVERED | `loads_hosts_like_python` (a -> 8794, explicit c -> 8793) |
| Tunnels.test_the_tunnel_never_lands_on_the_dispatchers_own_port | PORT-now-COVERED | `remote::ssh_tests::the_tunnel_never_lands_on_the_dispatchers_own_port` |
| Tunnels.test_a_tunnel_that_cannot_forward_exits_rather_than_idling | PORT-now-COVERED | `remote::ssh_tests::a_tunnel_that_cannot_forward_exits_rather_than_idling` |
| Tunnels.test_a_dropped_tunnel_backs_off | PORT-now-COVERED | `remote::ssh_tests::a_dropped_tunnel_backs_off` |
| ReaderChannel.test_items_come_back_rendered_by_the_same_code | PORT-now-COVERED | `remote::ssh_tests::items_come_back_rendered_by_the_same_code` |
| ReaderChannel.test_one_channel_serves_many_reads | PORT-now-COVERED | `remote::ssh_tests::one_channel_serves_many_reads` |
| ReaderChannel.test_a_failed_read_is_reported_and_the_channel_survives | PORT-now-COVERED | `remote::ssh_tests::a_failed_read_is_reported_and_the_channel_survives` |
| ReaderChannel.test_a_hung_read_kills_the_channel_rather_than_waiting | PORT-now-COVERED | `remote::ssh_tests::a_hung_read_kills_the_channel_rather_than_waiting` |
| ReaderChannel.test_a_dead_host_is_not_redialled_on_every_poll | PORT-now-COVERED | `remote::ssh_tests::a_dead_host_is_not_redialled_on_every_poll` |
| ReaderChannel.test_a_channel_that_exits_reports_why | PORT-now-COVERED | `remote::ssh_tests::a_channel_that_exits_reports_why` |
| ReaderChannel.test_the_interpreter_path_is_left_to_the_hosts_shell | PORT-now-COVERED | `remote::ssh_tests::the_binary_path_is_left_to_the_hosts_shell` |
| ReaderChannel.test_a_host_set_up_before_the_rename_keeps_its_reader | MOOT | venv-folder fallback (ccontrol vs imperatorium); the binary is pushed to one fixed path |
| PerHost.test_a_slow_host_does_not_hold_up_another | PORT-now-COVERED | `remote::ssh_tests::a_slow_host_does_not_hold_up_another` |
| PerHost.test_each_poll_that_lands_says_so | PORT-now-COVERED | `remote::ssh_tests::each_poll_that_lands_says_so` |
| PerHost.test_a_host_that_starts_failing_leaves_the_board | PORT-now-COVERED | `remote::ssh_tests::a_host_that_starts_failing_leaves_the_board` |
| PerHost.test_refresh_asks_a_parked_host_now | PORT-now-COVERED | `remote::ssh_tests::refresh_asks_a_parked_host_now` |
| StatsOnRows.test_each_row_carries_its_hosts_numbers | PORT-now-COVERED | `remote::ssh_tests::each_row_carries_its_hosts_numbers` |
| StatsOnRows.test_rows_stand_without_them | PORT-now-COVERED | `remote::ssh_tests::rows_stand_without_them` |
| StatsOnRows.test_stats_can_be_left_off | MOOT | `Fleet(stats=False)` option does not exist in the binary (stats are always attached, best-effort) - "not in binary" |
| StatsChannel.test_the_cache_and_context_come_back_read_on_the_host | PORT-now-COVERED | `remote::ssh_tests::the_cache_and_context_come_back_read_on_the_host` |
| StatsChannel.test_one_channel_serves_every_poll | PORT-now-COVERED | `remote::ssh_tests::one_channel_serves_every_stats_poll` |
| StatsChannel.test_it_runs_on_the_hosts_plain_python | MOOT | no host Python; the binary runs there |


## test_rename.py

Summary: 12 tests - 2 COVERED already, 7 PORT now COVERED by new Rust tests, 0 MOOT, 3 DIFFERS

| Test | Verdict | Rust test or reason |
|---|---|---|
| StateFolder.test_finding_the_old_folder_does_not_move_it | COVERED | new: tests/core_rename.rs `the_old_state_folder_is_found_and_left_in_place` |
| StateFolder.test_the_old_folder_is_moved_on_first_start | DIFFERS | WON'T FIX 2026-11-15 (user decision: a clean break with the Python version; old installs uninstall with their own version first). Was: the binary never moves ccontrol to imperatorium (no rename anywhere in src; paths::state_dir only prefers the old folder while the new one is absent). The Python dispatcher's migrate_state_dir has no counterpart. Recorded as the #[ignore]d tests/core_rename.rs `the_old_folder_is_moved_on_first_start` |
| StateFolder.test_a_folder_that_cannot_move_goes_on_being_used | COVERED | new: tests/core_rename.rs `the_old_state_folder_is_found_and_left_in_place (the move failing cannot happen: the binary never moves; the second half, the permission hook writing beside the old folder, is exactly what this test checks)` |
| StateFolder.test_a_new_install_uses_the_new_name | COVERED | new: tests/core_rename.rs `a_new_install_uses_the_new_name` |
| StateFolder.test_once_moved_the_old_one_is_ignored | COVERED | new: tests/core_rename.rs `once_the_new_folder_exists_the_old_one_is_ignored` |
| Hooks.test_an_entry_stamped_before_the_rename_is_still_ours | COVERED | install_compare.rs (start 'legacy ccontrol', stamped statusMessage 'ccontrol': replaced, not duplicated) |
| Hooks.test_anyone_elses_is_not | COVERED | install_compare.rs (start 'someone else's hooks' kept through install and uninstall) + install::tests::hooks_replace_ours_and_keep_others |
| Hooks.test_a_statusline_from_before_the_rename_still_reports | COVERED | new: usage::core_tests::provider_of_reads_either_name; tests/core_rename.rs ccontrol_statusline_event_is_ingested |
| Policy.test_the_policy_code_is_protected_under_either_name | COVERED | new: policy::core_tests::the_policy_code_is_protected_under_either_name |
| Units.test_the_old_unit_names_are_known_so_install_can_remove_them | DIFFERS | WON'T FIX 2026-11-15 (user decision: a clean break with the Python version; old installs uninstall with their own version first). Was: the binary's service.rs knows only the Imperatorium names (service::tests::names_per_backend); it does not list or remove the pre-rename ccontrol-* units / local.ccontrol.* agents, so an install from before the rename leaves them behind |
| Units.test_a_host_from_before_the_rename_is_noticed | COVERED | new: remote_cli::core_tests::a_host_from_before_the_rename_is_noticed |
| Tasks.test_install_and_uninstall_remove_the_old_task_names | DIFFERS | WON'T FIX 2026-11-15 (user decision: a clean break with the Python version; old installs uninstall with their own version first). Was: service.rs / service_ctl.rs have no CentralControl* legacy names (grep finds none); the binary's install and uninstall do not remove the old scheduled tasks. service/tasks.ps1, the file the Python test reads, still does |


## test_sessionstat.py

Summary: 21 tests - 21 COVERED

Also replayed whole-corpus against Python by tests/corpus.rs (golden stats_compare, at the clocks in clocks.json).

| Test | Verdict | Rust test or reason |
|---|---|---|
| Background.test_a_backgrounded_command_runs_until_its_notification | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_backgrounded_command_runs_until_its_notification` |
| Background.test_a_notification_that_lands_mid_turn_counts | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_notification_that_lands_mid_turn_counts` |
| Background.test_a_monitor_ends_at_its_timeout_or_on_task_stop | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_monitor_ends_at_its_timeout_or_on_task_stop` |
| Background.test_a_persistent_monitor_has_no_end | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_persistent_monitor_has_no_end` |
| Background.test_a_monitor_event_is_not_its_end | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_monitor_event_is_not_its_end` |
| Background.test_a_wakeup_is_pending_until_it_fires | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_wakeup_is_pending_until_it_fires` |
| Background.test_a_new_wakeup_replaces_the_last_and_stop_cancels_it | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_new_wakeup_replaces_the_last_and_stop_cancels_it` |
| Background.test_a_one_shot_cron_ends_when_it_fires | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_one_shot_cron_ends_when_it_fires` |
| Background.test_a_recurring_cron_lasts_until_deleted | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_recurring_cron_lasts_until_deleted` |
| Background.test_background_and_foreground_subagents | COVERED | src/sessionstat.rs `sessionstat::py_ported::background_and_foreground_subagents` |
| Background.test_a_teammate_runs_until_idle_and_again_when_messaged | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_teammate_runs_until_idle_and_again_when_messaged` |
| Background.test_messages_from_unknown_teammates_change_nothing | COVERED | src/sessionstat.rs `sessionstat::py_ported::messages_from_unknown_teammates_change_nothing` |
| Tokens.test_usage_is_counted_once_per_message | COVERED | src/sessionstat.rs `sessionstat::py_ported::usage_is_counted_once_per_message_across_the_chain` |
| Tokens.test_cache_lifetime_and_context_come_from_the_main_chain | COVERED | src/sessionstat.rs `sessionstat::py_ported::cache_lifetime_and_context_come_from_the_main_chain` |
| Tokens.test_the_rest | COVERED | src/sessionstat.rs `sessionstat::py_ported::the_rest_of_the_summary` |
| Folding.test_feeding_in_pieces_gives_the_same_answer | COVERED | src/sessionstat.rs `sessionstat::py_ported::feeding_in_pieces_gives_the_same_answer` |
| Folding.test_a_line_separator_inside_a_string_does_not_split_the_record | COVERED | src/sessionstat.rs `sessionstat::py_ported::a_line_separator_inside_a_string_does_not_split_the_record` |
| Folding.test_answered_calls_are_not_kept | COVERED | src/sessionstat.rs `sessionstat::py_ported::answered_calls_are_not_kept` |
| StatuslineExtras.test_durations_and_lines_are_kept_when_sent | COVERED | src/sessionstat.rs `sessionstat::py_ported::statusline_durations_and_lines_are_kept_when_sent` (calls `usage::reading`; kept in sessionstat.rs because usage.rs is shared) |
| ReaderTests.test_appends_are_read_and_a_half_written_line_waits | COVERED | tests/sessionstat_reader.rs `appends_are_read_and_a_half_written_line_waits` (through GET /api/session_status; the reader finds files via CLAUDE_CONFIG_DIR) |
| ReaderTests.test_unknown_or_malformed_id | COVERED | tests/sessionstat_reader.rs `unknown_or_malformed_id` and src/sessionstat.rs `sessionstat::py_ported::unknown_or_malformed_id` |


## test_settings.py

Summary: 23 tests - 23 COVERED

| Test | Verdict | Rust test or reason |
|---|---|---|
| Checks.test_numbers_are_checked_against_their_range | COVERED | src/settings.rs `settings::py_ported::numbers_are_checked_against_their_range` (golden settings_compare in tests/pure_checks.rs also covers every field over ~120 values) |
| Checks.test_thresholds_are_sorted_and_deduplicated | COVERED | src/settings.rs `settings::py_ported::thresholds_are_sorted_and_deduplicated` |
| Checks.test_the_form_cannot_choose_a_bypass_mode | COVERED | src/settings.rs `settings::py_ported::the_form_cannot_choose_a_bypass_mode` |
| Checks.test_text_fields_refuse_what_they_should | COVERED | src/settings.rs `settings::py_ported::text_fields_refuse_what_they_should` |
| Checks.test_the_form_shows_values_and_defaults_but_never_the_token | COVERED | src/settings.rs `settings::py_ported::the_form_shows_values_and_defaults_but_never_the_token` |
| Checks.test_apply_keeps_comments_and_drops_a_blank_background_option | COVERED | src/settings.rs `settings::py_ported::apply_keeps_comments_and_drops_a_blank_background_option` |
| Checks.test_one_bad_value_saves_nothing | COVERED | src/settings.rs `settings::py_ported::one_bad_value_saves_nothing` |
| Checks.test_restarts_name_each_process_once | COVERED | src/settings.rs `settings::py_ported::restarts_name_each_process_once` |
| Saving.test_a_save_writes_the_file_and_keeps_the_old_one | COVERED | tests/settings_http.rs `a_save_writes_the_file_and_keeps_the_old_one` |
| Saving.test_live_settings_apply_at_once | COVERED | tests/settings_http.rs `live_settings_apply_at_once` (the live-applied values are checked as the form and pages show them; the dispatcher's internal fields Python read directly are not visible over HTTP) |
| Saving.test_text_sizes_apply_at_once_and_reach_the_pages | COVERED | tests/settings_http.rs `text_sizes_apply_at_once_and_reach_the_pages` |
| Saving.test_highlighting_applies_at_once | COVERED | tests/settings_http.rs `highlighting_applies_at_once` |
| Saving.test_a_toggle_made_through_api_notify_shows_as_it_is | COVERED | tests/settings_http.rs `a_toggle_made_through_api_notify_shows_as_it_is` |
| Saving.test_the_form_carries_the_last_notification_error | COVERED | tests/settings_http.rs `the_form_carries_the_last_notification_error` (last_error is provoked with a fake ntfy server answering 403; the message is the binary's `ntfy: <ureq error>`, not Python's `ntfy: HTTP 403`, so the test checks the prefix and the 403) |
| Saving.test_errors_come_back_by_field_and_the_file_is_untouched | COVERED | tests/settings_http.rs `errors_come_back_by_field_and_the_file_is_untouched` |
| Saving.test_a_broken_file_is_not_overwritten | COVERED | tests/settings_http.rs `a_broken_file_is_not_overwritten` |
| Saving.test_without_a_config_file_there_is_no_window | COVERED | tests/settings_http.rs `without_a_config_file_the_form_shows_defaults_and_a_save_creates_it` (Python's `config_path = None` state does not exist in the binary: its dispatcher always reads config.json in its home; the equivalent here is a missing file, which shows the defaults and is created by the first save) |
| Routes.test_the_page_and_the_form_are_served | COVERED | tests/settings_http.rs `the_page_and_the_form_are_served` |
| Routes.test_the_windows_own_page_may_save_and_a_foreign_one_may_not | COVERED | tests/settings_http.rs `the_windows_own_page_may_save_and_a_foreign_one_may_not` |
| Routes.test_a_remote_hosts_tunnel_cannot_reach_it | COVERED | tests/settings_http.rs `a_remote_hosts_tunnel_cannot_reach_it` (tunnel: hook-only listener on a chosen free port, IMPERATORIUM_SSH a program that does not exist; answers 404) |
| Button.test_the_gear_sits_left_of_the_matrix_button | COVERED | tests/settings_http.rs `the_gear_sits_left_of_the_matrix_button` |
| Button.test_the_settings_script_parses | COVERED | tests/settings_http.rs `the_settings_script_parses` |
| Button.test_the_settings_css_has_no_control_characters | COVERED | tests/settings_http.rs `the_settings_css_has_no_control_characters` |


## test_setup.py (30 tests)

Summary: COVERED 7 existing + 19 ported (PORT now COVERED) = 26, MOOT 4, DIFFERS 0. New tests: install.rs/service.rs/remote_cli.rs `setup_cov` modules (appended), tests/setup_cli.rs

| Test | Verdict | Rust test or reason |
|---|---|---|
| Versions.test_reads_claude_code_version_output | COVERED | install.rs `tests::versions` |
| Versions.test_nothing_to_read_is_none | COVERED | install.rs `setup_cov::version_text_that_holds_no_version_is_none_and_compares_numerically` |
| Versions.test_minimum_is_compared_numerically | COVERED | install.rs `setup_cov::version_text_that_holds_no_version_is_none_and_compares_numerically` |
| Settings.test_config_takes_the_chosen_roots_and_voice | COVERED | install.rs `setup_cov::config_takes_the_chosen_roots_and_voice` |
| Settings.test_config_example_carries_nothing_from_this_machine | COVERED | install.rs `setup_cov::the_examples_carry_nothing_from_the_authors_machine` |
| Settings.test_policy_example_carries_no_project_paths | COVERED | install.rs `setup_cov::the_examples_carry_nothing_from_the_authors_machine` |
| Settings.test_auto_approval_is_opt_in | COVERED | install.rs `setup_cov::auto_approval_is_opt_in` |
| Settings.test_answers_from_the_window_work_without_auto_approval | COVERED | install.rs `setup_cov::answers_from_the_window_work_without_auto_approval` |
| Settings.test_generated_settings_do_not_alias_the_example | MOOT | build_config parses the embedded text afresh on each call; no shared value to alias |
| Hidapi.test_an_archive_with_the_wrong_hash_is_refused | MOOT | not in binary: hidapi.dll is vendored in the exe, not downloaded; archives are SHA-256 checked in update.rs (covered by the CLI port's tests) |
| Hidapi.test_a_matching_archive_with_a_wrong_dll_is_refused | MOOT | not in binary (same) |
| Choices.test_yes_takes_every_default_without_reading_input | COVERED | install.rs `setup_cov::yes_takes_every_default_without_reading_input` (ask_text is not in the binary) |
| Choices.test_default_root_is_the_checkouts_parent | MOOT | the binary has no checkout; default_roots is home-based (see next row) |
| Choices.test_a_checkout_in_home_prefers_a_code_folder | COVERED | install.rs `setup_cov::a_home_with_a_code_folder_prefers_it_and_otherwise_is_the_root` (re-runs itself as a child with HOME set) |
| Ntfy.test_topics_are_unguessable_and_distinct | COVERED | install.rs `setup_cov::ntfy_topics_are_unguessable_and_distinct` |
| Ntfy.test_choosing_push_switches_it_on_with_the_topic | COVERED | install.rs `setup_cov::choosing_push_switches_it_on_with_the_topic_and_it_is_off_by_default` |
| Ntfy.test_push_stays_off_by_default | COVERED | install.rs `setup_cov::choosing_push_switches_it_on_with_the_topic_and_it_is_off_by_default` |
| Service.test_systemd_unit_restarts_and_quotes_paths_with_spaces | COVERED | service.rs `setup_cov::the_systemd_unit_restarts_and_quotes_paths_with_spaces` |
| Service.test_launchd_plist_is_valid_xml_and_keeps_alive | COVERED | service.rs `setup_cov::the_launchd_plist_keeps_the_service_alive_and_escapes_what_it_holds` (string asserts; no XML parser in the crate) |
| Remote.test_probe_output_is_parsed | COVERED | remote_cli.rs `tests::the_probe_is_read` (sdk/python facts are gone: the binary runs on the host) |
| Remote.test_a_bare_host_probes_as_missing_everything | COVERED | remote_cli.rs `tests::the_probe_is_read` (empty probe = Facts::default(), version= empty is None) |
| Remote.test_the_probe_is_one_posix_script | COVERED | remote_cli.rs `setup_cov::the_probe_is_one_posix_script_that_does_not_depend_on_path` |
| Remote.test_claude_is_recorded_relative_to_home | COVERED | remote_cli.rs `tests::claude_is_written_home_relative` |
| Remote.test_host_entry | COVERED | remote_cli.rs `tests::entries_follow_the_python_shape` |
| Remote.test_adding_a_host_enables_remote_and_appends | COVERED | remote_cli.rs `tests::a_host_is_added_replaced_and_removed` |
| Remote.test_re_adding_a_host_replaces_it_and_keeps_its_own_keys | COVERED | remote_cli.rs `setup_cov::re_adding_a_host_replaces_it_keeps_its_own_keys_and_does_not_alias` |
| Remote.test_re_adding_without_a_tunnel_drops_it | COVERED | remote_cli.rs `tests::a_host_is_added_replaced_and_removed` ("--no-tunnel on an existing host") |
| Remote.test_removing_a_host | COVERED | remote_cli.rs `setup_cov::removing_a_host_keeps_the_others_in_order` |
| Remote.test_config_written_by_onboarding_is_read_by_the_dispatcher | COVERED | remote_cli.rs `setup_cov::a_config_written_by_onboarding_is_read_by_the_dispatcher` |
| CommandLine.test_bad_remote_usage_is_refused_without_touching_anything | COVERED | tests/setup_cli.rs `setup_bad_remote_and_service_usage_is_refused_without_touching_anything` |


## test_slash.py

Summary: 22 tests - 21 COVERED, 1 PORT

Also replayed against Python by tests/pure_checks.rs `slash_check_answers_as_python_did` (golden slash_compare: parse, refusal, clean_typed, typed_refusal over ~190 texts).

| Test | Verdict | Rust test or reason |
|---|---|---|
| Parse.test_a_command_and_its_arguments | COVERED | src/slash.rs `slash::py_ported::a_command_and_its_arguments` |
| Parse.test_text_that_only_mentions_a_command_is_a_prompt | COVERED | src/slash.rs `slash::py_ported::text_that_only_mentions_a_command_is_a_prompt` (the Python `None` input has no Rust form: `parse` takes &str) |
| Refusal.test_ordinary_commands_are_typed | COVERED | src/slash.rs `slash::py_ported::ordinary_commands_are_typed` |
| Refusal.test_a_menu_the_window_cannot_drive_is_refused | COVERED | src/slash.rs `slash::py_ported::a_menu_the_window_cannot_drive_is_refused` |
| Refusal.test_a_bare_menu_command_is_told_to_take_an_argument | COVERED | src/slash.rs `slash::py_ported::a_bare_menu_command_is_told_to_take_an_argument` |
| Refusal.test_exit_points_at_close | COVERED | src/slash.rs `slash::py_ported::exit_points_at_close` |
| Refusal.test_a_second_line_would_submit_the_first | COVERED | src/slash.rs `slash::py_ported::a_second_line_would_submit_the_first` |
| Blocked.test_an_idle_local_session_takes_it | COVERED | src/slash.rs `slash::py_ported::an_idle_local_session_takes_it` |
| Blocked.test_never_into_a_turn_or_a_prompt_waiting_on_you | COVERED | src/slash.rs `slash::py_ported::never_into_a_turn_or_a_prompt_waiting_on_you` |
| Blocked.test_never_while_a_permission_decision_is_parked_for_it | COVERED | src/slash.rs `slash::py_ported::never_while_a_permission_decision_is_parked_for_it` |
| Blocked.test_a_remote_session_has_no_console_here | COVERED | src/slash.rs `slash::py_ported::a_remote_session_has_no_console_here` |
| Blocked.test_a_session_without_a_process | COVERED | src/slash.rs `slash::py_ported::a_session_without_a_process` |
| Blocked.test_windows_only | COVERED | src/slash.rs `slash::py_ported::windows_only` (`#[cfg(not(windows))]`: the check is `cfg!(windows)`, fixed at build time, so this runs only on a non-Windows build) |
| Typing.test_clears_the_draft_then_types_the_line_and_enter | PORT | src/slash.rs `slash::py_ported::typing_needs_a_process_id` covers the guard only. NOT TESTABLE as Python did: `type_command` spawns the binary's own `type-into` child with no injectable typer, so the (Ctrl+C, line, Enter) sequence is verified by reading source (slash.rs `type_command`, closing.rs `type_keys`), not by a test. Seam would need a non-test change |
| CleanTyped.test_lines_become_one | COVERED | src/slash.rs `slash::py_ported::lines_become_one` |
| CleanTyped.test_plain_text_is_unchanged | COVERED | src/slash.rs `slash::py_ported::plain_text_is_unchanged` |
| CleanTyped.test_control_and_invisible_characters_go | COVERED | src/slash.rs `slash::py_ported::control_and_invisible_characters_go` |
| CleanTyped.test_emoji_stay | COVERED | src/slash.rs `slash::py_ported::emoji_stay` |
| CleanTyped.test_a_leading_shell_or_memory_key_is_dropped | COVERED | src/slash.rs `slash::py_ported::a_leading_shell_or_memory_key_is_dropped` |
| CleanTyped.test_a_command_is_kept_as_a_command | COVERED | src/slash.rs `slash::py_ported::a_command_is_kept_as_a_command` |
| TypedRefusal.test_empty_and_too_long | COVERED | src/slash.rs `slash::py_ported::typed_refusal_empty_and_too_long` |
| TypedRefusal.test_a_command_gets_the_command_rules | COVERED | src/slash.rs `slash::py_ported::a_command_gets_the_command_rules` |


## test_streamdeck.py

33 tests: 33 COVERED
The Python file skipped itself without pillow/streamdeck; the binary's surface logic has no such dependency. Extra test added: `a_press_on_a_session_focuses_it` (the press payload, `/api/focus`). No DIFFERS.

| Test | Verdict | Rust test or reason |
|---|---|---|
| test_hyphens_become_word_breaks | COVERED | `streamdeck::surface_tests::hyphens_become_word_breaks` (also `tests::wraps_like_python`) |
| test_long_single_word_is_truncated_with_an_ellipsis | COVERED | `streamdeck::surface_tests::long_single_word_is_truncated_with_an_ellipsis` |
| test_never_more_than_three_lines | COVERED | `streamdeck::surface_tests::never_more_than_three_lines` |
| test_empty_label_still_returns_a_line | COVERED | `streamdeck::surface_tests::empty_label_still_returns_a_line` |
| test_no_session_is_none | COVERED | `streamdeck::surface_tests::no_session_is_none` |
| test_attention_outranks_busy | COVERED | `streamdeck::surface_tests::attention_outranks_busy` |
| test_busy_and_working_both_read_as_working | COVERED | `streamdeck::surface_tests::busy_and_working_both_read_as_working` |
| test_idle_with_nothing_is_ready | COVERED | `streamdeck::surface_tests::idle_with_nothing_is_ready` |
| test_idle_waiting_on_a_background_task_is_tasks | COVERED | `streamdeck::surface_tests::idle_waiting_on_a_background_task_is_tasks` |
| test_complete_is_its_own_colour | COVERED | `streamdeck::surface_tests::complete_is_its_own_colour` (also `tests::states`) |
| test_finished_background_job_is_not_shown_as_live | COVERED | `streamdeck::surface_tests::finished_background_job_is_not_shown_as_live` |
| test_pinned_keys_keep_their_position | COVERED | `streamdeck::surface_tests::pinned_keys_keep_their_position` (also `tests::layout_pinned_first_then_blocked_then_by_name`) |
| test_unpinned_blocked_project_sorts_ahead_of_other_extras | COVERED | `streamdeck::surface_tests::unpinned_blocked_project_sorts_ahead_of_other_extras` |
| test_order_is_capped_at_the_key_count | COVERED | `streamdeck::surface_tests::order_is_capped_at_the_key_count` |
| test_every_session_of_a_project_gets_a_key | COVERED | `streamdeck::surface_tests::every_session_of_a_project_gets_a_key` |
| test_a_lone_session_has_no_counter | COVERED | `streamdeck::surface_tests::a_lone_session_has_no_counter` |
| test_pinned_project_expands_in_place | COVERED | `streamdeck::surface_tests::pinned_project_expands_in_place` |
| test_open_sessions_beat_pinned_projects_with_none | COVERED | `streamdeck::surface_tests::open_sessions_beat_pinned_projects_with_none` |
| test_finished_and_remote_sessions_get_no_key | COVERED | `streamdeck::surface_tests::finished_and_remote_sessions_get_no_key` |
| test_sessions_outside_the_catalogue_are_named_by_folder | COVERED | `streamdeck::surface_tests::sessions_outside_the_catalogue_are_named_by_folder` |
| test_a_press_on_an_empty_slot_does_nothing | COVERED | `streamdeck::surface_tests::a_press_on_an_empty_slot_does_nothing` |
| test_worktree_session_shares_its_projects_key | COVERED | `streamdeck::surface_tests::worktree_session_shares_its_projects_key` |
| test_first_sight_does_not_flash | COVERED | `streamdeck::surface_tests::first_sight_does_not_flash` |
| test_a_change_alternates_new_and_old_then_settles | COVERED | `streamdeck::surface_tests::a_change_alternates_new_and_old_then_settles` (also `tests::a_change_flashes`) |
| test_a_change_mid_flash_restarts_from_the_shown_state | COVERED | `streamdeck::surface_tests::a_change_mid_flash_restarts_from_the_shown_state` |
| test_no_pending_leaves_every_key_to_projects | COVERED | `streamdeck::surface_tests::no_pending_leaves_every_key_to_projects` |
| test_pending_takes_the_last_three_keys | COVERED | `streamdeck::surface_tests::pending_takes_the_last_three_keys` |
| test_a_press_before_the_key_is_armed_does_nothing | COVERED | `streamdeck::surface_tests::a_press_before_the_key_is_armed_does_nothing` (also `tests::decisions_arm_after_a_moment`) |
| test_the_info_key_never_decides | COVERED | `streamdeck::surface_tests::the_info_key_never_decides` (also `tests::decisions_arm_after_a_moment`) |
| test_a_new_request_rearms_the_keys | COVERED | `streamdeck::surface_tests::a_new_request_rearms_the_keys` |
| test_the_same_request_keeps_its_arm_time | COVERED | `streamdeck::surface_tests::the_same_request_keeps_its_arm_time` |
| test_layout_shrinks_while_a_decision_shows | COVERED | `streamdeck::surface_tests::layout_shrinks_while_a_decision_shows` |
| test_glyph_marks_the_top_left_corner_only | COVERED | `streamdeck::surface_tests::glyph_marks_the_top_left_corner_only` (renders with `render`, no font needed; Python drew it with PIL) |


## test_subproc.py (11 tests)

Summary: COVERED 5 (1 existing + 4 ported), MOOT 3, DIFFERS 3 (2 without a test: suspended leftover and sealed job do not exist in the binary). New tests: proc.rs `subproc_cov` (4 run + 1 ignored)

The binary's counterpart is src/proc.rs: run_timeout (bounded, drains both pipes, `taskkill /T /F` on timeout) and kill_tree. It has no job object, no keep= option and no sweep of what a clean exit leaves.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Tree.test_a_timeout_kills_the_grandchild_too | COVERED | proc.rs `subproc_cov::a_timeout_kills_the_grandchild_too` (windows) |
| Tree.test_keep_leaves_what_it_started_running | COVERED | proc.rs `subproc_cov::a_clean_exit_leaves_what_it_started_running` (windows; the binary's only mode) |
| Tree.test_a_clean_exit_kills_what_it_left | DIFFERS | FIXED 2026-11-15 (`b18199f`): run_timeout's job object; the test is no longer ignored. Was: Python's default killed a grandchild left after a clean exit (job object); the binary's run_timeout leaves it running. Ignored test: proc.rs `subproc_cov::a_clean_exit_kills_what_it_left` |
| Tree.test_a_child_left_suspended_by_a_clean_exit_is_killed | DIFFERS | FIXED 2026-11-15 (`b18199f`): proc.rs `a_member_left_suspended_goes_with_the_job`. Was: same cause: no sweep of leftovers after a clean exit in the binary |
| Tree.test_a_sealed_job_refuses_a_late_child | DIFFERS | FIXED 2026-11-15 (`b18199f`): proc.rs `a_sealed_job_refuses_a_late_child`. Was: not in binary: no job object / sealing in proc.rs (remote.rs uses a job only for its ssh child) |
| Tree.test_kill_tree_ends_a_long_lived_child_and_its_children | COVERED | proc.rs `subproc_cov::kill_tree_ends_a_long_lived_child_and_its_children_and_twice_is_harmless` (windows) |
| RunLike.test_output_input_and_return_code | COVERED | output: `collects_both_streams`; exit status and stdout: proc.rs `subproc_cov::the_exit_status_comes_back`; stdin input is not in the binary (stdin is null) |
| RunLike.test_check_raises | MOOT | Python subprocess.run API (check=True); callers in Rust inspect Output.status |
| RunLike.test_a_missing_program_raises_oserror | COVERED | proc.rs `missing_program_is_not_found` |
| Guard.test_a_module_using_subprocess_gets_the_tree_killing_run | MOOT | Python module monkeypatching |
| Guard.test_the_sdk_session_reader_is_guarded | MOOT | Agent SDK is gone from the binary (transcripts are read natively) |


## test_taskout.py

16 tests: 16 COVERED
No DIFFERS.

| Test | Verdict | Rust test or reason |
|---|---|---|
| test_found_by_session_id_whatever_the_project_slug | COVERED | `taskout::output_tests::found_by_session_id_whatever_the_project_slug` (the Rust `tasks_dir` has no `roots` argument; the test points CLAUDE_CODE_TMPDIR at its temp folder) |
| test_unknown_or_malformed_ids_find_nothing | COVERED | `taskout::output_tests::unknown_or_malformed_ids_find_nothing` |
| test_colour_codes_and_redrawn_lines_are_cleaned | COVERED | `taskout::output_tests::colour_codes_and_redrawn_lines_are_cleaned` (also `tests::terminal_output_is_cleaned`) |
| test_a_long_file_is_cut_at_a_line | COVERED | `taskout::output_tests::a_long_file_is_cut_at_a_line` |
| test_a_vanished_file_reads_as_empty | COVERED | `taskout::output_tests::a_vanished_file_reads_as_empty` |
| test_reading_leaves_the_writer_writing | COVERED | `taskout::output_tests::reading_leaves_the_writer_writing` |
| test_scheduled_wakeups_are_listed_with_when_they_fire | COVERED | `taskout::output_tests::scheduled_wakeups_are_listed_with_when_they_fire` |
| test_background_shells_and_monitors_by_their_ids | COVERED | `taskout::output_tests::background_shells_and_monitors_by_their_ids` |
| test_a_foreground_call_gets_the_file_no_record_names | COVERED | `taskout::output_tests::a_foreground_call_gets_the_file_no_record_names` |
| test_a_file_older_than_the_call_is_not_its_output | COVERED | `taskout::output_tests::a_file_older_than_the_call_is_not_its_output` |
| test_no_folder_still_lists_the_tasks | COVERED | `taskout::output_tests::no_folder_still_lists_the_tasks` |
| test_folder_is_beside_the_transcript | COVERED | `taskout::output_tests::folder_is_beside_the_transcript` |
| test_files_found_by_agent_id_and_by_the_call_that_started_them | COVERED | `taskout::output_tests::files_found_by_agent_id_and_by_the_call_that_started_them` |
| test_story_tells_text_calls_and_failures_but_not_the_task | COVERED | `taskout::output_tests::story_tells_text_calls_and_failures_but_not_the_task` (also `tests::a_subagent_is_told_as_lines`) |
| test_a_cut_tail_drops_the_partial_first_line | COVERED | `taskout::output_tests::a_cut_tail_drops_the_partial_first_line` (the Rust tail limit is fixed at 256 KiB, so the test cuts a >256 KiB first line) |
| test_background_and_foreground_subagents_get_their_transcripts | COVERED | `taskout::output_tests::background_and_foreground_subagents_get_their_transcripts` |


## test_voice.py

28 tests: 15 COVERED, 4 PORT, 8 MOOT, 1 DIFFERS
Worker tests run against a stand-in child (std-only program compiled with rustc at test time into %TEMP%/imp-fakevoice, selected with IMPERATORIUM_VOICE_BIN). The rust/voice transcription-guard tests are written but could not be built here.

| Test | Verdict | Rust test or reason |
|---|---|---|
| test_defaults_apply_when_there_is_no_config | COVERED | `voice::worker_tests::defaults_apply_when_there_is_no_config` (the binary's `settings_from(None)`; there is no `load_settings` file reader) |
| test_a_broken_config_falls_back_rather_than_raising | MOOT | `voice.load_settings(path)` is not in the binary. The dispatcher (like Python's, `dispatcher.py` line 273) builds a worker only when config.json has a `voice` object, so a broken or absent config means dictation off in both; `settings_from` itself is covered above. |
| test_config_overrides_only_known_keys | COVERED | `voice::worker_tests::config_overrides_only_known_keys` |
| test_disabling_voice_is_expressed_in_settings | COVERED | `voice::worker_tests::disabling_voice_is_expressed_in_settings` |
| test_the_cpu_gets_a_smaller_model_by_default | COVERED | `voice::worker_tests::the_cpu_gets_a_smaller_model_by_default` (the `cpu_model` setting; `on_cpu` and `compute_type=int8` are not in the binary, see test_cpu_config below) |
| test_a_model_named_in_config_is_kept_on_the_cpu | COVERED | `voice::worker_tests::a_model_named_in_config_is_kept_on_the_cpu` |
| test_an_input_device_index_survives | COVERED | `voice::worker_tests::an_input_device_index_survives` |
| test_empty_input_returns_empty_without_loading | PORT | RUN 2026-11-15, passing (rustoiceuild-voice.cmd test --release, with VS CMake and libclang). Was written, not run: `rust/voice` `tests::empty_and_silent_input_return_empty_without_loading`. The worker crate is its own workspace; whisper-rs needs cmake and a C++/GPU toolchain, none on this machine. |
| test_silence_returns_empty_without_loading | PORT | Same test as above (zeros and 0.001 faint input, the binary's threshold is 0.005); RUN 2026-11-15, passing. |
| test_audible_input_does_reach_the_model | PORT | RUN 2026-11-15, passing (rustoiceuild-voice.cmd test --release, with VS CMake and libclang). Was written, not run: `rust/voice` `tests::audible_input_does_reach_the_model` (a model that cannot load makes the call fail with `no model file`). |
| test_a_fresh_recorder_is_not_recording | MOOT | Python `voice.Recorder` (sounddevice microphone capture) is not in the binary: the window records audio and posts bytes, which `Worker::transcribe_bytes` sends on. |
| test_stopping_without_starting_yields_nothing | MOOT | Same: `Recorder` is Python-only. |
| test_settings_merge_over_defaults | MOOT | Same: `Recorder` is Python-only (the settings merge itself is `settings_from`, covered above). |
| test_no_gpu_falls_back_to_the_cpu_model | PORT | BLOCKED: the fallback is `Transcriber::load` in rust/voice, which builds a real whisper-rs `WhisperContext`; there is no seam to fake it without editing non-test code. |
| test_a_cpu_config_starts_on_the_cpu_model | DIFFERS | FIXED 2026-11-15: voice_plan.rs (shared with rust/voice) `a_cpu_config_starts_on_the_cpu_model`. Was: Python: `device: cpu` loads `cpu_model` (small) at int8. Binary (rust/voice `Transcriber::load`, the `Err(_)` arm for `!want_gpu`): loads `self.model` (the configured/default model, e.g. large-v3-turbo) on the CPU; only a GPU failure falls to `cpu_model`. Found by reading; no test (needs whisper). |
| test_audio_goes_through_and_text_comes_back | COVERED | `voice::worker_tests::audio_goes_through_and_text_comes_back` (against a stand-in child compiled from std-only source; Python used a thread over pipes) |
| test_the_worker_learns_where_the_model_runs | COVERED | `voice::worker_tests::the_worker_learns_where_the_model_runs` |
| test_nothing_recorded_never_starts_a_process | COVERED | `voice::worker_tests::nothing_recorded_never_starts_a_process` |
| test_load_builds_the_model_once_per_process | COVERED | `voice::worker_tests::load_builds_the_model_once_per_process` |
| test_a_failure_in_the_child_is_reported_and_the_child_kept | COVERED | `voice::worker_tests::a_failure_in_the_child_is_reported_and_the_child_kept` |
| test_a_child_that_died_is_started_again | COVERED | `voice::worker_tests::a_child_that_died_is_started_again` |
| test_an_idle_child_is_ended_and_a_busy_one_is_not | COVERED | `voice::worker_tests::an_idle_child_is_ended_and_a_busy_one_is_not` (reap takes no `now`: the test backdates `used`) |
| test_zero_means_never_unload | COVERED | `voice::worker_tests::zero_means_never_unload` |
| test_the_child_runs_this_module_without_a_console | COVERED | `voice::worker_tests::the_child_is_run_with_the_worker_flag_and_its_settings` (`--worker <settings json>`; the no-console flag, `proc::no_window`, is not asserted) |
| test_checking_availability_does_not_import_the_model | MOOT | Python import hygiene (`faster_whisper` in `sys.modules`); the binary has no such import. |
| test_availability_is_reported_not_assumed | MOOT | `voice.available()` is Python-only; the binary's analogue is `worker_check` (missing exe/model messages), untested but not in the Python tests. |
| test_at_least_one_input_device_is_visible | MOOT | `voice.list_inputs()` (sounddevice) is Python-only; skipped without the venv. |
| test_sixteen_kilohertz_is_what_the_model_wants | MOOT | `voice.SAMPLE_RATE` is a Python constant; rust/voice has its own `SAMPLE_RATE` and WAV resampling tests (`tests::mixes_and_resamples`). |
