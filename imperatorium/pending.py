"""Permission decisions waiting for a human.

When the policy says `ask`, the hook parks the request here and blocks. The
window shows it, you answer, and the hook returns your answer to Claude Code.
That is the only way this system can actually answer a permission prompt: a
message through a session's inbox is a peer message, and Claude Code refuses
those as consent by design.

The safety of the whole scheme rests on one documented behaviour: a hook that
exceeds its timeout does NOT block the tool call - "the call continues through
the normal permission flow". So if nobody is watching, the park expires and
the terminal prompts exactly as it would without any of this. Never answering
is always safe.
"""
import threading
import time
import uuid

# Long-polling cap. The hook's own timeout is the real deadline; this just
# keeps a single HTTP request from being held open indefinitely.
MAX_HOLD = 600.0


class Pending:
    def __init__(self):
        self._items = {}
        self._lock = threading.Condition()

    def park(self, request, wait=0.0):
        """Register a request and wait up to `wait` seconds for a decision.

        Returns (verdict, reason) where verdict is None if nobody answered.
        """
        item_id = uuid.uuid4().hex[:12]
        item = {
            "id": item_id,
            "at": time.time(),
            "session_id": request.get("session_id"),
            "cwd": request.get("cwd"),
            "tool": request.get("tool_name"),
            "input": _summarise(request.get("tool_input")),
            "reason": request.get("_reason"),
            "verdict": None,
            "decided_by": None,
        }
        with self._lock:
            self._items[item_id] = item
            self._lock.notify_all()

        deadline = time.time() + min(max(wait, 0.0), MAX_HOLD)
        with self._lock:
            while item["verdict"] is None and time.time() < deadline:
                self._lock.wait(timeout=min(1.0, max(0.05, deadline - time.time())))
            verdict = item["verdict"]
            if verdict is None:
                # Expired. Drop it so the window stops offering a decision
                # that can no longer be delivered.
                item["expired"] = True
                self._items.pop(item_id, None)
            return verdict, item.get("decided_by")

    def decide(self, item_id, verdict, who="window"):
        if verdict not in ("allow", "deny"):
            return False
        with self._lock:
            item = self._items.get(item_id)
            if not item or item["verdict"] is not None:
                return False
            item["verdict"] = verdict
            item["decided_by"] = who
            self._lock.notify_all()
            return True

    def waiting(self):
        """Everything still awaiting an answer, oldest first."""
        with self._lock:
            items = [dict(i) for i in self._items.values() if i["verdict"] is None]
        items.sort(key=lambda i: i["at"])
        return items

    def drop(self, item_id):
        with self._lock:
            self._items.pop(item_id, None)
            self._lock.notify_all()


def _summarise(tool_input):
    """The one line a person needs to decide, not the whole payload."""
    if not isinstance(tool_input, dict):
        return ""
    for key in ("command", "file_path", "path", "pattern", "url"):
        value = tool_input.get(key)
        if value:
            return str(value)[:400]
    return ""
