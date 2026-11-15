# test_costs.py

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
