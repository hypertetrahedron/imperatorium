"""subproc: the whole tree goes with the command, unless it is kept.

Run: python test_subproc.py
"""
import os
import subprocess
import sys
import tempfile
import time
import types
import unittest

from imperatorium import procs, subproc

# A child that starts a grandchild, writes the grandchild's pid, then either
# sleeps (to be timed out) or exits (to be let go).
PARENT = (
    "import subprocess, sys, time\n"
    "g = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'],\n"
    "                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,\n"
    "                     stderr=subprocess.DEVNULL)\n"
    "open(sys.argv[1], 'w').write(str(g.pid))\n"
    "if sys.argv[2] == 'sleep':\n"
    "    time.sleep(60)\n"
)

# A parent that exits while its child is still suspended - what is left when
# claude.exe ends inside CreateProcess for git.
SUSPENDED_PARENT = (
    "import subprocess, sys\n"
    "g = subprocess.Popen([sys.executable, '-c', 'pass'], creationflags=0x4,\n"
    "                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,\n"
    "                     stderr=subprocess.DEVNULL)\n"
    "open(sys.argv[1], 'w').write(str(g.pid))\n"
)


def alive(pid):
    return any(row[0] == pid for row in procs.process_table())


def gone_within(pid, seconds):
    end = time.time() + seconds
    while time.time() < end:
        if not alive(pid):
            return True
        time.sleep(0.2)
    return not alive(pid)


class Tree(unittest.TestCase):
    def setUp(self):
        fd, self.pidfile = tempfile.mkstemp()
        os.close(fd)
        self.grandchild = None

    def tearDown(self):
        if self.grandchild and alive(self.grandchild):
            subprocess.run(["taskkill", "/F", "/PID", str(self.grandchild)]
                           if sys.platform == "win32" else
                           ["kill", "-9", str(self.grandchild)], capture_output=True)
        os.remove(self.pidfile)

    def _grandchild(self):
        end = time.time() + 10
        while time.time() < end:
            with open(self.pidfile, encoding="utf-8") as fh:
                text = fh.read().strip()
            if text:
                return int(text)
            time.sleep(0.1)
        self.fail("the child never started its grandchild")

    def test_a_timeout_kills_the_grandchild_too(self):
        # The reported leak: `claude agents` timed out, claude.exe was killed
        # and the git.exe it had started was left behind.
        with self.assertRaises(subprocess.TimeoutExpired):
            subproc.run([sys.executable, "-c", PARENT, self.pidfile, "sleep"],
                        capture_output=True, timeout=3)
        self.grandchild = self._grandchild()
        self.assertTrue(gone_within(self.grandchild, 5),
                        "grandchild %d outlived the timeout" % self.grandchild)

    def test_keep_leaves_what_it_started_running(self):
        # `claude --bg` exits and its session must carry on.
        done = subproc.run([sys.executable, "-c", PARENT, self.pidfile, "exit"],
                           capture_output=True, timeout=30, keep=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.grandchild = self._grandchild()
        time.sleep(0.5)
        self.assertTrue(alive(self.grandchild), "a clean exit killed the grandchild")

    @unittest.skipUnless(sys.platform == "win32", "the sweep is the job's")
    def test_a_clean_exit_kills_what_it_left(self):
        done = subproc.run([sys.executable, "-c", PARENT, self.pidfile, "exit"],
                           capture_output=True, timeout=30)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.grandchild = self._grandchild()
        self.assertTrue(gone_within(self.grandchild, 5),
                        "grandchild %d outlived a clean exit" % self.grandchild)

    @unittest.skipUnless(sys.platform == "win32", "CREATE_SUSPENDED is Windows'")
    def test_a_child_left_suspended_by_a_clean_exit_is_killed(self):
        # The second report: claude.exe exits normally and the git it was
        # still creating stays suspended, its parent gone.
        done = subproc.run([sys.executable, "-c", SUSPENDED_PARENT, self.pidfile],
                           capture_output=True, timeout=30)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.grandchild = self._grandchild()
        self.assertTrue(gone_within(self.grandchild, 5),
                        "suspended grandchild %d was orphaned" % self.grandchild)

    @unittest.skipUnless(sys.platform == "win32", "job objects are Windows'")
    def test_a_sealed_job_refuses_a_late_child(self):
        # The third report: the real git joined the job ~90 ms after claude
        # exited, after a one-shot kill would already have run. kill_tree
        # seals the job before killing, so a late child cannot start at all.
        member = ("import subprocess, sys\n"
                  "print('ready', flush=True)\n"
                  "sys.stdin.readline()\n"
                  "try:\n"
                  "    subprocess.Popen([sys.executable, '-c', 'pass'])\n"
                  "    print('spawned')\n"
                  "except OSError:\n"
                  "    print('refused')\n")
        proc = subproc.popen([sys.executable, "-c", member], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            # Not before it runs: a venv's python.exe is a launcher, and a
            # seal that early would refuse the real interpreter.
            self.assertEqual(proc.stdout.readline().strip(), "ready")
            self.assertTrue(subproc._seal(proc._job))
            out, err = proc.communicate("go\n", timeout=30)
            self.assertEqual(out.strip(), "refused", err)
        finally:
            subproc.kill_tree(proc)

    def test_kill_tree_ends_a_long_lived_child_and_its_children(self):
        proc = subproc.popen([sys.executable, "-c", PARENT, self.pidfile, "sleep"],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
        self.grandchild = self._grandchild()
        subproc.kill_tree(proc)
        subproc.kill_tree(proc)  # twice is harmless
        self.assertIsNotNone(proc.poll())
        self.assertTrue(gone_within(self.grandchild, 5))


class RunLikeSubprocess(unittest.TestCase):
    def test_output_input_and_return_code(self):
        done = subproc.run([sys.executable, "-c",
                            "import sys; d = sys.stdin.read(); print(d.upper()); sys.exit(3)"],
                           input="hi", capture_output=True, text=True, timeout=30)
        self.assertEqual(done.returncode, 3)
        self.assertEqual(done.stdout.strip(), "HI")

    def test_check_raises(self):
        with self.assertRaises(subprocess.CalledProcessError):
            subproc.run([sys.executable, "-c", "raise SystemExit(1)"],
                        capture_output=True, timeout=30, check=True)

    def test_a_missing_program_raises_oserror(self):
        with self.assertRaises(OSError):
            subproc.run(["no-such-program-imperatorium"], capture_output=True, timeout=5)


class Guard(unittest.TestCase):
    def test_a_module_using_subprocess_gets_the_tree_killing_run(self):
        mod = types.SimpleNamespace(subprocess=subprocess)
        subproc.guard(mod)
        self.assertIs(mod.subprocess.run, subproc.run)
        self.assertIs(mod.subprocess.TimeoutExpired, subprocess.TimeoutExpired)
        self.assertIs(subprocess.run.__module__, "subprocess")  # global untouched

    def test_the_sdk_session_reader_is_guarded(self):
        try:
            from claude_agent_sdk._internal import sessions
        except Exception:
            self.skipTest("claude-agent-sdk is not installed")
        subproc.guard_sdk()
        self.assertIs(sessions.subprocess.run, subproc.run)


if __name__ == "__main__":
    unittest.main()
