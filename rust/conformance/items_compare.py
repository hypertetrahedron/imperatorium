"""`imperatorium sessions items` vs conversation.py, on every transcript here.

The window renders `to_items`; this checks that the binary's port gives the
same items as the Python, merged and unmerged, for every session on the
machine. Both read the same chain (`sessions chain`), so this isolates the
classification and rendering. Read-only.

A session written to while it is compared is retried once, then reported
as LIVE rather than as a difference.

    python rust/conformance/items_compare.py
"""
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, binary  # noqa: E402
from sessions_compare import first_difference, session_files, stamp  # noqa: E402

os.chdir(REPO)
os.environ.setdefault("IMPERATORIUM_BIN", binary())
from imperatorium import conversation  # noqa: E402


def rust_items(sid, merge):
    args = [binary(), "sessions", "items", sid] + (["--merge"] if merge else [])
    done = subprocess.run(args, capture_output=True, timeout=120)
    if done.returncode != 0:
        raise RuntimeError(done.stderr.decode("utf-8", "replace"))
    return json.loads(done.stdout.decode("utf-8"))


def compare(sid):
    out = {}
    for merge in (False, True):
        py = conversation.read(sid, merge=merge)
        out["merge" if merge else "plain"] = first_difference(py, rust_items(sid, merge))
    return out


def main():
    t0 = time.time()
    total = items = 0
    live, failures = [], []
    for sid, path in session_files():
        total += 1
        for _ in (1, 2):
            before = stamp(path)
            result = compare(sid)
            changed = stamp(path) != before
            bad = {k: v for k, v in result.items() if v}
            if not bad or not changed:
                break
        items += len(rust_items(sid, False))
        if bad and changed:
            live.append(sid)
        elif bad:
            failures.append((sid, bad))
    print("items: %d sessions, %d items, %d live, %.1fs" % (total, items, len(live), time.time() - t0))
    for sid, bad in failures[:20]:
        print("DIFF %s %s" % (sid, bad))
    print("items_compare: %s" % ("%d differences" % len(failures) if failures else "identical"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
