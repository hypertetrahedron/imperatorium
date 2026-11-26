# test_quick.py

Summary: 13 tests - 5 COVERED already, 8 PORT now COVERED by new Rust tests, 0 MOOT, 0 DIFFERS

| Test | Verdict | Rust test or reason |
|---|---|---|
| KeySetting.test_a_combination_is_stored_in_the_pages_order | COVERED | pure_checks.rs `settings_check_answers_as_python_did` (golden settings_compare: 'Ctrl + Alt + J' -> ctrl+alt+j, 'win+ctrl+x' -> ctrl+win+x, alt+shift+x) + settings::quick_tests::a_combination_is_stored_in_the_pages_order |
| KeySetting.test_shift_alone_or_no_modifier_is_refused | COVERED | new: settings::quick_tests::shift_alone_or_no_modifier_is_refused |
| KeySetting.test_a_key_the_prompt_box_already_uses_is_refused | COVERED | pure_checks.rs `settings_check_answers_as_python_did` (golden: ctrl+enter, ctrl+k, alt+1, alt+9 'is already ...') + settings::quick_tests::reserved_keys_are_refused |
| PhrasesSetting.test_one_per_line_and_commas_are_text | COVERED | pure_checks.rs `settings_check_answers_as_python_did` (golden: 'a,b\nc' -> ['a,b','c']) |
| PhrasesSetting.test_a_list_from_the_form | COVERED | pure_checks.rs `settings_check_answers_as_python_did` (golden: [' a ','','b'] -> ['a','b']) |
| PhrasesSetting.test_limits | COVERED | new: settings::quick_tests::phrase_limits |
| PhrasesSetting.test_defaults_include_the_common_ones | COVERED | new: settings::quick_tests::defaults_include_the_common_ones |
| Shortcuts.test_a_key_named_by_a_setting_is_a_real_setting | COVERED | new: settings::quick_tests::a_key_named_by_a_setting_is_a_real_setting |
| Shortcuts.test_every_reserved_key_is_on_the_list | COVERED | new: settings::quick_tests::every_reserved_key_is_on_the_list |
| Live.test_the_page_gets_the_configured_phrases | COVERED | new: tests/core_quick.rs `the_page_gets_the_configured_phrases` |
| Live.test_a_saved_change_applies_at_once | COVERED | tests/settings_twin.rs `settings_saves_answer_and_write_as_python_did` (branch python-out/twins-a): save quick.key 'Shift + Ctrl + Q' and quick.phrases, then GET /api/quick as recorded |
| Live.test_the_settings_window_gets_the_shortcut_list | COVERED | new: tests/core_quick.rs `the_settings_window_gets_the_shortcut_list` |
| ComboOf.test_matches_what_the_setting_stores | COVERED | new: tests/core_quick.rs `combo_of_matches_what_the_setting_stores (node runs web/shared.js comboOf; skipped without node)` |
