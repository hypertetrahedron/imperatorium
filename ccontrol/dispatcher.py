"""The control plane: one local HTTP server, three jobs.

  1. Hook receiver. Claude Code posts hook events here (hook type "http"),
     which is how the dispatcher learns a session exists, gets its messaging
     token, and hears that it is blocked or has finished a turn.
  2. Query surface. /api/sessions and /api/projects for the board, the Stream
     Deck, and any other front-end.
  3. Dispatch. /api/dispatch takes what a human said and routes it.

Bound to 127.0.0.1. POSTs must be JSON and must carry no Origin header or
this server's own: a cross-origin fetch with a plain-text body is a CORS
"simple request", so a page the user merely had open could otherwise drive
this.
"""
import hmac
import json
import os
import secrets
import sys
import threading
import time
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from . import (activity, background, changes, closing, conversation, convcache,
               deeplink, history, inbox, matrix, notify, pending, procs, projects,
               prs, registry, remote, sessionstat, slash, taskout, ui, usage, voice,
               winfocus)
from . import settings as settingsmod
from .store import Store

HOST = "127.0.0.1"
PORT = 8792
MAX_BODY = 4 * 1024 * 1024
WINDOWS = sys.platform == "win32"

HERE = os.path.dirname(os.path.abspath(__file__))
PROJECT_DIR = os.path.dirname(HERE)


def default_state_path():
    """Runtime state lives on local disk, never beside the source.

    The project itself sits on a mapped network drive here, and Windows
    refuses an atomic rename-over on SMB often enough to matter for a file
    that holds session tokens.
    """
    base = (
        os.environ.get("LOCALAPPDATA")
        or os.environ.get("XDG_STATE_HOME")
        or os.path.join(os.path.expanduser("~"), ".local", "state")
    )
    return os.path.join(base, "ccontrol", "state.json")


STATE_PATH = default_state_path()

# Hook events worth keeping. Everything else is acknowledged and dropped, so a
# broad hook configuration cannot flood the store.
INTERESTING = {
    "SessionStart",
    "SessionEnd",
    "Notification",
    "Stop",
    "StopFailure",
    "SubagentStop",
    "TaskCompleted",
}

# Not a hook: what `hooks/cc_statusline.py` labels the statusline JSON it
# forwards, so it can share the `/hook` route (and a remote host's tunnel).
STATUSLINE = "ccontrol_statusline"

# Notification types that mean a human is the blocker.
ATTENTION_NOTIFICATIONS = {
    "permission_prompt",
    "idle_prompt",
    "agent_needs_input",
    "elicitation_dialog",
    "elicitation_url_dialog",
}

# Events that mean the turn moved on, so nothing is waiting on a human.
# `StopFailure` was in this set and is the whole bug: a turn that ended in an
# API error is exactly when a person is wanted, and treating it as a clean
# finish actively erased the flag. It is handled on its own below.
CLEARING = {"Stop", "SubagentStop", "TaskCompleted"}

# Claude Code sends `idle_prompt` once a finished turn has sat a minute with
# nothing typed. Since 2.1.303 (2026-10-18) none has arrived here, though
# Stop still does, so no session started "waiting for you" and nothing was
# spoken, pushed or shown on the desktop. The dispatcher raises the same
# thing itself from Stop after this long; a real one, if it comes, is the
# same raise and is not announced twice.
IDLE_AFTER = 60.0

# How a session came to want a human. A permission prompt is answered by a
# person, so the session resuming work proves it was answered; a failed turn
# is not resolved by the session retrying, only by it finishing or by you.
BY_NOTIFICATION, BY_FAILURE = "notification", "failure"


def remote_key(host, session_id):
    """Where a remote session's hook records live in the store."""
    return "%s/%s" % (host, session_id)


NOT_RUNNING = "that session is not running here, so there is nothing to close"


class Control:
    """Everything the request handler needs, so the handler stays dumb."""

    # SSH is slow and a sleeping host is ordinary, so remote state is
    # refreshed on its own schedule and never on the request path.
    REMOTE_EVERY = 20.0

    # How often the one-line "what is it doing" is re-read from a session's
    # conversation when no tool hook has said so more cheaply.
    DOING_EVERY = 10.0
    # How old a snapshot may be when it is only used to find a session's row
    # for the chat, the status panel or the activity tab (see `_row`).
    LOOKUP_AGE = 15.0
    # Error analytics read whole transcripts; nobody needs them fresher.
    ERRORS_EVERY = 300.0

    def __init__(self, store, roots=None, background_options=None, hosts=None,
                 config=None):
        self.store = store
        self.roots = roots or projects.DEFAULT_ROOTS
        # model / permission_mode / agent for `claude --bg`, from config.json.
        self.background_options = background_options or {}
        self._cache = {"at": 0.0, "snapshot": None}
        self._lock = threading.Lock()
        self._changed = threading.Condition()
        self._settings_lock = threading.Lock()
        self._restart_needed = set()  # processes a saved setting waits on
        config = config or {}
        # Handed to the window's pages when they load and never written down:
        # what Send As Typed, and turning it on, ask for (see send_typed).
        self.window_token = secrets.token_urlsafe(24)
        # Which dispatcher this is: an open page that sees it change reloads,
        # so it does not keep running the code the last dispatcher served.
        self.boot_id = secrets.token_hex(8)
        self.send_as_typed = bool((config.get("chat") or {}).get("send_as_typed"))
        self.quick = {"key": settingsmod.value(config, settingsmod.BY_KEY["quick.key"]),
                      "phrases": settingsmod.value(config, settingsmod.BY_KEY["quick.phrases"])}

        budget = config.get("budget") or {}
        self.notifier = notify.Notifier(config, still_waiting=self._still_waiting)
        self.idle_after = IDLE_AFTER
        prefs = store.prefs().get("notify") or {}
        for channel, on in prefs.items():
            if channel in self.notifier.cfg:
                self.notifier.set_enabled(channel, on)
        # The account's windows are kept beside the store, so a restart shows
        # the meters at once rather than after the next statusline render.
        store_path = getattr(store, "path", None)
        self.usage = usage.Usage(
            thresholds=budget.get("thresholds") or usage.DEFAULT_THRESHOLDS,
            on_crossing=self._budget_crossed,
            path=os.path.join(os.path.dirname(store_path), "limits.json")
            if isinstance(store_path, str) else None)
        self.live = activity.Live()
        self.stats = sessionstat.Reader()
        # Parsed transcripts, reread only when one changes: the chat poll,
        # the doing lines and the activity tab all read through this.
        self.convs = convcache.Cache(
            reader=lambda sid: conversation.read(sid, limit=0, merge=False))
        self.prs = prs.Tracker()
        self.history = history.Index()
        self.cost_history = None  # costs.History, built on first ask
        self.subscribed = usage.subscription  # Pro/Max login: 5h and 7d windows
        self._doing = {}  # session id -> (at, line)
        self._errors = {"at": 0.0, "key": None, "result": None}
        self._tallies = {}  # session id -> (size, tally)

        self.pending = pending.Pending()
        # Dictation runs in a child process (voice.Worker), so the model's
        # ~3.3 GB is given back when it is ended. Only a config that has a
        # `voice` block turns it on: a test's Control never starts one.
        self._voice = None
        if isinstance(config.get("voice"), dict):
            settings = voice.settings_from(config["voice"])
            if settings.get("enabled") and voice.available():
                self._voice = voice.Worker(settings)
                if settings.get("preload"):
                    self._voice.warm()
        self.fleet = remote.Fleet(hosts or ())
        self.tunnels = {}  # host name -> remote.Tunnel, started by serve()
        self.readers = {}  # host name -> remote.Reader, opened on first read
        self._remote = {"at": 0.0, "rows": []}
        if hosts:
            threading.Thread(target=self._refresh_remote, name="remote",
                             daemon=True).start()

    def _refresh_remote(self):
        while True:
            try:
                rows = self.fleet.poll()
            except Exception:
                rows = []
            with self._lock:
                self._remote = {"at": time.time(), "rows": rows}
            time.sleep(self.REMOTE_EVERY)

    def remote_rows(self):
        with self._lock:
            return list(self._remote["rows"])

    def sessions(self, max_age=2.0):
        """Live sessions, cached briefly so a board refresh is not a fork bomb."""
        with self._lock:
            fresh = self._cache["snapshot"]
            if fresh and time.time() - self._cache["at"] < max_age:
                return fresh
        creds = self.store.credentials()
        snap = registry.snapshot(creds=creds)
        known = self.store.sessions()
        for row in snap["sessions"]:
            extra = known.get(row["sessionId"])
            if not extra:
                continue
            self._overlay(row, extra, row["sessionId"])
            # What this session is calling itself in its title bar. The tile
            # shows a derived name (`central-control-70`) and the window
            # shows the conversation's summary, and nothing connected the
            # two - so a tile could not be matched to a window by eye.
            # 1ms per session against a 4.5MB transcript, behind a 2s cache.
            row["window_title"] = conversation.title(row["sessionId"])
        snap["sessions"].extend(self._remote_with_hooks(known, creds))
        queued = self.store.queued()
        for row in snap["sessions"]:
            self._decorate(row, queued)
        snap["sessions"] = self._undismissed(snap["sessions"])
        with self._lock:
            self._cache = {"at": time.time(), "snapshot": snap}
        return snap

    def _undismissed(self, rows):
        """Rows less the ones dismissed from the window. A dismissed session
        that has a process again was resumed, so it comes back."""
        dismissed = self.store.dismissed()
        if not dismissed:
            return rows
        kept = []
        for row in rows:
            if row["key"] in dismissed:
                if not row.get("pid"):
                    continue
                self.store.undismiss(row["key"])
            kept.append(row)
        return kept

    def _decorate(self, row, queued):
        """What the newer features add to a row: doing, usage, PR, queue."""
        key = remote_key(row["host"], row["sessionId"]) if row.get("remote") \
            else row["sessionId"]
        row["key"] = key
        live = self.live.current(key)
        if live:
            row["doing"] = "%s %s" % (live["tool"], activity.first_line(live["summary"]))
            row["doing_since"] = live["since"]
        elif not row.get("remote") and row.get("state") not in ("done", "failed",
                                                                 "stopped"):
            row["doing"] = self._doing_line(row["sessionId"])
        got = self.usage.session(key)
        if got:
            row["usage"] = {k: got.get(k) for k in
                            ("context_pct", "context_size", "cost_usd", "model", "at",
                             "duration_ms", "api_duration_ms", "lines_added",
                             "lines_removed")}
        if not row.get("remote") and row.get("cwd"):
            pr = self.prs.get(row["cwd"])
            if pr:
                row["pr"] = pr
        if queued.get(key):
            row["queued"] = len(queued[key])
        if row.get("state") in ("done", "failed", "stopped") or (
                row.get("kind") == "background" and not row.get("pid")
                and not row.get("status")):
            row["resumable"] = True
        row["resume_command"] = " ".join(history.resume_command(row["sessionId"]))
        if not row.get("remote"):
            row["work"] = self._work(row)

    def _work(self, row):
        """What the tile and the status panel's header line show at a glance:
        when the prompt cache expires, and how many subagents, background
        shells, monitors and shell processes the session has going. Counts
        only while its process lives - nothing it started outlives it."""
        try:
            stat = self.stats.read(row["sessionId"], time.time()) or {}
        except Exception:
            stat = {}
        alive = isinstance(row.get("pid"), int) and bool(row.get("status"))
        # Context size rides along so the window can warn before a prompt
        # re-sends a large context to a cold cache, with what that costs.
        from . import costs
        cache = stat.get("cache") or {}
        out = {"cache_expires_at": cache.get("expires_at"),
               "context_tokens": stat.get("context_tokens"),
               "resume_cost_usd": costs.rewrite_cost(stat.get("model_id"), stat.get("context_tokens"),
                                                     cache.get("ttl")),
               "alive": alive}
        if alive:
            for name in ("agents", "shells", "monitors"):
                out[name] = len(stat.get(name) or [])
            out["processes"] = row.get("shell_processes") or 0
        return out

    def _doing_line(self, sid):
        now = time.time()
        with self._lock:
            hit = self._doing.get(sid)
        if hit and now - hit[0] < self.DOING_EVERY:
            return hit[1]
        try:
            line = activity.doing(self.convs.items(sid, limit=8))
        except Exception:
            line = hit[1] if hit else ""
        with self._lock:
            self._doing[sid] = (now, line)
        return line

    def _overlay(self, row, extra, key):
        """Fold what hooks recorded into a row `claude agents` produced."""
        # Hooks know things `claude agents --json` does not carry.
        for field in ("socket", "name", "cwd", "last_notification", "last_event",
                      "last_event_at", "last_failure", "last_failure_detail",
                      "window_title"):
            if extra.get(field) and not row.get(field):
                row[field] = extra[field]
        # Hook-recorded attention is sticky by design - it has to survive the
        # dispatcher restarting - so it needs something to contradict it.
        # Answering a permission prompt fires no hook, and `Stop` can be
        # minutes away, so without this check the tile kept saying
        # `permission_prompt` at a session already working.
        since = extra.get("attention_since")
        kind = extra.get("attention_kind")
        # Only a notification is settled by the session working again. A
        # failed turn is not: the retry that follows a 529 would otherwise
        # wipe the very flag the failure just raised, which is the "tile says
        # working after an error" report all over again.
        stale = (kind != BY_FAILURE) and registry.moved_on(row, since)
        if since and stale:
            self.store.clear_attention(key)
        elif since and kind != BY_FAILURE and registry.still_running(
                row, extra.get("last_notification")):
            pass
        elif since:
            row["attention_since"] = since
            row["attention"] = True
            row["attention_reason"] = extra.get("attention_reason")
            row["attention_kind"] = kind
        row["dispatchable"] = bool(row.get("socket") and row.get("token"))

    def _remote_with_hooks(self, known, creds):
        """Remote rows, with what that host's hooks reported folded in.

        Kept under host-qualified keys, so nothing a remote host posts can
        overwrite, forget or re-point a session on this machine.
        """
        out = []
        for row in self.remote_rows():
            row = dict(row)
            key = remote_key(row["host"], row["sessionId"])
            extra = known.get(key) or {}
            if not row.get("socket") and extra.get("socket"):
                row["socket"] = extra["socket"]
            one = {row["sessionId"]: row}
            registry.attach_credentials(
                one, {row["sessionId"]: creds[key]} if key in creds else {})
            if extra:
                self._overlay(row, extra, key)
            out.append(row)
        return out

    def host(self, name):
        return next((h for h in self.fleet.hosts if h["name"] == name), None)

    def hosts_status(self):
        status = self.fleet.status()
        out = []
        for h in self.fleet.hosts:
            entry = dict(h)
            tunnel = self.tunnels.get(h["name"])
            if tunnel:
                entry["tunnel_status"] = tunnel.status()
            out.append(entry)
        return {"hosts": out, "status": status}

    def conversation(self, session_id, limit=0):
        """What one session said. Empty rather than fatal when unreadable."""
        row = self._row(session_id, max_age=self.LOOKUP_AGE,
                        match=lambda r: r["sessionId"] == session_id
                        or r.get("id") == session_id)
        if row and row.get("remote"):
            return self._remote_conversation(row, limit)
        sid = row["sessionId"] if row else session_id
        try:
            if limit:
                items = self.convs.items(sid, limit=limit)
                sig = convcache.signature(items)
            else:
                items, sig = self.convs.conversation(sid)
        except Exception as exc:
            return {"session": session_id, "items": [],
                    "error": "%s: %s" % (type(exc).__name__, exc)}
        return {"session": session_id, "items": items, "sig": sig,
                "name": (row or {}).get("name"), "cwd": (row or {}).get("cwd")}

    def _remote_conversation(self, row, limit):
        """The same items, read on the session's own machine.

        The transcript lives there, so the reader runs there: one SSH channel
        per host, kept open, running this project's own `conversation.py`.
        """
        out = {"session": row["sessionId"], "items": [], "remote": True,
               "host": row.get("host"), "name": row.get("name"),
               "cwd": row.get("cwd")}
        host = self.host(row.get("host"))
        if host is None:
            out["error"] = "host %s is not configured" % row.get("host")
            return out
        with self._lock:
            reader = self.readers.get(host["name"])
            if reader is None:
                reader = self.readers[host["name"]] = remote.Reader(host)
        try:
            got = reader.read(row["sessionId"], limit=limit, cwd=row.get("cwd"))
        except remote.RemoteError as exc:
            out["error"] = "reading on %s: %s" % (host["name"], exc)
            return out
        out["items"] = got["items"]
        out["title"] = got["title"]
        return out

    # A remote tile reads this much of the tail over SSH on every poll, not
    # the whole conversation; the replies it keeps are recent anyway.
    MATRIX_REMOTE_ITEMS = 150

    def matrix(self):
        """The chat matrix's tiles: each running session's replies and the
        step it is on, plus what they have cost between them.

        Ordered by name, so a tile stays where it is while sessions change
        state around it.
        """
        rows = sorted((r for r in self.sessions()["sessions"] if matrix.active(r)),
                      key=lambda r: ((r.get("name") or "").lower(), r.get("key") or ""))
        now = time.time()
        account = self.usage.summary()
        try:
            spent = self.spend(now, budget=1.0, limits=account.get("limits") or {})
        except Exception:
            spent = {"global": {"windows": []}, "current": {"each": []}}
        per = {e["sessionId"]: e for e in spent["current"]["each"]}
        tiles = []
        for row in rows:
            sid = row["sessionId"]
            tile = {k: row.get(k) for k in
                    ("key", "sessionId", "name", "cwd", "status", "state", "attention",
                     "attention_kind", "attention_reason", "waitingFor",
                     "last_notification", "background_tasks", "remote", "host", "work")}
            mine = per.get(sid) or {}
            # The whole session's estimate; the statusline's figure only
            # covers Claude Code's current run, and is gone after a restart.
            tile["cost_usd"] = mine.get("cost_usd")
            tile["context_pct"] = (row.get("usage") or {}).get("context_pct")
            if row.get("remote"):
                got = self._remote_conversation(row, self.MATRIX_REMOTE_ITEMS)
                items, error = got.get("items") or [], got.get("error")
            else:
                try:
                    items, _ = self.convs.conversation(sid)
                    error = None
                except Exception as exc:
                    items, error = [], "%s: %s" % (type(exc).__name__, exc)
                tile["tokens"] = mine.get("tokens")
            tile.update(matrix.focus(items, live=matrix.working(row)
                                     or bool(row.get("attention"))))
            if error:
                tile["error"] = error
            tiles.append(tile)
        spend = matrix.spend(tiles, account.get("limits"))
        spend["plan"] = account.get("plan")
        spend["global"] = spent["global"]
        spend["models"] = spent["current"]["models"]
        spend["complete"] = spent.get("complete", True)
        return {"tiles": tiles, "now": now, "spend": spend}

    def transcribe(self, audio_bytes, hint=None):
        """Browser-recorded audio to text, in the dictation process.

        The mic belongs in the window, next to the prompt box it fills. The
        browser records (localhost is a secure context, so getUserMedia
        works); decoding and transcription happen in the child, where the
        model is resident - loading it costs seconds, using it hundredths.
        """
        if self._voice is None:
            return {"ok": False, "error": "dictation is not available"}
        try:
            text = self._voice.transcribe_bytes(audio_bytes, hint=hint)
        except Exception as exc:
            return {"ok": False, "error": "%s: %s" % (type(exc).__name__, exc)}
        return dict(self._voice.where or {}, ok=True, text=text)

    def warm_voice(self):
        """The mic was pressed: build the model while the user speaks."""
        if self._voice is None:
            return {"ok": False, "error": "dictation is not available"}
        self._voice.warm()
        # Where the last load landed, so the window can say "on CPU" while
        # the user is still speaking; None before the first load finishes.
        return dict(self._voice.where or {}, ok=True, running=self._voice.running)

    def decisions(self, limit=60):
        """Recent auto-approval verdicts, newest first.

        The permission hook writes these to a local file rather than posting
        them here: it sits on the critical path of every prompt, so it must
        not depend on this process being up. Reading its log is the only
        coupling between the two.
        """
        path = os.path.join(os.path.dirname(self.store.path), "decisions.jsonl")
        try:
            with open(path, encoding="utf-8") as fh:
                lines = fh.readlines()[-limit:]
        except OSError:
            return []
        out = []
        for line in reversed(lines):
            line = line.strip()
            if not line:
                continue
            try:
                out.append(json.loads(line))
            except ValueError:
                continue
        return out

    def catalogue(self):
        rows = self.sessions()["sessions"]
        local = [r.get("cwd") for r in rows if r.get("cwd") and not r.get("remote")]
        found = projects.discover(self.roots, extra=local)
        # A remote project is whatever a remote session has worked in. Reading
        # the host's directory tree would mean another SSH call per poll for
        # something this already answers.
        seen = set()
        for row in rows:
            if not row.get("remote") or not row.get("cwd"):
                continue
            key = (row["host"], row["cwd"])
            if key in seen:
                continue
            seen.add(key)
            # Host-qualified, because the same project name exists on more
            # than one machine here - bookoferrantpages is both local and on
            # developmenthost1 - and firing at the wrong machine is the same
            # class of mistake as firing at the wrong repository.
            leaf = os.path.basename(row["cwd"].rstrip("/")) or row["cwd"]
            found.append({
                "name": "%s/%s" % (row["host"], leaf),
                "path": row["cwd"],
                "source": "remote",
                "host": row["host"],
            })
        return found

    def invalidate(self):
        with self._lock:
            self._cache = {"at": 0.0, "snapshot": None}
        # Wakes the window's event streams, so a hook's news reaches the page
        # now rather than on its next poll.
        with self._changed:
            self._changed.notify_all()

    def wait_change(self, timeout):
        """Block until something invalidates the snapshot, or `timeout`."""
        with self._changed:
            self._changed.wait(timeout)

    def _costs(self):
        if self.cost_history is None:
            from . import costs
            self.cost_history = costs.History()
        return self.cost_history

    def costs(self, days=30.0):
        """Estimated spend by day, project and model, from transcripts."""
        try:
            return self._costs().summary(days=days)
        except Exception as exc:
            return {"ok": False, "error": "%s: %s" % (type(exc).__name__, exc)}

    # The windows a subscription's global figures cover, by length.
    SPAN = {"five_hour": 5 * 3600.0, "seven_day": 7 * 86400.0}

    def _global_windows(self, limits, now):
        """[(name, since, resets_at)] the global figures are counted over.

        A subscription's are its rate-limit windows: each began its length
        before it resets. Once it has reset, and until a session reports the
        new one, it runs from that reset. With no reading at all it is the
        last five hours or seven days. Either way `resets_at` is None. Any other account - Enterprise, Team, an
        API key, a cloud provider - has no windows to show, and gets the
        calendar month so far, the period such accounts are usually billed by.
        """
        if any(limits.get(n) for n in self.SPAN) or self.subscribed():
            out = []
            for name, span in self.SPAN.items():
                w = limits.get(name) or {}
                resets = w.get("resets_at")
                if resets:
                    out.append((name, resets - span, resets))
                elif w.get("waiting"):
                    # Reset, the new window not reported yet: it began then at the earliest.
                    out.append((name, w["reset_at"], None))
                else:
                    out.append((name, (now - span) // 60 * 60, None))
            return out
        start = datetime.fromtimestamp(now).replace(day=1, hour=0, minute=0, second=0,
                                                    microsecond=0)
        return [("month", start.timestamp(), None)]

    def spend(self, now=None, budget=3.0, limits=None):
        """Tokens and estimated cost in two parts, both from the transcripts,
        so a restart loses neither.

        `global` is everything sent from this machine within the account's
        windows (see `_global_windows`) - every session, open or not. Usage
        from other machines or claude.ai is not in a transcript here, so it
        can only show in the meters' percentages.

        `current` is the sessions open in Imperatorium (process up, not
        ended), each over its whole life, subagents included - not only
        since this dispatcher started. A remote session's transcript is not
        here; it counts with the cost its statusline reported, if any.

        Costs are estimates at list prices (see costs.py), the API price of
        the same tokens; on a subscription that is a measure, not a bill.
        """
        now = time.time() if now is None else now
        if limits is None:
            limits = self.usage.summary(now)["limits"]
        hist = self._costs()
        complete = True
        windows = []
        for name, since, resets in self._global_windows(limits, now):
            got = hist.totals(since, now=now, budget=budget)
            complete = complete and got.pop("complete")
            windows.append(dict(got, name=name, since=since, resets_at=resets))
        rows = [r for r in self.sessions()["sessions"] if matrix.active(r)]
        local = [r["sessionId"] for r in rows if not r.get("remote")]
        per, ok = hist.sessions(local, budget=budget)
        complete = complete and ok
        each, tokens, cost, models = [], {}, 0.0, {}
        for row in rows:
            got = per.get(row["sessionId"]) if not row.get("remote") else None
            usd = got["cost_usd"] if got else (row.get("usage") or {}).get("cost_usd")
            tk = got["tokens"] if got else None
            for k, v in (tk or {}).items():
                tokens[k] = tokens.get(k, 0) + v
            # Summed into a new dict: `got` is shared with the history's memo.
            for name, m in ((got or {}).get("models") or {}).items():
                into = models.setdefault(name, {"output": 0, "total_tokens": 0, "cost_usd": 0.0})
                for k in into:
                    into[k] += m[k]
            if isinstance(usd, (int, float)):
                cost += usd
            each.append({"key": row.get("key"), "sessionId": row["sessionId"],
                         "name": row.get("name"), "tokens": tk,
                         "total_tokens": got["total_tokens"] if got else 0,
                         "cost_usd": usd})
        each.sort(key=lambda e: -(e["cost_usd"] or 0))
        return {"global": {"windows": windows},
                "current": {"sessions": len(rows), "tokens": tokens,
                            "total_tokens": sum(tokens.values()),
                            "cost_usd": cost, "models": models, "each": each},
                "estimated": True, "complete": complete, "now": now}

    # -- settings ----------------------------------------------------------
    # config.json's path; serve() sets it. None (a test's Control) has no
    # Settings window.
    config_path = None

    def settings_view(self):
        """`GET /api/settings`: the Settings window's form (see settings.py)."""
        if not self.config_path:
            return {"ok": False, "error": "this dispatcher has no config file"}
        try:
            cfg = settingsmod.load(self.config_path)
        except (OSError, ValueError) as exc:
            return {"ok": False, "error": "config.json cannot be read: %s" % exc}
        out = settingsmod.describe(cfg)
        # The on/off switches are also kept beside the store (`/api/notify`,
        # impctl) and win over config.json at start, so the live state is
        # the true one.
        for f in out["fields"]:
            if f["key"] in ("notify.ntfy.enabled", "notify.speak.enabled"):
                f["value"] = self.notifier.enabled(f["key"].split(".")[1])
        return dict(out, ok=True, path=self.config_path,
                    waiting=sorted(self._restart_needed),
                    notify_error=self.notifier.last_error,
                    shortcuts=[{"where": w, "keys": k, "what": t}
                               for w, k, t in ui.SHORTCUTS])

    def save_settings(self, changes):
        """`POST /api/settings {changes: {key: value}}`: check every change,
        write config.json once, and apply at once what can be applied. The
        rest waits for a restart, which the answer names."""
        if not self.config_path:
            return {"ok": False, "error": "this dispatcher has no config file"}
        if not isinstance(changes, dict) or not changes:
            return {"ok": False, "error": "nothing to save"}
        with self._settings_lock:
            try:
                cfg = settingsmod.load(self.config_path)
                new = settingsmod.apply(cfg, changes)
            except settingsmod.Invalid as exc:
                return {"ok": False, "errors": exc.args[0] if isinstance(exc.args[0], dict) else {},
                        "error": "some values were not accepted"}
            except (OSError, ValueError) as exc:
                return {"ok": False, "error": "config.json cannot be read: %s" % exc}
            try:
                settingsmod.save(self.config_path, new)
            except OSError as exc:
                return {"ok": False, "error": "config.json cannot be written: %s" % exc}
            self._apply_settings(new, changes)
            need = settingsmod.restarts(changes)
            self._restart_needed.update(need)
        return dict(self.settings_view(), saved=sorted(changes), restart=need)

    def _apply_settings(self, config, changes):
        """What the running dispatcher can take without a restart."""
        if "chat.send_as_typed" in changes:
            self.send_as_typed = bool(changes["chat.send_as_typed"])
        for name in ("key", "phrases"):
            if "quick." + name in changes:
                self.quick[name] = settingsmod.value(
                    config, settingsmod.BY_KEY["quick." + name])
        fresh = notify.settings(config)
        for name, cfg in fresh.items():
            key = "notify.%s.enabled" % name
            cfg["enabled"] = (bool(changes[key]) if key in changes
                              else self.notifier.enabled(name))
        self.notifier.cfg = fresh
        flips = {k.split(".")[1]: bool(v) for k, v in changes.items()
                 if k in ("notify.ntfy.enabled", "notify.speak.enabled")}
        if flips:
            # Kept where `/api/notify` keeps its switches, which win at start.
            saved = dict(self.store.prefs().get("notify") or {})
            saved.update(flips)
            self.store.set_prefs(notify=saved)
        if "budget.thresholds" in changes:
            with self.usage._lock:
                self.usage.thresholds = tuple(sorted(
                    float(t) for t in (config.get("budget") or {}).get("thresholds") or ()))
        if any(k.startswith("background.") for k in changes):
            self.background_options = dict(config.get("background") or {})

    def usage_summary(self):
        """`GET /api/usage`: the meters, with the spend beside them."""
        out = self.usage.summary()
        try:
            out["spend"] = self.spend(budget=1.0, limits=out["limits"])
        except Exception as exc:
            out["spend"] = {"error": "%s: %s" % (type(exc).__name__, exc)}
        return out

    def warm_spend(self):
        """Read the transcripts once at start, so the first look at the
        spend has it all rather than what one request's budget allowed."""
        try:
            self.spend(budget=120.0)
        except Exception:
            pass

    # -- hooks -----------------------------------------------------------
    def on_hook(self, payload, host=None):
        """One hook event. `host` is set only by a host's tunnel listener.

        A remote session's records are kept under a host-qualified key, so a
        host can only ever describe its own sessions.
        """
        event = payload.get("hook_event_name") or "unknown"
        sid = payload.get("session_id")
        if sid and event == STATUSLINE:
            # Several a minute per session: memory only, never the store.
            self.usage.ingest(remote_key(host, sid) if host else sid, payload)
            return {"ok": True}
        if sid and event in activity.TOOL_EVENTS:
            self.live.ingest(remote_key(host, sid) if host else sid, payload)
            return {"ok": True}
        if event not in INTERESTING or not sid:
            return {"ok": True, "ignored": event}
        key = remote_key(host, sid) if host else sid

        meta = {
            "cwd": payload.get("cwd"),
            "last_event": event,
            "last_event_at": time.time(),
        }

        # Every hook process can see these - Claude Code exports them to hooks
        # and Bash children and records them nowhere else - so take them from
        # whichever event arrives, not just SessionStart. That event fires
        # once, so a session already running when the dispatcher starts would
        # otherwise stay unreachable until it was resumed; this way the next
        # Stop or Notification repairs it.
        socket = payload.get("messaging_socket")
        token = payload.get("messaging_token")
        if socket:
            meta["socket"] = socket

        if event == "SessionStart":
            meta["name"] = payload.get("session_name")
            self.store.register(key, token=token, **meta)
            self.store.undismiss(key)
            self.invalidate()
            self._event(host,
                {"kind": "session_start", "session_id": sid, "cwd": meta.get("cwd")}
            )
            return {"ok": True}

        if event == "SessionEnd":
            # Name and directory outlive the session in the event log, so the
            # window can offer to resume what just closed.
            gone = self.store.sessions().get(key) or {}
            self.store.forget(key)
            self.live.forget(key)
            self.usage.forget(key)
            self.invalidate()
            self._event(host, {"kind": "session_end", "session_id": sid,
                               "cwd": payload.get("cwd") or gone.get("cwd"),
                               "name": gone.get("name")})
            return {"ok": True}

        if event == "Notification":
            kind = payload.get("notification_type") or "notification"
            meta["last_notification"] = kind
            if kind in ATTENTION_NOTIFICATIONS:
                caption = registry.NOTIFICATION_CAPTIONS.get(kind, kind)
                self.store.raise_attention(key, caption, BY_NOTIFICATION, **meta)
                if token:
                    self.store.register(key, token=token)
                self._announce(key, caption)
            else:
                self.store.register(key, token=token, **meta)
            self.invalidate()
            self._event(host,
                {"kind": kind, "session_id": sid, "cwd": payload.get("cwd")}
            )
            return {"ok": True}

        summary = payload.get("last_assistant_message") or ""

        if event == "StopFailure":
            # The schema is `error`, `error_details`, `last_assistant_message`
            # - not the `error_type` this once read, which is why every
            # recorded failure carried an empty reason.
            error = payload.get("error")
            detail = payload.get("error_details") or summary
            caption = registry.failure_caption(error, detail)
            meta["last_failure"] = error or "unknown"
            meta["last_failure_detail"] = (detail or "")[:280]
            if caption:
                self.store.raise_attention(key, caption, BY_FAILURE, **meta)
                self._announce(key, caption)
            else:
                # Claude Code itself shrugs at this one, so neither do we.
                self.store.register(key, **meta)
            if token:
                self.store.register(key, token=token)
            self.invalidate()
            self._event(host, {
                "kind": "stopfailure", "session_id": sid,
                "cwd": payload.get("cwd"), "error": error,
                "summary": (detail or "")[:280],
            })
            return {"ok": True}

        self.store.register(key, token=token, **meta)
        if event in CLEARING:
            # `register` merges and drops None, so clearing needs its own write.
            self.store.clear_attention(key)
        self.invalidate()
        self._event(host,
            {
                "kind": event.lower(),
                "session_id": sid,
                "cwd": payload.get("cwd"),
                "summary": summary[:280],
            }
        )
        if event == "Stop":
            self.schedule(self.idle_after,
                          lambda: self._idle_check(key, meta["last_event_at"], host, sid))
            # The turn is over, so this is the moment a queued prompt was
            # waiting for. One per turn: a queue of three is three turns.
            delivered = self.drain(key)
            if delivered:
                return {"ok": True, "delivered": delivered}
        return {"ok": True}

    @staticmethod
    def schedule(delay, fn):
        """Run `fn` once after `delay` seconds. Tests replace it."""
        def run():
            try:
                fn()
            except Exception:
                pass  # a background check must not take a thread down noisily
        t = threading.Timer(delay, run)
        t.daemon = True
        t.start()

    def _idle_check(self, key, stop_at, host=None, sid=None):
        """The `idle_prompt` Claude Code no longer sends: a minute after
        `Stop`, a session that is idle, has had no hook since and has
        nothing running is waiting for you. True when it raised it."""
        rec = self.store.sessions().get(key) or {}
        if rec.get("last_event_at") != stop_at or rec.get("attention_since"):
            return False
        self.invalidate()
        row = self._row(key)
        if not row or row.get("status") != "idle":
            return False
        # Claude Code holds `idle_prompt` back while background work runs.
        work = row.get("work") or {}
        if (registry.still_running(row, "idle_prompt") or row.get("shell_processes")
                or work.get("agents") or work.get("shells") or work.get("monitors")):
            return False
        caption = registry.NOTIFICATION_CAPTIONS["idle_prompt"]
        self.store.raise_attention(key, caption, BY_NOTIFICATION,
                                   last_notification="idle_prompt")
        self._announce(key, caption)
        self.invalidate()
        self._event(host, {"kind": "idle_prompt", "session_id": sid or key,
                           "cwd": rec.get("cwd"), "summary": "raised by the dispatcher"})
        return True

    def _event(self, host, event):
        if host:
            event = dict(event, host=host)
        self.store.add_event(event)

    def park_wait(self, payload):
        """How long a parked permission request is held for an answer.

        While the hook waits, the terminal's own prompt is held back. An
        interactive session gets the short wait, because you may well be
        at its terminal; a background session has no terminal, so the
        window or the Stream Deck is the only way to answer it at all.
        """
        wait = float(payload.get("wait") or 0)
        wait_bg = payload.get("wait_background")
        if wait_bg is None:
            return wait
        row = next((r for r in self.sessions()["sessions"]
                    if r["sessionId"] == payload.get("session_id")), None)
        if row and row.get("kind") == "background":
            return float(wait_bg)
        return wait

    # -- notifications -----------------------------------------------------
    def _announce(self, key, caption):
        rec = self.store.sessions().get(key) or {}
        since = rec.get("attention_since")
        name = rec.get("name") or os.path.basename(
            str(rec.get("cwd") or "").rstrip("/\\")) or None
        # The project reads better aloud and on a lock screen than the
        # numbered session name (`gem-trip-4`).
        project = os.path.basename(str(rec.get("cwd") or "").replace("\\", "/")
                                   .rstrip("/")) or name
        self.notifier.attention(key, project, caption, since)

    def _still_waiting(self, key, since):
        """Is `key` still waiting on the raise that started at `since`?

        Goes through `sessions()` first, because that is where a permission
        prompt answered at the terminal is noticed - no hook says so.
        """
        self.invalidate()
        try:
            self.sessions()
        except Exception:
            pass
        rec = self.store.sessions().get(key) or {}
        return bool(since) and rec.get("attention_since") == since

    def _budget_crossed(self, crossing):
        text = self.notifier.budget(crossing)
        self.store.add_event({"kind": "budget", "summary": text,
                              "window": crossing["window"],
                              "used": crossing["used"]})

    # -- queued prompts ----------------------------------------------------
    def enqueue(self, session_id, prompt):
        """Hold `prompt` for a session until its turn ends.

        A session that is idle and reachable right now gets it immediately:
        queueing behind nothing would only add a wait.
        """
        if not prompt or not prompt.strip():
            return {"ok": False, "error": "no prompt"}
        row = self._row(session_id)
        if row is None:
            return {"ok": False, "error": "no such session"}
        if slash.is_command(prompt):
            # Refuse now what could never be typed, rather than leave it at
            # the head of the queue failing on every sweep.
            why = slash.refusal(prompt) or slash.unreachable(row)
            if why:
                return {"ok": False, "error": why}
        key = row.get("key") or session_id
        item = self.store.enqueue(key, prompt)
        self.invalidate()
        if (row.get("status") == "idle" and not row.get("attention")
                and row.get("dispatchable")):
            self.drain(key)
        return {"ok": True, "queued": item, "session": session_id,
                "waiting": len(self.store.queued(key))}

    def drain(self, key):
        """Deliver the oldest queued prompt for `key`. Its id, or None."""
        if not self.store.queued(key):
            return None
        self.invalidate()
        row = next((r for r in self.sessions()["sessions"] if r.get("key") == key), None)
        if row is None or not row.get("dispatchable"):
            return None  # The sweep retries once it registers.
        item = self.store.take_queued(key)
        if item is None:
            return None
        if slash.is_command(item["prompt"]):
            got = self.run_command(row, item["prompt"])
            error = None if got["ok"] else got["error"]
        else:
            error = self._deliver(row, item["prompt"])
        if error:
            self.store.requeue_front(key, item, error)
            return None
        self.invalidate()
        return item["id"]

    def sweep_queue(self):
        """Deliver to queued sessions that are idle and not waiting on you.

        `Stop` is the normal trigger; this catches a prompt queued for a
        session that had no credential yet, or a Stop that was missed.
        """
        out = []
        queued = self.store.queued()
        if not queued:
            return out
        self.invalidate()
        rows = {r.get("key"): r for r in self.sessions()["sessions"]}
        for key in queued:
            row = rows.get(key)
            if (row and row.get("status") == "idle" and not row.get("attention")
                    and row.get("dispatchable")):
                got = self.drain(key)
                if got:
                    out.append(got)
        return out

    def _sweep_loop(self, every=15.0):
        while True:
            time.sleep(every)
            try:
                self.sweep_queue()
            except Exception:
                pass

    def _row(self, session_id, max_age=2.0, match=None):
        """The session's row. A `max_age` past the usual 2s accepts an older
        snapshot - rebuilding one runs `claude agents`, ~0.25 s, which a
        click on a tile should not wait for - and looks again in a fresh one
        only when the session is not in it."""
        match = match or (lambda r: r["sessionId"] == session_id
                          or r.get("key") == session_id)
        row = next((r for r in self.sessions(max_age)["sessions"] if match(r)), None)
        if row is None and max_age > 2.0:
            row = next((r for r in self.sessions()["sessions"] if match(r)), None)
        return row

    # -- changes, worktrees, activity --------------------------------------
    def _local_cwd(self, session_id):
        row = self._row(session_id)
        if row is None:
            return None, "no such session"
        if row.get("remote"):
            return None, ("%s is on %s - changes are only read on this machine "
                          "so far" % (row.get("name") or session_id, row.get("host")))
        return row.get("cwd"), None

    def changes(self, session_id):
        cwd, error = self._local_cwd(session_id)
        if error:
            return {"ok": False, "error": error}
        return changes.changes(cwd)

    def _known_project(self, path):
        """Is `path` a project this dispatcher knows? Guards git actions.

        Anything the API is told to merge or delete must be a directory the
        catalogue or a live session already vouches for, never a path taken
        on trust from the request.
        """
        if not path:
            return False
        want = os.path.normcase(os.path.abspath(path))
        known = [p.get("path") for p in self.catalogue() if not p.get("host")]
        known += [r.get("cwd") for r in self.sessions()["sessions"]
                  if not r.get("remote")]
        return any(k and os.path.normcase(os.path.abspath(k)) == want for k in known)

    def worktrees(self, project):
        if not self._known_project(project):
            return {"ok": False, "error": "not a known project: %s" % (project,)}
        return changes.worktrees(project)

    def worktree_diff(self, project, path):
        if not self._known_project(project):
            return {"ok": False, "error": "not a known project: %s" % (project,)}
        return changes.worktree_diff(project, path)

    def finish_worktree(self, project, path, action, force=False):
        if not self._known_project(project):
            return {"ok": False, "error": "not a known project: %s" % (project,)}
        got = changes.finish(project, path, action, force=force)
        if got.get("ok"):
            self.store.add_event({"kind": "worktree_" + action, "cwd": project,
                                  "summary": "%s %s" % (action, got.get("branch"))})
        return got

    def activity(self, session_id, limit=200):
        """The timeline of one session, from hooks when they exist."""
        row = self._row(session_id, max_age=self.LOOKUP_AGE)
        key = (row or {}).get("key") or session_id
        events = self.live.events(key)
        if events:
            calls = events
            source = "hooks"
        elif row and row.get("remote"):
            return {"ok": True, "source": "none", "timeline": [],
                    "tally": activity.tally([]),
                    "note": "remote sessions report activity only through tool hooks"}
        else:
            try:
                items = self.convs.items((row or {}).get("sessionId") or session_id,
                                         limit=limit)
            except Exception as exc:
                return {"ok": False, "error": "%s: %s" % (type(exc).__name__, exc)}
            calls = activity.timeline(items)
            source = "conversation"
        return {"ok": True, "source": source, "timeline": calls[-limit:],
                "tally": activity.tally(calls),
                "current": self.live.current(key)}

    def awaited(self, session_id):
        """The panel under the chat: the end of what each shell and monitor
        the session is waiting on has printed, and what each subagent has
        said and done, read from the files Claude Code writes them to.
        Empty once the session's process has gone."""
        row = self._row(session_id, max_age=self.LOOKUP_AGE)
        if row is None:
            return {"ok": False, "error": "no such session"}
        now = time.time()
        if row.get("remote") or not (isinstance(row.get("pid"), int)
                                     and row.get("status")):
            return {"ok": True, "now": now, "tasks": []}
        try:
            stat = self.stats.read(row["sessionId"], now) or {}
            folder = taskout.tasks_dir(row["sessionId"])
            agents = taskout.subagents_dir(sessionstat.transcript_path(row["sessionId"])) \
                if stat.get("agents") else None
            tasks = taskout.awaited(stat, folder, agents=agents)
        except Exception as exc:
            return {"ok": False, "error": "%s: %s" % (type(exc).__name__, exc)}
        return {"ok": True, "now": now, "tasks": tasks}

    def session_status(self, session_id):
        """The window's status panel: what the transcript and process tree add
        to the row - background work, prompt cache, tokens.

        Background work only runs while the session's process does, so a
        session that has ended reports none of it, whatever its transcript
        last said.
        """
        row = self._row(session_id, max_age=self.LOOKUP_AGE)
        if row is None:
            return {"ok": False, "error": "no such session"}
        if row.get("remote"):
            return {"ok": True, "now": time.time(), "note":
                    "remote sessions report only what their tile shows"}
        now = time.time()
        try:
            stat = self.stats.read(row["sessionId"], now)
        except Exception as exc:
            return {"ok": False, "error": "%s: %s" % (type(exc).__name__, exc)}
        if stat:
            stat.pop("known_tasks", None)
        out = {"ok": True, "now": now, "stat": stat or {}}
        try:
            got, _ = self._costs().sessions([row["sessionId"]], budget=1.0)
            out["spend"] = got.get(row["sessionId"])
        except Exception:
            out["spend"] = None
        pid = row.get("pid")
        alive = isinstance(pid, int) and bool(row.get("status"))
        out["alive"] = alive
        if stat and not alive:
            for name in ("agents", "shells", "monitors"):
                stat[name] = []
        if alive:
            out["shell_processes"] = procs.waiting_shells([pid]).get(pid, 0)
        return out

    def errors(self, days=7.0, max_sessions=120):
        """Tool failures across recent sessions, by tool, category and project."""
        now = time.time()
        cache_key = (days, max_sessions)
        with self._lock:
            hit = self._errors
            if hit["key"] == cache_key and now - hit["at"] < self.ERRORS_EVERY:
                return hit["result"]
        try:
            rows = [r for r in self.history.list()
                    if now - r["modified"] <= days * 86400][:max_sessions]
        except Exception as exc:
            return {"ok": False, "error": str(exc)}
        per_project = {}
        tallies = []
        for r in rows:
            cached = self._tallies.get(r["session_id"])
            if cached and cached[0] == r["size"]:
                t = cached[1]
            else:
                try:
                    items = conversation.read(r["session_id"], limit=0)
                except Exception:
                    continue
                t = activity.tally(activity.timeline(items))
                self._tallies[r["session_id"]] = (r["size"], t)
            tallies.append(t)
            name = os.path.basename(str(r.get("cwd") or "?").replace("\\", "/")
                                    .rstrip("/")) or "?"
            p = per_project.setdefault(name, {"calls": 0, "errors": 0, "sessions": 0})
            p["calls"] += t["calls"]
            p["errors"] += t["errors"]
            p["sessions"] += 1
        result = dict(activity.merge_tallies(tallies), ok=True, days=days,
                      sessions=len(tallies), by_project=per_project, at=now)
        with self._lock:
            self._errors = {"at": now, "key": cache_key, "result": result}
        return result

    # -- history -------------------------------------------------------------
    def search(self, query, limit=40):
        try:
            got = self.history.search(query, limit=limit)
        except Exception as exc:
            return {"ok": False, "error": "%s: %s" % (type(exc).__name__, exc)}
        live = {r["sessionId"] for r in self.sessions()["sessions"]
                if r.get("pid") or r.get("status")}
        for r in got["results"]:
            r["live"] = r["session_id"] in live
        return dict(got, ok=True)

    def resume(self, session_id, where="window", fork=False, prompt=None, cwd=None):
        """Resume or fork a local session, in a new tab or in the background."""
        row = self._row(session_id)
        if row and row.get("remote"):
            return {"ok": False, "error": "resuming a session on %s is not "
                    "supported yet" % row.get("host")}
        cwd = cwd or (row or {}).get("cwd")
        if not cwd:
            found = next((r for r in self.history.list()
                          if r["session_id"] == session_id), None)
            cwd = (found or {}).get("cwd")
        if row and (row.get("pid") or row.get("status")) and not fork \
                and where == "window":
            return {"ok": False, "error": "that session is still running - focus "
                    "it, or fork it to branch off"}
        try:
            got = history.resume(session_id, cwd, where=where, fork=fork,
                                 prompt=prompt, options=self.background_options)
        except history.ResumeError as exc:
            return {"ok": False, "error": str(exc)}
        self.store.add_event({"kind": "resume" if not fork else "fork",
                              "session_id": session_id, "cwd": cwd,
                              "summary": got.get("command")})
        self.invalidate()
        return got

    def new_session(self, session_id=None, cwd=None, close=False):
        """A fresh session in the same project as `session_id`, in a new tab.

        With `close`, the old session is ended once the new one is launched -
        for moving to a clean context without hunting for the old tab.
        """
        row = self._row(session_id) if session_id else None
        if row and row.get("remote"):
            return {"ok": False, "error": "starting a session on %s is not "
                    "supported yet" % row.get("host")}
        if close and not (row and row.get("pid")):
            return {"ok": False, "error": NOT_RUNNING}
        cwd = cwd or (row or {}).get("cwd")
        try:
            got = history.new_session(cwd)
        except history.ResumeError as exc:
            return {"ok": False, "error": str(exc)}
        self.store.add_event({"kind": "new", "cwd": cwd, "summary": got["command"],
                              "session_id": got["session_id"]})
        if close:
            got["closed"] = self._close_row(row)
        self.invalidate()
        return got

    def close_session(self, session_id):
        """End a local session and, with it, its terminal tab."""
        row = self._row(session_id) if session_id else None
        if row and row.get("remote"):
            return {"ok": False, "error": "closing a session on %s is not "
                    "supported yet" % row.get("host")}
        if not (row and row.get("pid")):
            return {"ok": False, "error": NOT_RUNNING}
        got = self._close_row(row)
        self.invalidate()
        return got

    def dismiss_session(self, session_id):
        """Take a session with no process off the list. There is nothing to
        end, and Claude Code goes on listing a finished background job or a
        crashed session's file, so the dispatcher hides it until it starts
        again. The conversation stays resumable."""
        row = self._row(session_id) if session_id else None
        if row is None:
            return {"ok": False, "error": "no such session"}
        if row.get("remote"):
            return {"ok": False, "error": "dismissing a session on %s is not "
                    "supported yet" % row.get("host")}
        if row.get("pid"):
            return {"ok": False, "error": "that session is still running - close it instead"}
        self.store.dismiss(row["key"])
        self.store.forget(row["sessionId"])
        self.store.add_event({"kind": "dismiss", "cwd": row.get("cwd"),
                              "session_id": row["sessionId"], "name": row.get("name")})
        self.invalidate()
        return {"ok": True, "how": "dismissed"}

    def _close_row(self, row):
        got = closing.close(row["pid"])
        self.store.add_event({"kind": "close", "cwd": row.get("cwd"),
                              "session_id": row["sessionId"],
                              "summary": got.get("how") or got.get("error")})
        return got

    # -- dispatch --------------------------------------------------------
    def dispatch(self, text=None, project=None, prompt=None, mode="auto",
                 confirm=False, session_id=None):
        """Route an utterance. mode is auto | inject | launch | background | dry-run.

        `session_id` short-circuits project matching: the window has already
        picked a session, so guessing from a name would only add a way to be
        wrong.
        """
        if session_id and prompt and mode in ("auto", "inject"):
            row = next((r for r in self.sessions()["sessions"]
                        if r["sessionId"] == session_id), None)
            if row is None:
                return {"ok": False, "error": "no such session"}
            if slash.is_command(prompt):
                return self.run_command(row, prompt)
            if not row.get("dispatchable"):
                return {"ok": False, "error":
                        "that session is not reachable (no credential yet - it "
                        "registers on its next hook event)"}
            error = self._deliver(row, prompt)
            if error:
                return {"ok": False, "error": error}
            return {"ok": True, "action": "inject", "session": session_id,
                    "session_name": row.get("name"), "host": row.get("host")}

        if text and not prompt:
            spoken, prompt = projects.parse_utterance(text)
            project = project or spoken
        if not prompt:
            return {"ok": False, "error": "no prompt"}
        if not project:
            return {"ok": False, "error": "no project named"}

        res = projects.resolve(project, self.catalogue())
        out = {
            "heard_project": project,
            "prompt": prompt,
            "confidence": res["confidence"],
            "score": res["score"],
            "candidates": res["candidates"],
        }
        if res["confidence"] == "none":
            out.update(ok=False, error="no project matched %r" % (project,))
            return out
        # A merely-probable match is never acted on silently. Requiring the
        # caller to confirm is what makes a voice front-end safe: a misheard
        # name costs a keystroke, not a prompt fired at the wrong repository.
        if res["confidence"] != "certain" and not confirm:
            out.update(
                ok=False,
                needs_confirmation=True,
                error="ambiguous project %r" % (project,),
            )
            return out

        match = res["match"]
        out["project"] = match
        if mode == "dry-run":
            out.update(ok=True, action="dry-run")
            return out

        if slash.is_command(prompt):
            # Never a launch or a background run: a new session started on
            # "/compact" would be a stranger doing nothing useful.
            target = None if match.get("host") else self._session_for(match["path"])
            if target is None:
                out.update(ok=False, error="no running session on this machine "
                           "for that project to run the command in")
                return out
            out.update(self.run_command(target, prompt))
            return out

        if match.get("host"):
            host = self.host(match["host"])
            if host is None:
                out.update(ok=False, error="host %s is not configured"
                           % match["host"])
                return out
            # A live session there whose hooks have reported in is reached
            # the same way as a local one. Otherwise - and always for launch
            # or background, since there is no window here to open - the
            # work becomes a background session over there.
            if mode in ("auto", "inject"):
                target = self._session_for(match["path"], host=host["name"])
                error = self._deliver(target, prompt) if target else (
                    "no dispatchable session for that project")
                if not error:
                    out.update(ok=True, action="inject", host=host["name"],
                               session=target["sessionId"],
                               session_name=target.get("name"))
                    return out
                if mode == "inject":
                    out.update(ok=False, error=error)
                    return out
            try:
                started = remote.start_background(
                    host, match["path"], prompt, self.background_options
                )
            except remote.RemoteError as exc:
                out.update(ok=False, error=str(exc))
                return out
            self.store.add_event({
                "kind": "dispatch_remote",
                "cwd": match["path"],
                "summary": prompt[:280],
                "host": host["name"],
                "bg_id": started["id"],
            })
            out.update(ok=True, action="remote", host=host["name"],
                       bg_id=started["id"])
            return out

        if mode == "background":
            # Work you want done, rather than a window you want to sit in.
            # Claude Code isolates it in a worktree on its own.
            try:
                started = background.start(
                    prompt, match["path"], **(self.background_options or {})
                )
            except background.BackgroundError as exc:
                out.update(ok=False, error=str(exc))
                return out
            self.store.add_event(
                {
                    "kind": "dispatch_background",
                    "cwd": match["path"],
                    "summary": prompt[:280],
                    "bg_id": started["id"],
                }
            )
            self.invalidate()
            out.update(ok=True, action="background", bg_id=started["id"])
            return out

        if mode in ("auto", "inject"):
            target = self._session_for(match["path"])
            if target:
                error = self._deliver(target, prompt)
                if error:
                    if mode == "inject":
                        out.update(ok=False, error=error)
                        return out
                else:
                    out.update(
                        ok=True,
                        action="inject",
                        session=target["sessionId"],
                        session_name=target.get("name"),
                    )
                    return out
            elif mode == "inject":
                out.update(ok=False, error="no dispatchable session for that project")
                return out

        try:
            url = deeplink.launch(match["path"], prompt)
        except deeplink.DeepLinkError as exc:
            out.update(ok=False, error=str(exc))
            return out
        self.store.add_event(
            {"kind": "dispatch_launch", "cwd": match["path"], "summary": prompt[:280]}
        )
        out.update(ok=True, action="launch", url=url)
        return out

    def focus_row(self, row):
        """Raise the window this session is actually in, not just its terminal.

        One `WindowsTerminal.exe` hosts every session here - four sessions,
        one pid, three windows - so walking the process tree identifies the
        terminal and then has to guess which of its windows to raise. It
        guessed wrong, which is the bug this exists to fix.

        The session's identity on screen is its title, so that is what is
        matched. Three routes, best first, and the answer says which was
        used, because "focused the right window" and "focused the window it
        was in last time" are different promises.
        """
        sid, pid = row["sessionId"], row.get("pid")
        want = conversation.title(sid)
        host = winfocus.terminal_for(pid)
        windows = winfocus.titled_windows(host) if host else []

        hwnd = winfocus.match_title(want, windows) if want else None
        if hwnd and winfocus.focus_hwnd(hwnd):
            self.store.register(sid, window_hwnd=hwnd, window_title=want)
            return {"ok": True, "action": "focus", "session": sid, "pid": pid,
                    "how": "title", "title": want}

        # The tab is not the one its window is showing, so no title matches.
        # A tab stays in the window it was opened in, so the window this
        # session was last seen in is where it still is - it is just behind
        # another tab, which no API here can bring forward.
        remembered = (self.store.sessions().get(sid) or {}).get("window_hwnd")
        if remembered and winfocus.owner_pid(remembered) == host:
            if winfocus.focus_hwnd(remembered):
                return {"ok": True, "action": "focus", "session": sid,
                        "pid": pid, "how": "remembered", "title": want,
                        "note": "its tab is not in front - look for %r" % want}

        if winfocus.focus_pid(pid):
            return {"ok": True, "action": "focus", "session": sid, "pid": pid,
                    "how": "terminal", "title": want,
                    "note": "raised its terminal; could not tell its window apart"}
        return None

    def focus(self, project=None, session_id=None):
        """Put a project's session in front of the user.

        Falls back to opening one when nothing is running, so a button press
        always does something rather than silently failing.
        """
        rows = self.sessions()["sessions"]
        row = None
        path = None
        if session_id:
            row = next((r for r in rows if r["sessionId"] == session_id), None)
            if row and row.get("remote"):
                # Its pid is a process on another machine; treated as one here
                # it would raise whatever local window shares the number.
                return {"ok": False, "error": "%s is on %s - there is no window "
                        "here to raise" % (row.get("name") or session_id,
                                           row.get("host"))}
            path = row.get("cwd") if row else None
        if row is None:
            if not project:
                return {"ok": False, "error": "no project or session named"}
            res = projects.resolve(project, self.catalogue())
            if res["confidence"] == "none":
                return {"ok": False, "error": "no project matched %r" % (project,)}
            if res["match"].get("host"):
                # Nothing to raise, and a Linux path is no place to open a
                # window on this machine.
                return {"ok": False, "error": "%s is on %s - there is no window "
                        "here to raise" % (res["match"]["name"],
                                           res["match"]["host"])}
            path = res["match"]["path"]
            want = os.path.normcase(os.path.abspath(path))
            live = [
                r
                for r in rows
                if r.get("cwd")
                and os.path.normcase(os.path.abspath(r["cwd"])) == want
                and r.get("pid")
            ]
            # Whatever is asking for a human wins; otherwise the busy one is
            # the interesting one.
            live.sort(key=lambda r: (not r.get("attention"), r.get("status") != "busy"))
            row = live[0] if live else None

        if row and row.get("pid"):
            got = self.focus_row(row)
            if got:
                return got
            # Opening a window is the fallback for a session that is not
            # running. This one is, so launching would put a second session
            # on the same project - which is what a failed focus did once,
            # and is worse than saying it could not be raised.
            return {"ok": False, "error": "could not raise its window",
                    "session": row["sessionId"], "pid": row.get("pid")}
        if not path:
            return {"ok": False, "error": "nothing to focus and nowhere to open"}
        try:
            url = deeplink.launch(path, "")
        except deeplink.DeepLinkError as exc:
            return {"ok": False, "error": str(exc)}
        return {"ok": True, "action": "launch", "url": url}

    def run_command(self, row, text):
        """Type slash command `text` at `row`'s prompt (see slash.py)."""
        why = slash.refusal(text)
        if why:
            return {"ok": False, "error": why}
        # Status moves under us; decide on a fresh look, not the one the
        # caller found the row in.
        self.invalidate()
        row = self._row(row.get("key") or row["sessionId"]) or row
        why = slash.blocked(row, self.pending.waiting())
        if why:
            return {"ok": False, "error": why}
        if not slash.type_command(row["pid"], text):
            return {"ok": False, "error": "could not type into its terminal"}
        self.store.add_event({"kind": "command", "session_id": row["sessionId"],
                              "cwd": row.get("cwd"), "summary": text.strip()[:280]})
        self.invalidate()
        return {"ok": True, "action": "command", "command": text.strip(),
                "session": row["sessionId"], "session_name": row.get("name")}

    def send_typed(self, session_id, text):
        """Send As Typed: type `text` at the session's prompt as one line.

        Typed input is the person's own, with all the authority of a prompt
        typed at the terminal - which is the point (an inbox message is a
        peer's), and why this is off until they turn it on in Settings and
        reached only from the window (Handler checks its token). Text is
        cleaned to one line; a slash command goes the run_command way.
        """
        if not self.send_as_typed:
            return {"ok": False, "error": "Send As Typed is off - turn it on "
                    "in Settings, under Chat"}
        row = self._row(session_id) if session_id else None
        if row is None:
            return {"ok": False, "error": "no such session"}
        clean, changed = slash.clean_typed(text)
        why = slash.typed_refusal(clean)
        if why:
            return {"ok": False, "error": why}
        if slash.is_command(clean):
            return dict(self.run_command(row, clean), cleaned=changed)
        self.invalidate()
        row = self._row(row.get("key") or row["sessionId"]) or row
        why = slash.blocked(row, self.pending.waiting())
        if why:
            return {"ok": False, "error": why}
        if not slash.type_command(row["pid"], clean):
            return {"ok": False, "error": "could not type into its terminal"}
        self.store.add_event({"kind": "typed", "session_id": row["sessionId"],
                              "cwd": row.get("cwd"), "summary": clean[:280]})
        self.invalidate()
        return {"ok": True, "action": "typed", "cleaned": changed,
                "session": row["sessionId"], "session_name": row.get("name")}

    def _deliver(self, row, prompt):
        """Put `prompt` in `row`'s inbox, wherever it is. None, or the error."""
        prompt = conversation.with_origin(prompt)
        try:
            if row.get("remote"):
                host = self.host(row.get("host"))
                if host is None:
                    return "host %s is not configured" % row.get("host")
                remote.inject(host, row["socket"], row["token"], prompt)
            else:
                inbox.send(row["socket"], row["token"], prompt)
        except (inbox.InboxError, remote.RemoteError) as exc:
            return str(exc)
        self._event(row.get("host"), {"kind": "dispatch_inject",
                                      "session_id": row["sessionId"],
                                      "cwd": row.get("cwd"),
                                      "summary": conversation.strip_origin(prompt)[:280]})
        return None

    def _session_for(self, path, host=None):
        """The best dispatchable session in `path`, on `host` (None: here)."""
        if host:
            # A POSIX path on another machine: normcase/abspath here would
            # turn it into a Windows path on the current drive.
            want = path.rstrip("/")
            same = lambda cwd: cwd.rstrip("/") == want  # noqa: E731
        else:
            want = os.path.normcase(os.path.abspath(path))
            same = lambda cwd: os.path.normcase(os.path.abspath(cwd)) == want  # noqa: E731
        best = None
        for row in self.sessions()["sessions"]:
            cwd = row.get("cwd")
            if not cwd or not row.get("dispatchable"):
                continue
            if row.get("host") != host or bool(row.get("remote")) != bool(host):
                continue
            if not same(cwd):
                continue
            # Prefer an idle session: injecting into a busy one queues the
            # message behind whatever that session is already doing.
            if best is None or (
                row.get("status") == "idle" and best.get("status") != "idle"
            ):
                best = row
        return best


class Handler(BaseHTTPRequestHandler):
    server_version = "ccontrol"
    protocol_version = "HTTP/1.1"
    control = None

    def log_message(self, fmt, *args):
        pass

    def _send(self, code, body, ctype="application/json; charset=utf-8",
              cache="no-store"):
        if isinstance(body, (dict, list)):
            body = json.dumps(body).encode("utf-8")
        elif isinstance(body, str):
            body = body.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", cache)
        self.end_headers()
        self.wfile.write(body)

    def _from_window(self):
        """This server's own page, carrying the token it was served with.

        Anything running as you can still forge both, so this is not a
        boundary against a determined process; it keeps a plain API call -
        impctl, curl, another session - from typing as you by accident.
        """
        origin = self.headers.get("Origin")
        token = self.headers.get("X-Imperatorium-Window") or ""
        return (origin is not None and self._same_origin()
                and hmac.compare_digest(token, self.control.window_token))

    def _same_origin(self):
        """No Origin (curl, hooks, the palette), or this server's own page.

        Browsers put Origin on every POST, same-origin ones included, so
        refusing any Origin at all refused the window's own Send - every
        dispatch that ever worked came from outside a browser. A foreign page
        cannot forge the header, so matching our own origin keeps the guard.
        """
        origin = self.headers.get("Origin")
        if origin is None:
            return True
        port = self.server.server_address[1]
        return origin in ("http://127.0.0.1:%d" % port,
                          "http://localhost:%d" % port)

    def _read_json(self):
        if not self._same_origin():
            self._send(403, {"error": "cross-origin requests are refused"})
            return None
        ctype = (self.headers.get("Content-Type") or "").split(";")[0].strip()
        if ctype != "application/json":
            self._send(415, {"error": "expected application/json"})
            return None
        try:
            length = int(self.headers.get("Content-Length") or 0)
        except ValueError:
            self._send(400, {"error": "bad Content-Length"})
            return None
        if length <= 0 or length > MAX_BODY:
            self._send(413, {"error": "body must be 1..%d bytes" % MAX_BODY})
            return None
        try:
            return json.loads(self.rfile.read(length).decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            self._send(400, {"error": "body is not JSON"})
            return None

    def _read_binary(self):
        """Raw body, for audio. Same cross-origin refusal as the JSON path."""
        if not self._same_origin():
            self._send(403, {"error": "cross-origin requests are refused"})
            return None
        try:
            length = int(self.headers.get("Content-Length") or 0)
        except ValueError:
            self._send(400, {"error": "bad Content-Length"})
            return None
        if length <= 0 or length > MAX_BODY * 8:
            self._send(413, {"error": "body out of range"})
            return None
        return self.rfile.read(length)

    # A stream is ended after this long and the browser's EventSource opens
    # a new one, so a page that vanished without closing its socket costs a
    # thread for minutes, not forever.
    STREAM_SECONDS = 600.0
    STREAM_EVERY = 1.0      # how often the session list is looked at
    STREAM_MIN_GAP = 0.5    # and never more often than this
    STREAM_KEEPALIVE = 15.0

    def _stream(self):
        """Server-sent events: the session list, sent whenever it changes.

        A hook event wakes the loop at once (Control.invalidate), so a
        session asking for you shows without waiting for a poll; otherwise
        the list is looked at every second and sent only when it differs.
        """
        c = self.control
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.close_connection = True
        last, quiet, end = None, time.time(), time.time() + self.STREAM_SECONDS
        looked = 0.0
        try:
            while time.time() < end:
                # Tool hooks can arrive many times a second; each clears the
                # snapshot, so looks are spaced out or every one would fork
                # `claude agents`.
                gap = self.STREAM_MIN_GAP - (time.time() - looked)
                if gap > 0:
                    time.sleep(gap)
                looked = time.time()
                snap = c.sessions(max_age=self.STREAM_EVERY)
                # `at` is when the snapshot was taken, new every time; the
                # rest is what decides whether anything changed.
                same = json.dumps({k: v for k, v in snap.items() if k != "at"},
                                  sort_keys=True)
                if same != last:
                    self.wfile.write(b"event: sessions\ndata: "
                                     + json.dumps(snap).encode("utf-8") + b"\n\n")
                    self.wfile.flush()
                    last, quiet = same, time.time()
                elif time.time() - quiet >= self.STREAM_KEEPALIVE:
                    self.wfile.write(b": keepalive\n\n")
                    self.wfile.flush()
                    quiet = time.time()
                c.wait_change(self.STREAM_EVERY)
        except (OSError, ValueError):
            pass  # the page went away

    def _arg(self, name, default=""):
        from urllib.parse import parse_qs, urlparse
        return (parse_qs(urlparse(self.path).query).get(name) or [default])[0]

    def do_GET(self):
        path = self.path.split("?")[0].rstrip("/") or "/"
        c = self.control
        if path == "/":
            self._send(200, ui.render_shell(c.window_token, c.boot_id), "text/html; charset=utf-8")
        elif path == "/matrix":
            self._send(200, ui.render_matrix(c.boot_id), "text/html; charset=utf-8")
        elif path == "/settings":
            self._send(200, ui.render_settings(c.window_token, c.boot_id), "text/html; charset=utf-8")
        elif path == "/api/matrix":
            self._send(200, c.matrix())
        elif path.startswith("/fonts/"):
            data = ui.font(path[len("/fonts/"):])
            if data is None:
                self._send(404, {"error": "no such font"})
            else:
                # Fonts only change with the code, so a day's cache is safe.
                self._send(200, data, "font/woff2", cache="max-age=86400")
        elif path == "/api/sessions":
            self._send(200, c.sessions())
        elif path == "/api/stream":
            self._stream()
        elif path == "/api/costs":
            try:
                days = max(1, min(90, int(self._arg("days") or 30)))
            except ValueError:
                days = 30
            self._send(200, c.costs(days))
        elif path == "/api/projects":
            self._send(200, {"projects": c.catalogue()})
        elif path == "/api/events":
            self._send(200, {"events": c.store.events(100)})
        elif path == "/api/decisions":
            self._send(200, {"decisions": c.decisions()})
        elif path == "/api/conversation":
            from urllib.parse import parse_qs, urlparse
            args = parse_qs(urlparse(self.path).query)
            session = (args.get("session") or [""])[0]
            if not session:
                self._send(400, {"error": "session is required"})
                return
            # 0, the default, is the whole conversation, back through every
            # compaction.
            try:
                limit = max(0, int((args.get("limit") or ["0"])[0]))
            except ValueError:
                limit = 0
            body = c.conversation(session, limit)
            # The window polls every 1.5s; a long session is megabytes of
            # items, so an unchanged one is answered with its signature only.
            # A local one's comes from the cache; a remote one is hashed here.
            sig = body.get("sig") or convcache.signature(body.get("items") or [])
            if (args.get("known") or [""])[0] == sig:
                body = {"session": body.get("session"), "unchanged": True}
            body["sig"] = sig
            self._send(200, body)
        elif path == "/api/changes":
            self._send(200, c.changes(self._arg("session")))
        elif path == "/api/worktrees":
            self._send(200, c.worktrees(self._arg("project")))
        elif path == "/api/worktree_diff":
            self._send(200, c.worktree_diff(self._arg("project"), self._arg("path")))
        elif path == "/api/awaited":
            self._send(200, c.awaited(self._arg("session")))
        elif path == "/api/session_status":
            self._send(200, c.session_status(self._arg("session")))
        elif path == "/api/activity":
            self._send(200, c.activity(self._arg("session")))
        elif path == "/api/errors":
            try:
                days = max(0.1, min(90.0, float(self._arg("days") or 7)))
            except ValueError:
                days = 7.0
            self._send(200, c.errors(days))
        elif path == "/api/usage":
            self._send(200, c.usage_summary())
        elif path == "/api/history":
            self._send(200, c.search(self._arg("q")))
        elif path == "/api/queue":
            self._send(200, {"queue": c.store.queued()})
        elif path == "/api/prefs":
            self._send(200, {"prefs": c.store.prefs()})
        elif path == "/api/notify":
            self._send(200, c.notifier.status())
        elif path == "/api/settings":
            self._send(200, c.settings_view())
        elif path == "/api/quick":
            self._send(200, dict(c.quick, ok=True))
        elif path == "/api/pending":
            self._send(200, {"pending": c.pending.waiting()})
        elif path == "/api/hosts":
            self._send(200, c.hosts_status())
        elif path == "/api/health":
            self._send(200, {"ok": True, "pid": os.getpid(), "boot": self.control.boot_id})
        else:
            self._send(404, {"error": "no such path"})

    def do_POST(self):
        path = self.path.split("?")[0].rstrip("/") or "/"
        if path == "/api/transcribe":
            audio = self._read_binary()
            if audio is None:
                return
            self._send(200, self.control.transcribe(audio))
            return
        payload = self._read_json()
        if payload is None:
            return
        c = self.control
        if path == "/hook":
            self._send(200, c.on_hook(payload if isinstance(payload, dict) else {}))
        elif path == "/api/voice/warm":
            self._send(200, c.warm_voice())
        elif path == "/api/focus":
            if not isinstance(payload, dict):
                self._send(400, {"error": "expected an object"})
                return
            self._send(200, c.focus(payload.get("project"),
                                    payload.get("session_id")))
        elif path in ("/api/queue", "/api/unqueue", "/api/worktree",
                      "/api/resume", "/api/new", "/api/close", "/api/dismiss",
                      "/api/prefs", "/api/notify", "/api/settings"):
            if not isinstance(payload, dict):
                self._send(400, {"error": "expected an object"})
                return
            self._send(200, self._post_api(path, payload))
        elif path == "/permission/park":
            # The hook blocks here. Held open until a human answers or the
            # wait expires; an expiry is a silent "ask", which is what the
            # terminal would have done anyway.
            verdict, who = c.pending.park(payload, wait=c.park_wait(payload))
            self._send(200, {"verdict": verdict, "decided_by": who})
        elif path == "/api/decide":
            ok = c.pending.decide(payload.get("id"), payload.get("verdict"),
                                  payload.get("who") or "window")
            self._send(200 if ok else 409, {"ok": ok})
        elif path == "/api/typed":
            if not isinstance(payload, dict):
                self._send(400, {"error": "expected an object"})
                return
            if not self._from_window():
                self._send(403, {"ok": False, "error": "Send As Typed is only "
                                 "available from the Imperatorium window"})
                return
            self._send(200, c.send_typed(payload.get("session_id"),
                                         payload.get("prompt")))
        elif path == "/api/dispatch":
            if not isinstance(payload, dict):
                self._send(400, {"error": "expected an object"})
                return
            self._send(
                200,
                c.dispatch(
                    text=payload.get("text"),
                    project=payload.get("project"),
                    prompt=payload.get("prompt"),
                    mode=payload.get("mode", "auto"),
                    confirm=bool(payload.get("confirm")),
                    session_id=payload.get("session_id"),
                ),
            )
        else:
            self._send(404, {"error": "no such path"})


    def _post_api(self, path, p):
        c = self.control
        if path == "/api/queue":
            return c.enqueue(p.get("session_id"), p.get("prompt"))
        if path == "/api/unqueue":
            ok = c.store.unqueue(p.get("session"), p.get("id"))
            c.invalidate()
            return {"ok": ok}
        if path == "/api/worktree":
            return c.finish_worktree(p.get("project"), p.get("path"),
                                     p.get("action"), force=bool(p.get("force")))
        if path == "/api/resume":
            return c.resume(p.get("session_id"), where=p.get("where") or "window",
                            fork=bool(p.get("fork")), prompt=p.get("prompt"),
                            cwd=p.get("cwd"))
        if path == "/api/new":
            return c.new_session(p.get("session_id"), cwd=p.get("cwd"),
                                 close=bool(p.get("close")))
        if path == "/api/close":
            return c.close_session(p.get("session_id"))
        if path == "/api/dismiss":
            return c.dismiss_session(p.get("session_id"))
        if path == "/api/prefs":
            allowed = {k: p[k] for k in ("pinned", "archived", "group", "filter")
                       if k in p}
            return {"ok": True, "prefs": c.store.set_prefs(**allowed)}
        if path == "/api/notify":
            try:
                c.notifier.set_enabled(p.get("channel"), p.get("enabled"))
            except ValueError as exc:
                return {"ok": False, "error": str(exc)}
            saved = dict(c.store.prefs().get("notify") or {})
            saved[p.get("channel")] = bool(p.get("enabled"))
            c.store.set_prefs(notify=saved)
            return dict(c.notifier.status(), ok=True)
        if path == "/api/settings":
            ch = p.get("changes")
            # Turning Send As Typed on lets a message speak as you, so only
            # the Settings window may do it. Turning it off is anyone's.
            if (isinstance(ch, dict) and ch.get("chat.send_as_typed")
                    and not self._from_window()):
                return {"ok": False, "errors": {"chat.send_as_typed":
                        "can only be turned on from the Settings window"},
                        "error": "some values were not accepted"}
            return c.save_settings(ch)
        return {"ok": False, "error": "no such path"}


class HookOnlyHandler(Handler):
    """What a remote host's tunnel lands on: `/hook`, and nothing else.

    Everything else the dispatcher serves - dispatch, focus, decisions - would
    let any process on that host drive the sessions on this machine. The host
    is known from which listener the connection arrived at, never from the
    payload, so one host cannot report as another.
    """

    def do_GET(self):
        self._send(404, {"error": "no such path"})

    def do_POST(self):
        path = self.path.split("?")[0].rstrip("/") or "/"
        if path != "/hook":
            self._send(404, {"error": "no such path"})
            return
        payload = self._read_json()
        if payload is None:
            return
        self._send(200, self.control.on_hook(
            payload if isinstance(payload, dict) else {},
            host=self.server.ccontrol_host))


class Server(ThreadingHTTPServer):
    daemon_threads = True
    # On Windows SO_REUSEADDR lets a second socket bind a port a live socket
    # already owns, silently splitting traffic between two dispatchers.
    allow_reuse_address = not WINDOWS

    def server_bind(self):
        if WINDOWS:
            import socket as _socket

            self.socket.setsockopt(
                _socket.SOL_SOCKET, _socket.SO_EXCLUSIVEADDRUSE, 1
            )
        ThreadingHTTPServer.server_bind(self)


def load_config():
    path = os.path.join(PROJECT_DIR, "config.json")
    try:
        with open(path, encoding="utf-8") as fh:
            loaded = json.load(fh)
        return loaded if isinstance(loaded, dict) else {}
    except (OSError, ValueError):
        return {}


def start_tunnels(control):
    """A hook-only listener and a reverse tunnel for each host that asks.

    Started after the main port is bound, so a second dispatcher that fails
    to bind never gets as far as fighting the first for the hosts' ports.
    """
    for h in control.fleet.hosts:
        cfg = h.get("tunnel")
        if not cfg:
            continue
        HookOnlyHandler.control = control
        try:
            listener = Server((HOST, cfg["local_port"]), HookOnlyHandler)
        except OSError as exc:
            print("tunnel %s: cannot listen on %d: %s"
                  % (h["name"], cfg["local_port"], exc))
            continue
        listener.ccontrol_host = h["name"]
        threading.Thread(target=listener.serve_forever,
                         name="hooks-%s" % h["name"], daemon=True).start()
        control.tunnels[h["name"]] = remote.Tunnel(
            h, cfg["local_port"], cfg["remote_port"]).start()
        print("tunnel %s: host 127.0.0.1:%d -> hooks on 127.0.0.1:%d"
              % (h["name"], cfg["remote_port"], cfg["local_port"]))


def serve(host=HOST, port=PORT, state_path=STATE_PATH, roots=None):
    config = load_config()
    roots = roots or [r for r in (config.get("roots") or []) if isinstance(r, str)] or None
    Handler.control = Control(
        Store(state_path), roots,
        background_options=config.get("background"),
        hosts=remote.load_hosts(config),
        config=config,
    )
    Handler.control.config_path = os.path.join(PROJECT_DIR, "config.json")
    srv = Server((host, port), Handler)
    print("ccontrol listening on http://%s:%d" % (host, port))
    start_tunnels(Handler.control)
    # Started only once the port is ours, so a second dispatcher that fails
    # to bind never runs a second set of gh calls or queue deliveries.
    Handler.control.prs.start()
    threading.Thread(target=Handler.control._sweep_loop, name="queue",
                     daemon=True).start()
    threading.Thread(target=Handler.control.warm_spend, name="spend",
                     daemon=True).start()
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        srv.server_close()


if __name__ == "__main__":
    import argparse

    ap = argparse.ArgumentParser(description="Imperatorium dispatcher")
    ap.add_argument("--host", default=HOST)
    ap.add_argument("--port", type=int, default=PORT)
    ap.add_argument("--state", default=STATE_PATH)
    ap.add_argument("--root", action="append", dest="roots")
    args = ap.parse_args()
    serve(args.host, args.port, args.state, args.roots)
