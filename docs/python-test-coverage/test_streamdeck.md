# test_streamdeck.py

33 tests: map in progress (initial pass: everything not obviously MOOT is PORT)

| Test | Verdict | Rust test or reason |
|---|---|---|
| test_hyphens_become_word_breaks | PORT | TBD |
| test_long_single_word_is_truncated_with_an_ellipsis | PORT | TBD |
| test_never_more_than_three_lines | PORT | TBD |
| test_empty_label_still_returns_a_line | PORT | TBD |
| test_no_session_is_none | PORT | TBD |
| test_attention_outranks_busy | PORT | TBD |
| test_busy_and_working_both_read_as_working | PORT | TBD |
| test_idle_with_nothing_is_ready | PORT | TBD |
| test_idle_waiting_on_a_background_task_is_tasks | PORT | TBD |
| test_complete_is_its_own_colour | PORT | TBD |
| test_finished_background_job_is_not_shown_as_live | PORT | TBD |
| test_pinned_keys_keep_their_position | PORT | TBD |
| test_unpinned_blocked_project_sorts_ahead_of_other_extras | PORT | TBD |
| test_order_is_capped_at_the_key_count | PORT | TBD |
| test_every_session_of_a_project_gets_a_key | PORT | TBD |
| test_a_lone_session_has_no_counter | PORT | TBD |
| test_pinned_project_expands_in_place | PORT | TBD |
| test_open_sessions_beat_pinned_projects_with_none | PORT | TBD |
| test_finished_and_remote_sessions_get_no_key | PORT | TBD |
| test_sessions_outside_the_catalogue_are_named_by_folder | PORT | TBD |
| test_a_press_on_an_empty_slot_does_nothing | PORT | TBD |
| test_worktree_session_shares_its_projects_key | PORT | TBD |
| test_first_sight_does_not_flash | PORT | TBD |
| test_a_change_alternates_new_and_old_then_settles | PORT | TBD |
| test_a_change_mid_flash_restarts_from_the_shown_state | PORT | TBD |
| test_no_pending_leaves_every_key_to_projects | PORT | TBD |
| test_pending_takes_the_last_three_keys | PORT | TBD |
| test_a_press_before_the_key_is_armed_does_nothing | PORT | TBD |
| test_the_info_key_never_decides | PORT | TBD |
| test_a_new_request_rearms_the_keys | PORT | TBD |
| test_the_same_request_keeps_its_arm_time | PORT | TBD |
| test_layout_shrinks_while_a_decision_shows | PORT | TBD |
| test_glyph_marks_the_top_left_corner_only | PORT | TBD |
