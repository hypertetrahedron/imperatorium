"""Dictation for the prompt box.

Voice fills the *prompt*, never the project. The project is picked from a list
with the keyboard, because these project names - laser-ledger, amarantha,
bookoferrantpages, fiducials - are close to a worst case for speech
recognition, and a misheard name does not garble text, it fires a prompt at
the wrong repository. Dictating the body is where speech actually wins:
roughly 150 words a minute against 50 typed.

Runs on the local GPU, so nothing is sent anywhere. Measured on this machine
with large-v3-turbo at float16: 3.1s to load the model once, then about 0.09s
to transcribe five seconds of speech. The load is the only slow part, so the
model is built on first use and kept.

Needs the project venv (faster-whisper, sounddevice); the rest of the control
plane stays standard-library only.
"""
import os
import threading

SAMPLE_RATE = 16000  # what Whisper wants; resampling anything else is waste
CHANNELS = 1

DEFAULTS = {
    "model": "large-v3-turbo",
    "device": "cuda",
    "compute_type": "float16",
    "language": "en",
    "beam_size": 1,
    "input_device": None,
    "max_seconds": 120,
    # Build the model at startup rather than on the first dictation. Costs
    # ~2GB of VRAM held, buys a first dictation that is as fast as the rest.
    "preload": True,
    "enabled": True,
}


class VoiceError(RuntimeError):
    pass


def available():
    """True when this interpreter can actually record and transcribe."""
    try:
        import faster_whisper  # noqa: F401
        import sounddevice  # noqa: F401
    except Exception:
        return False
    return True


def list_inputs():
    """[(index, name)] for every input device, for choosing one in config."""
    import sounddevice as sd

    out = []
    for index, device in enumerate(sd.query_devices()):
        if device.get("max_input_channels", 0) > 0:
            out.append((index, device.get("name", "?")))
    return out


def default_input():
    import sounddevice as sd

    try:
        return sd.query_devices(kind="input").get("name")
    except Exception:
        return None


class Recorder:
    """Push-to-talk capture into memory.

    Deliberately a toggle rather than hold-to-talk: key auto-repeat makes
    hold-to-talk fiddly in a GUI toolkit, and a toggle is what a foot pedal
    drives cleanly too.
    """

    def __init__(self, settings=None):
        self.settings = dict(DEFAULTS, **(settings or {}))
        self._stream = None
        self._frames = []
        self._lock = threading.Lock()

    @property
    def recording(self):
        return self._stream is not None

    def start(self):
        import numpy as np  # noqa: F401  (sounddevice pulls it in)
        import sounddevice as sd

        if self.recording:
            return
        with self._lock:
            self._frames = []

        def on_audio(indata, _frames, _time, status):
            # `status` carries overflows; dropping a frame is better than
            # raising inside the audio callback and killing the stream.
            with self._lock:
                self._frames.append(indata.copy())

        try:
            self._stream = sd.InputStream(
                samplerate=SAMPLE_RATE,
                channels=CHANNELS,
                dtype="float32",
                device=self.settings.get("input_device"),
                callback=on_audio,
            )
            self._stream.start()
        except Exception as exc:
            self._stream = None
            raise VoiceError("could not open the microphone: %s" % exc) from exc

    def stop(self):
        """Stop and return mono float32 at 16 kHz, or None if nothing came in."""
        import numpy as np

        stream, self._stream = self._stream, None
        if stream is not None:
            try:
                stream.stop()
                stream.close()
            except Exception:
                pass
        with self._lock:
            frames, self._frames = self._frames, []
        if not frames:
            return None
        audio = np.concatenate(frames, axis=0).reshape(-1)
        limit = int(self.settings["max_seconds"]) * SAMPLE_RATE
        if len(audio) > limit:
            audio = audio[-limit:]
        return audio


class Transcriber:
    """Holds the model. Built on first use, then kept - loading dominates."""

    def __init__(self, settings=None):
        self.settings = dict(DEFAULTS, **(settings or {}))
        self._model = None
        self._lock = threading.Lock()

    def load(self):
        with self._lock:
            if self._model is not None:
                return self._model
            try:
                from faster_whisper import WhisperModel
            except Exception as exc:
                raise VoiceError("faster-whisper is not installed: %s" % exc) from exc
            try:
                self._model = WhisperModel(
                    self.settings["model"],
                    device=self.settings["device"],
                    compute_type=self.settings["compute_type"],
                )
            except Exception as exc:
                # A missing CUDA runtime is the usual cause; CPU still works,
                # just slower, and that beats no dictation at all.
                if self.settings["device"] != "cpu":
                    self.settings = dict(self.settings, device="cpu",
                                         compute_type="int8")
                    try:
                        from faster_whisper import WhisperModel

                        self._model = WhisperModel(
                            self.settings["model"], device="cpu", compute_type="int8"
                        )
                    except Exception as inner:
                        raise VoiceError("could not load the model: %s" % inner) from inner
                else:
                    raise VoiceError("could not load the model: %s" % exc) from exc
            return self._model

    def transcribe(self, audio, hint=None):
        """Audio (float32, 16 kHz) to text. `hint` biases rare words."""
        import numpy as np

        if audio is None or len(audio) == 0:
            return ""
        # Near-silence is a fumbled key, not an utterance. Transcribing it
        # invents words, which is worse than returning nothing.
        if float(np.max(np.abs(audio))) < 0.005:
            return ""
        model = self.load()
        segments, _info = model.transcribe(
            audio,
            language=self.settings["language"],
            beam_size=int(self.settings["beam_size"]),
            initial_prompt=hint or None,
            vad_filter=True,
        )
        return " ".join(segment.text for segment in segments).strip()


def load_settings(config_path=None):
    """The `voice` block of config.json, over the defaults."""
    import json

    path = config_path or os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "config.json"
    )
    try:
        with open(path, encoding="utf-8") as fh:
            block = (json.load(fh) or {}).get("voice") or {}
    except (OSError, ValueError):
        block = {}
    return dict(DEFAULTS, **{k: v for k, v in block.items() if k in DEFAULTS})


if __name__ == "__main__":
    print("available :", available())
    if available():
        print("default   :", default_input())
        print("inputs    : (set voice.input_device in config.json to an index)")
        for index, name in list_inputs():
            print("  %3d  %s" % (index, name))
    print("settings  :", {k: v for k, v in load_settings().items()
                          if k != "_comment"})
