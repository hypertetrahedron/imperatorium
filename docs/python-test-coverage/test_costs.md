# test_costs.py

34 tests: map in progress (initial pass: everything not obviously MOOT is PORT)

| Test | Verdict | Rust test or reason |
|---|---|---|
| test_input_output | PORT | TBD |
| test_streamed_entries_count_once_and_last_wins | PORT | TBD |
| test_cache_write_5m_vs_1h | PORT | TBD |
| test_cache_write_without_split_is_5m | PORT | TBD |
| test_cache_read_multipliers | PORT | TBD |
| test_dated_and_bracketed_model_ids | PORT | TBD |
| test_family_fallback | PORT | TBD |
| test_unknown_model_is_unpriced | PORT | TBD |
| test_rewriting_a_cold_cache | PORT | TBD |
| test_synthetic_and_non_assistant_skipped | PORT | TBD |
| test_local_date_and_zero_fill | PORT | TBD |
| test_late_evening_stays_on_its_local_day | PORT | TBD |
| test_usage_outside_window_not_counted | PORT | TBD |
| test_old_file_ignored | PORT | TBD |
| test_old_file_dropped_from_memory | PORT | TBD |
| test_worktree_cwd_maps_to_repo | PORT | TBD |
| test_sessions_counted_and_subagent_joins_parent | PORT | TBD |
| test_same_message_in_session_and_subagent_counts_once | PORT | TBD |
| test_sorted_by_cost | PORT | TBD |
| test_appended_lines_only_are_parsed | PORT | TBD |
| test_unchanged_file_not_reread | PORT | TBD |
| test_later_entry_replaces_earlier_across_reads | PORT | TBD |
| test_partial_line_waits_for_its_newline | PORT | TBD |
| test_shrunk_file_is_reread | PORT | TBD |
| test_replaced_larger_file_is_reread | PORT | TBD |
| test_budget_zero_is_incomplete | PORT | TBD |
| test_cached_files_count_when_budget_runs_out | PORT | TBD |
| test_missing_root_is_empty | PORT | TBD |
| test_a_window_counts_only_messages_sent_inside_it | PORT | TBD |
| test_streamed_entries_count_once_in_a_window | PORT | TBD |
| test_a_session_is_its_whole_life_with_its_subagents | PORT | TBD |
| test_each_answer_splits_by_model | PORT | TBD |
| test_answers_follow_the_files | PORT | TBD |
| test_not_a_session_id_finds_nothing | PORT | TBD |
