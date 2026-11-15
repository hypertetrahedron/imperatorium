# test_impctl.py (20 tests)

Summary: COVERED 3, PORT 10, MOOT 4, DIFFERS 3 (initial map; PORT rows are ported below)

The Rust CLI (src/cli.rs) is black-boxed here against a recording fake dispatcher (tests/common FakeServer) via IMPERATORIUM_URL.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Help.test_every_command_is_listed_in_help_and_every_listed_one_exists | COVERED | cli.rs `every_command_is_in_the_help`, `every_subcommand_of_main_is_listed` |
| Help.test_help_prints_each_command_with_its_parameters | PORT | `help` output shows each command with its parameters (send, restart); untested end to end |
| Help.test_no_arguments_prints_help_too | PORT | bare `imperatorium` prints the help, exit 0 |
| Help.test_an_unknown_help_topic_is_an_error_not_a_traceback | PORT | `help bogus` exits 1 with "no command"; cli.rs asserts only help_text Err |
| Passthrough.test_leading_flags_reach_the_script | MOOT | Python impctl handed argv to install.py; the binary dispatches in-process (install flags: cli.rs `arguments_read_as_argparse_does`, install.rs `opts`) |
| Passthrough.test_remote_keeps_its_subcommand_and_flags | MOOT | same; `remote add ...` is handled in-binary (remote_cli.rs tests) |
| Passthrough.test_unknown_flags_on_other_commands_are_still_refused | COVERED | cli.rs `arguments_read_as_argparse_does` (--nope refused); port adds the exit code 2 black-box |
| Uninstall.test_a_no_runs_nothing | DIFFERS | `service uninstall` without --yes and no terminal refuses (exit 1): ported. Top-level `uninstall` in the binary (install.rs uninstall_main) does NOT ask at all; Python asked. Not testable safely (it edits settings/removes tasks) |
| Uninstall.test_yes_skips_the_question | PORT | cli.rs `confirmed(_, true)` untested |
| Uninstall.test_install_does_not_ask | PORT | `service install --dry-run` never asks, prints its plan, exit 0 |
| FindSession.test_by_exact_name | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` |
| FindSession.test_by_id_prefix | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` |
| FindSession.test_by_window_title | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` |
| FindSession.test_ambiguous_names_both | COVERED | cli.rs `a_session_is_found_by_id_name_or_title` ('alph' lists both) |
| FindSession.test_resume_falls_back_to_a_past_session_by_id_prefix | PORT | find_any_session via /api/history; untested |
| FindSession.test_a_past_session_that_does_not_match_the_prefix_is_not_taken | PORT | same, failure side |
| Send.test_to_a_session_injects_by_its_full_id | PORT | `send -s` posts {prompt, mode, session_id} to /api/dispatch |
| Send.test_needs_exactly_one_target | PORT | neither / both of -s, -p exit 1 |
| Send.test_a_refusal_exits_non_zero | PORT | prints "refused: nope", exit 1 |
| Down.test_a_dead_dispatcher_says_how_to_start_it | PORT | unreachable dispatcher exit 1 and "imperatorium start" hint |
