# test_settings.py

Summary: 23 tests - 23 PORT

| Test | Verdict | Rust test or reason |
|---|---|---|
| Checks.test_numbers_are_checked_against_their_range | PORT | src/settings.rs `settings::py_ported::numbers_are_checked_against_their_range` (golden settings_compare in tests/pure_checks.rs also covers every field over ~120 values) |
| Checks.test_thresholds_are_sorted_and_deduplicated | PORT | src/settings.rs `settings::py_ported::thresholds_are_sorted_and_deduplicated` |
| Checks.test_the_form_cannot_choose_a_bypass_mode | PORT | src/settings.rs `settings::py_ported::the_form_cannot_choose_a_bypass_mode` |
| Checks.test_text_fields_refuse_what_they_should | PORT | src/settings.rs `settings::py_ported::text_fields_refuse_what_they_should` |
| Checks.test_the_form_shows_values_and_defaults_but_never_the_token | PORT | src/settings.rs `settings::py_ported::the_form_shows_values_and_defaults_but_never_the_token` |
| Checks.test_apply_keeps_comments_and_drops_a_blank_background_option | PORT | src/settings.rs `settings::py_ported::apply_keeps_comments_and_drops_a_blank_background_option` |
| Checks.test_one_bad_value_saves_nothing | PORT | src/settings.rs `settings::py_ported::one_bad_value_saves_nothing` |
| Checks.test_restarts_name_each_process_once | PORT | src/settings.rs `settings::py_ported::restarts_name_each_process_once` |
| Saving.test_a_save_writes_the_file_and_keeps_the_old_one | PORT | tests/settings_http.rs `a_save_writes_the_file_and_keeps_the_old_one` |
| Saving.test_live_settings_apply_at_once | PORT | tests/settings_http.rs `live_settings_apply_at_once` |
| Saving.test_text_sizes_apply_at_once_and_reach_the_pages | PORT | tests/settings_http.rs `text_sizes_apply_at_once_and_reach_the_pages` |
| Saving.test_highlighting_applies_at_once | PORT | tests/settings_http.rs `highlighting_applies_at_once` |
| Saving.test_a_toggle_made_through_api_notify_shows_as_it_is | PORT | tests/settings_http.rs `a_toggle_made_through_api_notify_shows_as_it_is` |
| Saving.test_the_form_carries_the_last_notification_error | PORT | tests/settings_http.rs `the_form_carries_the_last_notification_error` |
| Saving.test_errors_come_back_by_field_and_the_file_is_untouched | PORT | tests/settings_http.rs `errors_come_back_by_field_and_the_file_is_untouched` |
| Saving.test_a_broken_file_is_not_overwritten | PORT | tests/settings_http.rs `a_broken_file_is_not_overwritten` |
| Saving.test_without_a_config_file_there_is_no_window | PORT | tests/settings_http.rs `without_a_config_file_there_is_no_window` |
| Routes.test_the_page_and_the_form_are_served | PORT | tests/settings_http.rs `the_page_and_the_form_are_served` |
| Routes.test_the_windows_own_page_may_save_and_a_foreign_one_may_not | PORT | tests/settings_http.rs `the_windows_own_page_may_save_and_a_foreign_one_may_not` |
| Routes.test_a_remote_hosts_tunnel_cannot_reach_it | PORT | tests/settings_http.rs `a_remote_hosts_tunnel_cannot_reach_it` |
| Button.test_the_gear_sits_left_of_the_matrix_button | PORT | tests/settings_http.rs `the_gear_sits_left_of_the_matrix_button` |
| Button.test_the_settings_script_parses | PORT | tests/settings_http.rs `the_settings_script_parses` |
| Button.test_the_settings_css_has_no_control_characters | PORT | tests/settings_http.rs `the_settings_css_has_no_control_characters` |
