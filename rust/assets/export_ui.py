"""Write what the binary embeds that is still computed by Python.

The window's pages are no longer written here: the binary compiles in the
files in imperatorium/web/ itself (see ui.rs), the same ones ui.py reads. What
remains is settings_fields.json, the Settings schema from settings.py, which
settings.rs reads.

    python rust/assets/export_ui.py          # write
    python rust/assets/export_ui.py --check  # exit 1 if a written file is stale

test_native.py runs the check, so a change to settings.py or ui.SHORTCUTS that
is not exported fails the suite rather than shipping a stale schema.
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
    shortcuts = [{"where": w, "keys": k, "what": t} for w, k, t in ui.SHORTCUTS]
    return json.dumps({"fields": fields, "sections": settings.SECTIONS,
                       "restart": settings.RESTART, "shortcuts": shortcuts,
                       # settings.check's rules for the `key` type.
                       "reserved_keys": settings.RESERVED_KEYS,
                       "modifiers": list(settings._ORDER)},
                      indent=1, ensure_ascii=False) + "\n"


def pages():
    return {"settings_fields.json": settings_schema()}


def manifest(rendered):
    return {name: {"sha256": hashlib.sha256(text.encode("utf-8")).hexdigest()}
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
