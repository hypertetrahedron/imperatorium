"""Remote hosts: command construction, parsing, and backoff.

SSH itself is stubbed. The live path was verified against developmenthost1 -
a real background session dispatched over SSH, run, and removed - so what is
pinned here is the logic that was wrong the first time round and the failure
handling that keeps one dead host from stalling a poll.

Run: python test_remote.py
"""
import json
import subprocess
import time
import unittest

from ccontrol import remote

HOST = {"name": "devbox", "claude": "$HOME/.local/bin/claude", "label": "devbox"}


class Done:
    def __init__(self, stdout="", stderr="", returncode=0):
        self.stdout, self.stderr, self.returncode = stdout, stderr, returncode


class Stubbed(unittest.TestCase):
    def setUp(self):
        self.calls = []
        self._orig = remote._run
        remote._run = self.fake
        self.reply = Done("[]")

    def tearDown(self):
        remote._run = self._orig

    def fake(self, argv, timeout):
        self.calls.append(argv)
        if isinstance(self.reply, Exception):
            raise self.reply
        return self.reply

    @property
    def last(self):
        return self.calls[-1]


class Config(unittest.TestCase):
    def test_a_bare_string_is_a_host(self):
        hosts = remote.load_hosts({"remote": {"hosts": ["boxa"]}})
        self.assertEqual(hosts[0]["name"], "boxa")
        self.assertEqual(hosts[0]["claude"], remote.DEFAULT_CLAUDE)

    def test_disabled_means_no_hosts(self):
        self.assertEqual(
            remote.load_hosts({"remote": {"enabled": False, "hosts": ["boxa"]}}), []
        )

    def test_entries_without_a_name_are_skipped(self):
        hosts = remote.load_hosts({"remote": {"hosts": [{"claude": "/x"}, "boxb"]}})
        self.assertEqual([h["name"] for h in hosts], ["boxb"])

    def test_no_remote_block_is_not_an_error(self):
        self.assertEqual(remote.load_hosts({}), [])
        self.assertEqual(remote.load_hosts(None), [])


class Polling(Stubbed):
    def test_claude_is_called_by_absolute_path(self):
        # `claude` is not on PATH for a non-interactive SSH session on Ubuntu:
        # .bashrc returns early. Relying on PATH is why this failed at first.
        remote.agents(HOST)
        self.assertIn("$HOME/.local/bin/claude agents --json --all", self.last[-1])

    def test_ssh_is_bounded_and_never_prompts(self):
        remote.agents(HOST)
        flags = " ".join(self.last)
        self.assertIn("BatchMode=yes", flags)
        self.assertIn("ConnectTimeout", flags)

    def test_rows_are_tagged_with_their_host(self):
        self.reply = Done('[{"sessionId":"a","cwd":"/home/les/x","status":"idle"}]')
        rows = remote.agents(HOST)
        self.assertEqual(rows[0]["host"], "devbox")
        self.assertTrue(rows[0]["remote"])

    def test_a_remote_session_is_never_marked_dispatchable_by_the_poll(self):
        # A token only ever comes from that host's hooks, and the dispatcher
        # does the pairing; the poll alone must not imply it is reachable.
        self.reply = Done('[{"sessionId":"a","dispatchable":true}]')
        self.assertFalse(remote.agents(HOST)[0]["dispatchable"])

    def test_a_missing_binary_is_reported_with_its_reason(self):
        self.reply = Done("", "bash: claude: No such file or directory", 127)
        with self.assertRaises(remote.RemoteError) as caught:
            remote.agents(HOST)
        self.assertIn("No such file", str(caught.exception))

    def test_an_old_claude_without_json_is_reported(self):
        # raspberrypi.local runs 2.1.49, where --json does not exist.
        self.reply = Done("", "error: unknown option '--json'", 1)
        with self.assertRaises(remote.RemoteError):
            remote.agents(HOST)

    def test_non_json_output_is_refused(self):
        self.reply = Done("Welcome to Ubuntu!\n")
        with self.assertRaises(remote.RemoteError):
            remote.agents(HOST)

    def test_a_timeout_becomes_a_remote_error(self):
        self.reply = subprocess.TimeoutExpired(cmd="ssh", timeout=1)
        with self.assertRaises(remote.RemoteError):
            remote.agents(HOST)


class Dispatch(Stubbed):
    def setUp(self):
        super().setUp()
        self.reply = Done("backgrounded · 31886b01")

    def test_the_interpreter_path_is_left_unquoted_so_HOME_expands(self):
        # The first live dispatch failed exactly here: quoting the whole
        # command line turned $HOME into a literal.
        remote.start_background(HOST, "/home/les/proj", "do the thing")
        command = self.last[-1]
        self.assertIn("$HOME/.local/bin/claude", command)
        self.assertNotIn("'$HOME", command)

    def test_the_prompt_is_quoted_so_spaces_and_quotes_survive(self):
        remote.start_background(HOST, "/home/les/proj",
                                "fix the thing; rm -rf /  # not really")
        command = self.last[-1]
        self.assertIn("'fix the thing; rm -rf /  # not really'", command)

    def test_the_working_directory_is_quoted(self):
        remote.start_background(HOST, "/home/les/my proj", "go")
        self.assertIn("cd '/home/les/my proj'", self.last[-1])

    def test_the_session_id_comes_back(self):
        out = remote.start_background(HOST, "/home/les/proj", "go")
        self.assertEqual(out["id"], "31886b01")
        self.assertEqual(out["host"], "devbox")

    def test_an_empty_prompt_never_reaches_the_wire(self):
        with self.assertRaises(remote.RemoteError):
            remote.start_background(HOST, "/home/les/proj", "   ")
        self.assertEqual(self.calls, [])

    def test_a_failure_is_reported_rather_than_claimed_as_success(self):
        self.reply = Done("", "Permission denied (publickey).", 255)
        with self.assertRaises(remote.RemoteError) as caught:
            remote.start_background(HOST, "/home/les/proj", "go")
        self.assertIn("publickey", str(caught.exception))

    def test_a_start_with_no_id_is_not_silently_retried(self):
        # It may well have started; claiming failure would tempt a retry and
        # start a second one.
        self.reply = Done("something happened")
        with self.assertRaises(remote.RemoteError) as caught:
            remote.start_background(HOST, "/home/les/proj", "go")
        self.assertIn("no session id", str(caught.exception))


class Backoff(Stubbed):
    def test_a_failing_host_is_parked_and_skipped(self):
        self.reply = Done("", "unreachable", 255)
        fleet = remote.Fleet([HOST])
        self.assertEqual(fleet.poll(), [])
        self.assertEqual(fleet.status()["devbox"]["fails"], 1)
        before = len(self.calls)
        # Parked: the next poll must not pay the connect timeout again.
        self.assertEqual(fleet.poll(), [])
        self.assertEqual(len(self.calls), before)

    def test_the_delay_grows_with_each_failure(self):
        fleet = remote.Fleet([HOST])
        delays = []
        for _ in range(3):
            fleet._failed("devbox", "unreachable")
            delays.append(fleet.status()["devbox"]["parked_for"])
        self.assertEqual(delays, sorted(delays))
        self.assertGreater(delays[-1], delays[0])

    def test_a_parked_host_becomes_due_again_once_the_delay_passes(self):
        fleet = remote.Fleet([HOST])
        fleet._failed("devbox", "unreachable")
        self.assertFalse(fleet._due("devbox"))
        with fleet._lock:
            fleet._state["devbox"]["next_attempt"] = time.time() - 1
        self.assertTrue(fleet._due("devbox"))

    def test_the_delay_is_capped(self):
        fleet = remote.Fleet([HOST])
        for _ in range(20):
            fleet._failed("devbox", "nope")
        self.assertLessEqual(fleet.status()["devbox"]["parked_for"],
                             remote.BACKOFF_MAX)

    def test_success_clears_the_park(self):
        fleet = remote.Fleet([HOST])
        fleet._failed("devbox", "nope")
        fleet._ok("devbox")
        self.assertEqual(fleet.status()["devbox"]["fails"], 0)
        self.assertTrue(fleet._due("devbox"))

    def test_one_dead_host_does_not_hide_a_working_one(self):
        # The property that matters: servicehost1 being off must not cost you
        # sight of developmenthost1.
        good = {"name": "good", "claude": "c", "label": "good"}
        bad = {"name": "bad", "claude": "c", "label": "bad"}

        def selective(argv, timeout):
            self.calls.append(argv)
            if "bad" in argv:
                return Done("", "unreachable", 255)
            return Done('[{"sessionId":"s1","cwd":"/w"}]')

        remote._run = selective
        rows = remote.Fleet([bad, good]).poll()
        self.assertEqual([r["host"] for r in rows], ["good"])

    def test_poll_never_raises(self):
        self.reply = OSError("ssh vanished")
        self.assertEqual(remote.Fleet([HOST]).poll(), [])


SESSIONS = (
    '[{"sessionId":"a","cwd":"/home/les/x","kind":"interactive","pid":7}]\n'
    '\n' + remote.FILES_MARK + '\n'
    '{"sessionId":"a","pid":7,"status":"waiting","statusUpdatedAt":1790000000000,'
    '"messagingSocketPath":"/tmp/cc-msg-a.sock","name":"x-1"}\n'
    '{"sessionId":"ghost","pid":9,"status":"idle",'
    '"messagingSocketPath":"/tmp/cc-msg-g.sock"}\n'
)


class SessionFiles(Stubbed):
    def test_status_and_inbox_come_from_the_hosts_session_files(self):
        self.reply = Done(SESSIONS)
        row = remote.agents(HOST)[0]
        self.assertEqual(row["status"], "waiting")
        self.assertEqual(row["socket"], "/tmp/cc-msg-a.sock")
        self.assertEqual(row["status_changed_at"], 1790000000.0)
        # `waiting` is a session asking for you - true on any machine.
        self.assertTrue(row["attention"])

    def test_a_session_file_without_a_live_process_is_not_a_session(self):
        self.reply = Done(SESSIONS)
        self.assertEqual([r["sessionId"] for r in remote.agents(HOST)], ["a"])

    def test_the_same_call_reads_the_files(self):
        remote.agents(HOST)
        self.assertEqual(len(self.calls), 1)
        self.assertIn(".claude/sessions/*.json", self.last[-1])

    def test_no_session_files_is_not_an_error(self):
        self.reply = Done('[{"sessionId":"a"}]\n\n' + remote.FILES_MARK + '\n')
        self.assertEqual(remote.agents(HOST)[0]["sessionId"], "a")

    def test_a_torn_session_file_is_skipped(self):
        self.reply = Done('[{"sessionId":"a"}]\n' + remote.FILES_MARK
                          + '\n{"sessionId":"a","stat\n')
        self.assertIsNone(remote.agents(HOST)[0].get("socket"))


class Inject(unittest.TestCase):
    def setUp(self):
        self.calls = []
        self._orig = remote._run_input
        remote._run_input = self.fake
        self.reply = Done(b"", b"", 0)

    def tearDown(self):
        remote._run_input = self._orig

    def fake(self, argv, data, timeout):
        self.calls.append((argv, data))
        return self.reply

    def test_the_token_goes_on_stdin_never_the_command_line(self):
        remote.inject(HOST, "/tmp/cc-msg-a.sock", "s3cret", "do it")
        argv, data = self.calls[0]
        self.assertNotIn("s3cret", " ".join(argv))
        lines = [json.loads(l) for l in data.decode("utf-8").splitlines()]
        self.assertEqual(lines[0], {"type": "auth", "token": "s3cret"})
        self.assertEqual(lines[1]["message"]["content"], "do it")

    def test_the_socket_path_is_quoted(self):
        remote.inject(HOST, "/tmp/my dir/s.sock", "t", "go")
        self.assertIn("'/tmp/my dir/s.sock'", self.calls[0][0][-1])

    def test_it_runs_on_the_hosts_python(self):
        remote.inject(dict(HOST, python="/usr/bin/python3.11"), "/s", "t", "go")
        self.assertTrue(self.calls[0][0][-1].startswith("/usr/bin/python3.11 -c "))

    def test_the_writer_actually_writes_to_a_socket(self):
        # The script is only ever run on another machine; run it here too,
        # against a real Unix socket where the platform has them.
        import socket as _s
        if not hasattr(_s, "AF_UNIX"):
            self.skipTest("no AF_UNIX here")
        import os, subprocess as sp, sys, tempfile, threading
        path = os.path.join(tempfile.mkdtemp(), "s.sock")
        srv = _s.socket(_s.AF_UNIX, _s.SOCK_STREAM)
        srv.bind(path)
        srv.listen(1)
        got = []

        def accept():
            conn, _ = srv.accept()
            got.append(conn.makefile("rb").read())
            conn.close()

        t = threading.Thread(target=accept)
        t.start()
        sp.run([sys.executable, "-c", remote.INJECT_SCRIPT, path],
               input=b"hello\n", check=True, timeout=10)
        t.join(10)
        srv.close()
        self.assertEqual(got, [b"hello\n"])

    def test_a_windows_pipe_path_is_refused(self):
        with self.assertRaises(remote.RemoteError):
            remote.inject(HOST, r"\\.\pipe\x", "t", "go")
        self.assertEqual(self.calls, [])

    def test_no_token_is_refused_before_the_wire(self):
        with self.assertRaises(remote.RemoteError):
            remote.inject(HOST, "/s", None, "go")
        self.assertEqual(self.calls, [])

    def test_an_empty_prompt_is_refused_before_the_wire(self):
        with self.assertRaises(remote.RemoteError):
            remote.inject(HOST, "/s", "t", "  ")
        self.assertEqual(self.calls, [])

    def test_the_hosts_own_error_is_what_is_reported(self):
        self.reply = Done(b"", b"Traceback...\nConnectionRefusedError: [Errno 111]"
                          b" Connection refused\n", 1)
        with self.assertRaises(remote.RemoteError) as caught:
            remote.inject(HOST, "/s", "t", "go")
        self.assertIn("Connection refused", str(caught.exception))


class Tunnels(unittest.TestCase):
    def test_tunnel_true_gets_a_port_of_its_own(self):
        hosts = remote.load_hosts({"remote": {"hosts": [
            {"name": "a", "tunnel": True}, {"name": "b", "tunnel": True},
            {"name": "c"}]}})
        ports = [h["tunnel"] and h["tunnel"]["local_port"] for h in hosts]
        self.assertEqual(ports, [8793, 8794, None])
        self.assertEqual(hosts[0]["tunnel"]["remote_port"], 8792)

    def test_an_explicit_port_is_kept_and_not_reused(self):
        hosts = remote.load_hosts({"remote": {"hosts": [
            {"name": "a", "tunnel": True},
            {"name": "b", "tunnel": {"local_port": 8793}}]}})
        self.assertEqual([h["tunnel"]["local_port"] for h in hosts], [8794, 8793])

    def test_the_tunnel_never_lands_on_the_dispatchers_own_port(self):
        argv = remote.Tunnel(HOST, 8793).argv()
        forward = argv[argv.index("-R") + 1]
        self.assertEqual(forward, "127.0.0.1:8792:127.0.0.1:8793")

    def test_a_tunnel_that_cannot_forward_exits_rather_than_idling(self):
        # Without this ssh stays connected with no forward, and the host's
        # hooks talk to nothing while the tunnel looks up.
        flags = " ".join(remote.Tunnel(HOST, 8793).argv())
        self.assertIn("ExitOnForwardFailure=yes", flags)
        self.assertIn("ServerAliveInterval", flags)
        self.assertIn("BatchMode=yes", flags)

    def test_a_dropped_tunnel_backs_off(self):
        t = remote.Tunnel(HOST, 8793)
        delays = [t._down("gone") for _ in range(4)]
        self.assertEqual(delays, sorted(delays))
        self.assertFalse(t.status()["up"])
        self.assertEqual(t.status()["error"], "gone")



FAKE_SDK = '''
import types
SESSIONS = {
    "s1": [
        {"type": "user", "message": {"role": "user", "content": "how large is this project?"}},
        {"type": "assistant", "message": {"role": "assistant", "content": [
            {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "du -sh ."}}]}},
        {"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": "4.2M\\t."}]}},
        {"type": "assistant", "message": {"role": "assistant", "content": [
            {"type": "text", "text": "About 4.2 MB."}]}},
    ],
}
def get_session_messages(session_id, directory=None):
    if session_id == "boom":
        raise ValueError("transcript unreadable")
    if session_id == "slow":
        import time; time.sleep(30)
    return [types.SimpleNamespace(parent_tool_use_id=None, parent_agent_id=None, **m)
            for m in SESSIONS.get(session_id, [])]
def get_session_info(session_id):
    return types.SimpleNamespace(custom_title=None, summary="Project size")
'''


class ReaderChannel(unittest.TestCase):
    """The shipped host-side loop, run locally against a fake SDK.

    Only SSH is replaced: `argv` starts this machine's python with the same
    boot line, and the same conversation.py source goes down the pipe.
    """

    def setUp(self):
        import os
        import sys
        import tempfile
        self.dir = tempfile.mkdtemp()
        with open(os.path.join(self.dir, "claude_agent_sdk.py"), "w") as fh:
            fh.write(FAKE_SDK)
        self._env = os.environ.get("PYTHONPATH")
        os.environ["PYTHONPATH"] = self.dir
        self.reader = remote.Reader({"name": "local"})
        self.reader.argv = lambda: [sys.executable, "-u", "-c", remote.READER_BOOT]

    def tearDown(self):
        import os
        import shutil
        self.reader.close()
        if self._env is None:
            os.environ.pop("PYTHONPATH", None)
        else:
            os.environ["PYTHONPATH"] = self._env
        shutil.rmtree(self.dir, ignore_errors=True)

    def test_items_come_back_rendered_by_the_same_code(self):
        got = self.reader.read("s1")
        self.assertEqual([i["speaker"] for i in got["items"]], ["you", "claude"])
        # The tool call and its result join Claude's turn, as they do locally.
        kinds = [b["kind"] for b in got["items"][1]["blocks"]]
        self.assertEqual(kinds, ["tool", "result", "text"])
        self.assertEqual(got["title"], "Project size")

    def test_one_channel_serves_many_reads(self):
        self.reader.read("s1")
        proc = self.reader._proc
        for _ in range(3):
            self.reader.read("s1")
        self.assertIs(self.reader._proc, proc)

    def test_a_failed_read_is_reported_and_the_channel_survives(self):
        with self.assertRaises(remote.RemoteError) as caught:
            self.reader.read("boom")
        self.assertIn("transcript unreadable", str(caught.exception))
        self.assertEqual(len(self.reader.read("s1")["items"]), 2)

    def test_a_hung_read_kills_the_channel_rather_than_waiting(self):
        with self.assertRaises(remote.RemoteError) as caught:
            self.reader.read("slow", timeout=2)
        self.assertIn("no reply", str(caught.exception))
        self.assertIsNone(self.reader._proc)

    def test_a_dead_host_is_not_redialled_on_every_poll(self):
        self.reader.argv = lambda: ["ssh-that-does-not-exist-anywhere"]
        with self.assertRaises(remote.RemoteError):
            self.reader.read("s1")
        started = []
        self.reader._start = lambda: started.append(1)
        with self.assertRaises(remote.RemoteError):
            self.reader.read("s1")
        self.assertEqual(started, [])

    def test_a_channel_that_exits_reports_why(self):
        import sys
        self.reader.argv = lambda: [sys.executable, "-c",
                                    "import sys; sys.stderr.write('Permission denied (publickey).\\n')"]
        with self.assertRaises(remote.RemoteError) as caught:
            self.reader.read("s1")
        self.assertIn("publickey", str(caught.exception))

    def test_the_interpreter_path_is_left_to_the_hosts_shell(self):
        argv = remote.Reader({"name": "h"}).argv()
        self.assertTrue(argv[-1].startswith("$HOME/.claude/ccontrol/venv/bin/python -u -c "))


if __name__ == "__main__":
    unittest.main(verbosity=2)
