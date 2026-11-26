# test_setup.py (30 tests)

Summary: COVERED 7 existing + 19 ported (PORT now COVERED) = 26, MOOT 4, DIFFERS 0. New tests: install.rs/service.rs/remote_cli.rs `setup_cov` modules (appended), tests/setup_cli.rs

| Test | Verdict | Rust test or reason |
|---|---|---|
| Versions.test_reads_claude_code_version_output | COVERED | install.rs `tests::versions` |
| Versions.test_nothing_to_read_is_none | COVERED | install.rs `setup_cov::version_text_that_holds_no_version_is_none_and_compares_numerically` |
| Versions.test_minimum_is_compared_numerically | COVERED | install.rs `setup_cov::version_text_that_holds_no_version_is_none_and_compares_numerically` |
| Settings.test_config_takes_the_chosen_roots_and_voice | COVERED | install.rs `setup_cov::config_takes_the_chosen_roots_and_voice` |
| Settings.test_config_example_carries_nothing_from_this_machine | COVERED | install.rs `setup_cov::the_examples_carry_nothing_from_the_authors_machine` |
| Settings.test_policy_example_carries_no_project_paths | COVERED | install.rs `setup_cov::the_examples_carry_nothing_from_the_authors_machine` |
| Settings.test_auto_approval_is_opt_in | COVERED | install.rs `setup_cov::auto_approval_is_opt_in` |
| Settings.test_answers_from_the_window_work_without_auto_approval | COVERED | install.rs `setup_cov::answers_from_the_window_work_without_auto_approval` |
| Settings.test_generated_settings_do_not_alias_the_example | MOOT | build_config parses the embedded text afresh on each call; no shared value to alias |
| Hidapi.test_an_archive_with_the_wrong_hash_is_refused | MOOT | not in binary: hidapi.dll is vendored in the exe, not downloaded; archives are SHA-256 checked in update.rs (covered by the CLI port's tests) |
| Hidapi.test_a_matching_archive_with_a_wrong_dll_is_refused | MOOT | not in binary (same) |
| Choices.test_yes_takes_every_default_without_reading_input | COVERED | install.rs `setup_cov::yes_takes_every_default_without_reading_input` (ask_text is not in the binary) |
| Choices.test_default_root_is_the_checkouts_parent | MOOT | the binary has no checkout; default_roots is home-based (see next row) |
| Choices.test_a_checkout_in_home_prefers_a_code_folder | COVERED | install.rs `setup_cov::a_home_with_a_code_folder_prefers_it_and_otherwise_is_the_root` (re-runs itself as a child with HOME set) |
| Ntfy.test_topics_are_unguessable_and_distinct | COVERED | install.rs `setup_cov::ntfy_topics_are_unguessable_and_distinct` |
| Ntfy.test_choosing_push_switches_it_on_with_the_topic | COVERED | install.rs `setup_cov::choosing_push_switches_it_on_with_the_topic_and_it_is_off_by_default` |
| Ntfy.test_push_stays_off_by_default | COVERED | install.rs `setup_cov::choosing_push_switches_it_on_with_the_topic_and_it_is_off_by_default` |
| Service.test_systemd_unit_restarts_and_quotes_paths_with_spaces | COVERED | service.rs `setup_cov::the_systemd_unit_restarts_and_quotes_paths_with_spaces` |
| Service.test_launchd_plist_is_valid_xml_and_keeps_alive | COVERED | service.rs `setup_cov::the_launchd_plist_keeps_the_service_alive_and_escapes_what_it_holds` (string asserts; no XML parser in the crate) |
| Remote.test_probe_output_is_parsed | COVERED | remote_cli.rs `tests::the_probe_is_read` (sdk/python facts are gone: the binary runs on the host) |
| Remote.test_a_bare_host_probes_as_missing_everything | COVERED | remote_cli.rs `tests::the_probe_is_read` (empty probe = Facts::default(), version= empty is None) |
| Remote.test_the_probe_is_one_posix_script | COVERED | remote_cli.rs `setup_cov::the_probe_is_one_posix_script_that_does_not_depend_on_path` |
| Remote.test_claude_is_recorded_relative_to_home | COVERED | remote_cli.rs `tests::claude_is_written_home_relative` |
| Remote.test_host_entry | COVERED | remote_cli.rs `tests::entries_follow_the_python_shape` |
| Remote.test_adding_a_host_enables_remote_and_appends | COVERED | remote_cli.rs `tests::a_host_is_added_replaced_and_removed` |
| Remote.test_re_adding_a_host_replaces_it_and_keeps_its_own_keys | COVERED | remote_cli.rs `setup_cov::re_adding_a_host_replaces_it_keeps_its_own_keys_and_does_not_alias` |
| Remote.test_re_adding_without_a_tunnel_drops_it | COVERED | remote_cli.rs `tests::a_host_is_added_replaced_and_removed` ("--no-tunnel on an existing host") |
| Remote.test_removing_a_host | COVERED | remote_cli.rs `setup_cov::removing_a_host_keeps_the_others_in_order` |
| Remote.test_config_written_by_onboarding_is_read_by_the_dispatcher | COVERED | remote_cli.rs `setup_cov::a_config_written_by_onboarding_is_read_by_the_dispatcher` |
| CommandLine.test_bad_remote_usage_is_refused_without_touching_anything | COVERED | tests/setup_cli.rs `setup_bad_remote_and_service_usage_is_refused_without_touching_anything` |
