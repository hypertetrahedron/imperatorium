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


class Environment(unittest.TestCase):
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
