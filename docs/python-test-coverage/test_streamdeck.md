# test_streamdeck.py

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
