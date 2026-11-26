# test_slash.py

Summary: 22 tests - 21 COVERED, 1 PORT

Also replayed against Python by tests/pure_checks.rs `slash_check_answers_as_python_did` (golden slash_compare: parse, refusal, clean_typed, typed_refusal over ~190 texts).

| Test | Verdict | Rust test or reason |
|---|---|---|
| Parse.test_a_command_and_its_arguments | COVERED | src/slash.rs `slash::py_ported::a_command_and_its_arguments` |
| Parse.test_text_that_only_mentions_a_command_is_a_prompt | COVERED | src/slash.rs `slash::py_ported::text_that_only_mentions_a_command_is_a_prompt` (the Python `None` input has no Rust form: `parse` takes &str) |
| Refusal.test_ordinary_commands_are_typed | COVERED | src/slash.rs `slash::py_ported::ordinary_commands_are_typed` |
| Refusal.test_a_menu_the_window_cannot_drive_is_refused | COVERED | src/slash.rs `slash::py_ported::a_menu_the_window_cannot_drive_is_refused` |
| Refusal.test_a_bare_menu_command_is_told_to_take_an_argument | COVERED | src/slash.rs `slash::py_ported::a_bare_menu_command_is_told_to_take_an_argument` |
| Refusal.test_exit_points_at_close | COVERED | src/slash.rs `slash::py_ported::exit_points_at_close` |
| Refusal.test_a_second_line_would_submit_the_first | COVERED | src/slash.rs `slash::py_ported::a_second_line_would_submit_the_first` |
| Blocked.test_an_idle_local_session_takes_it | COVERED | src/slash.rs `slash::py_ported::an_idle_local_session_takes_it` |
| Blocked.test_never_into_a_turn_or_a_prompt_waiting_on_you | COVERED | src/slash.rs `slash::py_ported::never_into_a_turn_or_a_prompt_waiting_on_you` |
| Blocked.test_never_while_a_permission_decision_is_parked_for_it | COVERED | src/slash.rs `slash::py_ported::never_while_a_permission_decision_is_parked_for_it` |
| Blocked.test_a_remote_session_has_no_console_here | COVERED | src/slash.rs `slash::py_ported::a_remote_session_has_no_console_here` |
| Blocked.test_a_session_without_a_process | COVERED | src/slash.rs `slash::py_ported::a_session_without_a_process` |
| Blocked.test_windows_only | COVERED | src/slash.rs `slash::py_ported::windows_only` (`#[cfg(not(windows))]`: the check is `cfg!(windows)`, fixed at build time, so this runs only on a non-Windows build) |
| Typing.test_clears_the_draft_then_types_the_line_and_enter | PORT | src/slash.rs `slash::py_ported::typing_needs_a_process_id` covers the guard only. NOT TESTABLE as Python did: `type_command` spawns the binary's own `type-into` child with no injectable typer, so the (Ctrl+C, line, Enter) sequence is verified by reading source (slash.rs `type_command`, closing.rs `type_keys`), not by a test. Seam would need a non-test change |
| CleanTyped.test_lines_become_one | COVERED | src/slash.rs `slash::py_ported::lines_become_one` |
| CleanTyped.test_plain_text_is_unchanged | COVERED | src/slash.rs `slash::py_ported::plain_text_is_unchanged` |
| CleanTyped.test_control_and_invisible_characters_go | COVERED | src/slash.rs `slash::py_ported::control_and_invisible_characters_go` |
| CleanTyped.test_emoji_stay | COVERED | src/slash.rs `slash::py_ported::emoji_stay` |
| CleanTyped.test_a_leading_shell_or_memory_key_is_dropped | COVERED | src/slash.rs `slash::py_ported::a_leading_shell_or_memory_key_is_dropped` |
| CleanTyped.test_a_command_is_kept_as_a_command | COVERED | src/slash.rs `slash::py_ported::a_command_is_kept_as_a_command` |
| TypedRefusal.test_empty_and_too_long | COVERED | src/slash.rs `slash::py_ported::typed_refusal_empty_and_too_long` |
| TypedRefusal.test_a_command_gets_the_command_rules | COVERED | src/slash.rs `slash::py_ported::a_command_gets_the_command_rules` |
