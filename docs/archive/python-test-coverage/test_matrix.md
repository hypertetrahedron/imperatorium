# test_matrix.py (23 tests)

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
