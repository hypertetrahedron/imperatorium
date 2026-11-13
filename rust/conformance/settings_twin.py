"""Saving Settings, Python and the binary, each writing a config.json of its
own in a temporary folder (the homes notify_twin.py sets up).

Compared: the answer to every save (refusals and per-key errors included),
the config.json each writes byte for byte, its .bak, and the GETs that show
what a save applies at once (quick phrases, text sizes and highlights,
usage thresholds, the notification switches) or leaves waiting for a
restart.

    python rust/conformance/settings_twin.py
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from inbox_twin import post_window, window_token  # noqa: E402
from notify_twin import PY_PORT, RS_PORT, start  # noqa: E402
from twin import get, post  # noqa: E402


def same_text(a, root):
    """A twin's answer with its own home and ntfy topic made neutral."""
    text = json.dumps(a, sort_keys=True)
    for side in ("py", "rs"):
        home = os.path.join(root, side + "-home")
        text = text.replace(json.dumps(home)[1:-1], "<home>").replace("twin-" + side, "twin-<side>")
    return text


def main():
    root = tempfile.mkdtemp(prefix="imp-settings-twin-")
    procs = start(root)
    failures, steps = [], 0
    try:
        tokens = {port: window_token(port) for port in (PY_PORT, RS_PORT)}

        def both(body, window=False, label=""):
            nonlocal steps
            steps += 1
            got = []
            for port in (PY_PORT, RS_PORT):
                if window:
                    got.append(post_window(port, "/api/settings", body, tokens[port]))
                else:
                    got.append(post(port, "/api/settings", body))
            if same_text(got[0], root) != same_text(got[1], root):
                failures.append(("POST %s" % (label or body), "%s\n  vs %s" % (
                    same_text(got[0], root)[:600], same_text(got[1], root)[:600])))
            return got[0]

        def files(label):
            for name in ("config.json", "config.json.bak"):
                texts = []
                for side in ("py", "rs"):
                    try:
                        with open(os.path.join(root, side + "-home", name), "rb") as fh:
                            texts.append(fh.read().replace(("twin-" + side).encode(), b"twin-<side>"))
                    except OSError:
                        texts.append(None)
                if texts[0] != texts[1]:
                    failures.append(("%s after %s" % (name, label), "%r\n  vs %r" % tuple(texts)))

        def gets(label):
            for path in ("/api/settings", "/api/quick", "/api/notify"):
                a, b = get(PY_PORT, path), get(RS_PORT, path)
                if same_text(a, root) != same_text(b, root):
                    failures.append(("GET %s after %s" % (path, label), "%s\n  vs %s" % (
                        same_text(a, root)[:400], same_text(b, root)[:400])))
            a, b = get(PY_PORT, "/api/usage")["thresholds"], get(RS_PORT, "/api/usage")["thresholds"]
            if a != b:
                failures.append(("thresholds after %s" % label, "%r vs %r" % (a, b)))
            for port, name in ((PY_PORT, "py"), (RS_PORT, "rs")):
                with urllib.request.urlopen("http://127.0.0.1:%d/api/health" % port, timeout=10) as fh:
                    look = json.loads(fh.read()).get("look")
                if name == "py":
                    want = look
                elif look != want:
                    failures.append(("look after %s" % label, "%r vs %r" % (want, look)))

        gets("start")
        both({"changes": {}}, label="nothing")
        both({"changes": [1]}, label="a list")
        both({}, label="no changes")
        both({"changes": {"chat.send_as_typed": True}}, label="send as typed, not from the window")
        bad = both({"changes": {"chat.font_size": 99, "no.such": 1, "quick.key": "ctrl+k",
                                "budget.thresholds": "50, abc", "notify.ntfy.priority": "loud"}},
                   label="all wrong")
        print("refused: %s" % sorted((bad[1].get("errors") or {}).items()))
        files("refusals")
        good = {"chat.font_size": 17, "chat.cold_cache_tokens": "250000", "quick.phrases": "go on\n\nthanks",
                "quick.key": "Shift + Ctrl + Q", "budget.thresholds": "95, 50,50", "highlight.code": False,
                "highlight.colors.keyword": " #ABCDEF ", "highlight.colors.string": "",
                "background.model": "", "background.permission_mode": "plan",
                "notify.ntfy.enabled": False, "notify.speak.rate": 3, "list.hide_finished_hours": 12,
                "roots": [root], "palette.hotkey": "ctrl+alt+j", "voice.language": "EN",
                "terminal.hc_session": "imptest", "notify.ntfy.topic": "shared-topic"}
        out = both({"changes": good}, label="a good save")
        print("saved %d keys; restart for %r; waiting %r" % (len(out[1].get("saved") or []),
                                                             out[1].get("restart"), out[1].get("waiting")))
        files("a good save")
        gets("a good save")
        # Again, with a colour cleared and a background option removed.
        both({"changes": {"highlight.colors.keyword": "", "background.permission_mode": "",
                          "notify.ntfy.enabled": True}}, label="clearing")
        files("clearing")
        gets("clearing")
        out = both({"changes": {"chat.send_as_typed": True}}, window=True, label="send as typed, from the window")
        print("from the window: ok=%r" % out[1].get("ok"))
        files("send as typed")
    finally:
        for p in procs:
            p.terminate()
        for p in procs:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()
        time.sleep(0.5)
        shutil.rmtree(root, ignore_errors=True)
    for what, diff in failures:
        print("DIFF %s\n     %s" % (what, diff))
    print("settings_twin: %d saves to each; %s" % (
        steps, "identical" if not failures else "%d differences" % len(failures)))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
