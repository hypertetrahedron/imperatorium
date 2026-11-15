# test_modlink.py (20 tests)

Summary: 2 COVERED, 18 PORT-now-COVERED, 0 MOOT, 0 DIFFERS. 5 unit tests in `modlink.rs` (`python_tests`), 9 integration tests in `tests/modlink_dispatch.rs` (Rig on a temp home), 2 in `tests/modlink_install.rs`.

| Test | Verdict | Rust test or reason |
|---|---|---|
| LinkTests.test_the_token_is_made_once_and_kept | PORT-now-COVERED | `modlink::python_tests::the_token_is_made_once_and_kept` |
| LinkTests.test_a_beat_makes_a_mod_live_until_it_stops | PORT-now-COVERED | `modlink::python_tests::a_beat_makes_a_mod_live_until_it_stops` |
| LinkTests.test_a_prompt_is_handed_out_once_accepted | COVERED | `modlink::tests::a_prompt_is_handed_out_once_then_settles`; also `python_tests::a_prompt_is_handed_out_once_accepted` |
| LinkTests.test_a_dropped_prompt_says_why | PORT-now-COVERED | `modlink::python_tests::a_dropped_prompt_says_why` |
| LinkTests.test_usage_becomes_a_statusline_payload | PORT-now-COVERED | `modlink::python_tests::usage_becomes_a_statusline_payload` |
| ControlTests.test_a_live_mod_takes_the_prompt_instead_of_the_socket | PORT-now-COVERED | `modlink_dispatch.rs::a_live_mod_takes_the_prompt_instead_of_the_socket` |
| ControlTests.test_only_the_window_speaks_as_the_person | PORT-now-COVERED | `modlink_dispatch.rs::only_the_window_speaks_as_the_person` |
| ControlTests.test_without_a_mod_the_socket_is_used_as_before | PORT-now-COVERED | `modlink_dispatch.rs::without_a_mod_the_socket_is_used_as_before` |
| ControlTests.test_a_live_mod_makes_a_session_reachable_and_says_busy | PORT-now-COVERED | `modlink_dispatch.rs::a_live_mod_makes_a_session_reachable_and_says_busy` |
| ControlTests.test_the_queue_remembers_who_queued | PORT-now-COVERED | `modlink_dispatch.rs::the_queue_remembers_who_queued` |
| ControlTests.test_usage_from_a_beat_reaches_the_meters | PORT-now-COVERED | `modlink_dispatch.rs::usage_from_a_beat_reaches_the_meters` |
| ControlTests.test_the_status_line_names_who_is_waiting | PORT-now-COVERED | `modlink_dispatch.rs::the_status_line_names_who_is_waiting` |
| ControlTests.test_the_policy_is_asked_only_once_auto_approval_is_on | PORT-now-COVERED | `modlink_dispatch.rs::the_policy_is_asked_only_once_auto_approval_is_on` |
| RouteTests.test_a_beat_needs_the_token | PORT-now-COVERED | `modlink_dispatch.rs::a_beat_needs_the_token` |
| RouteTests.test_a_check_needs_the_token | PORT-now-COVERED | `modlink_dispatch.rs::a_check_needs_the_token` |
| RouteTests.test_a_dispatch_without_the_window_token_is_framed | PORT-now-COVERED | `modlink_dispatch.rs::only_the_window_speaks_as_the_person` (plain API caller framed) |
| RouteTests.test_a_dispatch_from_the_window_speaks_as_the_person | PORT-now-COVERED | `modlink_dispatch.rs::a_live_mod_takes_the_prompt_instead_of_the_socket` (as_user true from the window) and `a_dispatch_from_the_window_speaks_as_the_person_and_one_without_its_token_does_not` |
| InstallTests.test_added_once_beside_what_is_there | COVERED | `install::tests::mod_dirs_are_kept` and the install_compare golden ("plugin dirs" x `--mod`, once and twice) |
| InstallTests.test_uninstall_removes_only_ours | PORT-now-COVERED | `modlink_install.rs::uninstall_removes_only_ours` (CLI, temp settings) |
| InstallTests.test_a_strange_env_is_left_alone | PORT-now-COVERED | `modlink_install.rs::a_strange_env_is_left_alone` (CLI prints SKIP, env untouched) |
