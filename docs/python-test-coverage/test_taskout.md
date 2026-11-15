# test_taskout.py

16 tests: 16 COVERED
No DIFFERS.

| Test | Verdict | Rust test or reason |
|---|---|---|
| test_found_by_session_id_whatever_the_project_slug | COVERED | `taskout::output_tests::found_by_session_id_whatever_the_project_slug` (the Rust `tasks_dir` has no `roots` argument; the test points CLAUDE_CODE_TMPDIR at its temp folder) |
| test_unknown_or_malformed_ids_find_nothing | COVERED | `taskout::output_tests::unknown_or_malformed_ids_find_nothing` |
| test_colour_codes_and_redrawn_lines_are_cleaned | COVERED | `taskout::output_tests::colour_codes_and_redrawn_lines_are_cleaned` (also `tests::terminal_output_is_cleaned`) |
| test_a_long_file_is_cut_at_a_line | COVERED | `taskout::output_tests::a_long_file_is_cut_at_a_line` |
| test_a_vanished_file_reads_as_empty | COVERED | `taskout::output_tests::a_vanished_file_reads_as_empty` |
| test_reading_leaves_the_writer_writing | COVERED | `taskout::output_tests::reading_leaves_the_writer_writing` |
| test_scheduled_wakeups_are_listed_with_when_they_fire | COVERED | `taskout::output_tests::scheduled_wakeups_are_listed_with_when_they_fire` |
| test_background_shells_and_monitors_by_their_ids | COVERED | `taskout::output_tests::background_shells_and_monitors_by_their_ids` |
| test_a_foreground_call_gets_the_file_no_record_names | COVERED | `taskout::output_tests::a_foreground_call_gets_the_file_no_record_names` |
| test_a_file_older_than_the_call_is_not_its_output | COVERED | `taskout::output_tests::a_file_older_than_the_call_is_not_its_output` |
| test_no_folder_still_lists_the_tasks | COVERED | `taskout::output_tests::no_folder_still_lists_the_tasks` |
| test_folder_is_beside_the_transcript | COVERED | `taskout::output_tests::folder_is_beside_the_transcript` |
| test_files_found_by_agent_id_and_by_the_call_that_started_them | COVERED | `taskout::output_tests::files_found_by_agent_id_and_by_the_call_that_started_them` |
| test_story_tells_text_calls_and_failures_but_not_the_task | COVERED | `taskout::output_tests::story_tells_text_calls_and_failures_but_not_the_task` (also `tests::a_subagent_is_told_as_lines`) |
| test_a_cut_tail_drops_the_partial_first_line | COVERED | `taskout::output_tests::a_cut_tail_drops_the_partial_first_line` (the Rust tail limit is fixed at 256 KiB, so the test cuts a >256 KiB first line) |
| test_background_and_foreground_subagents_get_their_transcripts | COVERED | `taskout::output_tests::background_and_foreground_subagents_get_their_transcripts` |
