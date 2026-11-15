# test_finished.py

11 tests (by name): PORT-now-COVERED 11.

| Test | Verdict | Rust test or reason |
|---|---|---|
| `Finished.test_only_a_completed_background_job_without_a_process` | COVERED | new: registry.rs::finished_tests::only_a_completed_background_job_without_a_process |
| `Finished.test_when_it_last_did_anything` | COVERED | new: dispatch_finished.rs::when_it_last_did_anything (through the list: a hook event now outranks a start 30 h ago; no time at all is hidden) |
| `Finished.test_the_setting_is_read_safely` | COVERED | new: dispatch_finished.rs::the_setting_is_read_safely (config values -1, "24", true, null, [24] never hide; 24 does) |
| `Attention.test_a_stale_notification_is_cleared_for_a_finished_job` | COVERED | new: dispatch_finished.rs::a_stale_notification_is_cleared_for_a_finished_job |
| `Attention.test_even_a_failure_flag_goes_once_the_job_is_done` | COVERED | new: dispatch_finished.rs::even_a_failure_flag_goes_once_the_job_is_done |
| `Attention.test_a_blocked_job_still_asks` | COVERED | new: dispatch_finished.rs::a_blocked_job_still_asks |
| `Hiding.test_off_by_default` | COVERED | new: dispatch_finished.rs::off_by_default |
| `Hiding.test_only_old_finished_jobs_are_hidden` | COVERED | new: dispatch_finished.rs::only_old_finished_jobs_are_hidden |
| `Hiding.test_it_is_a_setting_applied_at_once` | COVERED | new: dispatch_finished.rs::it_is_a_setting_applied_at_once |
| `Window.test_a_tile_with_no_process_can_be_taken_off_the_list` | COVERED | new: dispatch_finished.rs::a_tile_with_no_process_can_be_taken_off_the_list (fetches /ui/app.js; same string checks) |
| `Window.test_a_finished_job_reads_finished` | COVERED | new: dispatch_finished.rs::a_finished_job_reads_finished (fetches /ui/shared.js) |
