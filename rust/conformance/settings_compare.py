"""settings.check, Python and the binary, over every field and a corpus of
awkward values: what each would store, or the message it refuses with.

    python rust/conformance/settings_compare.py
"""
import json
import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, binary  # noqa: E402

sys.path.insert(0, REPO)
from imperatorium import settings  # noqa: E402

HERE_DIR = tempfile.gettempdir()
VALUES = [
    True, False, None, 0, 1, -1, 5, 12, 12.0, 12.5, 1e20, 100, 101, 3000, 10 ** 12,
    "", " ", "12", " 12 ", "1_000", "1__0", "_1", "12.0", "abc", "+7", "-3", "0x10",
    [], ["a"], [" a ", "", "b"], [1, None, True, 2.5], {"x": 1},
    "#3A7BD5", " #3a7bd5 ", "#3a7bd", "#3a7bd5\n", "#3a7bd5x", "#GGGGGG",
    "http://host:8080/x/", "https://ntfy.sh", "https://a.b/c d", "http://h\n/", "ftp://x",
    "http://ex\u00e4mple.com", "https://h/\u00e9/", "http://h:/",
    "ntfy-topic_1", "x" * 64, "x" * 65, "has space",
    "en", "EN", " de ", "engl", "e1",
    "a,b\nc", "1,2,3", "5\n50\n95", "80, 95, 80", "0.5", "1e2", "nan", "inf", "-inf",
    ["80", 95, "95.0"], "1,2,3,4,5,6,7", "1,1,1,1,1,1,1", "1,2.5", "50,", " , ",
    "ctrl+alt+j", "Ctrl + Alt + J", "ctrl+", "+x", "shift+a", "alt+shift+x", "win+ctrl+x",
    "ctrl+enter", "ctrl+k", "alt+1", "alt+9", "ctrl+space", "ctrl+ctrl", "foo+x", "ctrl+alt",
    "ctrl+alt+j, win+shift+k", ["ctrl+alt+j", "WIN+K"], ["ctrl+\u00e9"], "ctrl+\u00e9",
    "a/b", "proj<1>", 'say "hi"', "ok-name", "x" * 100, "x" * 101, "back\\slash",
    "line1\r\nline2\u2028line3\x0bline4", "\u00e9t\u00e9", " ", "\n\n",
    ["phrase one", "  ", "two"], "p" * 1001, ["p"] * 51, "\n".join(["p"] * 50),
    "name.with:colons[1]-x_y", "bad name!", "x\n",
    HERE_DIR, HERE_DIR + "\n" + REPO, [HERE_DIR, "C:\\no\\such\\folder"], "C:\\no\\such\\folder",
    "default", "acceptEdits", "plan", "hc", "terminal", "custom-mode",
]


def python_check(field, value, current):
    try:
        return {"ok": settings.check(field, value, current)}
    except settings.Invalid as exc:
        return {"error": str(exc)}
    except Exception as exc:  # what Python would answer with a 500
        return {"raises": type(exc).__name__}


def main():
    cases = []
    for f in settings.FIELDS:
        for v in VALUES:
            currents = [None]
            if f["type"] == "choice":
                currents += ["custom-mode", v if isinstance(v, str) else None]
            for cur in currents:
                cases.append({"key": f["key"], "value": v, "current": cur})
    want = [python_check(settings.BY_KEY[c["key"]], c["value"], c["current"]) for c in cases]
    lines = "".join(json.dumps(c) + "\n" for c in cases)
    out = subprocess.run([binary(), "settings-check"], input=lines.encode("utf-8"),
                         capture_output=True, check=True).stdout.decode("utf-8").splitlines()
    got = [json.loads(x) for x in out]
    if len(got) != len(cases):
        print("settings_compare: %d answers for %d cases" % (len(got), len(cases)))
        return 1
    diffs, raises = [], 0
    for case, a, b in zip(cases, want, got):
        if "raises" in a:
            raises += 1  # Python itself fails; nothing to match
            continue
        if json.dumps(a, sort_keys=True) != json.dumps(b, sort_keys=True):
            diffs.append((case, a, b))
    kinds = sorted({f["type"] for f in settings.FIELDS})
    print("settings_compare: %d cases over %d fields (%d types); %d where Python raises"
          % (len(cases), len(settings.FIELDS), len(kinds), raises))
    for case, a, b in diffs[:15]:
        print("DIFF %s %r (current %r)\n     python %r\n     binary %r"
              % (case["key"], case["value"], case["current"], a, b))
    print("settings_compare: %s" % ("identical" if not diffs else "%d differences" % len(diffs)))
    return 1 if diffs else 0


if __name__ == "__main__":
    sys.exit(main())
