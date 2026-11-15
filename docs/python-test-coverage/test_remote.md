# test_remote.py (62 tests)

Summary: 5 COVERED, 49 PORT, 8 MOOT (initial map). The host-side Python (reader, stats loop, inject script, venv) became the binary's own `remote-channel` / `inbox-send`; ports use a fake `ssh` (IMPERATORIUM_SSH) and never a real host.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Config.test_a_bare_string_is_a_host | COVERED | `remote::tests::loads_hosts_like_python` ("b", default claude path) |
| Config.test_disabled_means_no_hosts | COVERED | `loads_hosts_like_python` (enabled:false) |
| Config.test_entries_without_a_name_are_skipped | COVERED | `loads_hosts_like_python` (label-only entry dropped) |
| Config.test_no_remote_block_is_not_an_error | PORT | `load_hosts({})` / Null not tested |
| Polling.test_claude_is_called_by_absolute_path | PORT | `agents` command text |
| Polling.test_ssh_is_bounded_and_never_prompts | PORT | BatchMode / ConnectTimeout in the argv |
| Polling.test_rows_are_tagged_with_their_host | PORT | |
| Polling.test_a_remote_session_is_never_marked_dispatchable_by_the_poll | PORT | |
| Polling.test_a_missing_binary_is_reported_with_its_reason | PORT | |
| Polling.test_an_old_claude_without_json_is_reported | PORT | |
| Polling.test_non_json_output_is_refused | PORT | |
| Polling.test_a_timeout_becomes_a_remote_error | PORT | |
| Dispatch.test_the_interpreter_path_is_left_unquoted_so_HOME_expands | PORT | `start_background` |
| Dispatch.test_the_prompt_is_quoted_so_spaces_and_quotes_survive | PORT | |
| Dispatch.test_the_working_directory_is_quoted | PORT | |
| Dispatch.test_the_session_id_comes_back | PORT | |
| Dispatch.test_an_empty_prompt_never_reaches_the_wire | PORT | |
| Dispatch.test_a_failure_is_reported_rather_than_claimed_as_success | PORT | |
| Dispatch.test_a_start_with_no_id_is_not_silently_retried | PORT | |
| Backoff.test_a_failing_host_is_parked_and_skipped | PORT | `Fleet` |
| Backoff.test_the_delay_grows_with_each_failure | PORT | |
| Backoff.test_a_parked_host_becomes_due_again_once_the_delay_passes | PORT | |
| Backoff.test_the_delay_is_capped | PORT | |
| Backoff.test_success_clears_the_park | PORT | |
| Backoff.test_one_dead_host_does_not_hide_a_working_one | PORT | |
| Backoff.test_poll_never_raises | PORT | |
| SessionFiles.test_status_and_inbox_come_from_the_hosts_session_files | PORT | |
| SessionFiles.test_a_session_file_without_a_live_process_is_not_a_session | PORT | |
| SessionFiles.test_the_same_call_reads_the_files | PORT | |
| SessionFiles.test_no_session_files_is_not_an_error | PORT | |
| SessionFiles.test_a_torn_session_file_is_skipped | PORT | |
| Inject.test_the_token_goes_on_stdin_never_the_command_line | PORT | `remote::inject` |
| Inject.test_the_socket_path_is_quoted | PORT | |
| Inject.test_it_runs_on_the_hosts_python | MOOT | no host Python in the binary: the host runs `imperatorium inbox-send` (`old_python_fields_are_accepted_and_ignored` covers the config keys) |
| Inject.test_the_writer_actually_writes_to_a_socket | MOOT | the INJECT_SCRIPT is gone; `inbox-send` is the host side and Unix-only (Python skipped this test on Windows too) |
| Inject.test_a_windows_pipe_path_is_refused | PORT | |
| Inject.test_no_token_is_refused_before_the_wire | PORT | |
| Inject.test_an_empty_prompt_is_refused_before_the_wire | PORT | |
| Inject.test_the_hosts_own_error_is_what_is_reported | PORT | |
| Tunnels.test_tunnel_true_gets_a_port_of_its_own | PORT | `loads_hosts_like_python` covers a mixed case; the plain a,b,c sequence 8793/8794/none and remote port 8792 are not asserted |
| Tunnels.test_an_explicit_port_is_kept_and_not_reused | COVERED | `loads_hosts_like_python` (a -> 8794, explicit c -> 8793) |
| Tunnels.test_the_tunnel_never_lands_on_the_dispatchers_own_port | PORT | `Tunnel::argv` |
| Tunnels.test_a_tunnel_that_cannot_forward_exits_rather_than_idling | PORT | |
| Tunnels.test_a_dropped_tunnel_backs_off | PORT | |
| ReaderChannel.test_items_come_back_rendered_by_the_same_code | PORT | `Channel::read` against the real `remote-channel` |
| ReaderChannel.test_one_channel_serves_many_reads | PORT | |
| ReaderChannel.test_a_failed_read_is_reported_and_the_channel_survives | PORT | |
| ReaderChannel.test_a_hung_read_kills_the_channel_rather_than_waiting | PORT | |
| ReaderChannel.test_a_dead_host_is_not_redialled_on_every_poll | PORT | |
| ReaderChannel.test_a_channel_that_exits_reports_why | PORT | |
| ReaderChannel.test_the_interpreter_path_is_left_to_the_hosts_shell | PORT | the binary path stays `"$HOME/..."` in double quotes |
| ReaderChannel.test_a_host_set_up_before_the_rename_keeps_its_reader | MOOT | venv-folder fallback (ccontrol vs imperatorium); the binary is pushed to one fixed path |
| PerHost.test_a_slow_host_does_not_hold_up_another | PORT | |
| PerHost.test_each_poll_that_lands_says_so | PORT | |
| PerHost.test_a_host_that_starts_failing_leaves_the_board | PORT | |
| PerHost.test_refresh_asks_a_parked_host_now | PORT | |
| StatsOnRows.test_each_row_carries_its_hosts_numbers | PORT | |
| StatsOnRows.test_rows_stand_without_them | PORT | |
| StatsOnRows.test_stats_can_be_left_off | MOOT | `Fleet(stats=False)` option does not exist in the binary (stats are always attached, best-effort) - "not in binary" |
| StatsChannel.test_the_cache_and_context_come_back_read_on_the_host | PORT | `Channel::stats` against the real `remote-channel` |
| StatsChannel.test_one_channel_serves_every_poll | PORT | |
| StatsChannel.test_it_runs_on_the_hosts_plain_python | MOOT | no host Python; the binary runs there |
