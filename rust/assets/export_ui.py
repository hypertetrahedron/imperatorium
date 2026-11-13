"""Write the window's pages from imperatorium/ui.py for the binary to embed.

ui.py stays the one source of the pages until the Python is retired: this
renders each page with `ui._with_meta` switched off, so what is written is
the page itself, and the binary inserts the per-start parts exactly as
_with_meta does (the window token, the boot id, and look_head's text sizes,
highlights and colours; see server.rs `with_meta`).

    python rust/assets/export_ui.py          # write
    python rust/assets/export_ui.py --check  # exit 1 if a written page is stale

test_native.py runs the check, so a change to ui.py that is not exported
fails the suite rather than shipping a binary with yesterday's window.
"""
import hashlib
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
OUT = os.path.join(os.path.dirname(HERE), "imperatorium", "assets")
sys.path.insert(0, REPO)

from imperatorium import ui  # noqa: E402

# page -> (renderer, does _with_meta add look_head for it)
PAGES = {
    "shell.html": (lambda: ui.render_shell(), True),
    "matrix.html": (lambda: ui.render_matrix(), True),
    "settings.html": (lambda: ui.render_settings(), False),
}


def settings_schema():
    """settings.FIELDS and SECTIONS as JSON. A default computed from the
    config (`voice.*`: whether config.json has a voice block) is written as
    {"from": "voice_block"}, and computed the same way by settings.rs."""
    from imperatorium import settings
    fields = []
    for f in settings.FIELDS:
        f = dict(f)
        if callable(f.get("default")):
            assert f["default"] is settings._voice_on, f["key"]
            f["default"] = {"from": "voice_block"}
        fields.append(f)
    return json.dumps({"fields": fields, "sections": settings.SECTIONS,
                       "restart": settings.RESTART}, indent=1, ensure_ascii=False) + "\n"


def pages():
    real = ui._with_meta
    ui._with_meta = lambda token, page, boot="", look=False: page
    try:
        out = {name: render() for name, (render, _) in PAGES.items()}
    finally:
        ui._with_meta = real
    out["settings_fields.json"] = settings_schema()
    return out


def manifest(rendered):
    return {name: {"sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
                   "look": PAGES[name][1] if name in PAGES else None}
            for name, text in sorted(rendered.items())}


def main():
    rendered = pages()
    want = manifest(rendered)
    path = os.path.join(OUT, "manifest.json")
    if "--check" in sys.argv:
        try:
            with open(path, encoding="utf-8") as fh:
                have = json.load(fh)
        except (OSError, ValueError):
            have = None
        stale = [n for n in want if (have or {}).get(n) != want[n]]
        for name in stale:
            print("stale: rust/imperatorium/assets/%s (run rust/assets/export_ui.py)" % name)
        return 1 if stale else 0
    os.makedirs(OUT, exist_ok=True)
    for name, text in rendered.items():
        with open(os.path.join(OUT, name), "w", encoding="utf-8", newline="") as fh:
            fh.write(text)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(want, fh, indent=2)
        fh.write("\n")
    print("wrote %s" % ", ".join(sorted(rendered)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
