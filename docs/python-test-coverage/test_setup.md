# test_setup.py (30 tests)

Summary: COVERED 12, PORT 14, MOOT 4, DIFFERS 0 (initial map; PORT rows are ported below)

| Test | Verdict | Rust test or reason |
|---|---|---|
| Versions.test_reads_claude_code_version_output | COVERED | install.rs `tests::versions` |
| Versions.test_nothing_to_read_is_none | PORT | install.rs `tests::versions` only asserts "nope" |
| Versions.test_minimum_is_compared_numerically | PORT | tuple compare (2,1,1000) vs MIN_CLAUDE not asserted |
| Settings.test_config_takes_the_chosen_roots_and_voice | PORT | build_config untested |
| Settings.test_config_example_carries_nothing_from_this_machine | PORT | embedded CONFIG_EXAMPLE untested |
| Settings.test_policy_example_carries_no_project_paths | PORT | embedded POLICY_EXAMPLE untested |
| Settings.test_auto_approval_is_opt_in | PORT | build_policy untested |
| Settings.test_answers_from_the_window_work_without_auto_approval | PORT | build_policy(false) keeps decision_format object + park |
| Settings.test_generated_settings_do_not_alias_the_example | MOOT | build_config parses the embedded text afresh on each call; no shared value to alias |
| Hidapi.test_an_archive_with_the_wrong_hash_is_refused | MOOT | not in binary: hidapi.dll is vendored in the exe, not downloaded; archives are SHA-256 checked in update.rs (covered by the CLI port's tests) |
| Hidapi.test_a_matching_archive_with_a_wrong_dll_is_refused | MOOT | not in binary (same) |
| Choices.test_yes_takes_every_default_without_reading_input | PORT | install.rs `choose` with --yes; untested (ask_text not in binary) |
| Choices.test_default_root_is_the_checkouts_parent | MOOT | the binary has no checkout; default_roots is home-based (see next row) |
| Choices.test_a_checkout_in_home_prefers_a_code_folder | PORT | default_roots prefers a conventional code folder in home; untested |
| Ntfy.test_topics_are_unguessable_and_distinct | PORT | install.rs ntfy_topic untested |
| Ntfy.test_choosing_push_switches_it_on_with_the_topic | PORT | build_config(.., Some(topic)) |
| Ntfy.test_push_stays_off_by_default | PORT | build_config(.., None) |
| Service.test_systemd_unit_restarts_and_quotes_paths_with_spaces | PORT | service.rs `systemd_install...` asserts only ExecStart; Restart/WorkingDirectory/PATH/WantedBy untested |
| Service.test_launchd_plist_is_valid_xml_and_keeps_alive | PORT | service.rs `launchd_install_and_uninstall` asserts Label and log only; ProgramArguments, KeepAlive, RunAtLoad, escaping untested |
| Remote.test_probe_output_is_parsed | COVERED | remote_cli.rs `tests::the_probe_is_read` (sdk/python facts are gone: the binary runs on the host) |
| Remote.test_a_bare_host_probes_as_missing_everything | COVERED | remote_cli.rs `tests::the_probe_is_read` (empty probe = Facts::default(), version= empty is None) |
| Remote.test_the_probe_is_one_posix_script | PORT | remote_cli.rs PROBE keeps the `$HOME/.local/bin/claude` lookup; untested |
| Remote.test_claude_is_recorded_relative_to_home | COVERED | remote_cli.rs `tests::claude_is_written_home_relative` |
| Remote.test_host_entry | COVERED | remote_cli.rs `tests::entries_follow_the_python_shape` |
| Remote.test_adding_a_host_enables_remote_and_appends | COVERED | remote_cli.rs `tests::a_host_is_added_replaced_and_removed` |
| Remote.test_re_adding_a_host_replaces_it_and_keeps_its_own_keys | PORT | keeps the existing label/other keys when the new entry sets none, and the other hosts in place; only partly asserted |
| Remote.test_re_adding_without_a_tunnel_drops_it | COVERED | remote_cli.rs `tests::a_host_is_added_replaced_and_removed` ("--no-tunnel on an existing host") |
| Remote.test_removing_a_host | PORT | removal keeps order of the others (a, b, c); test only removes a sole host |
| Remote.test_config_written_by_onboarding_is_read_by_the_dispatcher | PORT | upsert_host output goes through remote::load_hosts; untested |
| CommandLine.test_bad_remote_usage_is_refused_without_touching_anything | PORT | `remote bogus`, `remote add` (no host), `service bogus` exit 2; untested (checked black-box) |

Also MOOT, not counted above: nothing else.
