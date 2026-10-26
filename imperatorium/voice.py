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

The dispatcher does not hold the model itself: `Worker` runs it in a child
process. Importing faster-whisper commits ~0.8 GB and the loaded model takes
a process to ~3.3 GB of private memory; dropping the model in-process only
gets back to ~1.5 GB (the CUDA runtime stays), while ending the child gives
all of it back. So the child is started on preload or when the mic is
pressed, and ended after `idle_unload` seconds without dictation.
"""
import importlib.util
import json
import os
import subprocess
import sys
import threading
import time

from . import subproc

SAMPLE_RATE = 16000  # what Whisper wants; resampling anything else is waste
CHANNELS = 1

DEFAULTS = {
    "model": "large-v3-turbo",
    # What runs when there is no usable GPU. large-v3-turbo on a CPU makes
    # every dictation a wait; small is quick enough there and still good at
    # prompt-length English. A config that names a model other than the
    # default but not this one gets that model on the CPU too - an explicit
    # choice is kept.
    "cpu_model": "small",
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
    # End the dictation process after this long unused (0: never). Pressing
    # the mic starts it again while you speak.
    "idle_unload": 1800,
}


class VoiceError(RuntimeError):
    pass


def available():
    """True when this interpreter can actually record and transcribe.

    Looks the packages up without importing them: importing faster-whisper
    commits ~0.8 GB, and the dispatcher only asks so it can start a `Worker`.
    """
    try:
        return all(importlib.util.find_spec(name) is not None
                   for name in ("faster_whisper", "sounddevice"))
    except Exception:
        return False


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
        self.settings = settings_from(settings)
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
        self.settings = settings_from(settings)
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
            if self.settings["device"] == "cpu":
                self.settings = on_cpu(self.settings)
            try:
                self._model = self._build(WhisperModel)
            except Exception as exc:
                # No GPU, or a missing CUDA runtime; CPU still works, just
                # slower, and that beats no dictation at all.
                if self.settings["device"] == "cpu":
                    raise VoiceError("could not load the model: %s" % exc) from exc
                self.settings = on_cpu(self.settings)
                try:
                    self._model = self._build(WhisperModel)
                except Exception as inner:
                    raise VoiceError("could not load the model: %s" % inner) from inner
            return self._model

    def _build(self, whisper_model):
        return whisper_model(self.settings["model"], device=self.settings["device"],
                             compute_type=self.settings["compute_type"])

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


def settings_from(block):
    """A `voice` config block over the defaults, unknown keys dropped."""
    block = block if isinstance(block, dict) else {}
    out = dict(DEFAULTS, **{k: v for k, v in block.items() if k in DEFAULTS})
    # Setup copies config.example.json, which names the default model; that
    # is not a choice of one for the CPU.
    if block.get("model") not in (None, "", DEFAULTS["model"]) and not block.get("cpu_model"):
        out["cpu_model"] = block["model"]
    return out


def on_cpu(settings):
    """`settings` as they run on the CPU: the CPU model, at int8."""
    return dict(settings, device="cpu", compute_type="int8",
                model=settings.get("cpu_model") or settings["model"])


def load_settings(config_path=None):
    """The `voice` block of config.json, over the defaults."""
    path = config_path or os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "config.json"
    )
    try:
        with open(path, encoding="utf-8") as fh:
            block = (json.load(fh) or {}).get("voice") or {}
    except (OSError, ValueError):
        block = {}
    return settings_from(block)


# -- the model in a child process ---------------------------------------------
# One request at a time over the child's stdin, one JSON line back on its
# stdout:
#   {"op": "load"}                    -> {"ok": true, "device": d, "model": m}
#   {"op": "transcribe", "hint": h, "bytes": n} + n bytes of encoded audio
#                                     -> {"ok": true, "text": t, "device": d, "model": m}
# `device` is "cpu" once it has fallen back, so the window can say why a
# dictation is slow.
# Failures come back as {"ok": false, "error": ...}. The child exits when its
# stdin closes, so it does not outlive the dispatcher.

PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPLY_TIMEOUT = 180.0  # a cold load plus a long utterance on CPU, with room


def worker_command(settings, exe=None):
    exe = exe or sys.executable
    # pythonw has no stdio to speak of; python with CREATE_NO_WINDOW shows no
    # console either, and its stdio is exactly the pipes it is given.
    if os.path.basename(exe).lower() == "pythonw.exe":
        console = os.path.join(os.path.dirname(exe), "python.exe")
        if os.path.exists(console):
            exe = console
    return [exe, "-m", "imperatorium.voice", "--worker", json.dumps(settings)]


def _spawn(settings):
    # In a job of its own: it dies with the dispatcher, and can be ended whole.
    return subproc.popen(
        worker_command(settings), cwd=PROJECT_ROOT,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)


def _kill_tree(proc):
    # On Windows a venv's python.exe is a launcher with the real interpreter
    # as its child, so killing `proc` alone would leave the model running.
    try:
        subproc.kill_tree(proc)
    except Exception:
        pass


class Worker:
    """`Transcriber`, run in a child process that can be ended to free it."""

    REAP_EVERY = 30.0

    def __init__(self, settings=None, spawn=None, clock=time.time, kill=None):
        self.settings = settings_from(settings)
        self._spawn = spawn or _spawn
        self._kill = kill or _kill_tree
        self._clock = clock
        self._proc = None
        self._used = 0.0
        self._lock = threading.Lock()  # one request at a time; start and stop
        self._reaper = None
        self.starts = 0
        self.where = None  # {device, model} the child last said it runs on

    @property
    def running(self):
        return self._proc is not None and self._proc.poll() is None

    def _ensure(self):
        if not self.running:
            self._stop_locked()
            self._proc = self._spawn(self.settings)
            self.starts += 1
            self._used = self._clock()
            if self._reaper is None and self.settings.get("idle_unload"):
                self._reaper = threading.Thread(target=self._reap_loop,
                                                name="whisper-reaper", daemon=True)
                self._reaper.start()
        return self._proc

    def _stop_locked(self):
        proc, self._proc = self._proc, None
        if proc is None:
            return
        try:
            proc.stdin.close()  # the child's cue to exit
            proc.wait(5)
        except Exception:
            self._kill(proc)
        try:
            proc.stdout.close()
        except Exception:
            pass

    def _call(self, header, payload=b""):
        proc = self._ensure()
        timer = threading.Timer(REPLY_TIMEOUT, self._kill, args=(proc,))
        timer.daemon = True
        timer.start()
        try:
            proc.stdin.write(json.dumps(header).encode("utf-8") + b"\n" + payload)
            proc.stdin.flush()
            line = proc.stdout.readline()
        except (OSError, ValueError) as exc:
            self._stop_locked()
            raise VoiceError("the dictation process failed: %s" % exc) from exc
        finally:
            timer.cancel()
        self._used = self._clock()
        if not line:
            self._stop_locked()
            raise VoiceError("the dictation process exited")
        try:
            reply = json.loads(line)
        except ValueError:
            self._stop_locked()
            raise VoiceError("the dictation process answered nonsense")
        if not reply.get("ok"):
            raise VoiceError(reply.get("error") or "transcription failed")
        if reply.get("device"):
            self.where = {"device": reply["device"], "model": reply.get("model")}
        return reply

    def load(self):
        """Start the child if needed and wait until its model is built."""
        with self._lock:
            return self._call({"op": "load"})

    def warm(self):
        """`load` in the background: a preload, or a mic just pressed."""
        def run():
            try:
                self.load()
            except Exception:
                pass
        threading.Thread(target=run, name="whisper-warm", daemon=True).start()

    def transcribe_bytes(self, data, hint=None):
        """Encoded audio (any container PyAV reads) to text."""
        if not data:
            return ""
        with self._lock:
            return self._call({"op": "transcribe", "hint": hint, "bytes": len(data)},
                              bytes(data)).get("text") or ""

    def stop(self):
        with self._lock:
            self._stop_locked()

    def reap(self, now=None):
        """End the child if it has been idle past `idle_unload`. True if it did."""
        idle = float(self.settings.get("idle_unload") or 0)
        now = self._clock() if now is None else now
        if not idle or not self.running or now - self._used < idle:
            return False
        if not self._lock.acquire(blocking=False):
            return False  # in use right now
        try:
            if now - self._used < idle:
                return False
            self._stop_locked()
            return True
        finally:
            self._lock.release()

    def _reap_loop(self):
        while True:
            time.sleep(self.REAP_EVERY)
            try:
                self.reap()
            except Exception:
                pass


def _read_exactly(stream, n):
    chunks, got = [], 0
    while got < n:
        chunk = stream.read(n - got)
        if not chunk:
            break
        chunks.append(chunk)
        got += len(chunk)
    return b"".join(chunks)


def _decode(data):
    import io

    from faster_whisper.audio import decode_audio

    return decode_audio(io.BytesIO(data), sampling_rate=SAMPLE_RATE)


def serve_worker(settings, inp=None, out=None, transcriber=None, decode=_decode):
    """The child's loop: requests on stdin, replies on stdout, until EOF."""
    if out is None:
        # Keep the real stdout for replies and point fd 1 at stderr, so a
        # library that prints cannot corrupt the protocol.
        out = os.fdopen(os.dup(1), "wb", buffering=0)
        os.dup2(2, 1)
        sys.stdout = sys.stderr
    inp = inp or sys.stdin.buffer
    t = transcriber or Transcriber(settings)

    def where():
        return {"device": t.settings.get("device"), "model": t.settings.get("model")}

    def reply(obj):
        out.write(json.dumps(obj).encode("utf-8") + b"\n")
        out.flush()

    while True:
        line = inp.readline()
        if not line:
            return
        try:
            req = json.loads(line)
            # Which device and model, after the call: a load can fall back.
            if req.get("op") == "load":
                t.load()
                reply(dict(where(), ok=True))
            elif req.get("op") == "transcribe":
                data = _read_exactly(inp, int(req.get("bytes") or 0))
                text = t.transcribe(decode(data), hint=req.get("hint"))
                reply(dict(where(), ok=True, text=text))
            else:
                reply({"ok": False, "error": "unknown op %r" % req.get("op")})
        except Exception as exc:
            reply({"ok": False, "error": "%s: %s" % (type(exc).__name__, exc)})


if __name__ == "__main__":
    if len(sys.argv) > 2 and sys.argv[1] == "--worker":
        serve_worker(settings_from(json.loads(sys.argv[2])))
        sys.exit(0)
    print("available :", available())
    if available():
        print("default   :", default_input())
        print("inputs    : (set voice.input_device in config.json to an index)")
        for index, name in list_inputs():
            print("  %3d  %s" % (index, name))
    print("settings  :", {k: v for k, v in load_settings().items()
                          if k != "_comment"})
