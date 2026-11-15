# test_voice.py

28 tests: map in progress (initial pass: everything not obviously MOOT is PORT)

| Test | Verdict | Rust test or reason |
|---|---|---|
| test_defaults_apply_when_there_is_no_config | PORT | TBD |
| test_a_broken_config_falls_back_rather_than_raising | MOOT | TBD |
| test_config_overrides_only_known_keys | PORT | TBD |
| test_disabling_voice_is_expressed_in_settings | PORT | TBD |
| test_the_cpu_gets_a_smaller_model_by_default | PORT | TBD |
| test_a_model_named_in_config_is_kept_on_the_cpu | PORT | TBD |
| test_an_input_device_index_survives | PORT | TBD |
| test_empty_input_returns_empty_without_loading | PORT | TBD |
| test_silence_returns_empty_without_loading | PORT | TBD |
| test_audible_input_does_reach_the_model | PORT | TBD |
| test_a_fresh_recorder_is_not_recording | MOOT | TBD |
| test_stopping_without_starting_yields_nothing | MOOT | TBD |
| test_settings_merge_over_defaults | MOOT | TBD |
| test_no_gpu_falls_back_to_the_cpu_model | PORT | TBD |
| test_a_cpu_config_starts_on_the_cpu_model | PORT | TBD |
| test_audio_goes_through_and_text_comes_back | PORT | TBD |
| test_the_worker_learns_where_the_model_runs | PORT | TBD |
| test_nothing_recorded_never_starts_a_process | PORT | TBD |
| test_load_builds_the_model_once_per_process | PORT | TBD |
| test_a_failure_in_the_child_is_reported_and_the_child_kept | PORT | TBD |
| test_a_child_that_died_is_started_again | PORT | TBD |
| test_an_idle_child_is_ended_and_a_busy_one_is_not | PORT | TBD |
| test_zero_means_never_unload | PORT | TBD |
| test_the_child_runs_this_module_without_a_console | PORT | TBD |
| test_checking_availability_does_not_import_the_model | MOOT | TBD |
| test_availability_is_reported_not_assumed | MOOT | TBD |
| test_at_least_one_input_device_is_visible | MOOT | TBD |
| test_sixteen_kilohertz_is_what_the_model_wants | MOOT | TBD |
