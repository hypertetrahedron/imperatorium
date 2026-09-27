"""Durable bits the dispatcher cannot rebuild by asking Claude Code.

Session tokens are the reason this exists: Claude Code exports
CLAUDE_CODE_MESSAGING_TOKEN to hooks and nowhere else, so if the dispatcher
forgets one it cannot dispatch to that session again until the session
restarts. Everything else here is a convenience.

Written through a temporary file and replaced atomically: the board reads
this while hooks write it.
"""
import json
import os
import tempfile
import threading
import time
import uuid

MAX_EVENTS = 400


class Store:
    def __init__(self, path):
        self.path = path
        self._lock = threading.Lock()
        self._data = {"tokens": {}, "events": [], "sessions": {}, "queue": {},
                      "prefs": {}}
        self.load()

    def load(self):
        try:
            with open(self.path, encoding="utf-8") as fh:
                data = json.load(fh)
        except (OSError, ValueError):
            return
        if isinstance(data, dict):
            for key in ("tokens", "events", "sessions", "queue", "prefs"):
                if isinstance(data.get(key), (dict, list)):
                    self._data[key] = data[key]

    def _flush_locked(self):
        d = os.path.dirname(os.path.abspath(self.path)) or "."
        os.makedirs(d, exist_ok=True)
        fd, tmp = tempfile.mkstemp(dir=d, prefix=".state-", suffix=".tmp")
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as fh:
                json.dump(self._data, fh, indent=1)
            # An atomic replace is the point of the temporary file, but on an
            # SMB share Windows answers a rename-over with ERROR_ACCESS_DENIED
            # often enough that failing the write would lose a session token.
            # Retry briefly, then write in place rather than drop the state.
            for attempt in range(5):
                try:
                    os.replace(tmp, self.path)
                    return
                except OSError:
                    if attempt == 4:
                        break
                    time.sleep(0.05 * (attempt + 1))
            with open(self.path, "w", encoding="utf-8") as fh:
                json.dump(self._data, fh, indent=1)
        finally:
            try:
                os.unlink(tmp)
            except OSError:
                pass

    def register(self, session_id, token=None, **meta):
        if not session_id:
            return
        with self._lock:
            if token:
                self._data["tokens"][session_id] = token
            rec = self._data["sessions"].setdefault(session_id, {})
            rec.update({k: v for k, v in meta.items() if v is not None})
            rec["seen_at"] = time.time()
            self._flush_locked()

    def raise_attention(self, session_id, reason, kind, **meta):
        """Mark a session as wanting a human, and say why.

        The clock is only started once. A stalled turn can fail over and over
        - three 529s in four minutes here - and restarting the clock on each
        would make a tile that has been stuck for ten minutes claim to be
        seconds old, which is the one number you look at to decide whether to
        go and rescue it.
        """
        if not session_id:
            return
        with self._lock:
            rec = self._data["sessions"].setdefault(session_id, {})
            rec.update({k: v for k, v in meta.items() if v is not None})
            rec.setdefault("attention_since", time.time())
            rec["attention_reason"] = reason
            rec["attention_kind"] = kind
            rec["seen_at"] = time.time()
            self._flush_locked()

    def clear_attention(self, session_id):
        """The turn moved on, so nothing is waiting on a human here.

        `register` merges and drops None values, which is what makes it safe
        for partial updates - so clearing a key needs its own write.
        """
        with self._lock:
            rec = self._data["sessions"].get(session_id)
            if not rec:
                return
            dropped = [rec.pop(k, None) for k in
                       ("attention_since", "attention_reason", "attention_kind")]
            if any(v is not None for v in dropped):
                self._flush_locked()

    def forget(self, session_id):
        with self._lock:
            self._data["tokens"].pop(session_id, None)
            self._data["sessions"].pop(session_id, None)
            self._flush_locked()

    def tokens(self):
        with self._lock:
            return dict(self._data["tokens"])

    def credentials(self):
        """{session_id: {socket, token}} - the pair, never the token alone.

        A session that restarts keeps its id but binds a NEW inbox, so a token
        keeps looking valid while it has silently stopped matching. Callers
        must check the socket before trusting the token.
        """
        with self._lock:
            out = {}
            for sid, token in self._data["tokens"].items():
                rec = self._data["sessions"].get(sid) or {}
                out[sid] = {"token": token, "socket": rec.get("socket")}
            return out

    def sessions(self):
        with self._lock:
            return {k: dict(v) for k, v in self._data["sessions"].items()}

    def add_event(self, event):
        with self._lock:
            event = dict(event)
            event.setdefault("at", time.time())
            self._data["events"].append(event)
            if len(self._data["events"]) > MAX_EVENTS:
                del self._data["events"][:-MAX_EVENTS]
            self._flush_locked()

    def events(self, limit=100):
        with self._lock:
            return list(self._data["events"][-limit:])[::-1]

    # -- queued prompts ---------------------------------------------------
    # A prompt held for a session until its current turn ends. Persisted,
    # because "I queued that before the restart" should not silently vanish.
    def enqueue(self, session_id, prompt, note=None):
        with self._lock:
            q = self._data["queue"].setdefault(session_id, [])
            # Random, not the clock: two prompts queued in the same tick
            # would share an id, and cancelling one would cancel both.
            item = {"id": uuid.uuid4().hex[:12], "prompt": prompt,
                    "at": time.time()}
            if note:
                item["note"] = note
            q.append(item)
            self._flush_locked()
            return dict(item)

    def queued(self, session_id=None):
        with self._lock:
            if session_id is not None:
                return [dict(i) for i in self._data["queue"].get(session_id, ())]
            return {k: [dict(i) for i in v]
                    for k, v in self._data["queue"].items() if v}

    def take_queued(self, session_id):
        """Pop the oldest queued prompt for a session, or None."""
        with self._lock:
            q = self._data["queue"].get(session_id)
            if not q:
                return None
            item = q.pop(0)
            if not q:
                self._data["queue"].pop(session_id, None)
            self._flush_locked()
            return item

    def requeue_front(self, session_id, item, error=None):
        """Put back a prompt whose delivery failed, saying why."""
        with self._lock:
            item = dict(item)
            if error:
                item["error"] = error
            self._data["queue"].setdefault(session_id, []).insert(0, item)
            self._flush_locked()

    def unqueue(self, session_id, item_id):
        with self._lock:
            q = self._data["queue"].get(session_id) or []
            kept = [i for i in q if i.get("id") != item_id]
            if len(kept) == len(q):
                return False
            if kept:
                self._data["queue"][session_id] = kept
            else:
                self._data["queue"].pop(session_id, None)
            self._flush_locked()
            return True

    # -- window preferences -------------------------------------------------
    # Pins, archived sessions and grouping. Kept here rather than in the
    # browser so the window, a tablet and the Stream Deck agree.
    def prefs(self):
        with self._lock:
            return json.loads(json.dumps(self._data["prefs"]))

    def set_prefs(self, **changes):
        with self._lock:
            for k, v in changes.items():
                if v is None:
                    self._data["prefs"].pop(k, None)
                else:
                    self._data["prefs"][k] = v
            self._flush_locked()
            return json.loads(json.dumps(self._data["prefs"]))
