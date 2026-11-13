"""slash.parse / refusal / clean_typed / typed_refusal, Python and the
binary, over a corpus of commands and awkward text.

    python rust/conformance/slash_compare.py
"""
import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import binary  # noqa: E402
from imperatorium import slash  # noqa: E402

COMMANDS = sorted(slash.MENU | slash.MENU_BARE | slash.EXIT) + [
    "compact", "clear", "model", "cost", "review", "init", "x", "Model", "MCP"]
TEXTS = (
    ["/" + c for c in COMMANDS]
    + ["/" + c + " some argument" for c in COMMANDS]
    + ["  /model opus  ", "/model opus\nmore", "/model\r", "/compact keep the tests", "//x", "/ x", "/",
       "", "   ", "hello", "hello\nworld", "a\r\nb\tc", "\u2028line\u2029", "x\x0by\x0cz\x85w",
       "!rm -rf /", "#remember this", "!! two", "# !mixed", "  ! spaced", "!/model opus",
       "zero\u200bwidth", "soft\u00adhyphen", "bidi\u202eevil\u202c", "joiner \U0001F468\u200d\U0001F469",
       "bom\ufeffhere", "ctrl\x01\x02\x1b[31mred", "del\x7fete", "c1\x9bcontrol", "tag\U000E0041s",
       "many    spaces   here", "\u3000ideographic\u3000space", "nbsp\u00a0inside", "x" * 8000,
       "x" * 8001, "y " * 4000, "/model " + "z" * 8000, "\u00e9t\u00e9 \u4e2d\u6587", "\U0001F600 emoji",
       "/Exit", "/QUIT now", "/help me", "/resume abc", "/resume", "/add-dir C:\\x", "/add-dir"]
)


def python_answer(text):
    clean, changed = slash.clean_typed(text)
    got = slash.parse(text)
    return {"parse": list(got) if got else None, "refusal": slash.refusal(text),
            "clean": [clean, changed], "typed_refusal": slash.typed_refusal(clean)}


def main():
    want = [python_answer(t) for t in TEXTS]
    lines = "".join(json.dumps({"text": t}) + "\n" for t in TEXTS)
    out = subprocess.run([binary(), "slash-check"], input=lines.encode("utf-8"),
                         capture_output=True, check=True).stdout.decode("utf-8").splitlines()
    got = [json.loads(x) for x in out]
    diffs = [(t, a, b) for t, a, b in zip(TEXTS, want, got)
             if json.dumps(a, sort_keys=True) != json.dumps(b, sort_keys=True)]
    if len(got) != len(TEXTS):
        diffs.append(("<count>", len(TEXTS), len(got)))
    print("slash_compare: %d texts" % len(TEXTS))
    for t, a, b in diffs[:10]:
        print("DIFF %r\n     python %r\n     binary %r" % (t[:80], a, b))
    print("slash_compare: %s" % ("identical" if not diffs else "%d differences" % len(diffs)))
    return 1 if diffs else 0


if __name__ == "__main__":
    sys.exit(main())
