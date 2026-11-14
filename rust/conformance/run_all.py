"""Build the binary and run the whole conformance suite against it.

    python rust/conformance/run_all.py [--release] [--no-build] [--require-all]

Exits non-zero if any part fails. Each part can also be run on its own.
A part that cannot run here (a missing module, or the wrong OS) is skipped
and says why; --require-all turns every such skip into a failure.
"""
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RUST = os.path.dirname(HERE)
PARTS = ["run_policy_tests.py", "differential.py", "hooks_blackbox.py", "sessions_compare.py",
         "items_compare.py", "stats_compare.py", "twin.py", "inbox_twin.py", "notify_twin.py",
         "settings_compare.py", "settings_twin.py", "worktree_twin.py", "bg_worktree_twin.py", "slash_compare.py", "projects_compare.py",
         "session_twin.py", "focus_compare.py", "remote_twin.py", "install_compare.py"]
# Not in PARTS, on purpose:
#   shadow.py         compares against a running dispatcher (see its doc).
#   remote_compare.py reads real transcripts on real ssh hosts (HOST arguments).
#   voice_compare.py  needs real WAV clips, a Whisper model and both workers.
#   bench_hooks.py    a benchmark, it asserts nothing.
# The reference these compare against; a part is skipped (and says so) when
# the Python running this cannot import it.
NEEDS = {"sessions_compare.py": "claude_agent_sdk"}
# Parts that only make sense on Windows (named pipes, claude.exe/wt.exe
# stand-ins). They would print "skipped" and exit 0 elsewhere; here that is
# said by the runner, so --require-all can count it.
# (The others are portable as far as their own code goes; none has been run
# off Windows yet.)
WINDOWS_ONLY = ["inbox_twin.py", "bg_worktree_twin.py", "session_twin.py", "focus_compare.py"]


def main():
    release = "--release" in sys.argv
    require_all = "--require-all" in sys.argv
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
        if part in WINDOWS_ONLY and os.name != "nt":
            print("SKIPPED: Windows only")
            if require_all:
                failed.append(part + " (skipped)")
            continue
        need = NEEDS.get(part)
        if need:
            probe = subprocess.run([sys.executable, "-c", "import %s" % need],
                                   capture_output=True)
            if probe.returncode != 0:
                print("SKIPPED: needs %s (pip install -r requirements.txt)" % need)
                if require_all:
                    failed.append(part + " (skipped)")
                continue
        if subprocess.run([sys.executable, os.path.join(HERE, part)], env=env).returncode != 0:
            failed.append(part)
    print("\nconformance: %s" % ("FAILED: " + ", ".join(failed) if failed else "all passed"))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
