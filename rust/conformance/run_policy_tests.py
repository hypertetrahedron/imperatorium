"""Run test_policy.py with `policy.decide` answered by the Rust binary.

Every assertion in that file is written against the Python implementation;
here the same assertions judge the port. `policy.is_sensitive` stays Python
(the binary has no separate entry point for it), but every path it is used
on also goes through `decide`, which is the binary.

    python rust/conformance/run_policy_tests.py
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, RustDecider  # noqa: E402

os.chdir(REPO)
from imperatorium import policy  # noqa: E402

import test_policy  # noqa: E402


def main():
    decider = RustDecider()
    policy.decide = decider
    try:
        suite = unittest.defaultTestLoader.loadTestsFromModule(test_policy)
        result = unittest.TextTestRunner(verbosity=1).run(suite)
    finally:
        decider.close()
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
