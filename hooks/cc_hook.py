"""Claude Code hook -> Imperatorium dispatcher.

Installed for every session, in user settings, so all projects report in
without touching twenty repositories.

Why a `command` hook and not the cheaper `http` type: an http hook posts the
hook's own JSON, and that JSON does not carry CLAUDE_CODE_MESSAGING_SOCKET or
CLAUDE_CODE_MESSAGING_TOKEN. Those are environment variables, exported to hook
processes and nowhere else, and the token is the only thing that makes a
session reachable. So SessionStart has to be a process that can read its own
environment. The rest could be http hooks; they are routed through here too so
there is one thing to configure and one thing to debug.

This must never break a session: every failure is swallowed, the exit code is
always 0, and stdout stays empty so Claude Code has nothing to interpret.
"""
import json
import os
import sys
import time
import urllib.error
import urllib.request

ENDPOINT = os.environ.get("CCONTROL_ENDPOINT", "http://127.0.0.1:8792/hook")
TIMEOUT = float(os.environ.get("CCONTROL_TIMEOUT", "3"))


def main():
    try:
        raw = sys.stdin.read()
    except (OSError, ValueError):
        return 0
    try:
        payload = json.loads(raw) if raw.strip() else {}
    except ValueError:
        return 0
    if not isinstance(payload, dict):
        return 0

    if payload.get("hook_event_name") in TOOL_EVENTS:
        payload = trim_tool_payload(payload)

    # The two values that exist only here.
    payload["messaging_socket"] = os.environ.get("CLAUDE_CODE_MESSAGING_SOCKET")
    payload["messaging_token"] = os.environ.get("CLAUDE_CODE_MESSAGING_TOKEN")
    payload["bridge_session_id"] = os.environ.get("CLAUDE_CODE_BRIDGE_SESSION_ID")

    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        ENDPOINT, data=body, headers={"Content-Type": "application/json"}
    )
    try:
        urllib.request.urlopen(req, timeout=TIMEOUT).read()
    except (urllib.error.URLError, OSError, ValueError) as exc:
        # The dispatcher being down is an ordinary state, not an error worth
        # showing a person mid-session. But a hook that fails invisibly is
        # unfalsifiable, so set CCONTROL_DEBUG_LOG to a path to find out why.
        _note("%s %s: %s" % (payload.get("hook_event_name"), type(exc).__name__, exc))
    return 0


TOOL_EVENTS = ("PreToolUse", "PostToolUse", "PostToolUseFailure")
SUMMARY_KEYS = ("command", "file_path", "path", "pattern", "url", "prompt",
                "query", "description")


def trim_tool_payload(payload):
    """Only what the activity view shows, never a file's contents.

    A tool hook fires on every call, and its payload carries the whole
    `tool_input` (a Write's content) and `tool_response` (a Read's file). The
    dispatcher needs one line of each, so the rest never leaves this process.
    """
    keep = {k: payload.get(k) for k in (
        "hook_event_name", "session_id", "cwd", "tool_name", "tool_use_id",
        "agent_id", "error")}
    tool_input = payload.get("tool_input")
    if isinstance(tool_input, dict):
        keep["tool_input"] = {k: str(tool_input[k])[:300]
                              for k in SUMMARY_KEYS if tool_input.get(k)}
    if payload.get("hook_event_name") == "PostToolUseFailure":
        response = payload.get("tool_response")
        keep["tool_response"] = (response if isinstance(response, str)
                                 else json.dumps(response))[:500] if response else None
    return keep


def _note(line):
    path = os.environ.get("CCONTROL_DEBUG_LOG")
    if not path:
        return
    try:
        with open(path, "a", encoding="utf-8") as fh:
            fh.write("%s %s\n" % (time.strftime("%Y-%m-%d %H:%M:%S"), line))
    except OSError:
        pass


if __name__ == "__main__":
    sys.exit(main())
