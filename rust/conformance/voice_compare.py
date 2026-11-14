"""Dictation: the Python worker (faster-whisper) against imperatorium-voice
(whisper.cpp), over the worker protocol both speak.

    python voice_compare.py CLIPS_DIR [--native EXE] [--home CHECKOUT]

CLIPS_DIR holds WAV files and optionally lines.txt (what say0.wav, say1.wav,
... say, one per line). Each clip goes to both workers; the script prints
both texts, whether they match each other and the script line (compared
case- and punctuation-blind), and each worker's warm time. Also: a second
of silence, quiet noise and a clip padded with five seconds of silence, where
Whisper is known to invent words, must come back empty / as the sentence.
"""
import json
import os
import random
import re
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
SETTINGS = {"model": "large-v3-turbo", "cpu_model": "small", "device": "cuda",
            "compute_type": "float16", "language": "en", "beam_size": 1,
            "input_device": None, "max_seconds": 120, "preload": True,
            "enabled": True, "idle_unload": 1800}


class Worker:
    def __init__(self, argv, cwd=None):
        self.p = subprocess.Popen(argv, cwd=cwd, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)

    def call(self, header, payload=b""):
        self.p.stdin.write(json.dumps(header).encode() + b"\n" + payload)
        self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=30)


def wav(samples, rate=16000):
    data = b"".join(struct.pack("<h", max(-32768, min(32767, int(s * 32767)))) for s in samples)
    return (b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt " +
            struct.pack("<IHHIIHH", 16, 1, 1, rate, rate * 2, 2, 16) +
            b"data" + struct.pack("<I", len(data)) + data)


def pcm_of(path):
    raw = open(path, "rb").read()
    i = raw.index(b"data") + 8
    n = (len(raw) - i) // 2
    return [s / 32768 for s in struct.unpack("<%dh" % n, raw[i:i + n * 2])]


def norm(t):
    return re.sub(r"[^a-z0-9 ]", "", t.lower().replace("-", " ")).split()


def main():
    args = sys.argv[1:]
    native = os.path.join(ROOT, "rust", "voice", "target", "release", "imperatorium-voice.exe")
    home = ROOT if os.path.isdir(os.path.join(ROOT, ".venv")) else \
        os.path.join(os.path.dirname(ROOT), "imperatorium")
    if "--native" in args:
        native = args[args.index("--native") + 1]
    if "--home" in args:
        home = args[args.index("--home") + 1]
    clips = args[0]
    py = os.path.join(home, ".venv", "Scripts", "python.exe")
    workers = {
        "python": Worker([py, "-m", "imperatorium.voice", "--worker", json.dumps(SETTINGS)], cwd=home),
        "native": Worker([native, "--worker", json.dumps(SETTINGS)]),
    }
    for name, w in workers.items():
        t = time.time()
        r = w.call({"op": "load"})
        print("%-6s load %.1fs %s" % (name, time.time() - t, r))
    lines = []
    if os.path.exists(os.path.join(clips, "lines.txt")):
        lines = open(os.path.join(clips, "lines.txt"), encoding="utf-8-sig").read().splitlines()
    cases = []
    for f in sorted(os.listdir(clips)):
        if f.endswith(".wav"):
            m = re.match(r"say(\d+)\.wav", f)
            want = lines[int(m.group(1))] if m and int(m.group(1)) < len(lines) else None
            cases.append((f, open(os.path.join(clips, f), "rb").read(), want))
    random.seed(7)
    cases.append(("silence 1s", wav([0.0] * 16000), ""))
    cases.append(("quiet noise 2s", wav([random.uniform(-0.003, 0.003) for _ in range(32000)]), ""))
    if lines:
        first = pcm_of(os.path.join(clips, "say0.wav"))
        cases.append(("say0 + 5s silence", wav(first + [0.0] * 80000), lines[0]))
    agree = right = {"python": 0, "native": 0}
    right = {"python": 0, "native": 0}
    agree = 0
    times = {"python": [], "native": []}
    for name, data, want in cases:
        got = {}
        for wname, w in workers.items():
            t = time.time()
            r = w.call({"op": "transcribe", "bytes": len(data), "hint": None}, data)
            times[wname].append(time.time() - t)
            got[wname] = r.get("text") if r.get("ok") else "ERROR " + str(r.get("error"))
        same = norm(got["python"]) == norm(got["native"])
        agree += same
        for wname in got:
            if want is not None and norm(got[wname]) == norm(want):
                right[wname] += 1
        print("%-18s %s\n   py  %r\n   bin %r%s" % (
            name, "same" if same else "DIFFER", got["python"], got["native"],
            "" if want is None else "\n   want %r" % want))
    for w in workers.values():
        w.close()
    n = len(cases)
    print("\nagree %d/%d; matches the script: python %d, native %d (of %d with a script)"
          % (agree, n, right["python"], right["native"], sum(1 for c in cases if c[2] is not None)))
    for wname, ts in times.items():
        print("%-6s median %.3fs, max %.3fs" % (wname, sorted(ts)[len(ts) // 2], max(ts)))
    return 0 if agree == n else 1


if __name__ == "__main__":
    sys.exit(main())
