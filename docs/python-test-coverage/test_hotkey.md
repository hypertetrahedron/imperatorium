# test_hotkey.py (16 tests)

Summary: 6 COVERED, 10 PORT-now-COVERED, 0 MOOT, 0 DIFFERS. The registration loop (`run`, broken-config file read in `main`) needs a real RegisterHotKey and is not exercised; `candidates(Null)` covers its fallback.

| Test | Verdict | Rust test or reason |
|---|---|---|
| Parsing.test_modifiers_combine | PORT-now-COVERED | `hotkey::python_tests::modifiers_combine` |
| Parsing.test_function_keys | COVERED | `hotkey::tests::parses_like_python` (f12); f1 also in `python_tests::function_keys_count_from_vk_f1` |
| Parsing.test_letters_become_virtual_keys | COVERED | `parses_like_python` ("Win+K" -> 'K') |
| Parsing.test_backquote_by_name_or_symbol | PORT-now-COVERED | `hotkey::python_tests::backquote_by_name_or_symbol` |
| Parsing.test_win_modifier | COVERED | `parses_like_python` |
| Parsing.test_nonsense_is_refused_rather_than_silently_bound | PORT-now-COVERED | `hotkey::python_tests::nonsense_is_refused_rather_than_silently_bound` |
| Parsing.test_every_default_candidate_parses | PORT-now-COVERED | `hotkey::python_tests::every_default_candidate_parses` |
| Candidates.test_an_explicit_override_wins | COVERED | `candidate_order` (Some("x+y") -> 1 candidate; exact value PORTed too) |
| Candidates.test_a_single_string_becomes_one_candidate | COVERED | `candidate_order` |
| Candidates.test_a_list_is_tried_in_order | COVERED | `candidate_order` |
| Candidates.test_missing_or_broken_config_falls_back_to_defaults | PORT-now-COVERED | `hotkey::python_tests::missing_config_falls_back_to_defaults` (Null = no/unparseable file; the file read itself is in `main`) |
| Candidates.test_an_empty_list_falls_back_rather_than_binding_nothing | PORT-now-COVERED | `hotkey::python_tests::an_empty_list_falls_back_rather_than_binding_nothing` |
| WindowLookup.test_find_window_returns_a_handle_or_none | PORT-now-COVERED | `hotkey::python_tests::window_lookup_returns_what_it_found_or_nothing` |
| WindowLookup.test_only_the_app_window_matches | PORT-now-COVERED | `hotkey::python_tests::only_the_app_window_matches` |
| WindowLookup.test_the_waiting_count_in_the_title_still_matches | PORT-now-COVERED | `hotkey::python_tests::the_waiting_count_in_the_title_still_matches` |
| WindowLookup.test_the_title_it_looks_for_matches_the_page | PORT-now-COVERED | `hotkey::python_tests::the_title_it_looks_for_matches_the_page` (the shell page) |
