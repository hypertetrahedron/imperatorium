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

AskUserQuestion goes through the same door. Claude Code treats the question
dialog as a permission prompt, so this hook is asked first; an `allow` that
carries `updatedInput.answers` is taken as the answers and the dialog never
shows. A question item therefore carries its questions, and is only ever
allowed with an answer for every one of them.
"""
import threading
import time
import uuid

# Long-polling cap. The hook's own timeout is the real deadline; this just
# keeps a single HTTP request from being held open indefinitely.
MAX_HOLD = 600.0

QUESTION_TOOL = "AskUserQuestion"
# A verdict that releases the hook without deciding: it says nothing, and the
# terminal shows its own prompt at once instead of when the wait runs out.
PASS = "pass"


class Pending:
    def __init__(self):
        self._items = {}
        self._lock = threading.Condition()

    def park(self, request, wait=0.0):
        """Register a request and wait up to `wait` seconds for a decision.

        Returns (verdict, decided_by, answers): verdict is None if nobody
        answered, answers is set only for a question that was answered.
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
        questions = _questions(request) if item["tool"] == QUESTION_TOOL else None
        if questions:
            item["questions"] = questions
            item["input"] = " / ".join(q["question"] for q in questions)[:400]
        with self._lock:
            self._items[item_id] = item
            self._lock.notify_all()

        deadline = time.time() + min(max(wait, 0.0), MAX_HOLD)
        with self._lock:
            while item["verdict"] is None and time.time() < deadline:
                self._lock.wait(timeout=min(1.0, max(0.05, deadline - time.time())))
            verdict = item["verdict"]
            if verdict == PASS:
                verdict = None
                self._items.pop(item_id, None)
            if verdict is None:
                # Expired. Drop it so the window stops offering a decision
                # that can no longer be delivered.
                item["expired"] = True
                self._items.pop(item_id, None)
            return verdict, item.get("decided_by"), item.get("answers")

    def decide(self, item_id, verdict, who="window", answers=None):
        if verdict not in ("allow", "deny", PASS):
            return False
        with self._lock:
            item = self._items.get(item_id)
            if not item or item["verdict"] is not None:
                return False
            if item["tool"] == QUESTION_TOOL and verdict == "allow":
                # Allowing a question without its answers would let the tool
                # run on nothing. The Stream Deck and impctl only send a bare
                # allow, so this is where they are turned away.
                answers = _checked_answers(item.get("questions"), answers)
                if answers is None:
                    return False
                item["answers"] = answers
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


def _questions(request):
    """The questions of an AskUserQuestion call, in the shape the window draws.

    None when the input is not what the tool takes, so the item falls back to
    a plain Allow/Deny, which then can only deny or pass.
    """
    tool_input = request.get("tool_input")
    raw = tool_input.get("questions") if isinstance(tool_input, dict) else None
    if not isinstance(raw, list) or not raw:
        return None
    out = []
    for q in raw:
        if not isinstance(q, dict) or not str(q.get("question") or "").strip():
            return None
        options = [{"label": str(o.get("label") or ""),
                    "description": str(o.get("description") or "")}
                   for o in (q.get("options") or []) if isinstance(o, dict)]
        out.append({"question": str(q["question"]),
                    "header": str(q.get("header") or ""),
                    "multiSelect": bool(q.get("multiSelect")),
                    "options": [o for o in options if o["label"]]})
    return out


def _checked_answers(questions, answers):
    """{question: answer} with one non-empty string per question, or None.

    The keys are the question texts, which is what Claude Code reads. A
    multi-select answer is its labels joined with ", ", as the terminal
    sends it; free text stands for the terminal's "Other".
    """
    if not questions or not isinstance(answers, dict):
        return None
    out = {}
    for q in questions:
        value = answers.get(q["question"])
        if not isinstance(value, str) or not value.strip():
            return None
        out[q["question"]] = value.strip()[:2000]
    return out
