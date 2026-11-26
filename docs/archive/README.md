# Archive

Finished records, kept for reference. Nothing here describes how the code
works today.

- `python-test-coverage.md` and `python-test-coverage/`: the ledger of the
  port from Python to Rust. Every Python unit test (`test_*.py`) is mapped to
  the Rust test that took it over, or to the reason it was dropped. The
  migration finished on 2026-11-15; the ledger is closed.

The Python sources the ledger names (the tests, `rust/conformance/` and the
Python implementation itself) were deleted from the repository on 2026-11-15.
They are at the git tag `last-python`: `git show last-python:<path>`, or check
the tag out in a separate worktree. The answers recorded from that Python live
on as the goldens in `rust/imperatorium/tests/golden/` (see its README); the
port's history is in `docs/native-port.md`.
