"""Who is running, where, and are they waiting on me.

Three sources, merged, in descending order of trust:

1. `claude agents --json --all` - the documented scripting interface, and the
   only authoritative answer to "is this session alive and what is it doing".
2. ~/.claude/sessions/<pid>.json - undocumented, but the only place the
   inbox path (`messagingSocketPath`) is written down for a session this
   process did not start. Treated as best-effort: absence is normal.
3. The dispatcher's own hook registrations - the only source of a session's
   messaging token, which Claude Code exports to hooks and nowhere else.

The docs say not to parse the transcript JSONL and not to depend on the
daemon roster, so neither is read here.
"""
import json
import os
import re
import subprocess
import sys
import time

WINDOWS = sys.platform == "win32"

# `status` applies to a live process, `state` to a background job. A session
# is "blocked" when it wants a human; that is the whole point of this module.
ATTENTION_STATUSES = {"waiting"}
ATTENTION_STATES = {"blocked", "failed"}

# The statuses that mean the session is executing. `shell` is one of them -
# it is a session running a command - and the full set is ["busy", "shell",
# "idle", "waiting"], read out of the binary rather than guessed, because a
# status nobody handles falls through to a grey dot and its raw enum name.
ACTIVE_STATUSES = {"busy", "shell"}
KNOWN_STATUSES = {"busy", "shell", "idle", "waiting"}

# What Claude Code itself calls each way a turn can fail, and the words it
# would use. Lifted from the binary's own error->caption table so a tile says
# what the session would say, rather than a second vocabulary invented here.
# `max_output_tokens` is absent on purpose: Claude Code maps it to no blocked
# state at all, so it must not raise a tile either.
FAILURE_CAPTIONS = {
    "overloaded": "API overloaded - wait and retry",
    "server_error": "API unavailable - retry",
    "rate_limit": "rate limited - wait and retry",
    "billing_error": "usage limit reached - check plan",
    "authentication_failed": "login required - run /login",
    "oauth_org_not_allowed": "org disabled OAuth - use API key or ask admin",
    "account_on_hold": "account on hold - see detail",
    "verification_required": "verification required - see detail",
    "cloud_credential_error": "cloud credentials unavailable - check or refresh",
    "invalid_request": "invalid API request - see detail",
    "model_not_found": "model unavailable - see detail",
    "unknown": "API error",
}
QUIET_FAILURES = {"max_output_tokens"}

# The same idea for the other way a session asks for you. These arrive as raw
# enum values and were being printed as-is, so a tile said "permission_prompt".
NOTIFICATION_CAPTIONS = {
    "permission_prompt": "needs permission",
    "idle_prompt": "waiting for you",
    "agent_needs_input": "needs input",
    "elicitation_dialog": "needs an answer",
    "elicitation_url_dialog": "needs a URL",
}
OVERSIZE = re.compile(r"\b(too long|too large|exceeds|token limit)\b", re.I)


def failure_caption(error, detail=""):
    """The one line a tile should show for a failed turn, or None to stay quiet.

    Returns None only for the failures Claude Code itself shrugs at, so a
    caller can treat None as "nothing a person needs to see".
    """
    if error in QUIET_FAILURES:
        return None
    if error == "invalid_request" and OVERSIZE.search(detail or ""):
        # Claude Code splits this one on the message text, because the fix is
        # completely different from any other rejected request.
        return "request too large - /compact or trim"
    return FAILURE_CAPTIONS.get(error) or FAILURE_CAPTIONS["unknown"]

# A status stamp written in the same instant as the notification is the
# prompt appearing, not a person answering it. Measured on a real prompt
# here, the answer took ten seconds; this only has to outrun the hook.
SETTLE_SECONDS = 3.0


def claude_home():
    return os.environ.get("CLAUDE_CONFIG_DIR") or os.path.join(
        os.path.expanduser("~"), ".claude"
    )


def agents(timeout=20.0, include_done=True):
    """Live sessions per `claude agents --json`. Returns [] if unavailable."""
    cmd = ["claude", "agents", "--json"]
    if include_done:
        cmd.append("--all")
    try:
        out = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            # A console window would flash on Windows once per poll.
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0) if WINDOWS else 0,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []
    if out.returncode != 0:
        return []
    try:
        parsed = json.loads(out.stdout)
    except ValueError:
        return []
    return parsed if isinstance(parsed, list) else []


def session_files():
    """messagingSocketPath and friends, keyed by sessionId. Best effort."""
    found = {}
    d = os.path.join(claude_home(), "sessions")
    try:
        names = os.listdir(d)
    except OSError:
        return found
    for name in names:
        if not name.endswith(".json"):
            continue
        try:
            with open(os.path.join(d, name), encoding="utf-8") as fh:
                rec = json.load(fh)
        except (OSError, ValueError):
            continue
        sid = rec.get("sessionId")
        if sid:
            found[sid] = rec
    return found


def merge(agent_rows, files, creds=None):
    """One row per session, richest source winning per field.

    `creds` is {session_id: {socket, token}} as recorded by the SessionStart
    hook. A token is attached only when the socket it was recorded against is
    still the socket the session is listening on: a resumed session keeps its
    id but binds a new pipe, and pairing a fresh socket with a stale token
    produces a session that looks dispatchable and fails at the auth line.
    """
    creds = creds or {}
    rows = {}
    for a in agent_rows:
        sid = a.get("sessionId")
        if not sid:
            continue
        rows[sid] = dict(a)
    # A session that binds an inbox but is missing from `agents` is still
    # reachable, so keep it rather than dropping it.
    for sid, rec in files.items():
        row = rows.setdefault(sid, {"sessionId": sid, "kind": rec.get("kind")})
        row.setdefault("cwd", rec.get("cwd"))
        row.setdefault("name", rec.get("name"))
        row.setdefault("pid", rec.get("pid"))
        row.setdefault("status", rec.get("status"))
        row["socket"] = rec.get("messagingSocketPath")
        row["bridgeSessionId"] = rec.get("bridgeSessionId")
        # When this session's status last changed. `claude agents --json`
        # reports the status but not its age, and the age is the only thing
        # that can tell a live "waiting for you" from one the session has
        # already moved past - answering a permission prompt fires no hook.
        stamp = rec.get("statusUpdatedAt")
        if isinstance(stamp, (int, float)) and not isinstance(stamp, bool):
            row["status_changed_at"] = stamp / 1000.0
            row["status_at_change"] = rec.get("status")
    attach_credentials(rows, creds)
    for row in rows.values():
        row["attention"] = needs_attention(row)
    return sorted(rows.values(), key=lambda r: (r.get("name") or "", r["sessionId"]))


def attach_credentials(rows, creds):
    """Pair each row with its recorded token, but only while the socket agrees.

    `rows` is {session_id: row}. Shared with remote hosts, whose sockets are
    Unix paths on their own machine but go stale in exactly the same way.
    """
    for sid, cred in (creds or {}).items():
        row = rows.get(sid)
        if not row or not cred.get("token"):
            continue
        recorded, live = cred.get("socket"), row.get("socket")
        if recorded and live and recorded == live:
            row["token"] = cred["token"]
        else:
            row["stale_credential"] = True
    for row in rows.values():
        row["dispatchable"] = bool(row.get("socket") and row.get("token"))


def moved_on(row, since):
    """Has this session resumed work since `since` raised attention?

    Answering a permission prompt fires no hook at all: the turn simply
    carries on, and `Stop` - the only event that clears attention - can be
    many minutes away. So the tile went on saying `permission_prompt` while
    the session was visibly working in the pane beside it.

    Claude Code stamps `statusUpdatedAt` whenever a session's status changes,
    and answering the prompt flips the session back to `busy`. Both halves
    are required. A bare timestamp comparison would also fire on the change
    *into* whatever state shows a prompt, so the session must be running
    again, not merely different.
    """
    if not since or row.get("status") not in ACTIVE_STATUSES:
        return False
    # The stamp belongs to the status the session file recorded, which is not
    # always the one `agents --json` reports - a session running a command is
    # `shell` in the file and `busy` from the command. Both mean running, so
    # the stamp still means what it should; a disagreement about anything
    # else makes it ambiguous, and an ambiguous clock clears nothing.
    at_change = row.get("status_at_change")
    if at_change is not None and at_change not in ACTIVE_STATUSES:
        return False
    changed = row.get("status_changed_at")
    return bool(changed) and changed > since + SETTLE_SECONDS


def needs_attention(row):
    """True when a human is the thing this session is waiting on."""
    if row.get("status") in ATTENTION_STATUSES:
        return True
    if row.get("state") in ATTENTION_STATES:
        return True
    return bool(row.get("waitingFor"))


def snapshot(creds=None):
    return {
        "at": time.time(),
        "sessions": merge(agents(), session_files(), creds),
    }


if __name__ == "__main__":
    snap = snapshot()
    for s in snap["sessions"]:
        print(
            "%-28s %-11s %-9s %s"
            % (
                s.get("name") or "(unnamed)",
                s.get("kind") or "?",
                s.get("state") or s.get("status") or "?",
                s.get("cwd") or "",
            )
        )
    print("\n%d session(s)" % len(snap["sessions"]))
