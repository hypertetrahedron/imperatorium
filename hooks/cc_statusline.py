"""Claude Code statusline -> Imperatorium, and a short line back.

The statusline is the one place Claude Code publishes a session's context
use and the account's rate-limit windows. This forwards the JSON it is given
to the dispatcher, then prints a compact line of its own so the session's
terminal still shows something useful:

    Opus 5.5 · ctx 23% · 5h 41% · 7d 12%

Like the hooks, it must never break a session: every failure is swallowed,
it always prints a line, and the post is bounded well under the statusline's
own budget so a dispatcher that is down costs a fraction of a second.
"""
import json
import os
import sys
import urllib.error
import urllib.request

ENDPOINT = os.environ.get("CCONTROL_ENDPOINT", "http://127.0.0.1:8792/hook")
TIMEOUT = float(os.environ.get("CCONTROL_STATUSLINE_TIMEOUT", "0.5"))
EVENT = "ccontrol_statusline"
# The payload never says where the session's requests go, and `rate_limits`
# only exists on a claude.ai Pro or Max plan (or behind a gateway with a
# spend limit). The provider is chosen by environment, which this script
# inherits from the session, so it is read here and forwarded alongside.
# Order is Claude Code's own precedence: a cloud provider flag wins over any
# key, and a key wins over the claude.ai login.
PROVIDER_FLAGS = (("CLAUDE_CODE_USE_BEDROCK", "bedrock"),
                  ("CLAUDE_CODE_USE_VERTEX", "vertex"),
                  ("CLAUDE_CODE_USE_FOUNDRY", "foundry"))
PROVIDER_LABELS = {"bedrock": "Bedrock", "vertex": "Vertex", "foundry": "Foundry",
                   "api": "API", "gateway": "gateway"}


def provider(env=None):
    """Where this session's requests go, or None for the claude.ai login."""
    env = os.environ if env is None else env
    for name, value in PROVIDER_FLAGS:
        if str(env.get(name) or "").strip().lower() in ("1", "true", "yes", "on"):
            return value
    if env.get("ANTHROPIC_BASE_URL"):
        return "gateway"
    if env.get("ANTHROPIC_API_KEY") or env.get("ANTHROPIC_AUTH_TOKEN"):
        return "api"
    return None


def line(payload):
    """The text to show. Pure, so it is tested without a session."""
    parts = []
    model = (payload.get("model") or {}).get("display_name")
    if model:
        parts.append(str(model))
    ctx = (payload.get("context_window") or {}).get("used_percentage")
    if isinstance(ctx, (int, float)):
        parts.append("ctx %d%%" % round(ctx))
    limits = payload.get("rate_limits") or {}
    for key, label in (("five_hour", "5h"), ("seven_day", "7d")):
        used = (limits.get(key) or {}).get("used_percentage")
        if isinstance(used, (int, float)):
            parts.append("%s %d%%" % (label, round(used)))
    if not limits and payload.get("ccontrol_provider") in PROVIDER_LABELS:
        parts.append(PROVIDER_LABELS[payload["ccontrol_provider"]])
    return " · ".join(parts)


def main():
    try:
        raw = sys.stdin.buffer.read().decode("utf-8", "replace")
        payload = json.loads(raw) if raw.strip() else {}
    except (OSError, ValueError):
        payload = {}
    if not isinstance(payload, dict):
        payload = {}
    try:
        found = provider()
    except Exception:
        found = None
    if found:
        payload["ccontrol_provider"] = found
    if payload.get("session_id"):
        body = json.dumps(dict(payload, hook_event_name=EVENT)).encode("utf-8")
        req = urllib.request.Request(
            ENDPOINT, data=body, headers={"Content-Type": "application/json"})
        try:
            urllib.request.urlopen(req, timeout=TIMEOUT).read()
        except (urllib.error.URLError, OSError, ValueError):
            pass
    try:
        out = line(payload)
        sys.stdout.buffer.write(out.encode("utf-8"))
        sys.stdout.flush()
    except Exception:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
