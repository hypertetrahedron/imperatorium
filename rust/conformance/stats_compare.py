"""`imperatorium sessions stats` vs sessionstat.py, on every transcript here.

The tiles' work line and the status panel read these numbers (background
shells, monitors, subagents, wakeups, tokens, cache expiry). Both sides fold
the same file up to its last complete line, at the same pinned clock, and
must agree exactly, types included. Read-only.

    python rust/conformance/stats_compare.py

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
from golden import clocks, corpus_setup, record_dir, write_json, write_jsonl  # noqa: E402

os.chdir(REPO)
from imperatorium import sessionstat  # noqa: E402


def python_stats(sid, now):
    path = sessionstat.transcript_path(sid)
    if not path:
        return None
    with open(path, "rb") as fh:
        data = fh.read()
    data = data[:data.rfind(b"\n") + 1]
    return sessionstat.summarise(sessionstat.parse(data.decode("utf-8", "replace")), now)


def rust_stats(sid, now):
    done = subprocess.run([binary(), "sessions", "stats", sid, "--now", repr(now)],
                          capture_output=True, timeout=120)
    if done.returncode != 0:
        raise RuntimeError(done.stderr.decode("utf-8", "replace"))
    return json.loads(done.stdout.decode("utf-8"))


def record(folder):
    _, places, now = corpus_setup()
    nows = clocks(now)
    write_json(os.path.join(folder, "clocks.json"), nows)
    rows = []
    for sid, _path in session_files():
        rows.append(places({"id": sid, "stats": [attempt(python_stats, sid, t) for t in nows]}))
    write_jsonl(os.path.join(folder, "stats.jsonl"), rows)
    print("recorded %d sessions x %d clocks" % (len(rows), len(nows)))
    return 0


def main():
    folder = record_dir()
    if folder:
        return record(folder)
    t0 = time.time()
    total, live, failures = 0, 0, []
    # Several clocks: cache warm/cold and Monitor/wakeup expiry depend on it.
    for sid, path in session_files():
        total += 1
        for now in (1.0, time.time(), time.time() + 7200.0, 4.0e9):
            for attempt in (1, 2):
                before = stamp(path)
                diff = first_difference(python_stats(sid, now), rust_stats(sid, now))
                if not diff or stamp(path) == before:
                    break
            if diff and stamp(path) != before:
                live += 1
            elif diff:
                failures.append((sid, now, diff))
    print("stats: %d sessions x 4 clocks, %d live, %.1fs" % (total, live, time.time() - t0))
    for sid, now, diff in failures[:20]:
        print("DIFF %s now=%r\n     %s" % (sid, now, diff))
    print("stats_compare: %s" % ("%d differences" % len(failures) if failures else "identical"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
