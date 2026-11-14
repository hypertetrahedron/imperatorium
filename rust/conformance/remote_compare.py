"""Remote reads: Python's Reader (conversation.py under the host's Agent SDK
venv) against the binary's `remote-channel`, on real hosts, read-only.

    python remote_compare.py HOST [HOST...] [--sessions N]

For each host: the N most recent transcripts there are read both ways, with
limit 0 and limit 60, and the items compared as JSON. Also the stats op
against Python's Stats channel (sessionstat.py on the host's python3), with
the clock-dependent cache fields left out. The binary must already be on
the host (`imperatorium remote-probe --read ID HOST` puts it there).
"""
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, ROOT)

from imperatorium import remote  # noqa: E402

BIN = '"$HOME/.claude/imperatorium/bin/imperatorium"'


def recent(host, n):
    cmd = "ls -t $HOME/.claude/projects/*/*.jsonl 2>/dev/null | head -%d" % n
    out = subprocess.run(remote.SSH_BASE + [host, cmd], capture_output=True,
                         text=True, timeout=30).stdout
    return [os.path.basename(l.strip())[:-6] for l in out.splitlines() if l.strip()]


def native(host, reqs):
    body = "".join(json.dumps(dict(r, id=i + 1)) + "\n" for i, r in enumerate(reqs))
    out = subprocess.run(remote.SSH_BASE + [host, BIN + " remote-channel"],
                         input=body, capture_output=True, text=True,
                         encoding="utf-8", timeout=300).stdout
    return [json.loads(l) for l in out.splitlines() if l.strip()]


def canon(v):
    return json.dumps(v, sort_keys=True, ensure_ascii=False)


def main():
    args = sys.argv[1:]
    n = 8
    if "--sessions" in args:
        i = args.index("--sessions")
        n = int(args[i + 1])
        del args[i:i + 2]
    failures = 0
    for name in args:
        host = {"name": name, "claude": remote.DEFAULT_CLAUDE, "label": name,
                "python": remote.DEFAULT_PYTHON}
        ids = recent(name, n)
        reader = remote.Reader(host)
        stats = remote.Stats(host)
        reqs = []
        for sid in ids:
            for limit in (0, 60):
                reqs.append({"op": "read", "session": sid, "limit": limit})
        reqs.append({"op": "stats", "sessions": ids})
        got = native(name, reqs)
        if len(got) != len(reqs):
            print("%s: %d replies to %d requests" % (name, len(got), len(reqs)))
            failures += 1
            continue
        same = items = 0
        for req, rep in zip(reqs[:-1], got[:-1]):
            try:
                want = reader.read(req["session"], limit=req["limit"])
            except remote.RemoteError as exc:
                want = {"error": str(exc)}
            have = ({"items": rep.get("items") or [], "title": rep.get("title") or ""}
                    if rep.get("ok") else {"error": rep.get("error")})
            if "error" in want and "error" in have:
                same += 1
                continue
            if canon(want) == canon(have):
                same += 1
                items += len(have["items"])
            else:
                failures += 1
                print("%s %s limit %s: differ" % (name, req["session"], req["limit"]))
                for k in ("title",):
                    if want.get(k) != have.get(k):
                        print("   %s: %r vs %r" % (k, want.get(k), have.get(k)))
                w, h = want.get("items") or [], have.get("items") or []
                print("   items: python %d, binary %d" % (len(w), len(h)))
                for a, b in zip(w, h):
                    if canon(a) != canon(b):
                        print("   first difference:\n     py  %s\n     bin %s"
                              % (canon(a)[:400], canon(b)[:400]))
                        break
        py_stats = stats.read(ids)
        bin_stats = got[-1].get("stats") or {}

        def strip_clock(d):
            out = {}
            for sid, s in d.items():
                s = dict(s)
                s.pop("cache", None)  # expires_at depends on the moment read
                out[sid] = s
            return out
        stats_same = canon(strip_clock(py_stats)) == canon(strip_clock(bin_stats))
        if not stats_same:
            failures += 1
            for sid in ids:
                a, b = strip_clock(py_stats).get(sid), strip_clock(bin_stats).get(sid)
                if canon(a) != canon(b):
                    print("%s stats %s differ:\n   py  %s\n   bin %s"
                          % (name, sid, canon(a)[:300], canon(b)[:300]))
        reader.close()
        stats.close()
        print("%s: %d sessions, %d/%d reads identical (%d items), stats %s"
              % (name, len(ids), same, len(reqs) - 1, items,
                 "identical" if stats_same else "DIFFER"))
    print("remote_compare: %s" % ("identical" if not failures else "%d differences" % failures))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
