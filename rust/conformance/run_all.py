"""Build the binary and run the whole conformance suite against it.

    python rust/conformance/run_all.py [--release] [--no-build]

Exits non-zero if any part fails. Each part can also be run on its own.
"""
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RUST = os.path.dirname(HERE)
PARTS = ["run_policy_tests.py", "differential.py", "hooks_blackbox.py", "sessions_compare.py",
         "items_compare.py", "stats_compare.py", "twin.py", "inbox_twin.py", "notify_twin.py",
         "settings_compare.py", "settings_twin.py", "worktree_twin.py"]
# shadow.py is not here: it compares against a running dispatcher (see its doc).
# The reference these compare against; a part is skipped (and says so) when
# the Python running this cannot import it.
NEEDS = {"sessions_compare.py": "claude_agent_sdk"}


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
        need = NEEDS.get(part)
        if need:
            probe = subprocess.run([sys.executable, "-c", "import %s" % need],
                                   capture_output=True)
            if probe.returncode != 0:
                print("SKIPPED: needs %s (run this with the .venv python)" % need)
                continue
        if subprocess.run([sys.executable, os.path.join(HERE, part)], env=env).returncode != 0:
            failed.append(part)
    print("\nconformance: %s" % ("FAILED: " + ", ".join(failed) if failed else "all passed"))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
