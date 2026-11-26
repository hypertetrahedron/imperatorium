# test_settings.py

Summary: 23 tests - 23 COVERED

| Test | Verdict | Rust test or reason |
|---|---|---|
| Checks.test_numbers_are_checked_against_their_range | COVERED | src/settings.rs `settings::py_ported::numbers_are_checked_against_their_range` (golden settings_compare in tests/pure_checks.rs also covers every field over ~120 values) |
| Checks.test_thresholds_are_sorted_and_deduplicated | COVERED | src/settings.rs `settings::py_ported::thresholds_are_sorted_and_deduplicated` |
| Checks.test_the_form_cannot_choose_a_bypass_mode | COVERED | src/settings.rs `settings::py_ported::the_form_cannot_choose_a_bypass_mode` |
| Checks.test_text_fields_refuse_what_they_should | COVERED | src/settings.rs `settings::py_ported::text_fields_refuse_what_they_should` |
| Checks.test_the_form_shows_values_and_defaults_but_never_the_token | COVERED | src/settings.rs `settings::py_ported::the_form_shows_values_and_defaults_but_never_the_token` |
| Checks.test_apply_keeps_comments_and_drops_a_blank_background_option | COVERED | src/settings.rs `settings::py_ported::apply_keeps_comments_and_drops_a_blank_background_option` |
| Checks.test_one_bad_value_saves_nothing | COVERED | src/settings.rs `settings::py_ported::one_bad_value_saves_nothing` |
| Checks.test_restarts_name_each_process_once | COVERED | src/settings.rs `settings::py_ported::restarts_name_each_process_once` |
| Saving.test_a_save_writes_the_file_and_keeps_the_old_one | COVERED | tests/settings_http.rs `a_save_writes_the_file_and_keeps_the_old_one` |
| Saving.test_live_settings_apply_at_once | COVERED | tests/settings_http.rs `live_settings_apply_at_once` (the live-applied values are checked as the form and pages show them; the dispatcher's internal fields Python read directly are not visible over HTTP) |
| Saving.test_text_sizes_apply_at_once_and_reach_the_pages | COVERED | tests/settings_http.rs `text_sizes_apply_at_once_and_reach_the_pages` |
| Saving.test_highlighting_applies_at_once | COVERED | tests/settings_http.rs `highlighting_applies_at_once` |
| Saving.test_a_toggle_made_through_api_notify_shows_as_it_is | COVERED | tests/settings_http.rs `a_toggle_made_through_api_notify_shows_as_it_is` |
| Saving.test_the_form_carries_the_last_notification_error | COVERED | tests/settings_http.rs `the_form_carries_the_last_notification_error` (last_error is provoked with a fake ntfy server answering 403; the message is the binary's `ntfy: <ureq error>`, not Python's `ntfy: HTTP 403`, so the test checks the prefix and the 403) |
| Saving.test_errors_come_back_by_field_and_the_file_is_untouched | COVERED | tests/settings_http.rs `errors_come_back_by_field_and_the_file_is_untouched` |
| Saving.test_a_broken_file_is_not_overwritten | COVERED | tests/settings_http.rs `a_broken_file_is_not_overwritten` |
| Saving.test_without_a_config_file_there_is_no_window | COVERED | tests/settings_http.rs `without_a_config_file_the_form_shows_defaults_and_a_save_creates_it` (Python's `config_path = None` state does not exist in the binary: its dispatcher always reads config.json in its home; the equivalent here is a missing file, which shows the defaults and is created by the first save) |
| Routes.test_the_page_and_the_form_are_served | COVERED | tests/settings_http.rs `the_page_and_the_form_are_served` |
| Routes.test_the_windows_own_page_may_save_and_a_foreign_one_may_not | COVERED | tests/settings_http.rs `the_windows_own_page_may_save_and_a_foreign_one_may_not` |
| Routes.test_a_remote_hosts_tunnel_cannot_reach_it | COVERED | tests/settings_http.rs `a_remote_hosts_tunnel_cannot_reach_it` (tunnel: hook-only listener on a chosen free port, IMPERATORIUM_SSH a program that does not exist; answers 404) |
| Button.test_the_gear_sits_left_of_the_matrix_button | COVERED | tests/settings_http.rs `the_gear_sits_left_of_the_matrix_button` |
| Button.test_the_settings_script_parses | COVERED | tests/settings_http.rs `the_settings_script_parses` |
| Button.test_the_settings_css_has_no_control_characters | COVERED | tests/settings_http.rs `the_settings_css_has_no_control_characters` |
