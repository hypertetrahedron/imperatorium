# test_features.py: Python test coverage in Rust

203 tests (199 defined, 4 inherited by QuestionWait): COVERED 65, PORT (still to port) 137, MOOT 1, DIFFERS 0.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Changes.test_clean_repo_has_no_files_or_diff | PORT | changes() is only parse-tested (changes::tests::status_lines) |
| Changes.test_dirty_repo_reports_modified_and_untracked_files | PORT | same |
| Changes.test_non_repo_reports_repo_false_without_error | PORT | same |
| Changes.test_missing_directory_is_a_clean_error | PORT | same |
| Worktrees.test_ahead_count_and_agent_flag | PORT | worktrees() only parse-tested |
| Worktrees.test_a_hand_made_worktree_is_not_flagged_as_agent | PORT | - |
| Worktrees.test_dirty_worktree_is_reported_dirty | PORT | - |
| Worktrees.test_no_worktrees_on_a_plain_repo | PORT | - |
| Worktrees.test_not_a_repo | PORT | - |
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
| Finish.test_pr_uses_the_injected_runner_never_git_push_or_gh | PORT | the binary has no runner seam; worktree_twin "pr" runs the real push against a local origin but gh fails there, so the URL taken from gh's last line is untested |
| Finish.test_pr_reports_a_push_failure_without_touching_gh | PORT | worktree_twin pushes successfully; a failing push is untested |
| Finish.test_a_merge_git_refuses_is_reported_and_changes_nothing | PORT | worktree_twin covers only the conflict path, not a refusal without conflict markers |
| Finish.test_pr_refuses_a_dirty_worktree_before_touching_the_network | COVERED | worktree_twin step "pr dirty" (branch python-out/twins-b) |
| MadeWorktrees.test_made_from_the_sources_branch_not_main | COVERED | changes::tests::made_worktrees_branch_from_the_source_and_merge_back_into_it |
| MadeWorktrees.test_names_do_not_collide | COVERED | same (second make gives worktree-fix-the-thing-2) |
| MadeWorktrees.test_a_detached_source_gives_its_commit_and_no_base | PORT | - |
| MadeWorktrees.test_not_a_repo_is_none | PORT | - |
| MadeWorktrees.test_merge_goes_into_its_base_where_that_is_checked_out | COVERED | same |
| MadeWorktrees.test_merge_refuses_a_dirty_base_checkout | COVERED | same |
| MadeWorktrees.test_merge_refuses_a_base_checked_out_nowhere | PORT | - |
| MadeWorktrees.test_diff_is_against_its_base | COVERED | same |
| MadeWorktrees.test_discard_forgets_the_base | COVERED | same |
| MadeWorktrees.test_drop_worktree_undoes_it | PORT | the existing test checks the folder is gone, not that the branch is deleted |
| ChecksState.test_no_rollup_is_none | COVERED | prs::tests::checks_fold_both_shapes |
| ChecksState.test_any_failure_wins | PORT | conclusion FAILURE next to SUCCESS is not asserted |
| ChecksState.test_pending_beats_passing | COVERED | prs::tests::checks_fold_both_shapes |
| ChecksState.test_all_passing | PORT | NEUTRAL conclusion not asserted |
| ChecksState.test_status_context_shape | PORT | FAILURE / PENDING contexts not asserted |
| ChecksState.test_mixed_shapes_failure_still_wins | COVERED | prs::tests::checks_fold_both_shapes (IN_PROGRESS beside a state ERROR) |
| Summarise.test_maps_the_fields_the_badge_needs | PORT | summary_fields uses different input (no approved review, title, branch) |
| Tracker.test_get_registers_interest_and_returns_none_before_a_fetch | PORT | - |
| Tracker.test_poll_once_caches_the_injected_fetch_never_calling_gh | PORT | no fetch seam (gh is hardwired); tested with a pre-seeded fresh cache entry |
| Tracker.test_a_runtime_error_from_fetch_disables_the_tracker | MOOT | no fetch seam in the binary; gh missing is the only trigger and a test cannot make gh missing without touching PATH |
| Tracker.test_directories_nobody_asked_about_recently_are_dropped | PORT | - |
| Reading.test_extracts_the_fields_the_meter_needs | PORT | a_reading_keeps_what_matters does not assert cost_usd or the resets_at of a window |
| Reading.test_missing_fields_are_none_not_a_crash | PORT | - |
| Pace.test_too_little_history_says_nothing | COVERED | usage::tests::pace_needs_five_minutes_of_readings |
| Pace.test_a_rising_pace_forecasts_when_it_hits_100 | COVERED | same (per_hour 60, full_at 5400) |
| Pace.test_before_reset_is_true_only_when_full_at_precedes_it | COVERED | same (reset 3000 vs 6000) |
| Pace.test_flat_or_falling_usage_never_predicts_full | PORT | - |
| Pace.test_old_samples_outside_the_pace_span_are_ignored | PORT | - |
| UsageTracking.test_a_threshold_crossing_fires_once | PORT | - |
| UsageTracking.test_jumping_straight_past_both_thresholds_reports_the_top_one | PORT | - |
| UsageTracking.test_a_new_reset_time_starts_a_fresh_history | PORT | - |
| UsageTracking.test_a_window_that_reset_waits_at_zero_for_a_new_reading | PORT | - |
| UsageTracking.test_forget_drops_a_sessions_reading | PORT | - |
| Timeline.test_pairs_tool_calls_with_their_results | PORT | - |
| Timeline.test_a_call_still_running_has_ok_none | PORT | - |
| Doing.test_a_running_tool_call_wins | COVERED | activity::tests::the_newest_thing_wins |
| Doing.test_claudes_last_text_after_a_finished_turn | PORT | - |
| Doing.test_a_pending_human_prompt_is_marked_as_asked | COVERED | activity::tests::the_newest_thing_wins |
| Doing.test_empty_conversation_is_a_blank_line | PORT | - |
| Categorise.test_known_shapes | PORT | - |
| Tally.test_counts_calls_and_errors_by_tool_and_category | PORT | - |
| Tally.test_merge_tallies_adds_them_up | PORT | - |
| Live.test_pre_then_post_pairs_into_one_finished_call | PORT | - |
| Live.test_a_running_call_is_current_until_it_ends | PORT | - |
| Live.test_a_call_stuck_for_ten_minutes_is_not_reported_as_current | PORT | - |
| Live.test_a_failure_event_is_recorded_not_ok | PORT | - |
| Live.test_a_subagents_calls_are_not_recorded | PORT | - |
| Live.test_a_non_tool_event_is_not_ingested | PORT | - |
| Live.test_forget_drops_a_sessions_events | PORT | - |
| Matches.test_every_word_must_appear_in_any_order | COVERED | history::tests::every_word_somewhere |
| Matches.test_a_prompt_only_match_is_reported_as_prompt | COVERED | history::tests::every_word_somewhere |
| Matches.test_a_body_text_only_match_needs_the_text_argument | PORT | the "text" result is asserted, a miss without the text is not |
| Matches.test_empty_query_matches_nothing | PORT | - |
| Snippet.test_returns_the_match_in_context | COVERED | history::tests::snippets_match_python |
| Snippet.test_no_match_is_empty | COVERED | history::tests::snippets_match_python |
| IndexTests.test_list_is_sorted_newest_first_and_cached | PORT | - |
| IndexTests.test_text_is_cached_by_size_and_rebuilt_when_it_changes | PORT | - |
| IndexTests.test_search_finds_a_title_match_without_reading_the_transcript | PORT | - |
| IndexTests.test_search_falls_back_to_full_text | PORT | - |
| IndexTests.test_search_marks_incomplete_when_the_budget_runs_out | PORT | - |
| IndexTests.test_empty_query_returns_the_plain_list | PORT | - |
| Resume.test_requires_a_session_id | PORT | - |
| Resume.test_requires_a_real_directory | PORT | - |
| Resume.test_window_launches_the_terminal_command_through_the_injected_popen | PORT | - |
| Resume.test_fork_adds_fork_session_to_the_window_command | PORT | - |
| Resume.test_background_uses_the_injected_bg_start_not_a_real_process | PORT | - |
| Resume.test_a_background_error_becomes_a_resume_error | PORT | - |
| Resume.test_an_unknown_where_is_refused | PORT | - |
| Resume.test_new_session_opens_plain_claude_in_a_tab | PORT | - |
| Resume.test_launches_drop_the_calling_sessions_identity | PORT | - |
| Resume.test_a_background_launch_drops_the_calling_sessions_identity_too | PORT | - |
| Resume.test_new_session_gets_a_fresh_id_each_time_or_the_one_given | PORT | - |
| Resume.test_new_session_requires_a_real_directory | PORT | - |
| Resume.test_window_needs_wt_on_path | PORT | - |
| NtfyRequest.test_builds_a_request_against_the_configured_server_and_topic | COVERED | notify::tests::ntfy_request_matches_python |
| NtfyRequest.test_a_token_adds_a_bearer_header | COVERED | notify::tests::ntfy_request_matches_python |
| NtfyRequest.test_missing_topic_refuses_to_build_a_request | COVERED | notify::tests::ntfy_request_matches_python |
| SpeakCommand.test_quotes_are_escaped_for_powershell | COVERED | notify::tests::lines_match_python |
| SpeakCommand.test_hyphens_and_underscores_read_better_spoken | COVERED | notify::tests::lines_match_python |
| NotifierTests.test_attention_schedules_every_enabled_channel_once | PORT | - |
| NotifierTests.test_firing_pushes_and_speaks_when_still_waiting | COVERED | notify_twin::notifications_push_and_count_as_python_did (branch python-out/twins-a); the speak channel is never enabled in a test (it would run PowerShell speech) |
| NotifierTests.test_answered_before_the_delay_never_sends_anything | COVERED | notify_twin (session twin-answered; branch python-out/twins-a) |
| NotifierTests.test_a_disabled_channel_is_never_scheduled | COVERED | notify_twin (ntfy switched off, then a Notification; branch python-out/twins-a) |
| NotifierTests.test_a_send_failure_is_recorded_not_raised | PORT | - |
| NotifierTests.test_budget_fires_immediately_with_no_timer | COVERED | notify_twin ("5-hour limit at 91%" push; branch python-out/twins-a) |
| NotifierTests.test_status_reports_enabled_and_configured | COVERED | notify_twin "get /api/notify" (branch python-out/twins-a) and twin.rs (branch python-out/twins-b) |
| NotifierTests.test_set_enabled_rejects_an_unknown_channel | COVERED | twin.rs "no such channel: nope" (branch python-out/twins-b) |
| QueueStore.test_enqueue_then_take_is_fifo | PORT | - |
| QueueStore.test_take_from_an_empty_queue_is_none | PORT | - |
| QueueStore.test_an_empty_queue_is_dropped_from_the_map | PORT | - |
| QueueStore.test_requeue_front_puts_it_back_first_with_the_error | PORT | - |
| QueueStore.test_unqueue_removes_one_item_by_id | PORT | - |
| QueueStore.test_enqueue_ids_are_unique_within_one_clock_tick | PORT | - |
| QueueStore.test_queue_survives_a_reload | PORT | - |
| QueueStore.test_prefs_round_trip_and_none_clears_a_key | PORT | - |
| StatuslineLine.test_full_payload | COVERED | hooks::tests::statusline_text |
| StatuslineLine.test_missing_fields_are_skipped_not_blank | COVERED | hooks::tests::statusline_text |
| StatuslineLine.test_non_numeric_percentages_are_ignored | COVERED | hooks::tests::statusline_text |
| StatuslineProvider.test_cloud_flags_win_in_claude_codes_order | PORT | providers() asserts bedrock only |
| StatuslineProvider.test_a_flag_set_to_zero_is_off | COVERED | hooks::tests::providers |
| StatuslineProvider.test_keys_and_gateways | COVERED | hooks::tests::providers |
| StatuslineProvider.test_the_claude_ai_login_is_none | COVERED | hooks::tests::providers |
| StatuslineProvider.test_the_line_names_the_provider_only_without_windows | PORT | only "Opus · API" is asserted |
| PlanWithoutWindows.test_provider_comes_from_the_script_or_the_model_id | PORT | - |
| PlanWithoutWindows.test_all_bedrock_sessions_name_bedrock_in_place_of_meters | PORT | - |
| PlanWithoutWindows.test_a_pro_or_max_login_before_its_first_reading_shows_nothing | PORT | - |
| PlanWithoutWindows.test_an_enterprise_login_names_the_plan | PORT | - |
| PlanWithoutWindows.test_beside_real_windows_only_the_uncovered_providers_are_named | PORT | - |
| PlanWithoutWindows.test_a_session_gone_quiet_stops_counting | PORT | - |
| PlanWithoutWindows.test_account_plan_reads_only_the_organization_type | PORT | - |
| Queue.test_idle_dispatchable_session_gets_immediate_delivery | PORT | - |
| Queue.test_a_busy_session_holds_the_prompt_until_a_stop_hook | PORT | - |
| Queue.test_a_failed_delivery_is_requeued_with_the_error | PORT | - |
| Queue.test_sweep_queue_delivers_to_a_session_that_registered_late | PORT | - |
| Queue.test_enqueue_with_no_prompt_is_refused | COVERED | twin.rs "queue prompt '  '" -> no prompt (branch python-out/twins-b) |
| Queue.test_enqueue_for_an_unknown_session_is_refused | COVERED | twin.rs "queue no-such-session" (branch python-out/twins-b) |
| StatuslineIngest.test_a_statusline_event_updates_usage_not_the_event_log | PORT | - |
| ActivityMethod.test_prefers_hooks_over_the_conversation | COVERED | twin.rs "get /api/activity" session 3, source hooks (branch python-out/twins-b) |
| ActivityMethod.test_falls_back_to_the_conversation_when_there_are_no_hook_events | PORT | twin.rs only has empty conversation timelines |
| ErrorsMethod.test_aggregates_tool_failures_across_recent_sessions_by_project | PORT | - |
| IdleAfterStop.test_an_idle_session_a_minute_after_stop_is_waiting_for_you | COVERED | notify_twin idle push + idle_prompt event (branch python-out/twins-a) |
| IdleAfterStop.test_only_the_latest_turns_check_counts | PORT | - |
| IdleAfterStop.test_a_busy_session_or_one_with_background_work_is_not_waiting | PORT | - |
| IdleAfterStop.test_a_session_that_scheduled_its_own_next_turn_is_not_waiting | PORT | - |
| IdleAfterStop.test_a_real_idle_prompt_first_is_not_announced_again | PORT | - |
| ResumeMethod.test_refuses_to_resume_a_still_running_session_into_a_window | PORT | - |
| ResumeMethod.test_forking_a_running_session_is_allowed | PORT | - |
| ResumeMethod.test_a_dead_session_resolves_its_cwd_from_history | PORT | - |
| ResumeMethod.test_new_session_uses_the_selected_sessions_cwd | PORT | - |
| ResumeMethod.test_new_and_close_launches_first_then_closes_the_old_one | PORT | - |
| ResumeMethod.test_close_session_ends_its_process | PORT | - |
| ResumeMethod.test_close_session_without_a_process_is_refused | PORT | - |
| ResumeMethod.test_dismissing_a_session_with_no_process_takes_it_off_the_list | PORT | - |
| ResumeMethod.test_a_running_session_is_not_dismissed | PORT | - |
| ResumeMethod.test_a_dismissed_session_comes_back_when_it_runs_again | PORT | - |
| ResumeMethod.test_a_dismissed_session_comes_back_on_session_start | PORT | - |
| ResumeMethod.test_new_and_close_without_a_process_launches_nothing | PORT | - |
| ResumeMethod.test_a_resume_error_is_reported_not_raised | PORT | - |
| FinishWorktreeGuard.test_an_unknown_project_is_refused_before_touching_git | COVERED | worktree_twin step "unknown project" (branch python-out/twins-b) |
| FinishWorktreeGuard.test_a_known_project_passes_through_and_logs_the_event | COVERED | worktree_twin "merge" + "events" steps (branch python-out/twins-b) |
| HttpRoutes.test_get_api_usage | COVERED | twin.rs "get /api/usage" (branch python-out/twins-b) |
| HttpRoutes.test_post_queue_delivers_then_get_queue_is_empty | PORT | - |
| HttpRoutes.test_post_worktree_on_an_unknown_project_answers_200_not_ok | COVERED | worktree_twin "unknown project" (branch python-out/twins-b) |
| HttpRoutes.test_post_resume_for_an_unknown_session_answers_200_not_ok | PORT | - |
| HttpRoutes.test_post_close_for_an_unknown_session_answers_200_not_ok | PORT | - |
| HttpRoutes.test_pages_and_health_name_the_same_dispatcher | PORT | - |
| HttpRoutes.test_post_dismiss_for_an_unknown_session_answers_200_not_ok | COVERED | twin.rs "post /api/dismiss" (branch python-out/twins-b) |
| HttpRoutes.test_post_new_without_a_project_answers_200_not_ok | PORT | - |
| HttpRoutes.test_bundled_fonts_are_served_and_cached | PORT | ui::tests::fonts_are_bundled checks the bytes, not the route, its Content-Type or Cache-Control |
| HttpRoutes.test_only_fonts_in_the_folder_are_served | PORT | - |
| HttpRoutes.test_the_chat_matrix_page_and_its_data | PORT | twin.rs covers the /api/matrix answer, not the page |
| HttpRoutes.test_get_api_errors | PORT | - |
| HookOutputs.test_the_documented_reply_is_a_decision_object | COVERED | hooks::tests::replies |
| HookOutputs.test_a_deny_carries_its_message | COVERED | hooks::tests::replies |
| HookOutputs.test_legacy_stays_the_default_until_switched | COVERED | hooks::tests::replies |
| HookOutputs.test_tool_payloads_lose_file_contents | COVERED | hooks::tests::tool_payloads_lose_file_contents |
| HookOutputs.test_a_failure_keeps_a_short_reason | COVERED | hooks::tests::a_failure_keeps_a_short_reason |
| ParkWait.test_a_background_session_gets_the_long_wait | PORT | - |
| ParkWait.test_an_interactive_session_gets_the_short_wait | PORT | - |
| ParkWait.test_an_unknown_session_gets_the_short_wait | PORT | - |
| ParkWait.test_an_older_hook_without_the_field_keeps_its_wait | PORT | - |
| ParkedQuestions.test_the_item_carries_the_questions | PORT | - |
| ParkedQuestions.test_answers_come_back_with_the_allow | PORT | - |
| ParkedQuestions.test_a_bare_allow_is_refused_for_a_question | PORT | a_question_needs_every_answer has one question, so "only the first answered" is untested |
| ParkedQuestions.test_pass_releases_it_to_the_terminal_at_once | PORT | - |
| ParkedQuestions.test_a_malformed_question_cannot_be_allowed | PORT | - |
| ParkedQuestions.test_other_tools_are_unchanged | PORT | - |
| ParkedQuestions.test_the_hook_sends_the_answers_as_updated_input | COVERED | hooks::tests::replies (deny never carries input) and hooks_blackbox "test_questions" (allow with answers -> updatedInput; branch rust) |
| ParkedQuestions.test_the_chat_line_shows_the_question | PORT | - |
| QuestionHook.test_answered_in_the_window | COVERED | hooks_blackbox::hooks_behave_as_the_python_hooks_did (cases "test_questions") |
| QuestionHook.test_an_allow_without_answers_says_nothing | COVERED | same |
| QuestionHook.test_declined | COVERED | same |
| QuestionHook.test_the_legacy_format_never_parks_a_question | COVERED | same |
| QuestionWait.test_an_interactive_question_gets_the_question_wait | PORT | - |
| QuestionWait.test_a_background_question_keeps_the_long_wait | PORT | - |
| QuestionWait.test_other_tools_ignore_it | PORT | - |
| QuestionWait.test_a_background_session_gets_the_long_wait | PORT | inherited from ParkWait |
| QuestionWait.test_an_interactive_session_gets_the_short_wait | PORT | inherited from ParkWait |
| QuestionWait.test_an_unknown_session_gets_the_short_wait | PORT | inherited from ParkWait |
| QuestionWait.test_an_older_hook_without_the_field_keeps_its_wait | PORT | inherited from ParkWait |
