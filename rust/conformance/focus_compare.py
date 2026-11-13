"""Focus, Python and the binary, without raising anything: normalise_title
and match_title over made-up titles, then the read-only half on this
machine's real windows - every visible titled window, who owns it, and the
terminal each running `claude` sits in.

Raising itself (SetForegroundWindow and the AttachThreadInput borrow) is not
run here: it would take the focus from whoever is at the keyboard.

    python rust/conformance/focus_compare.py
"""
import json
import os
import random
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import binary  # noqa: E402
from imperatorium import procs, winfocus  # noqa: E402

GLYPHS = ["", "✳ ", "⠋ ", "* ", "... ", "·", "  ", "(1) ", "\U0001F600"]
WORDS = ["Fix", "the", "build", "Straße", "ΟΔΟΣ", "ﬁle", "café", "Build",
         "BUILD", "tests", "İstanbul", "µs", "Ω", "x", "42", "a-b", "x_y", "\t", "　"]


def title(rng):
    return rng.choice(GLYPHS) + " ".join(rng.choice(WORDS) for _ in range(rng.randrange(0, 5)))


def main():
    rng = random.Random(11)
    cases, want = [], []
    texts = [title(rng) for _ in range(400)] + ["", "   ", "✳", "9 lives", "_a", "été", "A B"]
    for t in texts:
        cases.append({"normalise": t})
        want.append(winfocus.normalise_title(t))
    for _ in range(600):
        windows = [[rng.randrange(1, 1 << 20), title(rng)] for _ in range(rng.randrange(0, 6))]
        if windows and rng.random() < 0.5:
            summary = rng.choice(windows)[1]
            summary = summary[: rng.randrange(1, len(summary) + 1)] if summary and rng.random() < 0.3 else summary
        else:
            summary = title(rng)
        cases.append({"match": [summary, windows]})
        want.append(winfocus.match_title(summary, [tuple(w) for w in windows]))
    pure = len(cases)

    # The live, read-only half. Windows come and go and titles carry a
    # spinner, so it is compared as hwnd sets and normalised titles.
    windows = winfocus.titled_windows()
    cases.append({"titled": None})
    want.append(sorted(h for h, _ in windows))
    for h, _ in windows:
        cases.append({"owner": h})
        want.append(winfocus.owner_pid(h))
    claudes = [pid for pid, _ppid, name, *_ in procs.process_table() if name == "claude"] \
        if hasattr(procs, "process_table") else []
    for pid in claudes:
        cases.append({"terminal_for": pid})
        want.append(winfocus.terminal_for(pid))

    lines = "".join(json.dumps(c) + "\n" for c in cases)
    out = subprocess.run([binary(), "focus-check"], input=lines.encode("utf-8"),
                         capture_output=True, check=True).stdout.decode("utf-8").splitlines()
    got = [json.loads(x) for x in out]
    for i, c in enumerate(cases):
        if "titled" in c and i < len(got):
            got[i] = sorted(h for h, _ in got[i])
    diffs = [(c, a, b) for c, a, b in zip(cases, want, got) if a != b]
    if len(got) != len(cases):
        diffs.append(("<count>", len(cases), len(got)))
    print("focus_compare: %d pure cases (%d titles, %d matches); %d windows, %d claude processes"
          % (pure, len(texts), pure - len(texts), len(windows), len(claudes)))
    for c, a, b in diffs[:8]:
        print("DIFF %r\n     python %s\n     binary %s" % (c, json.dumps(a)[:300], json.dumps(b)[:300]))
    print("focus_compare: %s" % ("identical" if not diffs else "%d differences" % len(diffs)))
    return 1 if diffs else 0


if __name__ == "__main__":
    sys.exit(main())
