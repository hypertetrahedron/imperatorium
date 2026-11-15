# test_native.py

Summary: 9 tests - 4 COVERED already, 2 PORT now COVERED by new Rust tests, 3 MOOT, 0 DIFFERS

| Test | Verdict | Rust test or reason |
|---|---|---|
| NativeHooks.test_every_hook_runs_the_binary | COVERED | install_compare.rs `install_hooks_writes_what_install_py_wrote` (golden install_compare: settings.json from `install.py --native`, byte for byte, every event, args, async, permission --home) + install::tests::hooks_replace_ours_and_keep_others |
| NativeHooks.test_switching_replaces_the_python_hooks | COVERED | install_compare.rs (start 'python install' re-installed natively: replaced, one block per event). The way back to Python hooks is MOOT: the binary has no Python mode |
| NativeHooks.test_uninstall_finds_native_hooks | COVERED | install_compare.rs (variant --uninstall on 'someone else's hooks' and after install) |
| NativeHooks.test_native_statusline_is_recognised_as_ours | COVERED | install_compare.rs (--statusline variants, 'foreign statusline' start) + install::tests::statusline_refuses_a_stranger |
| NativeReader.test_the_chain_is_classified_as_before | COVERED | new: conversation::core_tests::the_chain_is_classified |
| NativeReader.test_a_failing_binary_is_an_error_not_an_empty_chat | COVERED | new: tests/core_convcache.rs `an_unreadable_transcript_is_an_error_not_an_empty_chat` |
| NativeReader.test_titles_take_one_call | MOOT | Python wrapper plumbing (conversation.titles calling native.session_titles once). The binary has `sessions titles <id>...` itself: corpus.rs `synthetic_corpus_reads_as_python_reads_it` compares it |
| NativeReader.test_history_lists_through_the_binary | MOOT | Python history.py calling the binary; the binary's own `sessions list` is compared by corpus.rs (golden sessions_compare) |
| NativeReader.test_the_binary_is_looked_for_where_it_is_installed | MOOT | Python native.candidates() lookup of the binary; the binary does not look for itself |
