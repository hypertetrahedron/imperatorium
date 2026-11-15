# test_closing.py

10 tests (by name): PORT 10.

| Test | Verdict | Rust test or reason |
|---|---|---|
| `Close.test_exit_is_typed_and_the_tab_closes` | PORT | planned: dispatch_close.rs::exit_is_typed_and_the_tab_closes |
| `Close.test_a_tab_shell_is_sent_exit_after_claude_leaves` | PORT | planned: dispatch_close.rs::a_tab_shell_is_sent_exit_after_claude_leaves |
| `Close.test_a_shell_under_something_else_is_left_alone` | PORT | planned: dispatch_close.rs::a_shell_under_something_else_is_left_alone |
| `Close.test_one_that_will_not_exit_is_killed` | PORT | planned: dispatch_close.rs::one_that_will_not_exit_is_killed |
| `Close.test_one_already_gone_is_not_touched` | PORT | planned: dispatch_close.rs::one_already_gone_is_not_touched |
| `Close.test_no_pid_is_an_error` | PORT | planned: dispatch_close.rs::no_pid_is_an_error |
| `TabShell.test_shell_directly_under_the_terminal` | PORT | planned: closing.rs (unit)::shell_directly_under_the_terminal |
| `TabShell.test_started_by_the_terminal_itself` | PORT | planned: closing.rs (unit)::started_by_the_terminal_itself |
| `TabShell.test_not_a_shell_parent` | PORT | planned: closing.rs (unit)::not_a_shell_parent |
| `Keys.test_control_keys_round_trip_by_name` | PORT | planned: dispatch_close.rs::control_keys_round_trip_by_name |
