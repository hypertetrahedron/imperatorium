"""Post a message into a running Claude Code session's inbox.

Each session binds an inbox - a named pipe on native Windows, a Unix domain
socket on macOS and Linux - and exports its path and a per-session token to
its hooks and Bash children as CLAUDE_CODE_MESSAGING_SOCKET and
CLAUDE_CODE_MESSAGING_TOKEN. The session registry under ~/.claude/sessions/
carries the same path as `messagingSocketPath`, so a dispatcher that collects
tokens at SessionStart can reach any session on this machine.

Two frame types matter here:

    {"type": "user",  "message": {"role": "user", "content": "..."}}
    {"type": "peer_message", ...}

A `user` frame is queued as a user turn. A peer message is explicitly not
user consent: it cannot answer a permission prompt, cannot change settings,
and a slash command inside it arrives as inert text. Dispatch therefore uses
the `user` frame, which is why the token matters - see AUTH below.

AUTH
    On native Windows the first line of the connection MUST be a valid auth
    line or Claude Code closes the connection and delivers nothing. On macOS
    and Linux the line is optional but is always sent here: it is also what
    identifies an own-child message on every platform.

TIMING
    Claude Code closes a connection that has not sent a complete line within
    30 seconds. Build the whole payload first, then connect.
"""
import json
import os
import socket
import sys

WINDOWS = sys.platform == "win32"

# Claude Code drops a connection whose line exceeds its cap. Stay well under
# the ~1,000,000 character same-machine message cap.
MAX_CONTENT = 900_000


class InboxError(RuntimeError):
    """The message could not be handed to the session's inbox."""


def _frames(token, content, attachments=None):
    """The exact line sequence to write, auth first."""
    lines = []
    if token:
        lines.append({"type": "auth", "token": token})
    msg = {"role": "user", "content": content}
    if attachments:
        msg["file_attachments"] = list(attachments)
    lines.append({"type": "user", "message": msg})
    return b"".join(
        json.dumps(line, ensure_ascii=False).encode("utf-8") + b"\n" for line in lines
    )


def send(socket_path, token, content, attachments=None, timeout=10.0):
    """Deliver `content` to the session listening at `socket_path`.

    Returns None on success and raises InboxError otherwise. Delivery to the
    inbox is not delivery to Claude: the receiving session's inbound controls
    still decide, and this call cannot see that outcome.
    """
    if not isinstance(content, str) or not content.strip():
        raise InboxError("content must be a non-empty string")
    if len(content) > MAX_CONTENT:
        raise InboxError(
            "content is %d chars; the cap is %d" % (len(content), MAX_CONTENT)
        )
    if WINDOWS and not token:
        raise InboxError("a token is required on native Windows")

    payload = _frames(token, content, attachments)
    if WINDOWS:
        _write_pipe(socket_path, payload, timeout)
    else:
        _write_uds(socket_path, payload, timeout)


def _write_pipe(path, payload, timeout):
    # A Windows named pipe is opened with ordinary file semantics. There is no
    # portable per-call timeout here, so the caller's own deadline applies;
    # the pipe is local and the server drains it, so a write does not block
    # in practice.
    if not path.startswith("\\\\"):
        raise InboxError("not a named-pipe path: %r" % (path,))
    try:
        fd = os.open(path, os.O_RDWR | os.O_BINARY)
    except OSError as exc:
        raise InboxError("cannot open %s: %s" % (path, exc)) from exc
    try:
        written = 0
        while written < len(payload):
            written += os.write(fd, payload[written:])
    except OSError as exc:
        raise InboxError("write to %s failed: %s" % (path, exc)) from exc
    finally:
        os.close(fd)


def _write_uds(path, payload, timeout):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(timeout)
    try:
        sock.connect(path)
        sock.sendall(payload)
    except OSError as exc:
        raise InboxError("send to %s failed: %s" % (path, exc)) from exc
    finally:
        sock.close()


def send_to_self(content, attachments=None):
    """Post into the session this process is a child of."""
    path = os.environ.get("CLAUDE_CODE_MESSAGING_SOCKET")
    token = os.environ.get("CLAUDE_CODE_MESSAGING_TOKEN")
    if not path:
        raise InboxError("CLAUDE_CODE_MESSAGING_SOCKET is not set")
    return send(path, token, content, attachments)


if __name__ == "__main__":
    text = " ".join(sys.argv[1:]) or "ping from ccontrol.inbox"
    send_to_self(text)
    print("delivered to inbox")
