# test_remote.py (62 tests)

Summary: 4 COVERED, 53 PORT-now-COVERED, 5 MOOT, 0 DIFFERS. 53 tests in `remote::ssh_tests` (src/remote.rs) against a fake `ssh` (`examples/fakessh.rs`, IMPERATORIUM_SSH; scripted by rules.json, logs every call) and, for the channels, the real `imperatorium remote-channel` run as if on the host over temp transcripts. Nothing reaches a real host.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Config.test_a_bare_string_is_a_host | COVERED | `remote::tests::loads_hosts_like_python` ("b", default claude path) |
| Config.test_disabled_means_no_hosts | COVERED | `loads_hosts_like_python` (enabled:false) |
| Config.test_entries_without_a_name_are_skipped | COVERED | `loads_hosts_like_python` (label-only entry dropped) |
| Config.test_no_remote_block_is_not_an_error | PORT-now-COVERED | `remote::ssh_tests::no_remote_block_is_not_an_error` |
| Polling.test_claude_is_called_by_absolute_path | PORT-now-COVERED | `remote::ssh_tests::claude_is_called_by_absolute_path` |
| Polling.test_ssh_is_bounded_and_never_prompts | PORT-now-COVERED | `remote::ssh_tests::ssh_is_bounded_and_never_prompts` |
| Polling.test_rows_are_tagged_with_their_host | PORT-now-COVERED | `remote::ssh_tests::rows_are_tagged_with_their_host` |
| Polling.test_a_remote_session_is_never_marked_dispatchable_by_the_poll | PORT-now-COVERED | `remote::ssh_tests::a_remote_session_is_never_marked_dispatchable_by_the_poll` |
| Polling.test_a_missing_binary_is_reported_with_its_reason | PORT-now-COVERED | `remote::ssh_tests::a_missing_binary_is_reported_with_its_reason` |
| Polling.test_an_old_claude_without_json_is_reported | PORT-now-COVERED | `remote::ssh_tests::an_old_claude_without_json_is_reported` |
| Polling.test_non_json_output_is_refused | PORT-now-COVERED | `remote::ssh_tests::non_json_output_is_refused` |
| Polling.test_a_timeout_becomes_a_remote_error | PORT-now-COVERED | `remote::ssh_tests::a_timeout_becomes_an_error` (through `run`, whose bound `agents` uses) |
| Dispatch.test_the_interpreter_path_is_left_unquoted_so_HOME_expands | PORT-now-COVERED | `remote::ssh_tests::the_interpreter_path_is_left_unquoted_so_home_expands` |
| Dispatch.test_the_prompt_is_quoted_so_spaces_and_quotes_survive | PORT-now-COVERED | `remote::ssh_tests::the_prompt_is_quoted_so_spaces_and_quotes_survive` |
| Dispatch.test_the_working_directory_is_quoted | PORT-now-COVERED | `remote::ssh_tests::the_working_directory_is_quoted` |
| Dispatch.test_the_session_id_comes_back | PORT-now-COVERED | `remote::ssh_tests::the_session_id_comes_back` |
| Dispatch.test_an_empty_prompt_never_reaches_the_wire | PORT-now-COVERED | `remote::ssh_tests::an_empty_prompt_never_reaches_the_wire` |
| Dispatch.test_a_failure_is_reported_rather_than_claimed_as_success | PORT-now-COVERED | `remote::ssh_tests::a_failure_is_reported_rather_than_claimed_as_success` |
| Dispatch.test_a_start_with_no_id_is_not_silently_retried | PORT-now-COVERED | `remote::ssh_tests::a_start_with_no_id_is_not_silently_retried` |
| Backoff.test_a_failing_host_is_parked_and_skipped | PORT-now-COVERED | `remote::ssh_tests::a_failing_host_is_parked_and_skipped` (the skip is `due`, which the poll loop checks) |
| Backoff.test_the_delay_grows_with_each_failure | PORT-now-COVERED | `remote::ssh_tests::the_delay_grows_with_each_failure` |
| Backoff.test_a_parked_host_becomes_due_again_once_the_delay_passes | PORT-now-COVERED | `remote::ssh_tests::a_parked_host_becomes_due_again_once_the_delay_passes` |
| Backoff.test_the_delay_is_capped | PORT-now-COVERED | `remote::ssh_tests::the_delay_is_capped` |
| Backoff.test_success_clears_the_park | PORT-now-COVERED | `remote::ssh_tests::success_clears_the_park` |
| Backoff.test_one_dead_host_does_not_hide_a_working_one | PORT-now-COVERED | `remote::ssh_tests::one_dead_host_does_not_hide_a_working_one` |
| Backoff.test_poll_never_raises | PORT-now-COVERED | `remote::ssh_tests::poll_never_raises` |
| SessionFiles.test_status_and_inbox_come_from_the_hosts_session_files | PORT-now-COVERED | `remote::ssh_tests::status_and_inbox_come_from_the_hosts_session_files` |
| SessionFiles.test_a_session_file_without_a_live_process_is_not_a_session | PORT-now-COVERED | `remote::ssh_tests::a_session_file_without_a_live_process_is_not_a_session` |
| SessionFiles.test_the_same_call_reads_the_files | PORT-now-COVERED | `remote::ssh_tests::the_same_call_reads_the_files` |
| SessionFiles.test_no_session_files_is_not_an_error | PORT-now-COVERED | `remote::ssh_tests::no_session_files_is_not_an_error` |
| SessionFiles.test_a_torn_session_file_is_skipped | PORT-now-COVERED | `remote::ssh_tests::a_torn_session_file_is_skipped` |
| Inject.test_the_token_goes_on_stdin_never_the_command_line | PORT-now-COVERED | `remote::ssh_tests::the_token_goes_on_stdin_never_the_command_line` |
| Inject.test_the_socket_path_is_quoted | PORT-now-COVERED | `remote::ssh_tests::the_socket_path_is_quoted` |
| Inject.test_it_runs_on_the_hosts_python | MOOT | no host Python in the binary: the host runs `imperatorium inbox-send` (`old_python_fields_are_accepted_and_ignored` covers the config keys) |
| Inject.test_the_writer_actually_writes_to_a_socket | MOOT | the INJECT_SCRIPT is gone; `inbox-send` is the host side and Unix-only (Python skipped this test on Windows too) |
| Inject.test_a_windows_pipe_path_is_refused | PORT-now-COVERED | `remote::ssh_tests::a_windows_pipe_path_is_refused` |
| Inject.test_no_token_is_refused_before_the_wire | PORT-now-COVERED | `remote::ssh_tests::no_token_is_refused_before_the_wire` |
| Inject.test_an_empty_prompt_is_refused_before_the_wire | PORT-now-COVERED | `remote::ssh_tests::an_empty_prompt_is_refused_before_the_wire` |
| Inject.test_the_hosts_own_error_is_what_is_reported | PORT-now-COVERED | `remote::ssh_tests::the_hosts_own_error_is_what_is_reported` |
| Tunnels.test_tunnel_true_gets_a_port_of_its_own | PORT-now-COVERED | `remote::ssh_tests::tunnel_true_gets_a_port_of_its_own` |
| Tunnels.test_an_explicit_port_is_kept_and_not_reused | COVERED | `loads_hosts_like_python` (a -> 8794, explicit c -> 8793) |
| Tunnels.test_the_tunnel_never_lands_on_the_dispatchers_own_port | PORT-now-COVERED | `remote::ssh_tests::the_tunnel_never_lands_on_the_dispatchers_own_port` |
| Tunnels.test_a_tunnel_that_cannot_forward_exits_rather_than_idling | PORT-now-COVERED | `remote::ssh_tests::a_tunnel_that_cannot_forward_exits_rather_than_idling` |
| Tunnels.test_a_dropped_tunnel_backs_off | PORT-now-COVERED | `remote::ssh_tests::a_dropped_tunnel_backs_off` |
| ReaderChannel.test_items_come_back_rendered_by_the_same_code | PORT-now-COVERED | `remote::ssh_tests::items_come_back_rendered_by_the_same_code` |
| ReaderChannel.test_one_channel_serves_many_reads | PORT-now-COVERED | `remote::ssh_tests::one_channel_serves_many_reads` |
| ReaderChannel.test_a_failed_read_is_reported_and_the_channel_survives | PORT-now-COVERED | `remote::ssh_tests::a_failed_read_is_reported_and_the_channel_survives` |
| ReaderChannel.test_a_hung_read_kills_the_channel_rather_than_waiting | PORT-now-COVERED | `remote::ssh_tests::a_hung_read_kills_the_channel_rather_than_waiting` |
| ReaderChannel.test_a_dead_host_is_not_redialled_on_every_poll | PORT-now-COVERED | `remote::ssh_tests::a_dead_host_is_not_redialled_on_every_poll` |
| ReaderChannel.test_a_channel_that_exits_reports_why | PORT-now-COVERED | `remote::ssh_tests::a_channel_that_exits_reports_why` |
| ReaderChannel.test_the_interpreter_path_is_left_to_the_hosts_shell | PORT-now-COVERED | `remote::ssh_tests::the_binary_path_is_left_to_the_hosts_shell` |
| ReaderChannel.test_a_host_set_up_before_the_rename_keeps_its_reader | MOOT | venv-folder fallback (ccontrol vs imperatorium); the binary is pushed to one fixed path |
| PerHost.test_a_slow_host_does_not_hold_up_another | PORT-now-COVERED | `remote::ssh_tests::a_slow_host_does_not_hold_up_another` |
| PerHost.test_each_poll_that_lands_says_so | PORT-now-COVERED | `remote::ssh_tests::each_poll_that_lands_says_so` |
| PerHost.test_a_host_that_starts_failing_leaves_the_board | PORT-now-COVERED | `remote::ssh_tests::a_host_that_starts_failing_leaves_the_board` |
| PerHost.test_refresh_asks_a_parked_host_now | PORT-now-COVERED | `remote::ssh_tests::refresh_asks_a_parked_host_now` |
| StatsOnRows.test_each_row_carries_its_hosts_numbers | PORT-now-COVERED | `remote::ssh_tests::each_row_carries_its_hosts_numbers` |
| StatsOnRows.test_rows_stand_without_them | PORT-now-COVERED | `remote::ssh_tests::rows_stand_without_them` |
| StatsOnRows.test_stats_can_be_left_off | MOOT | `Fleet(stats=False)` option does not exist in the binary (stats are always attached, best-effort) - "not in binary" |
| StatsChannel.test_the_cache_and_context_come_back_read_on_the_host | PORT-now-COVERED | `remote::ssh_tests::the_cache_and_context_come_back_read_on_the_host` |
| StatsChannel.test_one_channel_serves_every_poll | PORT-now-COVERED | `remote::ssh_tests::one_channel_serves_every_stats_poll` |
| StatsChannel.test_it_runs_on_the_hosts_plain_python | MOOT | no host Python; the binary runs there |
