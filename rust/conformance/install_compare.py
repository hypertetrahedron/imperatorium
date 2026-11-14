"""settings.json as `hooks/install.py --native` writes it and as the binary's
`install-hooks` writes it, from the same starting files, compared byte for
byte (line endings aside) - install, re-install, and uninstall, with and without the extras.
Everything happens in a temporary folder; ~/.claude is never touched.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from twin import binary  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(HERE))
INSTALL = os.path.join(ROOT, "hooks", "install.py")
MOD = os.path.join(ROOT, "mod", "imperatorium")
SEP = ";" if os.name == "nt" else ":"

STARTS = {
    "empty": None,
    "blank object": {},
    "someone else's hooks": {"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "notify-me"}]}],
                                       "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "guard"}]}]},
                             "model": "opus", "permissions": {"allow": ["Bash(ls:*)"]}},
    "legacy ccontrol": {"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "python",
                                                       "args": ["C:\\old\\hooks\\cc_hook.py"],
                                                       "statusMessage": "ccontrol", "async": True}]}]}},
    "python install": {"hooks": {"SessionStart": [{"matcher": "startup|resume|clear", "hooks": [
        {"type": "command", "command": "C:\\py\\python.exe", "args": ["C:\\x\\hooks\\cc_hook.py"],
         "timeout": 5, "statusMessage": "imperatorium", "async": True}]}]},
        "statusLine": {"type": "command", "command": "\"C:\\py\\python.exe\" \"C:\\x\\hooks\\cc_statusline.py\"", "padding": 0}},
    "foreign statusline": {"statusLine": {"type": "command", "command": "my-own-line"}},
    "plugin dirs": {"env": {"CLAUDE_CODE_PLUGIN_DIRS": "C:\\other\\plugin" + SEP + "D:\\more", "FOO": "1"}},
    "non-ascii": {"hooks": {}, "note": "caf\u00e9 \u2014 \u65e5\u672c"},
}
VARIANTS = [
    [],
    ["--tool-hooks"],
    ["--statusline"],
    ["--tool-hooks", "--statusline", "--mod"],
    ["--uninstall"],
]


def main():
    tmp = tempfile.mkdtemp(prefix="imp-install-compare-")
    home = os.path.join(tmp, "home")
    os.makedirs(home)
    with open(os.path.join(home, "policy.json"), "w") as fh:
        json.dump({"park": {"enabled": True, "wait_seconds": 40, "background_wait_seconds": 300}}, fh)
    native = binary()
    failures = n = 0
    for name, start in STARTS.items():
        for variant in VARIANTS:
            for twice in (False, True):
                files = {}
                for who in ("python", "binary"):
                    path = os.path.join(tmp, "%s.json" % who)
                    if os.path.exists(path):
                        os.remove(path)
                    if start is not None:
                        with open(path, "w", encoding="utf-8") as fh:
                            json.dump(start, fh, indent=2)
                    for _ in range(2 if twice else 1):
                        if who == "python":
                            argv = [sys.executable, INSTALL, "--settings", path, "--native", native,
                                    "--home", home] + variant
                        else:
                            argv = [native, "install-hooks", "--settings", path, "--native",
                                    os.path.abspath(native), "--home", home] + \
                                   [a for a in variant if a != "--mod"] + \
                                   (["--mod", MOD] if ("--mod" in variant or "--uninstall" in variant) else [])
                        done = subprocess.run(argv, capture_output=True, text=True)
                        if done.returncode != 0:
                            print("%s %s: %s exited %d: %s" % (name, variant, who, done.returncode,
                                                                done.stderr.strip()[:300]))
                    for leftover in os.listdir(tmp):
                        if leftover.startswith("%s.json.imperatorium-" % who):
                            os.remove(os.path.join(tmp, leftover))
                    # Python writes in text mode: CRLF on Windows. Compared
                    # without that; the binary writes LF, which JSON is
                    # indifferent to.
                    files[who] = (open(path, "rb").read().replace(b"\r\n", b"\n")
                                  if os.path.exists(path) else b"(none)")
                n += 1
                if files["python"] != files["binary"]:
                    failures += 1
                    print("DIFFER  %-22s %-45s %s" % (name, " ".join(variant), "twice" if twice else ""))
                    a, b = files["python"].decode(), files["binary"].decode()
                    for i, (x, y) in enumerate(zip(a.splitlines(), b.splitlines())):
                        if x != y:
                            print("   line %d\n     py  %s\n     bin %s" % (i + 1, x, y))
                            break
                    else:
                        print("   lengths %d vs %d" % (len(a), len(b)))
    shutil.rmtree(tmp, ignore_errors=True)
    print("install_compare: %d cases, %s" % (n, "identical" if not failures else "%d differ" % failures))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
