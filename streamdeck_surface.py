"""One key per project, coloured by what that project's session is doing.

Runs as its own process, against the venv that has `streamdeck` and `pillow`,
because the dispatcher itself is deliberately standard-library only. It talks
to the dispatcher over HTTP and holds no state of its own, so it can be
restarted at any time and can be killed without affecting anything else.

    .venv\\Scripts\\python.exe streamdeck_surface.py

Colours:
    orange  blocked - this session is waiting on you
    blue    working
    green   idle, session is live
    grey    no session running for this project

A press focuses that project's session, or opens one when nothing is running.

When a permission request is parked in the dispatcher (policy.json `park`),
the last three keys become a decision: what is asking, ALLOW, DENY. A key
that has just changed under your finger ignores presses for a moment, so a
press meant for a project key cannot land on ALLOW.
"""
import argparse

import json
import os
import sys
import threading
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
# hidapi.dll is vendored rather than installed: the venv's python.exe is a
# shim, so its directory is not on the default DLL search path.
if sys.platform == "win32" and os.path.isdir(os.path.join(HERE, "vendor")):
    os.add_dll_directory(os.path.join(HERE, "vendor"))

from PIL import ImageDraw, ImageFont  # noqa: E402
from StreamDeck.DeviceManager import DeviceManager  # noqa: E402
from StreamDeck.ImageHelpers import PILHelper  # noqa: E402

ENDPOINT = os.environ.get("CCONTROL_ENDPOINT_BASE", "http://127.0.0.1:8792")
POLL_SECONDS = 1.0
CONFIG = os.path.join(HERE, "config.json")

INK = (245, 245, 238)
STATES = {
    "blocked": ((176, 84, 30), INK),
    "working": ((45, 92, 138), INK),
    "idle": ((47, 107, 79), INK),
    "none": ((38, 38, 33), (140, 140, 132)),
    "offline": ((60, 24, 24), (200, 170, 170)),
    "ask": ((176, 84, 30), INK),
    "allow": ((32, 120, 64), INK),
    "deny": ((150, 30, 40), INK),
}
# Seconds a decision key must have been showing before a press counts.
ARM_SECONDS = 1.0
DECISION_KEYS = 3


def api(path, payload=None, timeout=6.0):
    url = ENDPOINT + path
    data = json.dumps(payload).encode() if payload is not None else None
    headers = {"Content-Type": "application/json"} if data else {}
    req = urllib.request.Request(url, data=data, headers=headers)
    with urllib.request.urlopen(req, timeout=timeout) as fh:
        return json.loads(fh.read().decode())


def load_font(size):
    # Segoe UI is present on every Windows install; fall back rather than fail
    # on a machine where it is not.
    for name in ("segoeui.ttf", "arial.ttf", "DejaVuSans.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def wrap(label, width=9):
    """Split a project name onto short lines a 72px key can show."""
    words = label.replace("_", " ").replace("-", " ").split()
    lines, current = [], ""
    for word in words:
        if len(word) > width:
            if current:
                lines.append(current)
                current = ""
            lines.append(word[: width - 1] + "…")
            continue
        candidate = (current + " " + word).strip()
        if len(candidate) <= width:
            current = candidate
        else:
            if current:
                lines.append(current)
            current = word
    if current:
        lines.append(current)
    return lines[:3] or [label[:width]]


def render_key(deck, label, state, badge=None):
    bg, fg = STATES.get(state, STATES["none"])
    image = PILHelper.create_image(deck, background=bg)
    draw = ImageDraw.Draw(image)
    w, h = image.size

    lines = wrap(label)
    font = load_font(13 if len(lines) < 3 else 11)
    line_h = font.size + 2
    y = (h - line_h * len(lines)) // 2
    for line in lines:
        span = draw.textbbox((0, 0), line, font=font)
        draw.text(((w - (span[2] - span[0])) // 2, y), line, font=font, fill=fg)
        y += line_h

    if badge:
        small = load_font(10)
        span = draw.textbbox((0, 0), badge, font=small)
        draw.text((w - (span[2] - span[0]) - 4, h - small.size - 4), badge,
                  font=small, fill=fg)
    return PILHelper.to_native_format(deck, image)


def state_for(row):
    if row is None:
        return "none"
    if row.get("attention"):
        return "blocked"
    if row.get("status") == "busy" or row.get("state") == "working":
        return "working"
    if row.get("status") == "idle":
        return "idle"
    return "none"


class Surface:
    def __init__(self, deck, pinned=None):
        self.deck = deck
        self.pinned = pinned or []
        self.slots = []          # key index -> project name
        self.last = {}           # key index -> (state, label)
        self.lock = threading.Lock()
        self.decision = None     # the pending request on the decision keys
        self.decision_at = 0.0   # when it appeared there

    def project_keys(self):
        """Keys left for projects: all of them, unless a decision is showing."""
        n = self.deck.key_count()
        return n - DECISION_KEYS if self.decision else n

    def set_decision(self, pending, now=None):
        """Show the oldest parked request, or clear the decision keys."""
        now = time.time() if now is None else now
        oldest = pending[0] if pending else None
        with self.lock:
            if (oldest or {}).get("id") != (self.decision or {}).get("id"):
                self.decision_at = now
            self.decision = oldest

    def decision_slot(self, key):
        """'info', 'allow', 'deny' for a decision key, else None."""
        if not self.decision:
            return None
        first = self.deck.key_count() - DECISION_KEYS
        if key < first:
            return None
        return ("info", "allow", "deny")[key - first]

    def decide_press(self, key, now=None):
        """The verdict a press on `key` means, or None. Pure apart from the clock."""
        now = time.time() if now is None else now
        with self.lock:
            slot = self.decision_slot(key)
            if slot not in ("allow", "deny"):
                return None
            if now - self.decision_at < ARM_SECONDS:
                return None
            return {"id": self.decision["id"], "verdict": slot,
                    "who": "streamdeck"}

    def layout(self, sessions, projects_by_path):
        """Pinned projects first, then whatever is live, then blanks."""
        live = {}
        for row in sessions:
            cwd = row.get("cwd")
            if not cwd:
                continue
            name = projects_by_path.get(os.path.normcase(os.path.abspath(cwd)))
            if not name:
                continue
            # A blocked session outranks whatever else shares its project.
            if name not in live or row.get("attention"):
                live[name] = row

        order = list(self.pinned)
        extras = sorted(
            (n for n in live if n not in order),
            key=lambda n: (not live[n].get("attention"), n.lower()),
        )
        order += extras
        return order[: self.project_keys()], live

    def paint(self, order, live):
        with self.lock:
            self.slots = order
            for key in range(self.deck.key_count()):
                slot = self.decision_slot(key)
                if slot:
                    d = self.decision
                    what = "%s %s" % (d.get("tool") or "?", os.path.basename(
                        str(d.get("cwd") or "").rstrip("/\\")))
                    state, label, badge = {
                        "info": ("ask", what, None),
                        "allow": ("allow", "ALLOW", None),
                        "deny": ("deny", "DENY", None),
                    }[slot]
                    if self.last.get(key) != (state, label, badge):
                        self.deck.set_key_image(key, render_key(self.deck, label,
                                                                state, badge))
                        self.last[key] = (state, label, badge)
                    continue
                name = order[key] if key < len(order) else None
                if not name:
                    state, label, badge = "none", "", None
                else:
                    row = live.get(name)
                    state = state_for(row)
                    label = name
                    badge = None
                    if row and row.get("attention") and row.get("attention_since"):
                        badge = "%dm" % max(
                            0, int((time.time() - row["attention_since"]) // 60)
                        )
                if self.last.get(key) == (state, label, badge):
                    continue
                self.deck.set_key_image(key, render_key(self.deck, label, state, badge))
                self.last[key] = (state, label, badge)

    def on_press(self, deck, key, pressed):
        if not pressed:
            return
        verdict = self.decide_press(key)
        if verdict:
            try:
                result = api("/api/decide", verdict)
                print("press %-2d %s -> %s" % (key, verdict["verdict"],
                                             "ok" if result.get("ok") else "too late"))
            except (urllib.error.URLError, OSError, ValueError) as exc:
                print("press %-2d %s -> dispatcher unreachable: %s"
                      % (key, verdict["verdict"], exc))
            return
        if self.decision_slot(key):
            return  # the info key, or a decision key not yet armed
        with self.lock:
            name = self.slots[key] if key < len(self.slots) else None
        if not name:
            return
        try:
            result = api("/api/focus", {"project": name})
            print("press %-2d %-24s -> %s" % (key, name, result.get("action") or
                                              result.get("error")))
        except (urllib.error.URLError, OSError, ValueError) as exc:
            print("press %-2d %-24s -> dispatcher unreachable: %s" % (key, name, exc))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--brightness", type=int, default=60)
    ap.add_argument("--once", action="store_true", help="paint one frame and exit")
    args = ap.parse_args()

    pinned = []
    try:
        with open(CONFIG, encoding="utf-8") as fh:
            pinned = json.load(fh).get("streamdeck", {}).get("keys", []) or []
    except (OSError, ValueError):
        pass

    decks = DeviceManager().enumerate()
    if not decks:
        raise SystemExit("no Stream Deck found")
    deck = decks[0]
    deck.open()
    deck.reset()
    deck.set_brightness(args.brightness)
    print("%s, %d keys" % (deck.deck_type(), deck.key_count()))

    surface = Surface(deck, pinned)
    deck.set_key_callback(surface.on_press)

    try:
        while True:
            try:
                sessions = api("/api/sessions")["sessions"]
                surface.set_decision(api("/api/pending").get("pending") or [])
                catalogue = api("/api/projects")["projects"]
                by_path = {
                    os.path.normcase(os.path.abspath(p["path"])): p["name"]
                    for p in catalogue
                }
                order, live = surface.layout(sessions, by_path)
                surface.paint(order, live)
            except (urllib.error.URLError, OSError, ValueError, KeyError):
                # The dispatcher restarting is normal; say so on the keys
                # rather than exiting and leaving a dead board. A decision
                # that can no longer be delivered stops being offered.
                surface.set_decision([])
                surface.paint([], {})
            if args.once:
                break
            time.sleep(POLL_SECONDS)
    except KeyboardInterrupt:
        pass
    finally:
        deck.reset()
        deck.close()


if __name__ == "__main__":
    main()
