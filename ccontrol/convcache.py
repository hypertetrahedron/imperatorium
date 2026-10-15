"""Parsed conversations, read again only when the transcript changes.

The window polls the selected session every 1.5 s, the sidebar wants a
"doing" line for every active session every 10 s, and the activity tab asks
every 3 s. Each of those used to parse the whole transcript: for a 27 MB one,
0.25-0.3 s of CPU and ~220 MB of short-lived objects, even to answer "nothing
changed" and even when only the last 8 items were wanted. Claude Code only
ever appends, so a transcript whose size and mtime are unchanged has nothing
new to say.

What is kept is the per-message items, before turns are merged, because a
limit applies before the merge (see `conversation.to_items`). The whole
merged conversation and its signature are kept alongside, since that is what
the poll asks for. Entries are dropped least recently used first once their
total weight (the JSON size of the merged items) passes a budget.
"""
import collections
import hashlib
import json
import os
import threading

from . import conversation, sessionstat


def signature(items):
    """What the window compares to decide whether to repaint."""
    return hashlib.sha1(json.dumps(items, sort_keys=True).encode("utf-8")
                        ).hexdigest()[:16]


class Cache:
    KEEP = 32
    BUDGET = 32 * 1024 * 1024  # bytes of merged-items JSON

    def __init__(self, reader=None, locate=None, keep=KEEP, budget=BUDGET):
        self._read = reader or (lambda sid: conversation.read(sid, limit=0, merge=False))
        self._locate = locate or sessionstat.transcript_path
        self.keep, self.budget = keep, budget
        self._lock = threading.Lock()
        self._entries = collections.OrderedDict()  # sid -> entry dict
        self._paths = {}  # sid -> transcript path, found once
        self._filling = {}  # sid -> lock, so one parse per session at a time
        self.parses = 0  # for tests and diagnosis

    def _stamp(self, sid):
        """(path, size, mtime_ns) of the transcript, or None if there is none."""
        for attempt in (0, 1):
            path = self._paths.get(sid)
            if path is None or attempt:
                path = self._locate(sid)
                if not path:
                    return None
                self._paths[sid] = path
            try:
                st = os.stat(path)
            except OSError:
                continue
            return path, st.st_size, st.st_mtime_ns
        return None

    def _entry(self, sid):
        stamp = self._stamp(sid)
        if stamp is None:
            # Not a local transcript this cache can watch (or not yet
            # written): read straight through, as before.
            raw = self._read(sid)
            merged = conversation.merge_turns(raw)
            return {"raw": raw, "merged": merged, "sig": signature(merged)}
        with self._lock:
            hit = self._entries.get(sid)
            if hit and hit["stamp"] == stamp:
                self._entries.move_to_end(sid)
                return hit
            fill = self._filling.setdefault(sid, threading.Lock())
        with fill:
            with self._lock:
                hit = self._entries.get(sid)
                if hit and hit["stamp"] == stamp:
                    self._entries.move_to_end(sid)
                    return hit
            raw = self._read(sid)
            merged = conversation.merge_turns(raw)
            text = json.dumps(merged, sort_keys=True)
            entry = {"stamp": stamp, "raw": raw, "merged": merged,
                     "sig": hashlib.sha1(text.encode("utf-8")).hexdigest()[:16],
                     "weight": len(text)}
            self.parses += 1
            with self._lock:
                self._entries[sid] = entry
                self._entries.move_to_end(sid)
                self._trim()
            return entry

    def _trim(self):
        total = sum(e["weight"] for e in self._entries.values())
        while len(self._entries) > 1 and (len(self._entries) > self.keep
                                          or total > self.budget):
            _, gone = self._entries.popitem(last=False)
            total -= gone["weight"]

    def items(self, session_id, limit=0):
        """`conversation.read(session_id, limit)`, from the cache.

        Callers get the cached lists: read them, do not change them.
        """
        entry = self._entry(session_id)
        if not limit:
            return entry["merged"]
        return conversation.merge_turns(entry["raw"][-limit:])

    def conversation(self, session_id):
        """(merged items, signature) for the whole conversation."""
        entry = self._entry(session_id)
        return entry["merged"], entry["sig"]

    def forget(self, session_id):
        with self._lock:
            self._entries.pop(session_id, None)
            self._paths.pop(session_id, None)
