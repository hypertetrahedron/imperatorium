"""The Settings window: which parts of config.json a person may change from
the browser, how each is checked, and writing them back.

Every field names the process that reads it and when. The dispatcher reads
config.json once at start, so most changes wait for a restart; the few it can
take at once (notifications, the alert thresholds, background-session
options) are applied live by `Control.save_settings`. The palette hotkeys
and the Stream Deck keys belong to their own processes.

Left out on purpose:
- policy.json, the permission auto-approval rules. Who may approve what is
  not something a form on a local web page should be able to widen.
- `bypassPermissions` (and `dontAsk`) as a background permission mode, for
  the same reason. A config.json that already says so is shown and kept,
  but the form cannot choose it.
- The ntfy access token: a secret, so it is never sent to the page.
- Remote hosts: adding one sets up SSH and a tunnel, which
  `setup_machine.py remote add|remove` does; the window lists them.
- Source constants (poll intervals, cache sizes): internal tuning, not
  settings.

The file is rewritten whole, as setup_machine.py does: `_comment` keys are
ordinary keys and survive, the previous file is kept as config.json.bak, and
the write is atomic. Formatting is normalised to two-space JSON.
"""
import json
import os
import re
import shutil
import tempfile

# Who has to restart for a change to show.
LIVE, DISPATCHER, HOTKEY, STREAMDECK = "live", "dispatcher", "hotkey", "streamdeck"
RESTART = {DISPATCHER: "python impctl.py restart dispatcher",
           HOTKEY: "restart the palette (hotkey.py)",
           STREAMDECK: "restart the Stream Deck surface (streamdeck_surface.py)"}

PERMISSION_MODES = ("", "default", "acceptEdits", "plan")
PRIORITIES = ("min", "low", "default", "high", "urgent")
_NAME = re.compile(r"^[A-Za-z0-9._:\[\]-]*$")
_TOPIC = re.compile(r"^[A-Za-z0-9_-]{0,64}$")
_HOTKEY = re.compile(r"^[a-z0-9]+(\+[a-z0-9]+)*$")
_LANG = re.compile(r"^[a-z]{0,3}$")


def _voice_on(config):
    # A voice block that does not say turns dictation on (voice.DEFAULTS);
    # no block at all leaves it off (Control starts no worker).
    return isinstance(config.get("voice"), dict)


SECTIONS = [
    ("projects", "Projects"),
    ("chat", "Chat"),
    ("quick", "Quick phrases"),
    ("background", "Background sessions"),
    ("notify", "Notifications"),
    ("budget", "Usage alerts"),
    ("voice", "Dictation"),
    ("remote", "Remote machines"),
    ("palette", "Palette"),
    ("streamdeck", "Stream Deck"),
]

# key, section, label, type, default, applies, help, extra
FIELDS = [
    dict(key="roots", section="projects", label="Project folders", type="paths", default=[],
         applies=DISPATCHER,
         help="Folders whose subfolders are your projects, one per line. Empty: the folder "
              "this checkout sits in."),
    dict(key="chat.send_as_typed", section="chat", label="Send As Typed", type="bool",
         default=False, applies=LIVE,
         help="Adds Send As Typed to the Send button: the message is typed at the "
              "session's prompt as one line, so it counts as you typing there - it can "
              "approve what a sent message cannot, such as spending. Only from this "
              "window, only into an idle session on this machine. Can only be turned "
              "on here."),
    dict(key="chat.cold_cache_tokens", section="chat", label="Cache-expired warning from (tokens)",
         type="int", min=0, max=10000000, default=0, applies=LIVE,
         help="Once a session's prompt cache has expired, the chat says so - with what the "
              "next prompt costs - when its context is at least this many tokens. 0 always "
              "says so."),
    dict(key="chat.matrix_status", section="chat", label="Always show cost in the matrix",
         type="bool", default=True, applies=LIVE,
         help="The chat matrix keeps a cell for the cost and usage tile even when the "
              "sessions fill the grid. Off, it shows only in cells the grid leaves empty."),
    dict(key="quick.key", section="quick", label="Shortcut", type="key", default="ctrl+space",
         applies=LIVE,
         help="Pressed in the prompt box, opens the phrase list. Ctrl, Alt or Win plus a "
              "key, e.g. ctrl+space or alt+q."),
    dict(key="quick.phrases", section="quick", label="Phrases", type="phrases",
         default=["continue as recommended", "implement as recommended",
                  "commit this", "run the tests and fix any failures"],
         applies=LIVE,
         help="One per line, in the order the list shows them. The chosen one is put "
              "into the prompt box at the cursor."),
    dict(key="background.permission_mode", section="background", label="Permission mode",
         type="choice", choices=PERMISSION_MODES, default="", applies=LIVE,
         help="For sessions started with `claude --bg` (New in bg, Fork in bg, dispatch). "
              "Blank uses your own Claude Code settings. Bypass modes can only be set in "
              "config.json."),
    dict(key="background.model", section="background", label="Model", type="name", default="",
         applies=LIVE, help="e.g. opus, sonnet, or a full model id. Blank uses your default."),
    dict(key="background.agent", section="background", label="Agent", type="name", default="",
         applies=LIVE, help="A subagent to run the session as. Blank for none."),
    dict(key="notify.ntfy.enabled", section="notify", label="Push to phone (ntfy)", type="bool",
         default=False, applies=LIVE,
         help="When a session is still waiting on you after the delay. Sends only "
              "'project: reason'."),
    dict(key="notify.ntfy.server", section="notify", label="ntfy server", type="url",
         default="https://ntfy.sh", applies=LIVE, help=""),
    dict(key="notify.ntfy.topic", section="notify", label="ntfy topic", type="topic", default="",
         applies=LIVE,
         help="On ntfy.sh the topic name is the only secret: make it unguessable."),
    dict(key="notify.ntfy.delay", section="notify", label="Push after (seconds)", type="int",
         min=0, max=86400, default=60, applies=LIVE, help=""),
    dict(key="notify.ntfy.priority", section="notify", label="Push priority", type="choice",
         choices=PRIORITIES, default="default", applies=LIVE, help=""),
    dict(key="notify.speak.enabled", section="notify", label="Speak aloud", type="bool",
         default=False, applies=LIVE, help="Windows speech, on this machine."),
    dict(key="notify.speak.delay", section="notify", label="Speak after (seconds)", type="int",
         min=0, max=3600, default=5, applies=LIVE, help=""),
    dict(key="notify.speak.rate", section="notify", label="Speech rate", type="int", min=-10,
         max=10, default=0, applies=LIVE, help="-10 slowest, 10 fastest."),
    dict(key="budget.thresholds", section="budget", label="Alert at (% used)", type="numbers",
         min=1, max=100, default=[80, 95], applies=LIVE,
         help="Rate-limit percentages that raise an alert, once per window. Comma separated."),
    dict(key="voice.enabled", section="voice", label="Dictation (F9)", type="bool",
         default=_voice_on, applies=DISPATCHER,
         help="Local speech to text; nothing leaves the machine. Needs the voice extras "
              "(setup_machine.py --voice)."),
    dict(key="voice.model", section="voice", label="Model", type="name", default="large-v3-turbo",
         applies=DISPATCHER, help="A Whisper model: large-v3-turbo, medium, small, …"),
    dict(key="voice.cpu_model", section="voice", label="Model without a GPU", type="name",
         default="small", applies=DISPATCHER, help="Used when no usable GPU is found."),
    dict(key="voice.device", section="voice", label="Device", type="choice",
         choices=("cuda", "cpu"), default="cuda", applies=DISPATCHER, help=""),
    dict(key="voice.compute_type", section="voice", label="Compute type", type="choice",
         choices=("float16", "int8_float16", "int8", "float32"), default="float16",
         applies=DISPATCHER, help=""),
    dict(key="voice.language", section="voice", label="Language", type="lang", default="en",
         applies=DISPATCHER, help="A language code such as en or de."),
    dict(key="voice.input_device", section="voice", label="Microphone", type="optint", min=0,
         max=999, default=None, applies=DISPATCHER,
         help="An index from `python -m imperatorium.voice`. Blank for the system default."),
    dict(key="voice.preload", section="voice", label="Load at start", type="bool",
         default=_voice_on, applies=DISPATCHER,
         help="Faster first dictation, at the cost of about 2 GB of VRAM held."),
    dict(key="voice.idle_unload", section="voice", label="Unload after idle (seconds)",
         type="int", min=0, max=86400, default=1800, applies=DISPATCHER, help="0 never unloads."),
    dict(key="voice.max_seconds", section="voice", label="Longest dictation (seconds)",
         type="int", min=5, max=600, default=120, applies=DISPATCHER, help=""),
    dict(key="remote.enabled", section="remote", label="Show remote sessions", type="bool",
         default=True, applies=DISPATCHER,
         help="Polled over SSH. Hosts are added and removed with "
              "`python setup_machine.py remote add|remove HOST`."),
    dict(key="palette.hotkey", section="palette", label="Hotkeys", type="hotkeys",
         default=["ctrl+alt+backquote", "ctrl+alt+j", "ctrl+shift+space", "ctrl+shift+f12"],
         applies=HOTKEY, help="One per line, e.g. ctrl+alt+j. The first that is free is used."),
    dict(key="streamdeck.keys", section="streamdeck", label="Pinned projects", type="names",
         default=[], applies=STREAMDECK,
         help="Projects on fixed keys, in order, one per line. Live sessions fill the rest."),
]
BY_KEY = {f["key"]: f for f in FIELDS}

# Browser key combinations, written the way the page builds them from an
# event (ui.py, comboOf): modifiers in this order, then the key.
MODIFIERS = {"ctrl", "alt", "shift", "win"}
_ORDER = ("ctrl", "alt", "shift", "win")
# Taken while the prompt box has focus (see ui.SHORTCUTS).
RESERVED_KEYS = {
    "ctrl+enter": "Background", "alt+enter": "Queue", "ctrl+k": "the session jump",
    "ctrl+f": "find in the conversation", "alt+n": "next waiting for you",
    "alt+arrowup": "previous message", "alt+arrowdown": "next message",
    **{"alt+%d" % n: "select session %d" % n for n in range(1, 10)},
}


def normal_key(combo):
    """`combo` with its modifiers in the page's order."""
    *mods, key = combo.split("+")
    return "+".join([m for m in _ORDER if m in mods] + [key])


class Invalid(ValueError):
    pass


def _get(config, key):
    node = config
    for part in key.split("."):
        if not isinstance(node, dict) or part not in node:
            return None, False
        node = node[part]
    return node, True


def value(config, field):
    got, found = _get(config, field["key"])
    if found:
        return got
    d = field["default"]
    return d(config) if callable(d) else d


def _lines(v):
    if isinstance(v, str):
        v = v.replace(",", "\n").splitlines() if v else []
    if not isinstance(v, list):
        raise Invalid("expected a list")
    return [str(x).strip() for x in v if str(x).strip()]


def check(field, v, current=None):
    """`v` as it should be stored, or raise Invalid. `current` is what the
    file holds now, which a choice may keep even if the form cannot pick it."""
    t = field["type"]
    if t == "bool":
        if not isinstance(v, bool):
            raise Invalid("expected on or off")
        return v
    if t in ("int", "optint"):
        if t == "optint" and v in (None, ""):
            return None
        try:
            n = int(v) if not isinstance(v, bool) else None
        except (TypeError, ValueError):
            n = None
        if n is None or (isinstance(v, float) and v != int(v)):
            raise Invalid("expected a whole number")
        if not field["min"] <= n <= field["max"]:
            raise Invalid("must be from %d to %d" % (field["min"], field["max"]))
        return n
    if t == "choice":
        v = "" if v is None else v
        if v in field["choices"] or (v and v == current):
            return v
        raise Invalid("must be one of: %s" % ", ".join(c or "(blank)" for c in field["choices"]))
    if not isinstance(v, (str, list)):
        raise Invalid("expected text")
    if t == "name":
        v = v.strip() if isinstance(v, str) else None
        if v is None or len(v) > 100 or not _NAME.match(v):
            raise Invalid("letters, digits and . _ - : [ ] only")
        return v
    if t == "url":
        v = v.strip().rstrip("/") if isinstance(v, str) else ""
        if not re.match(r"^https?://[A-Za-z0-9.-]+(:\d+)?(/[\w./-]*)?$", v):
            raise Invalid("expected an http(s) address")
        return v
    if t == "topic":
        v = v.strip() if isinstance(v, str) else None
        if v is None or not _TOPIC.match(v):
            raise Invalid("letters, digits, _ and - only, up to 64")
        return v
    if t == "lang":
        v = v.strip().lower() if isinstance(v, str) else None
        if v is None or not _LANG.match(v):
            raise Invalid("a language code such as en")
        return v
    if t == "numbers":
        out = []
        for x in _lines(v):
            try:
                n = float(x)
            except ValueError:
                raise Invalid("%r is not a number" % x)
            if not field["min"] <= n <= field["max"]:
                raise Invalid("each must be from %d to %d" % (field["min"], field["max"]))
            out.append(int(n) if n == int(n) else n)
        if len(out) > 6:
            raise Invalid("at most 6")
        return sorted(set(out))
    if t == "paths":
        out = _lines(v) if isinstance(v, list) else [x.strip() for x in v.splitlines() if x.strip()]
        for p in out:
            if not os.path.isdir(p):
                raise Invalid("no such folder: %s" % p)
        return out
    if t == "hotkeys":
        out = [x.lower().replace(" ", "") for x in _lines(v)]
        for x in out:
            if not _HOTKEY.match(x):
                raise Invalid("%r is not a hotkey like ctrl+alt+j" % x)
        if not out:
            raise Invalid("at least one hotkey")
        return out
    if t == "key":
        v = v.lower().replace(" ", "") if isinstance(v, str) else ""
        parts = v.split("+")
        if not _HOTKEY.match(v) or len(parts) < 2 or parts[-1] in MODIFIERS:
            raise Invalid("a modifier and a key, like ctrl+space")
        if not set(parts[:-1]) & {"ctrl", "alt", "win"} or not set(parts[:-1]) <= MODIFIERS:
            raise Invalid("needs ctrl, alt or win - shift alone just types")
        v = normal_key(v)
        if v in RESERVED_KEYS:
            raise Invalid("%s is already %s" % (v, RESERVED_KEYS[v]))
        return v
    if t == "phrases":
        out = [str(x).strip() for x in v] if isinstance(v, list) else \
            [x.strip() for x in v.splitlines()]
        out = [x for x in out if x]
        if len(out) > 50:
            raise Invalid("at most 50")
        for x in out:
            if len(x) > 1000:
                raise Invalid("each at most 1000 characters")
        return out
    if t == "names":
        out = _lines(v) if isinstance(v, list) else [x.strip() for x in v.splitlines() if x.strip()]
        for x in out:
            if len(x) > 100 or any(c in x for c in "\\/\"<>"):
                raise Invalid("%r is not a project name" % x)
        return out
    raise Invalid("unknown field type")


def describe(config):
    """The window's form: sections, fields with their values, and the
    remote hosts (shown, not edited)."""
    fields = []
    for f in FIELDS:
        out = {k: v for k, v in f.items() if k != "default"}
        out["value"] = value(config, f)
        if f["type"] == "choice" and out["value"] not in f["choices"]:
            out["choices"] = list(f["choices"]) + [out["value"]]
            out["locked"] = True   # kept as it is; the form cannot choose it
        fields.append(out)
    hosts = [{"name": h.get("name") if isinstance(h, dict) else h,
              "label": h.get("label") if isinstance(h, dict) else None,
              "tunnel": bool(h.get("tunnel")) if isinstance(h, dict) else False}
             for h in ((config.get("remote") or {}).get("hosts") or [])]
    return {"sections": [{"id": s, "title": t} for s, t in SECTIONS], "fields": fields,
            "hosts": hosts, "restart": RESTART}


def apply(config, changes):
    """A new config with `changes` ({key: value}) checked and set. Raises
    Invalid with {key: message} as its argument when any is wrong. A blank
    background option is removed, so Claude Code's own setting applies."""
    errors, out = {}, json.loads(json.dumps(config))
    for key, v in (changes or {}).items():
        f = BY_KEY.get(key)
        if f is None:
            errors[key] = "not a setting"
            continue
        try:
            got = check(f, v, _get(config, key)[0])
        except Invalid as exc:
            errors[key] = str(exc)
            continue
        *path, last = key.split(".")
        node = out
        for part in path:
            if not isinstance(node.get(part), dict):
                node[part] = {}
            node = node[part]
        if key.startswith("background.") and got == "":
            node.pop(last, None)
        else:
            node[last] = got
    if errors:
        raise Invalid(errors)
    return out


def load(path):
    """config.json as a dict; {} when it is missing. Raises ValueError when
    it is there but not a JSON object, so a save never replaces a file the
    person can still fix by hand."""
    try:
        with open(path, encoding="utf-8") as fh:
            got = json.load(fh)
    except FileNotFoundError:
        return {}
    if not isinstance(got, dict):
        raise ValueError("config.json is not a JSON object")
    return got


def save(path, config):
    """Atomic write; the old file is kept as config.json.bak."""
    if os.path.exists(path):
        shutil.copyfile(path, path + ".bak")
    fd, tmp = tempfile.mkstemp(prefix=".config-", suffix=".json", dir=os.path.dirname(path) or ".")
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as fh:
            fh.write(json.dumps(config, indent=2) + "\n")
        os.replace(tmp, path)
    except BaseException:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise


def restarts(keys):
    """Which processes must restart for `keys` to show, in a fixed order."""
    need = {BY_KEY[k]["applies"] for k in keys if k in BY_KEY}
    return [p for p in (DISPATCHER, HOTKEY, STREAMDECK) if p in need]
