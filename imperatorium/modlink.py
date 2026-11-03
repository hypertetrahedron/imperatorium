"""The dispatcher's side of the Imperatorium mod (mod/imperatorium).

The mod runs inside each Claude Code session (2.1.287+, loaded through
CLAUDE_CODE_PLUGIN_DIRS) and beats to `POST /api/mod/beat` every couple of
seconds, and at once when a turn starts or ends or usage changes. A beat
says what the session is doing; the reply carries the prompts waiting for
it, which the mod submits with `$.prompt.submit` once the session is idle,
and the line it shows under the prompt.

Why this replaces the inbox socket where it can: a prompt posted to a
session's socket arrives as a peer's message ("Another Claude session sent
a message"), which the receiving Claude treats as someone else's request and
which can never count as the person's consent. A mod's prompt submitted
`asUser` is read as the person's own words. That is exactly as much power
as typing at the terminal, so only a prompt the person sent from the window
- the page's token proves it (Handler._from_window) - goes `as_user`;
anything else (impctl, the palette, a script, another Claude session) is
submitted framed, under the mod's name, with the same limits a peer
message has.

The token. A beat or a check must carry the token in this folder's
`mod-token` file, which the mod reads with `$.fs.read`. Like the window's
token, it is not a boundary against a process running as you - such a
process can read the file - it keeps anything that merely reaches the port
from posing as a session's mod, taking its prompts or reporting its state.

Nothing here touches the network: Handler parses and authenticates, this
keeps the state. Thread-safe: the dispatcher serves requests on threads.
"""
import hmac
import os
import secrets
import threading
import time
import uuid

TOKEN_FILE = "mod-token"
# A mod that has not beaten for this long is gone: the session ended, the
# mod was unloaded, or the session is wedged. Its prompts go the old way.
LIVE_FOR = 8.0
# A prompt handed to a mod that never said it accepted it (the mod
# reloaded mid-beat) is offered again after this long.
CLAIM_FOR = 30.0
# How many settled prompts per session are remembered for the window.
SETTLED_KEEP = 20


def load_token(folder):
    """The mod token in `folder`, made on first use. User-only on POSIX."""
    path = os.path.join(folder, TOKEN_FILE)
    try:
        with open(path, encoding="utf-8") as fh:
            got = fh.read().strip()
        if len(got) >= 32:
            return got
    except OSError:
        pass
    os.makedirs(folder, exist_ok=True)
    token = secrets.token_urlsafe(32)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as fh:
        fh.write(token + "\n")
    return token


def statusline_payload(usage):
    """`$.session.usage()` as the statusline payload usage.reading() reads,
    so the meters, pace and alerts take a mod's reading unchanged. Pure."""
    usage = usage if isinstance(usage, dict) else {}
    ctx = usage.get("context") or {}
    out = {"context_window": {}, "cost": {}, "rate_limits": {}}
    if isinstance(ctx.get("percent"), (int, float)):
        out["context_window"]["used_percentage"] = ctx["percent"]
    if isinstance(ctx.get("window"), (int, float)):
        out["context_window"]["context_window_size"] = ctx["window"]
    cost = usage.get("cost") or {}
    if isinstance(cost.get("usd"), (int, float)):
        out["cost"]["total_cost_usd"] = cost["usd"]
    if usage.get("model"):
        out["model"] = {"display_name": str(usage["model"])}
    for w in usage.get("rateLimits") or []:
        if not isinstance(w, dict) or not isinstance(w.get("percentUsed"), (int, float)):
            continue
        entry = {"used_percentage": w["percentUsed"]}
        resets = _epoch(w.get("resetsAt"))
        if resets is not None:
            entry["resets_at"] = resets
        out["rate_limits"][str(w.get("kind") or "")] = entry
    return out


def _epoch(iso):
    """An ISO 8601 time as epoch seconds, or None."""
    if not isinstance(iso, str) or not iso:
        return None
    from datetime import datetime
    try:
        return datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


class ModLink:
    def __init__(self, folder, clock=time.time):
        self.token = load_token(folder)
        self.clock = clock
        self._lock = threading.Lock()
        self._mods = {}      # session id -> what its last beat said, and when
        self._outbox = {}    # session id -> [prompt], oldest first
        self._settled = {}   # session id -> [prompt], newest last

    def authentic(self, token):
        return isinstance(token, str) and hmac.compare_digest(token, self.token)

    # -- beats ------------------------------------------------------------
    def beat(self, beat):
        """Record one beat; return the prompts to submit now, and settle the
        ones it reports on. `beat` is the mod's JSON, already authenticated."""
        sid = str(beat.get("session_id") or "")
        if not sid:
            return []
        now = self.clock()
        with self._lock:
            known = self._mods.get(sid) or {}
            self._mods[sid] = {
                "at": now,
                "since": known.get("since", now),
                "cwd": beat.get("cwd") or known.get("cwd"),
                "busy": bool(beat.get("busy")),
                "turn_started": beat.get("turn_started"),
                "interactive": beat.get("interactive"),
                "surfaces": beat.get("surfaces") or [],
                "version": beat.get("version"),
                "mod_version": beat.get("mod_version"),
                "usage": beat.get("usage") or known.get("usage"),
            }
            for ack in beat.get("acks") or []:
                self._settle_locked(sid, ack, now)
            give = []
            for item in self._outbox.get(sid, ()):
                if item.get("accepted_at"):
                    continue  # In the engine's queue: never handed out twice.
                if item.get("claimed_at") and now - item["claimed_at"] < CLAIM_FOR:
                    continue
                item["claimed_at"] = now
                give.append({"id": item["id"], "text": item["text"],
                             "as_user": bool(item["as_user"])})
            return give

    def _settle_locked(self, sid, ack, now):
        if not isinstance(ack, dict):
            return
        box = self._outbox.get(sid) or []
        item = next((i for i in box if i["id"] == ack.get("id")), None)
        if item is None:
            return
        outcome = ack.get("outcome")
        if outcome == "accepted":
            # Handed to `$.prompt.submit`, which holds it until the session is
            # idle: still pending here, and never offered again.
            item["accepted_at"] = now
            return
        if outcome == "submitted":
            # Its turn has started.
            box.remove(item)
            item.update(outcome="submitted", settled_at=now)
        elif outcome == "dropped":
            box.remove(item)
            item.update(outcome="dropped", reason=str(ack.get("reason") or "")[:300],
                        settled_at=now)
        else:
            return
        if not box:
            self._outbox.pop(sid, None)
        kept = self._settled.setdefault(sid, [])
        kept.append(item)
        del kept[:-SETTLED_KEEP]

    def forget(self, sid):
        with self._lock:
            self._mods.pop(sid, None)

    # -- what the rest of the dispatcher asks ------------------------------
    def live(self, sid):
        with self._lock:
            got = self._mods.get(sid)
            return bool(got and self.clock() - got["at"] <= LIVE_FOR)

    def state(self, sid):
        """The last beat of a live mod, or None."""
        with self._lock:
            got = self._mods.get(sid)
            if not got or self.clock() - got["at"] > LIVE_FOR:
                return None
            return dict(got)

    def live_ids(self):
        now = self.clock()
        with self._lock:
            return {s for s, m in self._mods.items() if now - m["at"] <= LIVE_FOR}

    def offer(self, sid, text, as_user):
        """Queue `text` for session `sid`'s mod. The prompt's id."""
        item = {"id": uuid.uuid4().hex[:12], "text": text, "as_user": bool(as_user),
                "at": self.clock(), "claimed_at": None}
        with self._lock:
            self._outbox.setdefault(sid, []).append(item)
        return item["id"]

    def pending(self, sid=None):
        """Prompts offered and not yet settled, per session."""
        with self._lock:
            if sid is not None:
                return [dict(i) for i in self._outbox.get(sid, ())]
            return {s: [dict(i) for i in v] for s, v in self._outbox.items() if v}

    def settled(self, sid):
        with self._lock:
            return [dict(i) for i in self._settled.get(sid, ())]
