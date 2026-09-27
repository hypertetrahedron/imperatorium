"""Telling you a session wants you when you are not looking at the window.

Two channels, both off until `config.json` turns them on:

- `ntfy`: a push to your phone through an ntfy server (ntfy.sh or your own).
  The topic name is the only secret on ntfy.sh, so it belongs in config, not
  in code. Only the project name and the reason are sent - "gem-trip: needs
  permission" - never the conversation.
- `speak`: Windows' own speech synthesiser says the same line out loud.
  Nothing leaves the machine.

Each channel waits `delay` seconds and then checks the session is *still*
waiting before it fires. Answering a prompt at the desk within that window
should not buzz a phone, and that check is what makes a short delay safe.

Allow/Deny buttons on the push are not here: they would need the phone to
reach this machine, and answering a permission prompt from outside the
terminal is itself deferred (see ROADMAP.md).
"""
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

WINDOWS = sys.platform == "win32"

DEFAULTS = {
    "ntfy": {"enabled": False, "server": "https://ntfy.sh", "topic": "",
             "delay": 60, "priority": "default", "token": None},
    "speak": {"enabled": False, "delay": 5, "rate": 0},
}


def settings(config):
    """Channel settings from config.json's `notify` block, over the defaults."""
    raw = (config or {}).get("notify") or {}
    out = {}
    for name, base in DEFAULTS.items():
        got = dict(base)
        got.update({k: v for k, v in (raw.get(name) or {}).items()
                    if not k.startswith("_")})
        out[name] = got
    return out


def headline(name, reason):
    return "%s: %s" % (name or "a session", reason or "waiting for you")


def ntfy_request(cfg, title, message, tags=()):
    """The urllib Request for one push. Pure, so it is tested offline."""
    topic = str(cfg.get("topic") or "").strip().strip("/")
    if not topic:
        raise ValueError("ntfy topic is not set")
    url = "%s/%s" % (str(cfg.get("server") or DEFAULTS["ntfy"]["server"]).rstrip("/"),
                     topic)
    headers = {"Title": title.encode("utf-8").decode("latin-1", "replace"),
               "Priority": str(cfg.get("priority") or "default")}
    if tags:
        headers["Tags"] = ",".join(tags)
    if cfg.get("token"):
        headers["Authorization"] = "Bearer %s" % cfg["token"]
    return urllib.request.Request(url, data=message.encode("utf-8"),
                                  headers=headers, method="POST")


def speak_command(text, rate=0):
    """PowerShell that says `text` with System.Speech. Pure."""
    safe = text.replace("'", "''")
    script = ("Add-Type -AssemblyName System.Speech; "
              "$s = New-Object System.Speech.Synthesis.SpeechSynthesizer; "
              "$s.Rate = %d; $s.Speak('%s')" % (int(rate), safe))
    return ["powershell", "-NoProfile", "-NonInteractive", "-Command", script]


def spoken(name, reason):
    """What to say. Hyphens read badly aloud: `gem-trip` becomes `gem trip`."""
    return "%s %s" % (str(name or "a session").replace("-", " ").replace("_", " "),
                      reason or "is waiting for you")


class Notifier:
    def __init__(self, config=None, still_waiting=None, send=None, say=None):
        self.cfg = settings(config)
        # (key, since) -> bool: is this session still waiting on that same
        # raise? Supplied by the dispatcher, which owns attention state.
        self.still_waiting = still_waiting or (lambda key, since: True)
        self._send = send or self._urlopen
        self._say = say or self._run_speech
        self._lock = threading.Lock()
        self._scheduled = set()
        self.last_error = None
        self.sent = 0

    def enabled(self, channel):
        return bool(self.cfg.get(channel, {}).get("enabled"))

    def set_enabled(self, channel, on):
        if channel not in self.cfg:
            raise ValueError("no such channel: %s" % channel)
        self.cfg[channel]["enabled"] = bool(on)

    def status(self):
        return {name: {"enabled": bool(c.get("enabled")),
                       "delay": c.get("delay"),
                       "configured": bool(c.get("topic")) if name == "ntfy" else True}
                for name, c in self.cfg.items()} | {
            "last_error": self.last_error, "sent": self.sent}

    # -- attention --------------------------------------------------------
    def attention(self, key, name, reason, since, timer=None):
        """A session just started waiting. Schedule each enabled channel."""
        for channel in ("ntfy", "speak"):
            if not self.enabled(channel):
                continue
            mark = (channel, key, since)
            with self._lock:
                if mark in self._scheduled:
                    continue
                self._scheduled.add(mark)
            delay = float(self.cfg[channel].get("delay") or 0)
            fire = (lambda ch=channel, m=mark: self._fire(ch, m, key, name, reason, since))
            if timer is not None:
                timer(delay, fire)
            else:
                t = threading.Timer(delay, fire)
                t.daemon = True
                t.start()

    def _fire(self, channel, mark, key, name, reason, since):
        try:
            if not self.enabled(channel) or not self.still_waiting(key, since):
                return
            if channel == "ntfy":
                self._push(headline(name, reason), "Imperatorium",
                           ("warning",))
            else:
                self._say(spoken(name, reason))
        finally:
            with self._lock:
                self._scheduled.discard(mark)

    # -- budget -----------------------------------------------------------
    def budget(self, crossing):
        """A rate-limit threshold was crossed. Immediate; no one answers these."""
        label = {"five_hour": "5-hour", "seven_day": "weekly",
                 "spend_limit": "spend"}.get(crossing["window"], crossing["window"])
        text = "%s limit at %d%%" % (label, round(crossing["used"]))
        if crossing.get("resets_at"):
            text += ", resets %s" % time.strftime("%H:%M", time.localtime(
                crossing["resets_at"]))
        if self.enabled("ntfy"):
            self._push(text, "Imperatorium: usage", ("chart_with_upwards_trend",))
        if self.enabled("speak"):
            self._say(text.replace("%", " percent"))
        return text

    # -- transports -------------------------------------------------------
    def _push(self, message, title, tags):
        try:
            self._send(ntfy_request(self.cfg["ntfy"], title, message, tags))
            self.sent += 1
            self.last_error = None
        except Exception as exc:
            self.last_error = "ntfy: %s: %s" % (type(exc).__name__, exc)

    @staticmethod
    def _urlopen(req):
        urllib.request.urlopen(req, timeout=10).read()

    def _run_speech(self, text):
        if not WINDOWS:
            return
        try:
            subprocess.Popen(speak_command(text, self.cfg["speak"].get("rate") or 0),
                             creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        except OSError as exc:
            self.last_error = "speak: %s" % exc
