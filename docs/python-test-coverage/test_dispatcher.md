# test_dispatcher.py

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
