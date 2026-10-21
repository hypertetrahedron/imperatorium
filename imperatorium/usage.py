"""Context, cost and rate-limit readings, and when the limit will be hit.

The only place Claude Code hands these out is the statusline: every render
it pipes a JSON object to the configured command, carrying the session's
`context_window` and, for a subscription, the account's `rate_limits`
(`five_hour` and `seven_day`, each a percentage and a reset time). The
statusline script (`hooks/cc_statusline.py`) forwards that object here.

Kept in memory, not in the store: a statusline renders several times a
minute per session, and the store rewrites its whole file on every change.
The account's windows are the exception. They are written to a small file
of their own whenever a reading changes them, and read back at start, so a
restarted dispatcher shows the meters (and knows where each window began)
before any session renders again.

A window whose reset time has passed stays on the meter at 0%, marked as
waiting: Claude Code only reports the new window when a session's
statusline next renders, which can be a long while on a quiet machine, and
a meter that vanished at every reset read as broken.

The forecast is a straight line through the recent five-hour readings. That
is crude, and it is also exactly the question - "at this pace, do I run out
before it resets?" - so it is stated as a pace, not a prediction.
"""
import json
import os
import tempfile
import threading
import time

WINDOWS = ("five_hour", "seven_day", "spend_limit")
# Readings older than this say nothing about the current pace.
PACE_SPAN = 45 * 60.0
MIN_SPAN = 5 * 60.0
DEFAULT_THRESHOLDS = (80.0, 95.0)

# Who serves a session that has no rate-limit windows. The statusline script
# reads the provider from the session's environment (`imperatorium_provider`);
# the model id is the fallback for a script too old to send it - Bedrock
# names models by ARN or "us.anthropic.claude-...", Vertex as "claude-...@date".
PROVIDERS = {"bedrock": "AWS Bedrock", "vertex": "Google Vertex AI",
             "foundry": "Microsoft Foundry", "api": "Anthropic API",
             "gateway": "API gateway"}
# A session that has not rendered its statusline for this long no longer
# says what the account is on.
PLAN_SPAN = 30 * 60.0
# The claude.ai plan behind the login, from Claude Code's own profile cache.
# Only Pro and Max carry rate_limits; Team and Enterprise seats do not.
ACCOUNT_FILE = os.path.join(os.path.expanduser("~"), ".claude.json")
ACCOUNT_PLANS = (("enterprise", "Claude Enterprise"), ("team", "Claude Team"))


def _num(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    return float(value)


def provider_of(payload):
    """'bedrock', 'vertex', ... or None for the claude.ai login. Pure."""
    named = payload.get("imperatorium_provider") or payload.get("ccontrol_provider")
    if named in PROVIDERS:
        return named
    mid = str((payload.get("model") or {}).get("id") or "")
    if mid.startswith("arn:aws:bedrock") or ".anthropic.claude" in mid             or mid.startswith("anthropic.claude"):
        return "bedrock"
    if mid.startswith("claude-") and "@" in mid:
        return "vertex"
    return None


def _org_type(path=None, _cache={}):
    """`oauthAccount.organizationType` from Claude Code's profile cache,
    lower-cased ("claude_max", "claude_enterprise", ...), or "". Nothing
    else in that file is read. Re-read when the file changes."""
    path = path or ACCOUNT_FILE
    try:
        mtime = os.path.getmtime(path)
    except OSError:
        return ""
    if _cache.get("key") != (path, mtime):
        kind = ""
        try:
            with open(path, encoding="utf-8") as fh:
                kind = str(((json.load(fh) or {}).get("oauthAccount") or {})
                           .get("organizationType") or "")
        except (OSError, ValueError, AttributeError):
            pass
        _cache.update(key=(path, mtime), kind=kind.lower())
    return _cache["kind"]


def account_plan(path=None):
    """'Claude Enterprise' or 'Claude Team' when the login is one, else None.

    Pro and Max return None: their meters say all there is to say.
    """
    kind = _org_type(path)
    for word, label in ACCOUNT_PLANS:
        if word in kind:
            return label
    return None


def subscription(path=None):
    """Whether the login is a Pro or Max subscription - the plans with a
    five-hour and a seven-day window - so the windows are known before any
    session has rendered its statusline."""
    kind = _org_type(path)
    return "max" in kind or "pro" in kind


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
        "duration_ms": _num(cost.get("total_duration_ms")),
        "api_duration_ms": _num(cost.get("total_api_duration_ms")),
        "lines_added": _num(cost.get("total_lines_added")),
        "lines_removed": _num(cost.get("total_lines_removed")),
        "model": model.get("display_name") or model.get("id"),
        "provider": provider_of(payload),
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
    def __init__(self, thresholds=DEFAULT_THRESHOLDS, on_crossing=None,
                 account=account_plan, path=None, now=None):
        self._lock = threading.Lock()
        self.sessions = {}   # session key -> reading + at
        self.limits = {}     # window -> {used, resets_at, at}
        self._history = {}   # window -> [(t, pct)] for the current resets_at
        self._fired = set()  # (window, window start id, threshold)
        self._window = {}    # window -> resets_at when it was first seen
        self.thresholds = tuple(sorted(float(t) for t in thresholds))
        self.on_crossing = on_crossing
        self.account = account
        self.path = path     # where the windows are kept across restarts
        self._saved = None
        self._load(time.time() if now is None else now)

    def _load(self, now):
        """The windows the last dispatcher saw; one that has reset since
        shows as waiting (see `summary`). The thresholds they had crossed
        stay crossed, so a restart does not announce them again."""
        if not self.path:
            return
        try:
            with open(self.path, encoding="utf-8") as fh:
                got = json.load(fh)
        except (OSError, ValueError):
            return
        for name, w in ((got or {}).get("limits") or {}).items():
            if name not in WINDOWS or not isinstance(w, dict):
                continue
            used, resets, at = _num(w.get("used")), _num(w.get("resets_at")), _num(w.get("at"))
            if used is None:
                continue
            self.limits[name] = {"used": used, "resets_at": resets, "at": at or now}
            self._window[name] = resets
            self._history[name] = []
            for t in self.thresholds:
                if used >= t:
                    self._fired.add((name, resets, t))
        self._saved = self._signature()

    def _signature(self):
        return sorted((n, w.get("used"), w.get("resets_at")) for n, w in self.limits.items())

    def _save(self):
        """Write the windows if a reading changed them. Call with the lock
        held; a failed write only costs the meters after a restart."""
        if not self.path or self._signature() == self._saved:
            return
        try:
            folder = os.path.dirname(self.path) or "."
            os.makedirs(folder, exist_ok=True)
            fd, tmp = tempfile.mkstemp(dir=folder, prefix=".limits-", suffix=".tmp")
            with os.fdopen(fd, "w", encoding="utf-8") as fh:
                json.dump({"limits": self.limits}, fh)
            os.replace(tmp, self.path)
            self._saved = self._signature()
        except OSError:
            pass

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
            self._save()
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
                    # Reset, and no session has reported the new window
                    # yet: 0%, its next reset unknown, until one does.
                    out[name] = {"used": 0.0, "resets_at": None, "at": w.get("at"),
                                 "reset_at": resets, "waiting": True, "pace": None}
                    continue
                entry = dict(w)
                entry["pace"] = pace(self._history.get(name, []), now, resets)
                out[name] = entry
            seen = [s.get("provider") for s in self.sessions.values()
                    if now - s.get("at", 0) <= PLAN_SPAN]
        return {"limits": out, "thresholds": list(self.thresholds),
                "plan": self.plan(seen, bool(out))}

    def plan(self, seen, has_limits):
        """What to show where the meters go: {label, providers} or None.

        `seen` is the provider of each recently heard-from session (None for
        the claude.ai login). With no windows, the label names what the
        sessions run on instead - "AWS Bedrock" - because a blank space
        reads as "the meter is broken". Beside real windows it names only
        the other providers, which those windows do not cover.
        """
        others = sorted({p for p in seen if p})
        labels = [PROVIDERS[p] for p in others]
        if not has_limits and (not others or None in seen):
            try:
                acct = self.account() if self.account else None
            except Exception:
                acct = None
            if acct:
                labels.insert(0, acct)
        if not labels:
            return None
        return {"label": " · ".join(labels), "providers": others,
                "alongside": has_limits}
