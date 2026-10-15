"""Dictation settings and the guards around transcription.

The model and the microphone are not exercised here - those were verified
against real audio on this machine (SAPI-generated speech through the full
path, correct text out). What is tested is everything that decides whether
the model gets called at all, because those are the paths that run on a
fumbled keypress.

Run with the venv interpreter:
    .venv\\Scripts\\python.exe test_voice.py
"""
import json
import os
import shutil
import tempfile
import unittest

from ccontrol import voice

try:
    import numpy  # noqa: F401 - part of the optional voice extras
    NUMPY = True
except ImportError:
    NUMPY = False
NO_EXTRAS = "voice extras not installed (setup_machine.py --voice)"


class Settings(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.path = os.path.join(self.dir, "config.json")

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def write(self, block):
        with open(self.path, "w", encoding="utf-8") as fh:
            json.dump({"voice": block}, fh)

    def test_defaults_apply_when_there_is_no_config(self):
        settings = voice.load_settings(os.path.join(self.dir, "absent.json"))
        self.assertEqual(settings["model"], voice.DEFAULTS["model"])
        self.assertEqual(settings["language"], "en")

    def test_a_broken_config_falls_back_rather_than_raising(self):
        with open(self.path, "w", encoding="utf-8") as fh:
            fh.write("{not json")
        self.assertEqual(voice.load_settings(self.path), voice.DEFAULTS)

    def test_config_overrides_only_known_keys(self):
        # config.json is hand-edited and carries "_comment"; letting that
        # through would reach WhisperModel as a keyword argument.
        self.write({"model": "small", "_comment": "note to self",
                    "nonsense": True})
        settings = voice.load_settings(self.path)
        self.assertEqual(settings["model"], "small")
        self.assertNotIn("_comment", settings)
        self.assertNotIn("nonsense", settings)

    def test_disabling_voice_is_expressed_in_settings(self):
        self.write({"enabled": False})
        self.assertFalse(voice.load_settings(self.path)["enabled"])

    def test_an_input_device_index_survives(self):
        self.write({"input_device": 16})
        self.assertEqual(voice.load_settings(self.path)["input_device"], 16)


@unittest.skipUnless(NUMPY, NO_EXTRAS)
class TranscriptionGuards(unittest.TestCase):
    """Nothing reaches the model unless there is something to transcribe."""

    def setUp(self):
        # A model name that could never load: if a guard leaks, this raises
        # and the test fails loudly instead of quietly downloading something.
        self.t = voice.Transcriber({"model": "no-such-model-should-never-load"})

    def test_empty_input_returns_empty_without_loading(self):
        self.assertEqual(self.t.transcribe(None), "")
        self.assertEqual(self.t.transcribe([]), "")

    def test_silence_returns_empty_without_loading(self):
        import numpy as np

        # A fumbled key produces near-silence, and Whisper will invent words
        # for it - worse than returning nothing.
        self.assertEqual(self.t.transcribe(np.zeros(16000, dtype=np.float32)), "")
        faint = np.full(16000, 0.001, dtype=np.float32)
        self.assertEqual(self.t.transcribe(faint), "")

    def test_audible_input_does_reach_the_model(self):
        import numpy as np

        loud = np.full(16000, 0.5, dtype=np.float32)
        with self.assertRaises(voice.VoiceError):
            self.t.transcribe(loud)


@unittest.skipUnless(NUMPY, NO_EXTRAS)
class RecorderState(unittest.TestCase):
    def test_a_fresh_recorder_is_not_recording(self):
        self.assertFalse(voice.Recorder().recording)

    def test_stopping_without_starting_yields_nothing(self):
        self.assertIsNone(voice.Recorder().stop())

    def test_settings_merge_over_defaults(self):
        r = voice.Recorder({"max_seconds": 5})
        self.assertEqual(r.settings["max_seconds"], 5)
        self.assertEqual(r.settings["model"], voice.DEFAULTS["model"])


class FakeModel:
    """Stands in for Transcriber inside the worker loop."""

    def __init__(self, fail=None):
        self.settings = {"device": "cpu"}
        self.loads = 0
        self.fail = fail

    def load(self):
        self.loads += 1

    def transcribe(self, audio, hint=None):
        if self.fail:
            raise voice.VoiceError(self.fail)
        return "%d bytes, hint=%s" % (len(audio), hint)


class PipeProc:
    """A Worker child that is serve_worker on a thread, over real OS pipes."""

    def __init__(self, model):
        import threading

        r_in, w_in = os.pipe()
        r_out, w_out = os.pipe()
        self.stdin = os.fdopen(w_in, "wb")
        self.stdout = os.fdopen(r_out, "rb")
        child_in, child_out = os.fdopen(r_in, "rb"), os.fdopen(w_out, "wb")

        def run():
            try:
                voice.serve_worker({}, inp=child_in, out=child_out, transcriber=model,
                                   decode=lambda data: data)
            finally:
                child_out.close()
                child_in.close()
        self.thread = threading.Thread(target=run, daemon=True)
        self.thread.start()
        self.pid = 0

    def poll(self):
        return None if self.thread.is_alive() else 0

    def wait(self, timeout=None):
        self.thread.join(timeout)
        if self.thread.is_alive():
            raise TimeoutError


class WorkerProcess(unittest.TestCase):
    """The protocol between the dispatcher and the dictation process."""

    def setUp(self):
        self.now = [1000.0]
        self.procs = []
        self.model = FakeModel()

    def worker(self, **settings):
        def spawn(_settings):
            self.procs.append(PipeProc(self.model))
            return self.procs[-1]
        settings.setdefault("idle_unload", 0)
        w = voice.Worker(settings, spawn=spawn, clock=lambda: self.now[0],
                         kill=lambda proc: None)
        self.addCleanup(w.stop)
        return w

    def test_audio_goes_through_and_text_comes_back(self):
        w = self.worker()
        self.assertEqual(w.transcribe_bytes(b"abcde", hint="laser"), "5 bytes, hint=laser")
        self.assertEqual(w.transcribe_bytes(b"xy"), "2 bytes, hint=None")
        self.assertEqual(len(self.procs), 1)
        w.stop()
        self.assertFalse(self.procs[0].thread.is_alive())  # stdin closed: it exits

    def test_nothing_recorded_never_starts_a_process(self):
        w = self.worker()
        self.assertEqual(w.transcribe_bytes(b""), "")
        self.assertEqual(self.procs, [])

    def test_load_builds_the_model_once_per_process(self):
        w = self.worker()
        w.load()
        w.load()
        self.assertEqual((len(self.procs), self.model.loads), (1, 2))

    def test_a_failure_in_the_child_is_reported_and_the_child_kept(self):
        self.model.fail = "no GPU"
        w = self.worker()
        with self.assertRaisesRegex(voice.VoiceError, "no GPU"):
            w.transcribe_bytes(b"abc")
        self.assertTrue(w.running)
        w.stop()

    def test_a_child_that_died_is_started_again(self):
        w = self.worker()
        w.load()
        self.procs[0].stdin.close()   # as if it had exited
        self.procs[0].thread.join(2)
        self.procs[0].stdout.close()
        self.assertEqual(w.transcribe_bytes(b"a"), "1 bytes, hint=None")
        self.assertEqual(len(self.procs), 2)
        w.stop()

    def test_an_idle_child_is_ended_and_a_busy_one_is_not(self):
        w = self.worker(idle_unload=60)
        w.load()
        self.assertFalse(w.reap(now=self.now[0] + 30))
        with w._lock:  # mid-transcription
            self.assertFalse(w.reap(now=self.now[0] + 61))
        self.assertTrue(w.reap(now=self.now[0] + 61))
        self.assertFalse(w.running)
        w.load()  # comes back on demand
        self.assertEqual(len(self.procs), 2)
        w.stop()

    def test_zero_means_never_unload(self):
        w = self.worker(idle_unload=0)
        w.load()
        self.assertFalse(w.reap(now=self.now[0] + 10 ** 6))
        w.stop()

    def test_the_child_runs_this_module_without_a_console(self):
        cmd = voice.worker_command({"model": "small"}, exe=voice.sys.executable)
        self.assertEqual(cmd[1:4], ["-m", "ccontrol.voice", "--worker"])
        self.assertEqual(json.loads(cmd[4])["model"], "small")


class Environment(unittest.TestCase):
    def test_checking_availability_does_not_import_the_model(self):
        # Importing faster-whisper commits ~0.8 GB; the dispatcher asks.
        import subprocess
        import sys

        code = ("import sys; from ccontrol import voice; voice.available(); "
                "print('faster_whisper' in sys.modules)")
        out = subprocess.run([sys.executable, "-c", code], capture_output=True,
                             text=True, cwd=os.path.dirname(os.path.abspath(__file__)))
        self.assertEqual(out.stdout.strip(), "False", out.stderr)


    def test_availability_is_reported_not_assumed(self):
        self.assertIsInstance(voice.available(), bool)

    @unittest.skipUnless(voice.available(), "needs the venv")
    def test_at_least_one_input_device_is_visible(self):
        self.assertTrue(voice.list_inputs())

    @unittest.skipUnless(voice.available(), "needs the venv")
    def test_sixteen_kilohertz_is_what_the_model_wants(self):
        self.assertEqual(voice.SAMPLE_RATE, 16000)


if __name__ == "__main__":
    unittest.main(verbosity=2)
