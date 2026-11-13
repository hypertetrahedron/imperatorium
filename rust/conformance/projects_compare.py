"""Matching what was said to a project, Python and the binary: _norm,
resolve (difflib's ratio included, to the rounded score), parse_utterance,
and the deep link a launch opens.

The catalogue is this machine's own project list plus made-up names chosen
to collide (prefixes, near-misses, camelCase, punctuation); the spoken names
are those, misspelled, truncated and padded.

    python rust/conformance/projects_compare.py
"""
import json
import os
import random
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, binary  # noqa: E402
from imperatorium import deeplink, projects  # noqa: E402

MADE_UP = ["laser-ledger", "laser-ledger-app", "LaserLedger2", "blue_lamp_project", "gem-trip",
           "gem-trip-4", "imperatorium", "imperatorium-rust", "bookoferrantpages", "a", "ab", "x.y-z",
           "caf\u00e9-menu", "\u4e2d\u6587\u9879\u76ee", "Under_Score__Twice", "dots...and---dashes",
           "MiXeDCase99Name", "zz top", "the-the", "readme"]


def variants(name, rng):
    out = [name, name.upper(), name.replace("-", " "), name[: max(1, len(name) // 2)], name + " app",
           "the " + name, name[1:], name[:-1]]
    if len(name) > 3:
        i = rng.randrange(len(name) - 1)
        out.append(name[:i] + name[i + 1] + name[i] + name[i + 2:])  # a transposition
        out.append(name[:i] + "q" + name[i + 1:])                     # a slip
    return out


def main():
    rng = random.Random(7)
    root = os.path.dirname(REPO)
    real = projects.discover([root])
    catalogue = [{"name": p["name"], "path": p["path"]} for p in real] + \
                [{"name": n, "path": "C:\\made\\up\\" + n} for n in MADE_UP]
    names = [p["name"] for p in catalogue]
    spoken = sorted({v for n in names for v in variants(n, rng)}) + ["", "   ", "?!", "laser", "gem"]
    cases, want = [], []
    for s in spoken:
        cases.append({"spoken": s, "catalogue": catalogue})
        want.append(projects.resolve(s, catalogue))
    # A small catalogue too, where the runner-up gap decides.
    small = catalogue[-len(MADE_UP):][:6]
    for s in spoken[:200]:
        cases.append({"spoken": s, "catalogue": small})
        want.append(projects.resolve(s, small))
    for t in names + ["fooBar", "foo1Bar", "FOOBar", "a\u0301b", "tab\there", "  lots   of  space "]:
        cases.append({"norm": t})
        want.append(projects._norm(t))
    utterances = ["project: laser ledger, add the tests", "Projects laser-ledger: do it", "for project gem, x",
                  "project gem trip, fix the build, then ship", "no prefix at all", "project:, empty",
                  "  PROJECT : spaced ,  prompt  ", "project: name only", "project: a\nmulti, line\nprompt", ""]
    for u in utterances:
        cases.append({"utterance": u})
        want.append(list(projects.parse_utterance(u)))
    links = [(REPO, "do the thing & more?"), (REPO, ""), ("\\\\server\\share", "x"), (REPO + "\\..\\x", "y"),
             (REPO, "\u00e9t\u00e9 \U0001F600 100%"), (REPO, "q" * 5001), ("", "x"), ("relative\\dir", "p")]
    for cwd, prompt in links:
        cases.append({"deeplink": [cwd, prompt]})
        try:
            want.append({"ok": deeplink.build(cwd, prompt)})
        except deeplink.DeepLinkError as exc:
            want.append({"error": str(exc)})
    lines = "".join(json.dumps(c) + "\n" for c in cases)
    out = subprocess.run([binary(), "projects-check"], input=lines.encode("utf-8"),
                         capture_output=True, check=True).stdout.decode("utf-8").splitlines()
    got = [json.loads(x) for x in out]
    diffs = [(c, a, b) for c, a, b in zip(cases, want, got)
             if json.dumps(a, sort_keys=True) != json.dumps(b, sort_keys=True)]
    if len(got) != len(cases):
        diffs.append(("<count>", len(cases), len(got)))
    sure = sum(1 for w in want if isinstance(w, dict) and w.get("confidence") == "certain")
    print("projects_compare: %d cases (%d spoken names over %d projects; %d certain), %d norms, %d utterances, %d links"
          % (len(cases), len(spoken), len(catalogue), sure, len(names) + 6, len(utterances), len(links)))
    for c, a, b in diffs[:8]:
        brief = {k: v for k, v in c.items() if k != "catalogue"} if isinstance(c, dict) else c
        print("DIFF %r\n     python %s\n     binary %s" % (brief, json.dumps(a)[:300], json.dumps(b)[:300]))
    print("projects_compare: %s" % ("identical" if not diffs else "%d differences" % len(diffs)))
    return 1 if diffs else 0


if __name__ == "__main__":
    sys.exit(main())
