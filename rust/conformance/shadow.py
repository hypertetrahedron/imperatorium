"""The Python dispatcher vs `imperatorium serve`, on live data, read-only.

Sends the same GET requests to both and compares the JSON answers. Only GETs
of endpoints the binary has ported; nothing either side does here changes a
session or the state file (the binary never writes; these routes do not
write in Python either).

    python rust/conformance/shadow.py [--python URL] [--rust URL]

Defaults: the live dispatcher on 127.0.0.1:8792 and the binary on 8811
(start it with `imperatorium serve --port 8811 --home <live checkout>`).
A conversation that changed between the two reads is retried, then
reported as LIVE rather than as a difference.
"""
import json
import sys
import time
import urllib.parse
import urllib.request

import os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sessions_compare import first_difference  # noqa: E402

PY = "http://127.0.0.1:8792"
RS = "http://127.0.0.1:8811"
# Differ by design: which process answers, and when it started.
IGNORE = {"/api/health": ("pid", "boot")}


def get(base, path):
    try:
        with urllib.request.urlopen(base + path, timeout=60) as fh:
            return fh.status, json.loads(fh.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        return exc.code, json.loads(exc.read().decode("utf-8") or "null")


def compare(path):
    a, b = get(PY, path), get(RS, path)
    route = path.split("?")[0]
    for key in IGNORE.get(route, ()):
        for code, body in (a, b):
            if isinstance(body, dict):
                body.pop(key, None)
    if a[0] != b[0]:
        return "status %s vs %s" % (a[0], b[0])
    return first_difference(a[1], b[1])


# /api/sessions: what only the Python dispatcher can know, fed by POSTs the
# shadow never receives (step 2 ports them). Always left out of a row:
POST_FED = ("usage", "pr", "mod", "mod_pending", "doing_since")
# ...and on a row the mod reports on, what its beat rewrites (`status`,
# `dispatchable`) and what follows from that status in `work`:
MOD_FED = ("status", "dispatchable")
MOD_WORK = ("alive", "agents", "shells", "monitors", "wakeups", "wakes_at", "processes")


def project(py_row, rs_row, counts):
    """The two rows cut to what both can know, and a tally of what was cut."""
    drop = set(POST_FED)
    if "doing_since" in py_row:       # a live tool hook set `doing`
        drop.add("doing")
    modded = "mod" in py_row
    if modded:
        drop.update(MOD_FED)
    out = []
    for row in (py_row, rs_row):
        row = {k: v for k, v in row.items() if k not in drop}
        if modded and isinstance(row.get("work"), dict):
            row["work"] = {k: v for k, v in row["work"].items() if k not in MOD_WORK}
        out.append(row)
    counts["compared"] += len(out[0]) + len((out[0].get("work") or {}))
    counts["skipped"] += len(py_row) - len(out[0])
    return out


def ordered(value):
    """Rows compare by content: key order inside a row means nothing."""
    if isinstance(value, dict):
        return {k: ordered(value[k]) for k in sorted(value)}
    if isinstance(value, list):
        return [ordered(v) for v in value]
    return value


def compare_sessions(counts):
    _, a = get(PY, "/api/sessions")
    _, b = get(RS, "/api/sessions")
    py = [r for r in a.get("sessions", []) if not r.get("remote")]
    rs = b.get("sessions", [])
    if [r["sessionId"] for r in py] != [r["sessionId"] for r in rs]:
        return "rows %r vs %r" % ([r.get("name") for r in py], [r.get("name") for r in rs])
    for p, r in zip(py, rs):
        if "token" in r:
            return "the binary sent a token for %s" % r.get("name")
        p, r = project(p, r, counts)
        diff = first_difference(ordered(p), ordered(r), "$[%s]" % p.get("name"))
        if diff:
            return diff
    return None


def main():
    global PY, RS
    if "--python" in sys.argv:
        PY = sys.argv[sys.argv.index("--python") + 1]
    if "--rust" in sys.argv:
        RS = sys.argv[sys.argv.index("--rust") + 1]
    t0 = time.time()
    paths = ["/api/health", "/api/no-such-thing", "/api/conversation",
             "/api/conversation?session=00000000-0000-0000-0000-000000000000"]
    _, snap = get(PY, "/api/sessions")
    ids = [r["sessionId"] for r in snap.get("sessions", []) if not r.get("remote")]
    _, hist = get(PY, "/api/history")
    ids += [r["session_id"] for r in hist.get("results", [])[:25] if r["session_id"] not in ids]
    short = [r["id"] for r in snap.get("sessions", []) if r.get("id") and not r.get("remote")][:2]
    for sid in ids + short:
        q = urllib.parse.quote(sid)
        _, base = get(PY, "/api/conversation?session=" + q)
        sig = base.get("sig", "")
        paths += ["/api/conversation?session=%s" % q,
                  "/api/conversation?session=%s&limit=60" % q,
                  "/api/conversation?session=%s&blocks=400" % q,
                  "/api/conversation?session=%s&known=%s" % (q, sig),
                  "/api/conversation?session=%s&known=stale&blocks=3" % q]
    failures, live = [], 0
    for path in paths:
        diff = compare(path)
        if diff:
            time.sleep(0.5)
            diff2 = compare(path)
            if not diff2:
                live += 1
                continue
            failures.append((path, diff2))
    counts = {"compared": 0, "skipped": 0}
    diff = compare_sessions(counts)
    for _ in range(4):           # tiles move: a session working, a cache expiring
        if not diff:
            break
        time.sleep(1.5)
        counts = {"compared": 0, "skipped": 0}
        diff = compare_sessions(counts)
    if diff:
        failures.append(("/api/sessions", diff))
    print("sessions: %d fields compared, %d left out as POST-fed" % (counts["compared"], counts["skipped"]))
    print("shadow: %d requests over %d sessions, %d settled on retry, %.1fs"
          % (len(paths), len(ids), live, time.time() - t0))
    for path, diff in failures[:25]:
        print("DIFF %s\n     %s" % (path, diff))
    print("shadow: %s" % ("%d differences" % len(failures) if failures else "identical"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
