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
import json
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from . import (activity, background, changes, conversation, deeplink, history,
               inbox, notify, pending, projects, prs, registry, remote, ui,
               usage, voice, winfocus)
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

# How a session came to want a human. A permission prompt is answered by a
# person, so the session resuming work proves it was answered; a failed turn
# is not resolved by the session retrying, only by it finishing or by you.
BY_NOTIFICATION, BY_FAILURE = "notification", "failure"


def remote_key(host, session_id):
    """Where a remote session's hook records live in the store."""
    return "%s/%s" % (host, session_id)


class Control:
    """Everything the request handler needs, so the handler stays dumb."""

    # SSH is slow and a sleeping host is ordinary, so remote state is
    # refreshed on its own schedule and never on the request path.
    REMOTE_EVERY = 20.0

    # How often the one-line "what is it doing" is re-read from a session's
    # conversation when no tool hook has said so more cheaply.
    DOING_EVERY = 10.0
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
        config = config or {}

        budget = config.get("budget") or {}
        self.notifier = notify.Notifier(config, still_waiting=self._still_waiting)
        prefs = store.prefs().get("notify") or {}
        for channel, on in prefs.items():
            if channel in self.notifier.cfg:
                self.notifier.set_enabled(channel, on)
        self.usage = usage.Usage(
            thresholds=budget.get("thresholds") or usage.DEFAULT_THRESHOLDS,
            on_crossing=self._budget_crossed)
        self.live = activity.Live()
        self.prs = prs.Tracker()
        self.history = history.Index()
        self._doing = {}  # session id -> (at, line)
        self._errors = {"at": 0.0, "key": None, "result": None}
        self._tallies = {}  # session id -> (size, tally)

        self.pending = pending.Pending()
        self._voice = None
        settings = voice.load_settings()
        if settings.get("enabled") and voice.available():
            self._voice = voice.Transcriber(settings)
            if settings.get("preload"):
                threading.Thread(target=self._warm_voice, name="whisper-warm",
                                 daemon=True).start()
        self.fleet = remote.Fleet(hosts or ())
        self.tunnels = {}  # host name -> remote.Tunnel, started by serve()
        self.readers = {}  # host name -> remote.Reader, opened on first read
        self._remote = {"at": 0.0, "rows": []}
        if hosts:
            threading.Thread(target=self._refresh_remote, name="remote",
                             daemon=True).start()

    def _warm_voice(self):
        try:
            self._voice.load()
        except Exception:
            self._voice = None

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
        with self._lock:
            self._cache = {"at": time.time(), "snapshot": snap}
        return snap

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
                            ("context_pct", "context_size", "cost_usd", "model", "at")}
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

    def _doing_line(self, sid):
        now = time.time()
        with self._lock:
            hit = self._doing.get(sid)
        if hit and now - hit[0] < self.DOING_EVERY:
            return hit[1]
        try:
            line = activity.doing(conversation.read(sid, limit=8))
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

    def conversation(self, session_id, limit=60):
        """What one session said. Empty rather than fatal when unreadable."""
        row = next((r for r in self.sessions()["sessions"]
                    if r["sessionId"] == session_id
                    or r.get("id") == session_id), None)
        if row and row.get("remote"):
            return self._remote_conversation(row, limit)
        try:
            items = conversation.read(row["sessionId"] if row else session_id,
                                      limit=limit)
        except Exception as exc:
            return {"session": session_id, "items": [],
                    "error": "%s: %s" % (type(exc).__name__, exc)}
        return {"session": session_id, "items": items,
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

    def transcribe(self, audio_bytes, hint=None):
        """Browser-recorded audio to text, on the model this process holds.

        The mic belongs in the window, next to the prompt box it fills. The
        browser records (localhost is a secure context, so getUserMedia
        works) and the decode and transcription happen here, where the model
        is already resident - loading it costs seconds, using it costs
        hundredths.
        """
        if self._voice is None:
            return {"ok": False, "error": "dictation is not available"}
        try:
            import io

            from faster_whisper.audio import decode_audio

            audio = decode_audio(io.BytesIO(audio_bytes),
                                 sampling_rate=voice.SAMPLE_RATE)
            text = self._voice.transcribe(audio, hint=hint)
        except Exception as exc:
            return {"ok": False, "error": "%s: %s" % (type(exc).__name__, exc)}
        return {"ok": True, "text": text}

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
            # The turn is over, so this is the moment a queued prompt was
            # waiting for. One per turn: a queue of three is three turns.
            delivered = self.drain(key)
            if delivered:
                return {"ok": True, "delivered": delivered}
        return {"ok": True}

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

    def _row(self, session_id):
        return next((r for r in self.sessions()["sessions"]
                     if r["sessionId"] == session_id or r.get("key") == session_id),
                    None)

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
        row = self._row(session_id)
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
                items = conversation.read((row or {}).get("sessionId") or session_id,
                                          limit=limit)
            except Exception as exc:
                return {"ok": False, "error": "%s: %s" % (type(exc).__name__, exc)}
            calls = activity.timeline(items)
            source = "conversation"
        return {"ok": True, "source": source, "timeline": calls[-limit:],
                "tally": activity.tally(calls),
                "current": self.live.current(key)}

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

    def _deliver(self, row, prompt):
        """Put `prompt` in `row`'s inbox, wherever it is. None, or the error."""
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
                                      "summary": prompt[:280]})
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

    def _send(self, code, body, ctype="application/json; charset=utf-8"):
        if isinstance(body, (dict, list)):
            body = json.dumps(body).encode("utf-8")
        elif isinstance(body, str):
            body = body.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

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

    def _arg(self, name, default=""):
        from urllib.parse import parse_qs, urlparse
        return (parse_qs(urlparse(self.path).query).get(name) or [default])[0]

    def do_GET(self):
        path = self.path.split("?")[0].rstrip("/") or "/"
        c = self.control
        if path == "/":
            self._send(200, ui.render_shell(), "text/html; charset=utf-8")
        elif path == "/api/sessions":
            self._send(200, c.sessions())
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
            try:
                limit = max(1, min(400, int((args.get("limit") or ["60"])[0])))
            except ValueError:
                limit = 60
            self._send(200, c.conversation(session, limit))
        elif path == "/api/changes":
            self._send(200, c.changes(self._arg("session")))
        elif path == "/api/worktrees":
            self._send(200, c.worktrees(self._arg("project")))
        elif path == "/api/worktree_diff":
            self._send(200, c.worktree_diff(self._arg("project"), self._arg("path")))
        elif path == "/api/activity":
            self._send(200, c.activity(self._arg("session")))
        elif path == "/api/errors":
            try:
                days = max(0.1, min(90.0, float(self._arg("days") or 7)))
            except ValueError:
                days = 7.0
            self._send(200, c.errors(days))
        elif path == "/api/usage":
            self._send(200, c.usage.summary())
        elif path == "/api/history":
            self._send(200, c.search(self._arg("q")))
        elif path == "/api/queue":
            self._send(200, {"queue": c.store.queued()})
        elif path == "/api/prefs":
            self._send(200, {"prefs": c.store.prefs()})
        elif path == "/api/notify":
            self._send(200, c.notifier.status())
        elif path == "/api/pending":
            self._send(200, {"pending": c.pending.waiting()})
        elif path == "/api/hosts":
            self._send(200, c.hosts_status())
        elif path == "/api/health":
            self._send(200, {"ok": True, "pid": os.getpid()})
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
        elif path == "/api/focus":
            if not isinstance(payload, dict):
                self._send(400, {"error": "expected an object"})
                return
            self._send(200, c.focus(payload.get("project"),
                                    payload.get("session_id")))
        elif path in ("/api/queue", "/api/unqueue", "/api/worktree",
                      "/api/resume", "/api/prefs", "/api/notify"):
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
    srv = Server((host, port), Handler)
    print("ccontrol listening on http://%s:%d" % (host, port))
    start_tunnels(Handler.control)
    # Started only once the port is ours, so a second dispatcher that fails
    # to bind never runs a second set of gh calls or queue deliveries.
    Handler.control.prs.start()
    threading.Thread(target=Handler.control._sweep_loop, name="queue",
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
