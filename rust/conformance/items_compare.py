"""`imperatorium sessions items` vs conversation.py, on every transcript here.

The window renders `to_items`; this checks that the binary's port gives the
same items as the Python, merged and unmerged, for every session on the
machine. Both read the same chain (`sessions chain`), so this isolates the
classification and rendering. Read-only.

A session written to while it is compared is retried once, then reported
as LIVE rather than as a difference.

    python rust/conformance/items_compare.py

--record DIR [--corpus ROOT [--in-place]] [--now T]: Python only, over a
corpus (default the synthetic one in tests/fixtures/corpus; ROOT holds
projects/) with its mtimes pinned, written as goldens (golden.py,
tests/corpus.rs). NEVER record the real ~/.claude into the repository: the
full check copies it outside (see tests/golden/README.md).
"""
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, binary  # noqa: E402
from sessions_compare import attempt, first_difference, session_files, stamp  # noqa: E402
from golden import corpus_setup, record_dir, write_jsonl  # noqa: E402

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


def record(folder):
    """Python's to_items over the Agent SDK's reading (the reference), not
    over the binary's chain as the comparison does."""
    import claude_agent_sdk as sdk
    _, places, _ = corpus_setup()
    rows = []
    for sid, _path in session_files():
        def read(merge):
            return conversation.to_items(conversation._messages(sdk, sid, None), limit=0, merge=merge)
        rows.append(places({"id": sid, "plain": attempt(read, False), "merge": attempt(read, True)}))
    write_jsonl(os.path.join(folder, "items.jsonl"), rows)
    print("recorded %d sessions" % len(rows))
    return 0


def main():
    folder = record_dir()
    if folder:
        return record(folder)
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
