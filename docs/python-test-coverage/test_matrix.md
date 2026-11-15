# test_matrix.py (23 tests)

Summary: 0 COVERED, 23 PORT (initial map). Note: the file defines two classes named `Spend`; the first (`test_totals_and_each_session_most_costly_first`) is shadowed by the second and never ran in Python, but the behaviour exists in `matrix::spend`, so it is ported.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Focus.test_each_step_shows_the_replies_and_only_the_step_in_flight | PORT | `matrix.rs` has no tests at all |
| Focus.test_a_tool_shows_its_name_and_one_line | PORT | |
| Focus.test_the_prompt_and_results_are_not_shown | PORT | |
| Focus.test_a_thought_is_not_live_once_the_turn_is_over | PORT | |
| Focus.test_nothing_is_in_flight_in_a_session_that_is_not | PORT | |
| Focus.test_a_reply_or_a_prompt_closes_calls_that_never_got_a_result | PORT | |
| Focus.test_parallel_calls_show_the_newest_and_count_the_rest | PORT | |
| Focus.test_replies_carry_their_turn | PORT | |
| Focus.test_only_the_newest_replies_are_kept_and_long_ones_cut | PORT | Rust keeps a fixed 12 (`REPLIES`), Python's `keep` was a parameter |
| Focus.test_a_result_that_came_back_with_text_still_ends_its_call | PORT | |
| Focus.test_system_notes_are_not_replies | PORT | |
| Active.test_running_sessions_get_a_tile_and_ended_ones_do_not | PORT | `snapshot::matrix_active`, untested |
| Spend(unit).test_totals_and_each_session_most_costly_first | PORT | `matrix::spend`, untested (shadowed in Python) |
| Spend.test_a_subscription_counts_its_windows_and_open_sessions_their_whole_life | PORT | `Shared::spend` via /api/usage and /api/matrix, untested |
| Spend.test_a_window_waiting_after_its_reset_counts_from_the_reset | PORT | |
| Spend.test_a_closed_session_counts_globally_but_not_as_current | PORT | |
| Spend.test_an_account_without_windows_counts_the_month | PORT | |
| Spend.test_the_meters_come_back_after_a_restart | PORT | usage persistence across a restart; `usage.rs` unit tests may cover the pure half (checked when porting) |
| Spend.test_usage_carries_the_spend | PORT | |
| Method.test_a_running_session_becomes_a_tile | PORT | /api/matrix, untested |
| Method.test_an_unreadable_transcript_still_gets_its_tile | PORT | |
| Method.test_no_sessions_no_tiles | PORT | |
| Method.test_the_status_tile_is_always_shown_unless_turned_off | PORT | |
