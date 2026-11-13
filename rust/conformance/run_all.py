"""Build the binary and run the whole conformance suite against it.

    python rust/conformance/run_all.py [--release] [--no-build]

Exits non-zero if any part fails. Each part can also be run on its own.
"""
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RUST = os.path.dirname(HERE)
PARTS = ["run_policy_tests.py", "differential.py", "hooks_blackbox.py"]


def main():
    release = "--release" in sys.argv
    env = dict(os.environ)
    if "--no-build" not in sys.argv:
        cmd = ["cargo", "build"] + (["--release"] if release else [])
        if subprocess.run(cmd, cwd=RUST).returncode != 0:
            return 1
    if release:
        exe = "imperatorium.exe" if os.name == "nt" else "imperatorium"
        env["IMPERATORIUM_BIN"] = os.path.join(RUST, "target", "release", exe)
    failed = []
    for part in PARTS:
        print("== %s" % part, flush=True)
        if subprocess.run([sys.executable, os.path.join(HERE, part)], env=env).returncode != 0:
            failed.append(part)
    print("\nconformance: %s" % ("FAILED: " + ", ".join(failed) if failed else "all passed"))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
