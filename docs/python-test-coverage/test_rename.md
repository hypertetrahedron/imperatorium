# test_rename.py

Summary: 12 tests - 2 COVERED already, 7 PORT now COVERED by new Rust tests, 0 MOOT, 3 DIFFERS

| Test | Verdict | Rust test or reason |
|---|---|---|
| StateFolder.test_finding_the_old_folder_does_not_move_it | COVERED | new: tests/core_rename.rs `the_old_state_folder_is_found_and_left_in_place` |
| StateFolder.test_the_old_folder_is_moved_on_first_start | DIFFERS | the binary never moves ccontrol to imperatorium (no rename anywhere in src; paths::state_dir only prefers the old folder while the new one is absent). The Python dispatcher's migrate_state_dir has no counterpart. Recorded as the #[ignore]d tests/core_rename.rs `the_old_folder_is_moved_on_first_start` |
| StateFolder.test_a_folder_that_cannot_move_goes_on_being_used | COVERED | new: tests/core_rename.rs `the_old_state_folder_is_found_and_left_in_place (the move failing cannot happen: the binary never moves; the second half, the permission hook writing beside the old folder, is exactly what this test checks)` |
| StateFolder.test_a_new_install_uses_the_new_name | COVERED | new: tests/core_rename.rs `a_new_install_uses_the_new_name` |
| StateFolder.test_once_moved_the_old_one_is_ignored | COVERED | new: tests/core_rename.rs `once_the_new_folder_exists_the_old_one_is_ignored` |
| Hooks.test_an_entry_stamped_before_the_rename_is_still_ours | COVERED | install_compare.rs (start 'legacy ccontrol', stamped statusMessage 'ccontrol': replaced, not duplicated) |
| Hooks.test_anyone_elses_is_not | COVERED | install_compare.rs (start 'someone else's hooks' kept through install and uninstall) + install::tests::hooks_replace_ours_and_keep_others |
| Hooks.test_a_statusline_from_before_the_rename_still_reports | COVERED | new: usage::core_tests::provider_of_reads_either_name; tests/core_rename.rs ccontrol_statusline_event_is_ingested |
| Policy.test_the_policy_code_is_protected_under_either_name | COVERED | new: policy::core_tests::the_policy_code_is_protected_under_either_name |
| Units.test_the_old_unit_names_are_known_so_install_can_remove_them | DIFFERS | the binary's service.rs knows only the Imperatorium names (service::tests::names_per_backend); it does not list or remove the pre-rename ccontrol-* units / local.ccontrol.* agents, so an install from before the rename leaves them behind |
| Units.test_a_host_from_before_the_rename_is_noticed | COVERED | new: remote_cli::core_tests::a_host_from_before_the_rename_is_noticed |
| Tasks.test_install_and_uninstall_remove_the_old_task_names | DIFFERS | service.rs / service_ctl.rs have no CentralControl* legacy names (grep finds none); the binary's install and uninstall do not remove the old scheduled tasks. service/tasks.ps1, the file the Python test reads, still does |
