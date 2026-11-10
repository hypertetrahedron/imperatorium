"""herdr integration: the hc client, terminal pairing, typing through hc, and
the herdr-app mods' token.

No real hc is run: a fake runner records argv and answers as hc does.

Run: python test_herdr.py
"""
import base64
import json
import os
import subprocess
import unittest

from imperatorium import dispatcher, hcore, hosting, slash
import test_dispatcher as td

SID = td.SID


class FakeRun:
    """Stands in for subprocess.run: answers by subcommand."""

    def __init__(self, answers=None):
        self.calls = []
        self.answers = answers or {}

    def __call__(self, argv, **kw):
        self.calls.append((argv, kw))
        sub = argv[1]
        if sub == "server":
            sub = "server " + argv[2]
        got = self.answers.get(sub, {})
        if callable(got):
            got = got(argv)
        code = 0
        if isinstance(got, tuple):
            code, got = got
        out = got if isinstance(got, str) else json.dumps(got)
        return subprocess.CompletedProcess(argv, code, out.encode(), b"")


def b64(s):
    return base64.b64encode(s.encode()).decode()


class CleanEnv(unittest.TestCase):
    def test_claude_and_terminal_variables_are_dropped(self):
        env = {"PATH": "p", "CLAUDE_CODE_MESSAGING_TOKEN": "secret", "CLAUDECODE": "1",
               "claude_lower": "x", "HC_TERMINAL_ID": "t3", "HC_SCOPE": "s",
               "HC_SESSION": "keep", "AI_AGENT": "claude"}
        out = hcore.clean_env(env)
        self.assertEqual(out, {"PATH": "p", "HC_SESSION": "keep"})

    def test_a_named_core_is_selected_by_HC_SESSION(self):
        c = hcore.Core(exe="hc", session="imptest", env={"PATH": "p"})
        self.assertEqual(c.env()["HC_SESSION"], "imptest")
        self.assertNotIn("HC_SESSION", hcore.Core(exe="hc", env={"PATH": "p"}).env())


class Calls(unittest.TestCase):
    def core(self, **answers):
        self.run = FakeRun(answers)
        return hcore.Core(exe="hc", runner=self.run, env={"CLAUDE_CODE_X": "1", "PATH": "p"})

    def test_json_goes_before_a_trailing_argv(self):
        c = self.core(run={"address": "t7"})
        got = c.run(["claude", "--session-id", "abc"], cwd="C:/w",
                    labels={"herdr.tab": "tb1", "herdr.space": "sp1"})
        self.assertEqual(got["address"], "t7")
        argv, kw = self.run.calls[0]
        self.assertEqual(argv[:3], ["hc", "run", "--json"])
        self.assertEqual(argv[argv.index("--"):], ["--", "claude", "--session-id", "abc"])
        for flag in ("--bg", "--no-parent", "--keep"):
            self.assertIn(flag, argv)
        self.assertIn("herdr.space=sp1", argv)
        self.assertNotIn("CLAUDE_CODE_X", kw["env"], "terminals must not inherit it")

    def test_an_error_is_raised_with_hcs_message(self):
        c = self.core(send=(1, {"error": {"message": "no terminal t9"}}))
        with self.assertRaises(hcore.HcError) as cm:
            c.send("t9", "hi")
        self.assertIn("no terminal t9", str(cm.exception))

    def test_send_only_if_idle(self):
        c = self.core(send={"ok": True})
        c.send("t1", "hello", only_if="idle")
        argv = self.run.calls[0][0]
        self.assertEqual(argv[-5:], ["--if", "idle", "--", "t1", "hello"])

    def test_text_that_looks_like_an_option_stays_text(self):
        c = self.core(send={"ok": True})
        c.send("t1", "--force approve")
        argv = self.run.calls[0][0]
        self.assertEqual(argv[-3:], ["--", "t1", "--force approve"])
        self.assertNotIn("--force", argv[:-1])

    def test_an_address_from_a_hook_is_checked(self):
        c = self.core(get={})
        for bad in ("-x", "--force", "t1 t2", "", "a/b/c", "t1;rm"):
            with self.assertRaises(hcore.HcError, msg=bad):
                c.get(bad)
        self.assertEqual(self.run.calls, [])
        for good in ("t7", "t1b", "vps/t7", "review-2"):
            self.assertEqual(hcore.address(good), good)

    def test_not_installed(self):
        c = hcore.Core(exe=None, runner=FakeRun())
        c.exe = None
        with self.assertRaises(hcore.HcError):
            c.terminals()


class Ensure(unittest.TestCase):
    def test_a_down_core_is_started_and_checked(self):
        state = {"up": False}

        def status(argv):
            return {"pid": 5} if state["up"] else (1, {"error": "not running"})

        def start(argv):
            state["up"] = True
            return {}
        run = FakeRun({"server status": status, "server start": start,
                       "exec": {"code": 0, "stdout": b64("[]\n")}})
        c = hcore.Core(exe="hc", runner=run)
        self.assertEqual(c.ensure()["pid"], 5)
        subs = [a[0][1:3] for a in run.calls]
        self.assertIn(["server", "start"], subs)

    def test_a_core_carrying_claude_variables_is_refused(self):
        run = FakeRun({"server status": {"pid": 5},
                       "exec": {"code": 0, "stdout": b64('["CLAUDE_CODE_MESSAGING_TOKEN"]\n')}})
        with self.assertRaises(hcore.HcError) as cm:
            hcore.Core(exe="hc", runner=run).ensure()
        self.assertIn("CLAUDE_CODE_MESSAGING_TOKEN", str(cm.exception))


class FakeCore:
    def __init__(self):
        self.did = []

    def keys(self, address, *names):
        self.did.append(("keys", address) + names)

    def send(self, address, text, force=False, only_if=None):
        self.did.append(("send", address, text, only_if))


class Pairing(td.SlashCommands):
    """A session started in an hc terminal is known by that terminal."""

    def hc_hook(self, event="SessionStart", terminal="t4", core="imptest"):
        return self.control.on_hook({
            "hook_event_name": event, "session_id": SID, "cwd": "C:/w",
            "messaging_socket": "\\\\.\\pipe\\x", "messaging_token": "tok",
            "hc_terminal": terminal, "hc_session": core})

    def row(self):
        self.control.invalidate()
        return next(r for r in self.control.sessions()["sessions"] if r["sessionId"] == SID)

    def test_the_row_carries_its_terminal(self):
        self.live()
        self.hc_hook()
        self.assertEqual(self.row()["hc"], {"address": "t4", "core": "imptest"})

    def test_a_later_event_repairs_it(self):
        self.live()
        self.start_hook()
        self.assertFalse(self.row().get("hc"))
        self.hc_hook(event="Stop")
        self.assertEqual(self.row()["hc"]["address"], "t4")

    def test_a_claude_started_inside_the_terminal_is_not_paired_with_it(self):
        from imperatorium import procs
        self.live()

        class Core:
            def get(self, a):
                return {"pid": 20}
        self.control.hc_core = lambda name=None: Core()
        orig = procs.process_table
        procs.process_table = lambda: Ownership.TABLE
        try:
            self.control.on_hook({"hook_event_name": "SessionStart", "session_id": SID,
                                  "cwd": "C:/w", "hc_terminal": "t4", "hc_session": "imptest",
                                  "claude_pid": "50"})
            self.assertFalse(self.row().get("hc"))
            self.control.on_hook({"hook_event_name": "Stop", "session_id": SID,
                                  "cwd": "C:/w", "hc_terminal": "t4", "hc_session": "imptest",
                                  "claude_pid": "30"})
            self.assertEqual(self.row()["hc"]["address"], "t4")
        finally:
            procs.process_table = orig

    def test_a_bad_address_in_a_hook_is_dropped(self):
        self.live()
        self.hc_hook(terminal="--force")
        self.assertFalse(self.row().get("hc"))

    def test_resumed_outside_hc_it_is_forgotten(self):
        self.live()
        self.hc_hook()
        self.start_hook()
        self.assertFalse(self.row().get("hc"))

    def test_a_slash_command_is_typed_through_hc(self):
        self.live()
        self.agent_rows[0]["pid"] = 4242
        self.hc_hook()
        fake = FakeCore()
        self.control.hc_core = lambda name=None: fake
        out = self.control.dispatch(prompt="/compact", session_id=SID)
        self.assertEqual(out["action"], "command")
        self.assertEqual(fake.did, [("keys", "t4", "ctrl+c"),
                                    ("send", "t4", "/compact", "idle")])
        self.assertEqual(self.typed, [], "not through the console")

    def test_an_hc_failure_is_a_refusal_not_a_console_fallback(self):
        self.live()
        self.agent_rows[0]["pid"] = 4242
        self.hc_hook()

        class Broken(FakeCore):
            def send(self, *a, **k):
                raise hcore.HcError("blocked")
        self.control.hc_core = lambda name=None: Broken()
        out = self.control.dispatch(prompt="/compact", session_id=SID)
        self.assertFalse(out["ok"])
        self.assertEqual(self.typed, [])


class HerdrToken(td.Origins):
    def call(self, headers):
        import urllib.error
        import urllib.request
        h = {"Content-Type": "application/json"}
        h.update(headers)
        req = urllib.request.Request("http://127.0.0.1:%d/api/typed" % self.port,
                                     data=json.dumps({"session_id": SID, "prompt": "hi"}).encode(),
                                     headers=h)
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                return resp.status, json.loads(resp.read())
        except urllib.error.HTTPError as exc:
            return exc.code, json.loads(exc.read())

    def test_the_mods_token_counts_as_the_window(self):
        code, body = self.call({"X-Imperatorium-Herdr": self.control.herdr_token})
        self.assertEqual(code, 200)
        self.assertIn("Settings", body["error"])  # reached; Send As Typed is off

    def test_a_wrong_or_missing_token_is_refused(self):
        for h in ({}, {"X-Imperatorium-Herdr": "guessed"},
                  {"X-Imperatorium-Herdr": self.control.window_token}):
            self.assertEqual(self.call(h)[0], 403, h)

    def test_a_non_ascii_token_is_a_refusal_not_an_error(self):
        import http.client
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        conn.putrequest("POST", "/api/typed")
        conn.putheader("Content-Type", "application/json")
        conn.putheader("X-Imperatorium-Herdr", "tést".encode("latin-1"))
        body = json.dumps({"session_id": SID, "prompt": "hi"}).encode()
        conn.putheader("Content-Length", str(len(body)))
        conn.endheaders(body)
        resp = conn.getresponse()
        resp.read()
        conn.close()
        self.assertEqual(resp.status, 403)

    def test_a_browser_page_cannot_use_it(self):
        code, _ = self.call({"X-Imperatorium-Herdr": self.control.herdr_token,
                             "Origin": "http://127.0.0.1:%d" % self.port})
        self.assertEqual(code, 403)

    def test_it_is_written_beside_the_state(self):
        state = os.path.join(self.dir, "state.json")
        dispatcher.write_herdr_token(self.control, state)
        with open(dispatcher.herdr_token_path(state), encoding="utf-8") as fh:
            self.assertEqual(fh.read(), self.control.herdr_token)


class Dictate(td.Base):
    """Recorded by the dispatcher for the herdr mods, which have no mic."""

    class Rec:
        def __init__(self, settings):
            self.recording = False
            self.audio = None

        def start(self):
            self.recording = True

        def stop(self):
            self.recording = False
            return self.audio

    class Voice:
        settings = {}
        where = {"device": "cpu"}

        def __init__(self):
            self.got = []

        def warm(self):
            pass

        def transcribe_bytes(self, data, hint=None):
            self.got.append(data)
            return "hello there"

    def test_start_then_stop_gives_the_text(self):
        import numpy as np
        self.control._voice = v = self.Voice()
        made = []

        def factory(settings):
            made.append(self.Rec(settings))
            return made[-1]
        self.assertTrue(self.control.dictate("start", factory)["recording"])
        made[0].audio = np.zeros(16000, dtype="float32")
        out = self.control.dictate("stop")
        self.assertEqual(out["text"], "hello there")
        self.assertTrue(v.got[0].startswith(b"RIFF"), "sent as WAV")

    def test_a_mic_left_open_is_closed(self):
        self.control._voice = self.Voice()
        self.control.dictate("start", self.Rec)
        delay, fn = self.scheduled[-1]
        self.assertGreater(delay, 100)
        fn()
        self.assertFalse(self.control._recorder.recording)

    def test_an_expiry_from_an_earlier_start_does_nothing(self):
        self.control._voice = self.Voice()
        self.control.dictate("start", self.Rec)
        _, old = self.scheduled[-1]
        self.control.dictate("stop")
        self.control.dictate("start")
        old()
        self.assertTrue(self.control._recorder.recording)

    def test_nothing_recorded_is_empty_text(self):
        self.control._voice = self.Voice()
        self.control.dictate("start", self.Rec)
        self.assertEqual(self.control.dictate("stop")["text"], "")

    def test_off_without_a_voice_block(self):
        self.assertFalse(self.control.dictate("start")["ok"])

    def test_wav(self):
        import numpy as np
        import io
        import wave
        from imperatorium import voice
        data = voice.to_wav(np.array([0.0, 0.5, -1.0, 2.0], dtype="float32"))
        with wave.open(io.BytesIO(data)) as w:
            self.assertEqual((w.getnchannels(), w.getframerate(), w.getnframes()),
                             (1, 16000, 4))


class DictateRoute(HerdrToken):
    def test_only_the_window_or_mods_open_the_mic(self):
        import urllib.error
        import urllib.request

        def call(headers):
            h = {"Content-Type": "application/json"}
            h.update(headers)
            req = urllib.request.Request("http://127.0.0.1:%d/api/dictate" % self.port,
                                         data=b'{"action":"status"}', headers=h)
            try:
                with urllib.request.urlopen(req, timeout=5) as resp:
                    return resp.status
            except urllib.error.HTTPError as exc:
                return exc.code
        self.assertEqual(call({}), 403)
        self.assertEqual(call({"X-Imperatorium-Herdr": self.control.herdr_token}), 200)


class Ownership(unittest.TestCase):
    # (pid, ppid, exe, started)
    TABLE = [(10, 1, "hc", None), (20, 10, "pwsh", None), (30, 20, "claude", None),
             (40, 30, "bash", None), (50, 40, "claude", None), (60, 10, "claude", None)]

    def test_claude_as_the_terminals_program_or_under_its_shell(self):
        self.assertTrue(hosting.owns_terminal(self.TABLE, 60, 60))
        self.assertTrue(hosting.owns_terminal(self.TABLE, 30, 20))

    def test_a_claude_started_inside_another_does_not_own_it(self):
        self.assertFalse(hosting.owns_terminal(self.TABLE, 50, 20))

    def test_unknown_or_unrelated(self):
        self.assertFalse(hosting.owns_terminal(self.TABLE, 99, 20))
        self.assertFalse(hosting.owns_terminal(self.TABLE, 60, 20))
        self.assertFalse(hosting.owns_terminal(self.TABLE, None, 20))

    def test_shells(self):
        self.assertTrue(hosting.is_shell({"foreground": {"name": "pwsh.EXE"}}))
        self.assertFalse(hosting.is_shell({"foreground": {"name": "vim.exe"}}))
        self.assertFalse(hosting.is_shell({"foreground": {"name": "python.exe"}}))


class Labels(unittest.TestCase):
    def test_one_space_per_folder_one_tab_per_session(self):
        a = hosting.space_id("C:/src/My Project")
        self.assertEqual(a, hosting.space_id("c:\\src\\my project\\"))
        self.assertTrue(a.startswith("imp-my-project-"))
        self.assertNotEqual(a, hosting.space_id("D:/src/My Project"))
        got = hosting.labels(SID, "C:/src/x")
        self.assertEqual(got["herdr.tab"], "imp-" + SID[:12])
        self.assertEqual(got["imperatorium.session"], SID)

    def test_what_counts_as_claude_running(self):
        self.assertTrue(hosting.is_claude({"foreground": {"name": "claude.exe"}}))
        self.assertFalse(hosting.is_claude({"foreground": {"name": "pwsh.EXE",
                                                           "argv": ["pwsh"]}}))
        self.assertTrue(hosting.is_claude({"foreground": {"name": "node.exe",
                                                          "argv": ["node", "C:/x/claude"]}}))


class HcFake:
    """A core holding terminals in a dict; records what was done."""

    def __init__(self, terms=None, session=None):
        self.terms = terms or {}
        self.session = session
        self.did = []
        self.n = 10

    def ensure(self):
        return {"pid": 1}

    def get(self, addr):
        if addr not in self.terms:
            raise hcore.HcError("no terminal " + addr)
        return self.terms[addr]

    def send(self, addr, text, force=False, only_if=None):
        self.did.append(("send", addr, text))

    def keys(self, addr, *k):
        self.did.append(("keys", addr) + k)

    def label(self, addr, **labels):
        self.did.append(("label", addr, labels))

    def run(self, argv, cwd, labels=None, keep=True, name=None):
        self.n += 1
        addr = "t%d" % self.n
        self.did.append(("run", argv, cwd, labels))
        self.terms[addr] = {"state": "live", "labels": labels,
                            "foreground": {"name": "claude.exe"}}
        return {"address": addr}


class Restore(unittest.TestCase):
    def setUp(self):
        import tempfile
        from imperatorium.store import Store
        self.dir = tempfile.mkdtemp()
        self.store = Store(os.path.join(self.dir, "state.json"))
        self.core = HcFake()
        self.r = hosting.Restorer(self.store, lambda name: self.core, sleep=lambda s: None)

    def tearDown(self):
        import shutil
        shutil.rmtree(self.dir, ignore_errors=True)

    def test_a_running_session_is_left_alone(self):
        self.r.remember(SID, self.dir, {"address": "t1"})
        self.assertEqual(self.r.restore([SID]), [])
        self.assertEqual(self.core.did, [])

    def test_a_kept_terminal_back_as_a_shell_gets_the_resume_typed(self):
        self.r.remember(SID, self.dir, {"address": "t1"})
        self.core.terms["t1"] = {"state": "live", "labels": {"imperatorium.session": SID},
                                 "foreground": {"name": "pwsh.EXE", "argv": ["pwsh"]}}
        out = self.r.restore([])
        self.assertEqual(out[0]["result"], "resumed in place")
        self.assertEqual(self.core.did, [("send", "t1", "claude --resume " + SID)])

    def test_a_lost_terminal_is_replaced(self):
        self.r.remember(SID, self.dir, {"address": "t1"})
        out = self.r.restore([])
        self.assertEqual(out[0]["result"], "resumed in a new terminal")
        kind, argv, cwd, labels = self.core.did[0]
        self.assertEqual((kind, argv, cwd), ("run", ["claude", "--resume", SID], self.dir))
        self.assertEqual(labels["imperatorium.session"], SID)
        self.assertEqual(self.r.hosted()[SID]["address"], out[0]["address"])

    def test_never_typed_into_a_program_other_than_a_shell(self):
        self.r.remember(SID, self.dir, {"address": "t1"})
        self.core.terms["t1"] = {"state": "live", "labels": {"imperatorium.session": SID},
                                 "foreground": {"name": "vim.exe", "argv": ["vim"]}}
        self.assertEqual(self.r.restore([])[0]["result"], "terminal busy")
        self.assertEqual(self.core.did, [])

    def test_another_terminal_at_that_address_is_not_typed_into(self):
        self.r.remember(SID, self.dir, {"address": "t1"})
        self.core.terms["t1"] = {"state": "live", "labels": {"imperatorium.session": "other"},
                                 "foreground": {"name": "pwsh.EXE", "argv": ["pwsh"]}}
        self.r.restore([])
        self.assertNotIn("send", [d[0] for d in self.core.did])

    def test_a_shell_started_by_hand_in_the_same_folder_is_reused_and_labelled(self):
        self.r.remember(SID, self.dir, {"address": "t1"})
        self.core.terms["t1"] = {"state": "live", "labels": {"herdr.tab": "x"},
                                 "cwd": self.dir.replace("\\", "/"),
                                 "foreground": {"name": "pwsh.EXE", "argv": ["pwsh"]}}
        out = self.r.restore([])
        self.assertEqual(out[0]["result"], "resumed in place")
        self.assertEqual(self.core.did[0], ("label", "t1", {"imperatorium.session": SID}))
        self.assertEqual(self.core.did[1][0], "send")

    def test_an_unlabelled_shell_in_another_folder_is_left_alone(self):
        self.r.remember(SID, self.dir, {"address": "t1"})
        self.core.terms["t1"] = {"state": "live", "labels": {}, "cwd": "C:/elsewhere",
                                 "foreground": {"name": "pwsh.EXE", "argv": ["pwsh"]}}
        self.assertEqual(self.r.restore([])[0]["result"], "resumed in a new terminal")
        self.assertNotIn("send", [d[0] for d in self.core.did])

    def test_a_folder_that_is_gone_is_reported(self):
        self.r.remember(SID, os.path.join(self.dir, "gone"), {"address": "t1"})
        self.assertEqual(self.r.restore([])[0]["result"], "folder gone")

    def test_forget(self):
        self.r.remember(SID, self.dir, {"address": "t1"})
        self.r.forget(SID)
        self.assertEqual(self.r.hosted(), {})


class HostedLaunch(td.Base):
    def setUp(self):
        super().setUp()
        self.core = HcFake()
        self.control.hc_core = lambda name=None: self.core
        self.control.terminal_host = "hc"

    def test_new_starts_claude_in_a_labelled_terminal_and_remembers_it(self):
        out = self.control.new_session(cwd=self.dir)
        self.assertTrue(out["ok"], out)
        self.assertEqual(out["where"], "hc")
        kind, argv, cwd, labels = self.core.did[0]
        self.assertEqual(argv, ["claude", "--session-id", out["session_id"]])
        self.assertEqual(labels["herdr.space"], hosting.space_id(self.dir))
        self.assertIn(out["session_id"], self.control.hosting.hosted())

    def test_a_fork_gets_its_own_tab_and_no_session_label(self):
        self.control.resume(SID, cwd=self.dir, fork=True)
        kind, argv, cwd, labels = self.core.did[0]
        self.assertIn("--fork-session", argv)
        self.assertNotIn("imperatorium.session", labels)
        self.assertNotEqual(labels["herdr.tab"], hosting.tab_id(SID))
        self.assertEqual(self.control.hosting.hosted(), {})

    def test_a_fork_is_remembered_and_labelled_from_its_first_hook(self):
        labelled = []
        self.core.label = lambda a, **l: labelled.append((a, l))
        self.core.terms["t9"] = {"state": "live", "labels": {}, "pid": 1}
        self.control.on_hook({"hook_event_name": "Stop", "session_id": SID, "cwd": self.dir,
                              "hc_terminal": "t9", "hc_session": "default"})
        self.assertEqual(self.control.hosting.hosted()[SID]["address"], "t9")
        self.assertEqual(labelled, [("t9", {"imperatorium.session": SID})])

    def test_session_end_from_shutdown_keeps_it_for_restore(self):
        out = self.control.new_session(cwd=self.dir)
        sid = out["session_id"]
        self.control.on_hook({"hook_event_name": "SessionEnd", "session_id": sid,
                              "cwd": self.dir, "reason": "other"})
        self.assertIn(sid, self.control.hosting.hosted())

    def test_session_end_forgets_it(self):
        out = self.control.new_session(cwd=self.dir)
        sid = out["session_id"]
        self.control.on_hook({"hook_event_name": "SessionEnd", "session_id": sid,
                              "cwd": self.dir})
        self.assertNotIn(sid, self.control.hosting.hosted())

    def test_a_session_started_by_hand_in_hc_is_remembered(self):
        self.control.on_hook({"hook_event_name": "SessionStart", "session_id": SID,
                              "cwd": self.dir, "hc_terminal": "t3", "hc_session": "default"})
        self.assertEqual(self.control.hosting.hosted()[SID]["address"], "t3")

    def test_close_goes_through_hc_and_forgets_it(self):
        out = self.control.new_session(cwd=self.dir)
        sid, addr = out["session_id"], out["hc"]["address"]
        self.agent_rows = [{"sessionId": sid, "cwd": self.dir, "kind": "interactive",
                            "name": "x", "status": "idle", "pid": 77}]
        self.files = {sid: {"sessionId": sid, "cwd": self.dir, "kind": "interactive"}}
        self.control.on_hook({"hook_event_name": "Stop", "session_id": sid, "cwd": self.dir,
                              "hc_terminal": addr, "hc_session": "default"})

        def gone(a):
            self.core.terms[a]["state"] = "finished"
        orig_send = self.core.send
        self.core.send = lambda a, text, **k: (orig_send(a, text), gone(a))
        self.control.invalidate()
        got = self.control.close_session(sid)
        self.assertEqual((got["ok"], got["how"], got["tab_closed"]), (True, "exit", True))
        self.assertIn(("keys", addr, "ctrl+c"), self.core.did)
        self.assertIn(("send", addr, "/exit"), self.core.did)
        self.assertNotIn(sid, self.control.hosting.hosted())

    def test_start_up_ensures_the_core_and_restores(self):
        ensured = []
        self.core.ensure = lambda: ensured.append(1) or {"pid": 1}
        self.control.hosting.remember(SID, self.dir, {"address": "t1"})
        done = self.control.restore_hosted(wait=0)
        self.assertTrue(ensured)
        self.assertEqual(done[0]["result"], "resumed in a new terminal")
        kinds = [e["kind"] for e in self.control.store.events(10)]
        self.assertIn("restore", kinds)

    def test_a_tainted_core_is_reported_not_raised(self):
        def bad():
            raise hcore.HcError("leaks CLAUDE_CODE_MESSAGING_TOKEN")
        self.core.ensure = bad
        self.assertEqual(self.control.restore_hosted(wait=0), [])
        self.assertEqual(self.control.store.events(1)[0]["kind"], "hc_error")

    def test_off_by_default(self):
        self.control.terminal_host = "terminal"
        self.assertIsNone(self.control._hc_opener(SID))


if __name__ == "__main__":
    unittest.main()
