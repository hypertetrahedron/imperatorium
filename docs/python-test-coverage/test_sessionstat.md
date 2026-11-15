# test_sessionstat.py

Summary: 21 tests - 21 PORT

Also replayed whole-corpus against Python by tests/corpus.rs (golden stats_compare, at the clocks in clocks.json).

| Test | Verdict | Rust test or reason |
|---|---|---|
| Background.test_a_backgrounded_command_runs_until_its_notification | PORT | src/sessionstat.rs `sessionstat::py_ported::a_backgrounded_command_runs_until_its_notification` |
| Background.test_a_notification_that_lands_mid_turn_counts | PORT | src/sessionstat.rs `sessionstat::py_ported::a_notification_that_lands_mid_turn_counts` |
| Background.test_a_monitor_ends_at_its_timeout_or_on_task_stop | PORT | src/sessionstat.rs `sessionstat::py_ported::a_monitor_ends_at_its_timeout_or_on_task_stop` |
| Background.test_a_persistent_monitor_has_no_end | PORT | src/sessionstat.rs `sessionstat::py_ported::a_persistent_monitor_has_no_end` |
| Background.test_a_monitor_event_is_not_its_end | PORT | src/sessionstat.rs `sessionstat::py_ported::a_monitor_event_is_not_its_end` |
| Background.test_a_wakeup_is_pending_until_it_fires | PORT | src/sessionstat.rs `sessionstat::py_ported::a_wakeup_is_pending_until_it_fires` |
| Background.test_a_new_wakeup_replaces_the_last_and_stop_cancels_it | PORT | src/sessionstat.rs `sessionstat::py_ported::a_new_wakeup_replaces_the_last_and_stop_cancels_it` |
| Background.test_a_one_shot_cron_ends_when_it_fires | PORT | src/sessionstat.rs `sessionstat::py_ported::a_one_shot_cron_ends_when_it_fires` |
| Background.test_a_recurring_cron_lasts_until_deleted | PORT | src/sessionstat.rs `sessionstat::py_ported::a_recurring_cron_lasts_until_deleted` |
| Background.test_background_and_foreground_subagents | PORT | src/sessionstat.rs `sessionstat::py_ported::background_and_foreground_subagents` |
| Background.test_a_teammate_runs_until_idle_and_again_when_messaged | PORT | src/sessionstat.rs `sessionstat::py_ported::a_teammate_runs_until_idle_and_again_when_messaged` |
| Background.test_messages_from_unknown_teammates_change_nothing | PORT | src/sessionstat.rs `sessionstat::py_ported::messages_from_unknown_teammates_change_nothing` |
| Tokens.test_usage_is_counted_once_per_message | PORT | src/sessionstat.rs `sessionstat::py_ported::usage_is_counted_once_per_message_across_the_chain` |
| Tokens.test_cache_lifetime_and_context_come_from_the_main_chain | PORT | src/sessionstat.rs `sessionstat::py_ported::cache_lifetime_and_context_come_from_the_main_chain` |
| Tokens.test_the_rest | PORT | src/sessionstat.rs `sessionstat::py_ported::the_rest_of_the_summary` |
| Folding.test_feeding_in_pieces_gives_the_same_answer | PORT | src/sessionstat.rs `sessionstat::py_ported::feeding_in_pieces_gives_the_same_answer` |
| Folding.test_a_line_separator_inside_a_string_does_not_split_the_record | PORT | src/sessionstat.rs `sessionstat::py_ported::a_line_separator_inside_a_string_does_not_split_the_record` |
| Folding.test_answered_calls_are_not_kept | PORT | src/sessionstat.rs `sessionstat::py_ported::answered_calls_are_not_kept` |
| ReaderTests.test_appends_are_read_and_a_half_written_line_waits | PORT | src/sessionstat.rs `sessionstat::py_ported::appends_are_read_and_a_half_written_line_waits` |
| ReaderTests.test_unknown_or_malformed_id | PORT | src/sessionstat.rs `sessionstat::py_ported::unknown_or_malformed_id` |
| StatuslineExtras.test_durations_and_lines_are_kept_when_sent | PORT | src/sessionstat.rs `sessionstat::py_ported::statusline_durations_and_lines_are_kept_when_sent` (in src/usage.rs-adjacent test, kept in sessionstat.rs because usage.rs is shared; calls `usage::reading`) |
