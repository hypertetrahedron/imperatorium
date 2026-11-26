# test_convcache.py

Summary: 8 tests - 0 COVERED already, 6 PORT now COVERED by new Rust tests, 0 MOOT, 2 DIFFERS

| Test | Verdict | Rust test or reason |
|---|---|---|
| CacheTests.test_an_unchanged_transcript_is_parsed_once | COVERED | new: tests/core_convcache.rs `an_unchanged_transcript_is_not_parsed_again (same-size rewrite with mtime restored still answers the old text)` |
| CacheTests.test_an_appended_transcript_is_parsed_again | COVERED | new: tests/core_convcache.rs `an_appended_transcript_is_parsed_again` |
| CacheTests.test_the_limit_applies_before_turns_merge | COVERED | new: tests/core_convcache.rs `the_limit_applies_before_turns_merge` |
| CacheTests.test_merging_does_not_change_the_cached_messages | COVERED | new: tests/core_convcache.rs `merging_does_not_change_the_cached_messages` |
| CacheTests.test_without_a_transcript_file_it_reads_through | COVERED | new: tests/core_convcache.rs `without_a_transcript_file_nothing_is_cached` |
| CacheTests.test_least_recently_used_sessions_are_dropped | COVERED | new: tests/core_convcache.rs `least_recently_used_sessions_are_dropped` |
| CacheTests.test_a_weight_budget_keeps_at_least_the_newest | DIFFERS | the binary bounds the cache by count only (CONVS_KEPT = 32 in server/conv.rs); there is no byte-weight budget, so a huge transcript is kept like a small one. Nothing to test |
| CacheTests.test_concurrent_requests_share_one_parse | DIFFERS | the binary has no per-session fill lock: server/conv.rs `conversation` checks the cache, then parses outside any lock, so N simultaneous polls of one changed transcript parse N times. Not observable through the API (only the parse count would show it) |
