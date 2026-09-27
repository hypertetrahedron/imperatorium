"""Context, cost and rate-limit readings, and when the limit will be hit.

The only place Claude Code hands these out is the statusline: every render
it pipes a JSON object to the configured command, carrying the session's
`context_window` and, for a subscription, the account's `rate_limits`
(`five_hour` and `seven_day`, each a percentage and a reset time). The
statusline script (`hooks/cc_statusline.py`) forwards that object here.

Kept in memory, not in the store: a statusline renders several times a
minute per session, and the store rewrites its whole file on every change.
Losing these on restart costs one render to rebuild.

The forecast is a straight line through the recent five-hour readings. That
is crude, and it is also exactly the question - "at this pace, do I run out
before it resets?" - so it is stated as a pace, not a prediction.
"""
import threading
import time

WINDOWS = ("five_hour", "seven_day", "spend_limit")
# Readings older than this say nothing about the current pace.
PACE_SPAN = 45 * 60.0
MIN_SPAN = 5 * 60.0
DEFAULT_THRESHOLDS = (80.0, 95.0)


def _num(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    return float(value)


def reading(payload):
    """The fields worth keeping from one statusline payload. Pure."""
    ctx = payload.get("context_window") or {}
    cost = payload.get("cost") or {}
    model = payload.get("model") or {}
    limits = {}
    for name in WINDOWS:
        w = (payload.get("rate_limits") or {}).get(name) or {}
        pct, resets = _num(w.get("used_percentage")), _num(w.get("resets_at"))
        if pct is not None:
            limits[name] = {"used": pct, "resets_at": resets}
    return {
        "context_pct": _num(ctx.get("used_percentage")),
        "context_size": _num(ctx.get("context_window_size")),
        "cost_usd": _num(cost.get("total_cost_usd")),
        "model": model.get("display_name") or model.get("id"),
        "limits": limits,
    }


def same_window(a, b, slack=120.0):
    if a is None or b is None:
        return a is None and b is None
    return abs(a - b) <= slack


def pace(samples, now, resets_at=None):
    """Percent per hour over the recent samples, and when 100 is reached.

    `samples` is [(t, pct)] within one window. Returns {per_hour, full_at,
    before_reset} or None when there is too little history to say anything:
    two readings a minute apart would extrapolate noise into a panic.
    """
    recent = [(t, p) for t, p in samples if now - t <= PACE_SPAN]
    if len(recent) < 2 or recent[-1][0] - recent[0][0] < MIN_SPAN:
        return None
    n = float(len(recent))
    mt = sum(t for t, _ in recent) / n
    mp = sum(p for _, p in recent) / n
    var = sum((t - mt) ** 2 for t, _ in recent)
    if var <= 0:
        return None
    slope = sum((t - mt) * (p - mp) for t, p in recent) / var  # pct per second
    per_hour = slope * 3600.0
    last = recent[-1][1]
    if slope <= 0:
        return {"per_hour": max(0.0, per_hour), "full_at": None, "before_reset": False}
    full_at = now + (100.0 - last) / slope
    return {"per_hour": per_hour, "full_at": full_at,
            "before_reset": bool(resets_at and full_at < resets_at)}


class Usage:
    def __init__(self, thresholds=DEFAULT_THRESHOLDS, on_crossing=None):
        self._lock = threading.Lock()
        self.sessions = {}   # session key -> reading + at
        self.limits = {}     # window -> {used, resets_at, at}
        self._history = {}   # window -> [(t, pct)] for the current resets_at
        self._fired = set()  # (window, window start id, threshold)
        self._window = {}    # window -> resets_at when it was first seen
        self.thresholds = tuple(sorted(float(t) for t in thresholds))
        self.on_crossing = on_crossing

    def ingest(self, key, payload, now=None):
        """One statusline payload from session `key`. Returns crossings fired."""
        now = time.time() if now is None else now
        got = reading(payload)
        crossings = []
        with self._lock:
            self.sessions[key] = dict(got, at=now)
            for name, w in got["limits"].items():
                prev = self.limits.get(name)
                # Every session reports the same account-wide number; the
                # newest reading wins, and a new reset time starts a new
                # window with an empty history. "New" means moved by more
                # than a couple of minutes, so a reset time that jitters
                # between readings does not wipe the pace or re-fire alerts.
                window = self._window.get(name)
                if prev is None or not same_window(prev.get("resets_at"),
                                                   w["resets_at"]):
                    self._history[name] = []
                    window = self._window[name] = w["resets_at"]
                self.limits[name] = dict(w, at=now)
                hist = self._history.setdefault(name, [])
                if not hist or hist[-1][1] != w["used"] or now - hist[-1][0] > 60:
                    hist.append((now, w["used"]))
                    del hist[:-240]
                for t in self.thresholds:
                    mark = (name, window, t)
                    if w["used"] >= t and mark not in self._fired:
                        # Only the highest threshold crossed in one jump is
                        # announced; the lower ones are marked as passed.
                        self._fired.add(mark)
                        crossings.append({"window": name, "threshold": t,
                                          "used": w["used"],
                                          "resets_at": w["resets_at"]})
        if crossings:
            top = max(crossings, key=lambda c: c["threshold"])
            if self.on_crossing:
                self.on_crossing(top)
            return [top]
        return []

    def forget(self, key):
        with self._lock:
            self.sessions.pop(key, None)

    def session(self, key):
        with self._lock:
            got = self.sessions.get(key)
            return dict(got) if got else None

    def summary(self, now=None):
        """The account's windows, each with its pace; for the header meter."""
        now = time.time() if now is None else now
        out = {}
        with self._lock:
            for name, w in self.limits.items():
                resets = w.get("resets_at")
                if resets and resets < now:
                    continue  # Claude Code drops a window once it resets.
                entry = dict(w)
                entry["pace"] = pace(self._history.get(name, []), now, resets)
                out[name] = entry
        return {"limits": out, "thresholds": list(self.thresholds)}
