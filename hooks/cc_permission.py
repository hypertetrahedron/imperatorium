"""PermissionRequest hook: answer the cheap asks, forward the rest.

This sits on the critical path of every permission prompt in every session.
The policy is read from disk and the verdict computed in-process, so `allow`
and `deny` cost no network call and a dispatcher that is down cannot slow one
down or change it.

Only `ask` touches the network, and only when parking is enabled: the request
is offered to the window and this process blocks until someone answers or the
wait expires. That is the one way this system can answer a permission prompt,
since a message through a session's inbox is a peer message and Claude Code
refuses those as consent. Dispatcher down, nobody watching, anything else
wrong - it falls through to the terminal prompt.

Contract (hooks reference, "PermissionRequest decision control"): reply on
stdout with

    {"hookSpecificOutput": {"hookEventName": "PermissionRequest",
                            "decision": {"behavior": "allow" | "deny"}}}

`decision` is an object. See `reply()` for why that is behind a policy switch.

Exit code 2 is not honoured for this event, so the decision object is the only
thing that matters.

Staying silent means "ask" - Claude Code prompts you exactly as it would
without this hook. That is what every failure path here does: a missing policy
file, malformed JSON, an unreadable log, an unexpected exception. The worst
this can do when it breaks is nothing.
"""
import json
import os
import sys
import time
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

AUDIT_MAX_BYTES = 2 * 1024 * 1024
PARK_ENDPOINT = os.environ.get(
    "CCONTROL_PARK_ENDPOINT", "http://127.0.0.1:8792/permission/park"
)


def state_dir():
    base = (
        os.environ.get("LOCALAPPDATA")
        or os.environ.get("XDG_STATE_HOME")
        or os.path.join(os.path.expanduser("~"), ".local", "state")
    )
    return os.path.join(base, "ccontrol")


def policy_path():
    return os.environ.get("CCONTROL_POLICY") or os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "policy.json"
    )


def load_policy():
    try:
        with open(policy_path(), encoding="utf-8") as fh:
            loaded = json.load(fh)
        return loaded if isinstance(loaded, dict) else {}
    except (OSError, ValueError):
        # No policy, or a broken one, means no auto-approval. Never a default
        # that approves things.
        return {}


def audit(record):
    path = os.path.join(state_dir(), "decisions.jsonl")
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        # Cheap rotation: an audit log that fills a disk is its own incident.
        try:
            if os.path.getsize(path) > AUDIT_MAX_BYTES:
                os.replace(path, path + ".1")
        except OSError:
            pass
        with open(path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(record, ensure_ascii=False) + "\n")
    except OSError:
        pass


def park(request, reason, detail):
    """Offer the decision to the window; return its verdict, or None.

    The wait comes from policy.json. Zero (the default) keeps the old
    behaviour exactly: no network call, straight to the terminal prompt.
    """
    settings = (load_policy().get("park") or {})
    if not settings.get("enabled"):
        return None
    wait = float(settings.get("wait_seconds") or 0)
    # A background session has no terminal to fall back to, so it can wait
    # far longer; the dispatcher knows which kind this is and picks.
    wait_bg = float(settings.get("background_wait_seconds") or wait)
    if max(wait, wait_bg) <= 0:
        return None

    payload = dict(request)
    payload["_reason"] = reason
    payload["_detail"] = detail
    payload["wait"] = wait
    payload["wait_background"] = wait_bg
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        PARK_ENDPOINT, data=body, headers={"Content-Type": "application/json"}
    )
    try:
        # The dispatcher holds this open until someone answers or the wait
        # expires. Allow a little slack over `wait` for the round trip.
        with urllib.request.urlopen(req, timeout=max(wait, wait_bg) + 10) as fh:
            answer = json.loads(fh.read().decode())
    except (urllib.error.URLError, OSError, ValueError) as exc:
        _note("park failed, falling through to the terminal: %s" % exc)
        return None
    return answer.get("verdict")


def main():
    try:
        raw = sys.stdin.read()
        request = json.loads(raw) if raw.strip() else {}
    except (OSError, ValueError):
        return 0
    if not isinstance(request, dict):
        return 0

    try:
        from ccontrol import policy as policy_mod

        verdict, reason, tier = policy_mod.decide(request, load_policy())
    except Exception as exc:  # an import or policy failure must cost a prompt
        audit({
            "at": time.time(),
            "verdict": "ask",
            "reason": "hook error: %s" % type(exc).__name__,
            "tool": request.get("tool_name"),
            "session_id": request.get("session_id"),
        })
        return 0

    tool_input = request.get("tool_input")
    detail = ""
    if isinstance(tool_input, dict):
        detail = str(
            tool_input.get("command")
            or tool_input.get("file_path")
            or tool_input.get("pattern")
            or ""
        )[:200]

    audit({
        "at": time.time(),
        "verdict": verdict,
        "reason": reason,
        "tier": tier,
        "tool": request.get("tool_name"),
        "detail": detail,
        "cwd": request.get("cwd"),
        "session_id": request.get("session_id"),
    })

    if verdict == "ask":
        # Offer it to the window before falling back to the terminal. This is
        # the only way this system can answer a permission prompt at all: a
        # message through a session's inbox is a peer message, and Claude Code
        # refuses those as consent.
        #
        # Safe because a hook that exceeds its timeout does not block the tool
        # call - "the call continues through the normal permission flow". So
        # nobody watching means the terminal prompts exactly as it always did.
        parked = park(request, reason, detail)
        if parked in ("allow", "deny"):
            audit({"at": time.time(), "verdict": parked, "reason": "answered in the window",
                   "tier": tier, "tool": request.get("tool_name"), "detail": detail,
                   "cwd": request.get("cwd"), "session_id": request.get("session_id")})
            verdict = parked
        else:
            # "ask" is expressed by saying nothing, so a future change to the
            # reply contract cannot turn a silence into an approval.
            return 0

    json.dump(reply(verdict, reason, load_policy().get("decision_format")),
              sys.stdout)
    sys.stdout.flush()
    return 0


def reply(verdict, reason, fmt=None):
    """The JSON this hook prints for an allow or deny.

    `fmt="object"` is the documented contract, read on 2026-09-24 from the
    hooks reference's "PermissionRequest decision control": `decision` is an
    OBJECT, `{"behavior": "allow"|"deny", "message": ...}`, inside
    hookSpecificOutput. The legacy shape below sends `decision` as a bare
    string, which Claude Code ignores - that is why a background session
    stayed blocked after this hook said `allow`, and why nothing this hook
    has ever decided took effect.

    Legacy stays the default only so that switching auto-approval on is a
    deliberate choice (`"decision_format": "object"` in policy.json), not a
    side effect of a bug fix.
    """
    if fmt == "object":
        decision = {"behavior": verdict}
        if verdict == "deny" and reason:
            decision["message"] = reason
        return {"hookSpecificOutput": {"hookEventName": "PermissionRequest",
                                       "decision": decision}}
    return {
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": verdict,
            "permissionDecision": verdict,
            "reason": reason,
            "permissionDecisionReason": reason,
        }
    }


if __name__ == "__main__":
    sys.exit(main())
