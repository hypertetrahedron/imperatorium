# test_closing.py

10 tests (by name): COVERED 2, PORT-now-COVERED 8.

| Test | Verdict | Rust test or reason |
|---|---|---|
| `Close.test_exit_is_typed_and_the_tab_closes` | COVERED | new: dispatch_close.rs::exit_is_typed_and_the_tab_closes (fake console via POST /api/close; fakecli logs Ctrl+C then the line /exit) |
| `Close.test_a_tab_shell_is_sent_exit_after_claude_leaves` | COVERED | new: dispatch_close.rs::a_tab_shell_is_sent_exit_after_claude_leaves (real process tree: cmd.exe copies named windowsterminal / pwsh around a fake claude) |
| `Close.test_a_shell_under_something_else_is_left_alone` | COVERED | new: dispatch_close.rs::a_shell_under_something_else_is_left_alone (host copy named `code`) |
| `Close.test_one_that_will_not_exit_is_killed` | COVERED | new: dispatch_close.rs::one_that_will_not_exit_is_killed |
| `Close.test_one_already_gone_is_not_touched` | COVERED | new: dispatch_close.rs::one_already_gone_is_not_touched |
| `Close.test_no_pid_is_an_error` | COVERED | new: dispatch_close.rs::no_pid_is_an_error (via POST /api/close: a row without a process, an unknown session, no session) |
| `TabShell.test_shell_directly_under_the_terminal` | COVERED | closing.rs::tests::tab_shell_only_under_a_terminal (asserts Some(20) for claude under pwsh under windowsterminal) |
| `TabShell.test_started_by_the_terminal_itself` | COVERED | closing.rs::tests::tab_shell_only_under_a_terminal (its `direct` case asserts None) |
| `TabShell.test_not_a_shell_parent` | COVERED | new: closing.rs::tab_shell_more_tests::not_a_shell_parent |
| `Keys.test_control_keys_round_trip_by_name` | COVERED | new: dispatch_close.rs::control_keys_round_trip_by_name (Rust has no KEYS table; <ctrl-c>/<enter> are named inline and arrive as Ctrl+C and Enter in a fake console) |
