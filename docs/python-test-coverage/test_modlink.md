# test_modlink.py (20 tests)

Summary: 1 COVERED, 19 PORT (initial map).

| Test | Verdict | Rust test or reason |
|---|---|---|
| LinkTests.test_the_token_is_made_once_and_kept | PORT | `load_token` persistence / length and `authentic` on a loaded token not tested (`authentic` only on a made-up token) |
| LinkTests.test_a_beat_makes_a_mod_live_until_it_stops | PORT | `live`/`state` and LIVE_FOR expiry not tested |
| LinkTests.test_a_prompt_is_handed_out_once_accepted | COVERED | `modlink::tests::a_prompt_is_handed_out_once_then_settles` (claim, re-offer after CLAIM_FOR, accepted, submitted, settled) |
| LinkTests.test_a_dropped_prompt_says_why | PORT | the `dropped` ack and its reason are not tested |
| LinkTests.test_usage_becomes_a_statusline_payload | PORT | only `epoch` is tested, not `statusline_payload` |
| ControlTests.test_a_live_mod_takes_the_prompt_instead_of_the_socket | PORT | no test of `deliver`'s mod branch |
| ControlTests.test_only_the_window_speaks_as_the_person | PORT | as_user / origin framing through /api/dispatch not tested |
| ControlTests.test_without_a_mod_the_socket_is_used_as_before | PORT | no test of the fallback to the inbox |
| ControlTests.test_a_live_mod_makes_a_session_reachable_and_says_busy | PORT | the row's `mod`, `dispatchable`, `status` not tested |
| ControlTests.test_the_queue_remembers_who_queued | PORT | queue `as_user` carried to the mod on drain not tested |
| ControlTests.test_usage_from_a_beat_reaches_the_meters | PORT | beat usage -> usage meters not tested |
| ControlTests.test_the_status_line_names_who_is_waiting | PORT | `mod_status_line` not tested |
| ControlTests.test_the_policy_is_asked_only_once_auto_approval_is_on | PORT | `mod_check` not tested |
| RouteTests.test_a_beat_needs_the_token | PORT | /api/mod/beat 403 / 200 not tested |
| RouteTests.test_a_check_needs_the_token | PORT | /api/mod/check 403 not tested |
| RouteTests.test_a_dispatch_without_the_window_token_is_framed | PORT | not tested |
| RouteTests.test_a_dispatch_from_the_window_speaks_as_the_person | PORT | not tested |
| InstallTests.test_added_once_beside_what_is_there | COVERED | `install::tests::mod_dirs_are_kept` and the install_compare golden ("plugin dirs" x `--mod`, once and twice) |
| InstallTests.test_uninstall_removes_only_ours | PORT | `mod_dirs_are_kept` covers the others-stay half; the env-removed-when-empty half is untested (golden has no installed-mod start) |
| InstallTests.test_a_strange_env_is_left_alone | PORT | `env` that is not an object: no start in the golden, no unit test |
