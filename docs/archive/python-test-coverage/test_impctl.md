# test_impctl.py (20 tests)

Summary: COVERED 17 (6 existing + 11 ported), MOOT 2, DIFFERS 1 (the sub-case that can be tested is covered). New tests: tests/impctl_cli.rs (12), cli.rs `impctl_cov`, helpers tests/common/setup.rs

The Rust CLI (src/cli.rs) is black-boxed here against a recording fake dispatcher (tests/common FakeServer) via IMPERATORIUM_URL.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Help.test_every_command_is_listed_in_help_and_every_listed_one_exists | COVERED | cli.rs `every_command_is_in_the_help`, `every_subcommand_of_main_is_listed` |
| Help.test_help_prints_each_command_with_its_parameters | COVERED | tests/impctl_cli.rs `impctl_help_prints_each_command_with_its_parameters` |
| Help.test_no_arguments_prints_help_too | COVERED | tests/impctl_cli.rs `impctl_no_arguments_print_the_help_too` |
| Help.test_an_unknown_help_topic_is_an_error_not_a_traceback | COVERED | tests/impctl_cli.rs `impctl_an_unknown_help_topic_is_an_error_not_a_crash` |
| Passthrough.test_leading_flags_reach_the_script | MOOT | Python impctl handed argv to install.py; the binary dispatches in-process (install flags: cli.rs `arguments_read_as_argparse_does`, install.rs `opts`) |
| Passthrough.test_remote_keeps_its_subcommand_and_flags | MOOT | same; `remote add ...` is handled in-binary (remote_cli.rs tests) |
| Passthrough.test_unknown_flags_on_other_commands_are_still_refused | COVERED | cli.rs `arguments_read_as_argparse_does`; black-box exit 2 in tests/impctl_cli.rs `impctl_unknown_flags_are_refused` |
| Uninstall.test_a_no_runs_nothing | DIFFERS | `service uninstall` without --yes and without a terminal refuses and removes nothing (exit 1): tests/impctl_cli.rs `impctl_service_uninstall_without_yes_or_a_terminal_removes_nothing`. But Python's `impctl uninstall` also asked; the binary's top-level `uninstall` (install.rs uninstall_main, main.rs) does NOT ask: it edits settings.json and removes the tasks at once. Not exercised (unsafe), so no ignored test |
| Uninstall.test_yes_skips_the_question | COVERED | cli.rs `impctl_cov::yes_skips_the_question` |
| Uninstall.test_install_does_not_ask | COVERED | tests/impctl_cli.rs `impctl_service_install_does_not_ask` |
| FindSession.test_by_exact_name | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` |
| FindSession.test_by_id_prefix | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` |
| FindSession.test_by_window_title | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` |
| FindSession.test_ambiguous_names_both | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` ('alph' lists both) |
| FindSession.test_resume_falls_back_to_a_past_session_by_id_prefix | COVERED | tests/impctl_cli.rs `impctl_resume_falls_back_to_a_past_session_by_id_prefix` |
| FindSession.test_a_past_session_that_does_not_match_the_prefix_is_not_taken | COVERED | tests/impctl_cli.rs `impctl_a_past_session_that_does_not_match_the_prefix_is_not_taken` |
| Send.test_to_a_session_injects_by_its_full_id | COVERED | tests/impctl_cli.rs `impctl_send_to_a_session_injects_by_its_full_id` |
| Send.test_needs_exactly_one_target | COVERED | tests/impctl_cli.rs `impctl_send_needs_exactly_one_target` |
| Send.test_a_refusal_exits_non_zero | COVERED | tests/impctl_cli.rs `impctl_a_refusal_exits_non_zero` |
| Down.test_a_dead_dispatcher_says_how_to_start_it | COVERED | tests/impctl_cli.rs `impctl_a_dead_dispatcher_says_how_to_start_it` |
