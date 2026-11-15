# test_voice.py

28 tests: 15 COVERED, 4 PORT, 8 MOOT, 1 DIFFERS
Worker tests run against a stand-in child (std-only program compiled with rustc at test time into %TEMP%/imp-fakevoice, selected with IMPERATORIUM_VOICE_BIN). The rust/voice transcription-guard tests are written but could not be built here.

| Test | Verdict | Rust test or reason |
|---|---|---|
| test_defaults_apply_when_there_is_no_config | COVERED | `voice::worker_tests::defaults_apply_when_there_is_no_config` (the binary's `settings_from(None)`; there is no `load_settings` file reader) |
| test_a_broken_config_falls_back_rather_than_raising | MOOT | `voice.load_settings(path)` is not in the binary. The dispatcher (like Python's, `dispatcher.py` line 273) builds a worker only when config.json has a `voice` object, so a broken or absent config means dictation off in both; `settings_from` itself is covered above. |
| test_config_overrides_only_known_keys | COVERED | `voice::worker_tests::config_overrides_only_known_keys` |
| test_disabling_voice_is_expressed_in_settings | COVERED | `voice::worker_tests::disabling_voice_is_expressed_in_settings` |
| test_the_cpu_gets_a_smaller_model_by_default | COVERED | `voice::worker_tests::the_cpu_gets_a_smaller_model_by_default` (the `cpu_model` setting; `on_cpu` and `compute_type=int8` are not in the binary, see test_cpu_config below) |
| test_a_model_named_in_config_is_kept_on_the_cpu | COVERED | `voice::worker_tests::a_model_named_in_config_is_kept_on_the_cpu` |
| test_an_input_device_index_survives | COVERED | `voice::worker_tests::an_input_device_index_survives` |
| test_empty_input_returns_empty_without_loading | PORT | Written, NOT RUN: `rust/voice` `tests::empty_and_silent_input_return_empty_without_loading`. The worker crate is its own workspace; whisper-rs needs cmake and a C++/GPU toolchain, none on this machine. |
| test_silence_returns_empty_without_loading | PORT | Same test as above (zeros and 0.001 faint input, the binary's threshold is 0.005); written, not run. |
| test_audible_input_does_reach_the_model | PORT | Written, NOT RUN: `rust/voice` `tests::audible_input_does_reach_the_model` (a model that cannot load makes the call fail with `no model file`). |
| test_a_fresh_recorder_is_not_recording | MOOT | Python `voice.Recorder` (sounddevice microphone capture) is not in the binary: the window records audio and posts bytes, which `Worker::transcribe_bytes` sends on. |
| test_stopping_without_starting_yields_nothing | MOOT | Same: `Recorder` is Python-only. |
| test_settings_merge_over_defaults | MOOT | Same: `Recorder` is Python-only (the settings merge itself is `settings_from`, covered above). |
| test_no_gpu_falls_back_to_the_cpu_model | PORT | BLOCKED: the fallback is `Transcriber::load` in rust/voice, which builds a real whisper-rs `WhisperContext`; there is no seam to fake it without editing non-test code. |
| test_a_cpu_config_starts_on_the_cpu_model | DIFFERS | Python: `device: cpu` loads `cpu_model` (small) at int8. Binary (rust/voice `Transcriber::load`, the `Err(_)` arm for `!want_gpu`): loads `self.model` (the configured/default model, e.g. large-v3-turbo) on the CPU; only a GPU failure falls to `cpu_model`. Found by reading; no test (needs whisper). |
| test_audio_goes_through_and_text_comes_back | COVERED | `voice::worker_tests::audio_goes_through_and_text_comes_back` (against a stand-in child compiled from std-only source; Python used a thread over pipes) |
| test_the_worker_learns_where_the_model_runs | COVERED | `voice::worker_tests::the_worker_learns_where_the_model_runs` |
| test_nothing_recorded_never_starts_a_process | COVERED | `voice::worker_tests::nothing_recorded_never_starts_a_process` |
| test_load_builds_the_model_once_per_process | COVERED | `voice::worker_tests::load_builds_the_model_once_per_process` |
| test_a_failure_in_the_child_is_reported_and_the_child_kept | COVERED | `voice::worker_tests::a_failure_in_the_child_is_reported_and_the_child_kept` |
| test_a_child_that_died_is_started_again | COVERED | `voice::worker_tests::a_child_that_died_is_started_again` |
| test_an_idle_child_is_ended_and_a_busy_one_is_not | COVERED | `voice::worker_tests::an_idle_child_is_ended_and_a_busy_one_is_not` (reap takes no `now`: the test backdates `used`) |
| test_zero_means_never_unload | COVERED | `voice::worker_tests::zero_means_never_unload` |
| test_the_child_runs_this_module_without_a_console | COVERED | `voice::worker_tests::the_child_is_run_with_the_worker_flag_and_its_settings` (`--worker <settings json>`; the no-console flag, `proc::no_window`, is not asserted) |
| test_checking_availability_does_not_import_the_model | MOOT | Python import hygiene (`faster_whisper` in `sys.modules`); the binary has no such import. |
| test_availability_is_reported_not_assumed | MOOT | `voice.available()` is Python-only; the binary's analogue is `worker_check` (missing exe/model messages), untested but not in the Python tests. |
| test_at_least_one_input_device_is_visible | MOOT | `voice.list_inputs()` (sounddevice) is Python-only; skipped without the venv. |
| test_sixteen_kilohertz_is_what_the_model_wants | MOOT | `voice.SAMPLE_RATE` is a Python constant; rust/voice has its own `SAMPLE_RATE` and WAV resampling tests (`tests::mixes_and_resamples`). |
