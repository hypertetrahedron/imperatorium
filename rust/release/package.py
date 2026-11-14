"""A release folder from built binaries: each asset named by its target
triple, SHA256SUMS, and manifest.json, which `imperatorium update` and the
install scripts read.

    python rust/release/package.py --version 0.3.1 --out dist/0.3.1 \
        x86_64-pc-windows-msvc=target/release/imperatorium.exe \
        x86_64-unknown-linux-musl=target/x86_64-unknown-linux-musl/release/imperatorium \
        voice-x86_64-pc-windows-msvc=voice/target/release/imperatorium-voice.exe

A key starting `voice-` is the dictation worker for that triple. Missing
files are an error: a release with a hole in it is worse than none.
"""
import argparse
import hashlib
import json
import os
import shutil
import sys


def asset_name(key):
    if key.startswith("voice-"):
        triple = key[len("voice-"):]
        name = "imperatorium-voice-" + triple
    else:
        triple = key
        name = "imperatorium-" + triple
    return name + (".exe" if "windows" in triple else "")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--version", required=True)
    ap.add_argument("--channel", default=None, help="stable, or preview (default: from the version)")
    ap.add_argument("--out", required=True)
    ap.add_argument("assets", nargs="+", help="KEY=PATH")
    args = ap.parse_args()
    channel = args.channel or ("preview" if "-" in args.version else "stable")
    os.makedirs(args.out, exist_ok=True)
    manifest = {"version": args.version.lstrip("v"), "channel": channel, "assets": {}}
    sums = []
    for spec in args.assets:
        key, _, path = spec.partition("=")
        if not os.path.isfile(path):
            raise SystemExit("missing: %s (%s)" % (path, key))
        name = asset_name(key)
        dst = os.path.join(args.out, name)
        shutil.copyfile(path, dst)
        digest = hashlib.sha256(open(dst, "rb").read()).hexdigest()
        manifest["assets"][key] = {"file": name, "sha256": digest, "size": os.path.getsize(dst)}
        sums.append("%s  %s" % (digest, name))
        print("%-44s %s  %d bytes" % (name, digest[:12], os.path.getsize(dst)))
    with open(os.path.join(args.out, "SHA256SUMS"), "w", newline="\n") as fh:
        fh.write("\n".join(sums) + "\n")
    with open(os.path.join(args.out, "manifest.json"), "w", newline="\n") as fh:
        json.dump(manifest, fh, indent=2)
        fh.write("\n")
    for script in ("install.ps1", "install.sh", "install.cmd"):
        src = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), script)
        if os.path.isfile(src):
            shutil.copyfile(src, os.path.join(args.out, script))
    print("%s %s -> %s" % (channel, manifest["version"], args.out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
