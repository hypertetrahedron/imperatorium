# test_hotkey.py (16 tests)

Summary: 5 COVERED, 10 PORT, 1 MOOT (initial map; see the final counts in the report).

| Test | Verdict | Rust test or reason |
|---|---|---|
| Parsing.test_modifiers_combine | PORT | ctrl+alt+space (mods and 0x20) is not asserted; `hotkey::tests` only checks ctrl+alt+backquote |
| Parsing.test_function_keys | COVERED | `hotkey::tests::parses_like_python` asserts ctrl+shift+f12 = 0x7B (f1 = 0x70 not asserted: PORT as a table) |
| Parsing.test_letters_become_virtual_keys | COVERED | `parses_like_python` ("Win+K" -> 'K') |
| Parsing.test_backquote_by_name_or_symbol | PORT | name is covered, the "`" symbol is not |
| Parsing.test_win_modifier | COVERED | `parses_like_python` |
| Parsing.test_nonsense_is_refused_rather_than_silently_bound | PORT | only "ctrl+alt" and "ctrl+pagedown" are asserted; "" and "semicolon" are not |
| Parsing.test_every_default_candidate_parses | PORT | no test parses DEFAULT_HOTKEYS |
| Candidates.test_an_explicit_override_wins | COVERED | `candidate_order` (Some("x+y") -> 1 candidate; exact value PORTed too) |
| Candidates.test_a_single_string_becomes_one_candidate | COVERED | `candidate_order` |
| Candidates.test_a_list_is_tried_in_order | COVERED | `candidate_order` |
| Candidates.test_missing_or_broken_config_falls_back_to_defaults | PORT | `candidate_order` checks `{}` (len 4) only; Null (missing/unreadable file) and exact defaults are not |
| Candidates.test_an_empty_list_falls_back_rather_than_binding_nothing | PORT | not tested |
| WindowLookup.test_find_window_returns_a_handle_or_none | PORT | `winfocus::titled_windows(None)` + `is_app_title` filter never run in a test |
| WindowLookup.test_only_the_app_window_matches | PORT | `titles` has the positive and Chrome cases; "" is not asserted |
| WindowLookup.test_the_waiting_count_in_the_title_still_matches | PORT | `titles` has "(2) Imperatorium"; the two negatives are not |
| WindowLookup.test_the_title_it_looks_for_matches_the_page | PORT | nothing relates `WINDOW_TITLE` to the rendered shell page |
