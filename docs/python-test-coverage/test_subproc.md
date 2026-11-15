# test_subproc.py (11 tests)

Summary: COVERED 5 (1 existing + 4 ported), MOOT 3, DIFFERS 3 (2 without a test: suspended leftover and sealed job do not exist in the binary). New tests: proc.rs `subproc_cov` (4 run + 1 ignored)

The binary's counterpart is src/proc.rs: run_timeout (bounded, drains both pipes, `taskkill /T /F` on timeout) and kill_tree. It has no job object, no keep= option and no sweep of what a clean exit leaves.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Tree.test_a_timeout_kills_the_grandchild_too | COVERED | proc.rs `subproc_cov::a_timeout_kills_the_grandchild_too` (windows) |
| Tree.test_keep_leaves_what_it_started_running | COVERED | proc.rs `subproc_cov::a_clean_exit_leaves_what_it_started_running` (windows; the binary's only mode) |
| Tree.test_a_clean_exit_kills_what_it_left | DIFFERS | Python's default killed a grandchild left after a clean exit (job object); the binary's run_timeout leaves it running. Ignored test: proc.rs `subproc_cov::a_clean_exit_kills_what_it_left` |
| Tree.test_a_child_left_suspended_by_a_clean_exit_is_killed | DIFFERS | same cause: no sweep of leftovers after a clean exit in the binary |
| Tree.test_a_sealed_job_refuses_a_late_child | DIFFERS | not in binary: no job object / sealing in proc.rs (remote.rs uses a job only for its ssh child) |
| Tree.test_kill_tree_ends_a_long_lived_child_and_its_children | COVERED | proc.rs `subproc_cov::kill_tree_ends_a_long_lived_child_and_its_children_and_twice_is_harmless` (windows) |
| RunLike.test_output_input_and_return_code | COVERED | output: `collects_both_streams`; exit status and stdout: proc.rs `subproc_cov::the_exit_status_comes_back`; stdin input is not in the binary (stdin is null) |
| RunLike.test_check_raises | MOOT | Python subprocess.run API (check=True); callers in Rust inspect Output.status |
| RunLike.test_a_missing_program_raises_oserror | COVERED | proc.rs `missing_program_is_not_found` |
| Guard.test_a_module_using_subprocess_gets_the_tree_killing_run | MOOT | Python module monkeypatching |
| Guard.test_the_sdk_session_reader_is_guarded | MOOT | Agent SDK is gone from the binary (transcripts are read natively) |
