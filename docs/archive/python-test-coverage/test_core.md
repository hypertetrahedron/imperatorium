# test_core.py

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
