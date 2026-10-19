"""What the sessions cost, estimated from the transcripts.

The statusline's cost is the authority for a live session, but it forgets
once the session ends and says nothing about the days before. Every assistant
message in the JSONL carries the API's `usage`, so a history can be rebuilt:
tokens per (local day, project, model) times list prices.

- Claude Code writes several entries per API message while it streams, all
  with the same `message.id` and the usage growing; a message is counted once,
  from the last entry seen for its id, so a later entry replaces an earlier one.
- Subagent transcripts live beside the session's own, in
  `<session-id>/subagents/*.jsonl`. They are billed too, and belong to the
  parent session.
- The project is where the session was working: the repository, not the
  scratch worktree Claude Code made inside it.

An estimate, at list prices: no discounts, no batch rates, no tier surcharges.
The format is Claude Code's own and unversioned, so every field is optional.
"""
import json
import os
import re
import threading
import time
from datetime import datetime, timedelta, timezone

# List-price snapshot (2026-10), $ per million tokens: (input, output). The
# statusline's cost is authoritative for live sessions; this is an estimate.
PRICES = {
    "claude-fable-5-1": (10.0, 50.0),
    "claude-fable-5": (10.0, 50.0),
    "claude-mythos-5-1": (10.0, 50.0),
    "claude-opus-5-5": (4.0, 20.0),
    "claude-opus-5": (5.0, 25.0),
    "claude-opus-4-8": (5.0, 25.0),
    "claude-opus-4-7": (5.0, 25.0),
    "claude-opus-4-6": (5.0, 25.0),
    "claude-opus-4-5": (5.0, 25.0),
    "claude-opus-4-1": (15.0, 75.0),
    "claude-opus-4": (15.0, 75.0),
    "claude-sonnet-5-5": (2.0, 10.0),
    "claude-sonnet-5": (2.0, 10.0),
    "claude-sonnet-4-6": (3.0, 15.0),
    "claude-sonnet-4-5": (3.0, 15.0),
    "claude-sonnet-4": (3.0, 15.0),
    "claude-haiku-5-5": (0.10, 0.50),
    "claude-haiku-4-5": (1.0, 5.0),
    "claude-3-5-haiku": (0.80, 4.0),
}
# Newest of each family, for a model id this table has not heard of.
FAMILIES = {
    "fable": "claude-fable-5-1",
    "opus": "claude-opus-5-5",
    "sonnet": "claude-sonnet-5-5",
    "haiku": "claude-haiku-5-5",
}
WRITE_5M, WRITE_1H, READ = 1.25, 2.0, 0.1   # x the input price
READ_RATIO = {"claude-opus-5-5": 0.05, "claude-fable-5-1": 0.025,
              "claude-mythos-5-1": 0.025}
UNKNOWN = "(unknown)"

# Token counts are kept as [input, output, cache read, cache write 5m, 1h].
IN, OUT, CREAD, W5, W1 = range(5)
SLACK = 86400.0      # a file touched a day before the window may hold its first day
HEAD = 256           # bytes compared to tell an appended file from a replaced one
WORKTREE = "/.claude/worktrees/"


def projects_dir():
    base = os.environ.get("CLAUDE_CONFIG_DIR") or os.path.join(os.path.expanduser("~"), ".claude")
    return os.path.join(base, "projects")


def resolve(model):
    """(price key, priced) for a model id as a transcript spells it.

    Date suffixes and bracket suffixes ("[1m]") are ignored: the longest known
    id that the name starts with wins, then the family's newest. An id that is
    neither comes back as itself, unpriced.
    """
    name = re.sub(r"\[.*?\]", "", str(model or "")).strip().lower()
    best = None
    for known in PRICES:
        if (name == known or name.startswith(known + "-")) and (best is None or len(known) > len(best)):
            best = known
    if best:
        return best, True
    for family, known in FAMILIES.items():
        if family in name:
            return known, True
    return name or UNKNOWN, False


def project_of(cwd):
    """The repository a working directory belongs to."""
    path = str(cwd or "").replace("\\", "/")
    if WORKTREE in path + "/":
        path = (path + "/").split(WORKTREE)[0]
    path = path.rstrip("/")
    return path.rsplit("/", 1)[-1] or UNKNOWN if path else UNKNOWN


def price(key, vec):
    """Dollars for one token vector at a priced model's list prices."""
    pin, pout = PRICES[key]
    read = READ_RATIO.get(key, READ)
    return (vec[IN] * pin + vec[OUT] * pout + vec[CREAD] * pin * read
            + vec[W5] * pin * WRITE_5M + vec[W1] * pin * WRITE_1H) / 1e6


def rewrite_cost(model, tokens, ttl):
    """Dollars to write `tokens` of context back into the prompt cache, which
    is what the first prompt after the cache expired costs; None for a model
    with no known price."""
    key, priced = resolve(model)
    if not priced or not tokens:
        return None
    return tokens * PRICES[key][0] * (WRITE_1H if (ttl or 0) >= 3600 else WRITE_5M) / 1e6


def _num(value):
    return int(value) if isinstance(value, (int, float)) and not isinstance(value, bool) else 0


def _when(stamp):
    """ISO UTC timestamp -> aware datetime, or None."""
    if not isinstance(stamp, str):
        return None
    try:
        at = datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    except ValueError:
        return None
    if at.tzinfo is None:   # naive means UTC here, as Claude Code writes it
        at = at.replace(tzinfo=timezone.utc)
    return at


def _day(stamp):
    """ISO UTC timestamp -> local 'YYYY-MM-DD', or None."""
    at = _when(stamp)
    return at.astimezone().strftime("%Y-%m-%d") if at else None


def _vector(usage):
    made = usage.get("cache_creation")
    total = _num(usage.get("cache_creation_input_tokens"))
    w5 = w1 = 0
    if isinstance(made, dict) and ("ephemeral_5m_input_tokens" in made
                                   or "ephemeral_1h_input_tokens" in made):
        w5 = _num(made.get("ephemeral_5m_input_tokens"))
        w1 = _num(made.get("ephemeral_1h_input_tokens"))
        w5 += max(0, total - w5 - w1)   # what the split does not account for
    else:
        w5 = total
    return [_num(usage.get("input_tokens")), _num(usage.get("output_tokens")),
            _num(usage.get("cache_read_input_tokens")), w5, w1]


def _priced(model, v, vec, cost, unpriced):
    """Add one message's vector into `vec`; return (cost, unpriced) updated."""
    for i, n in enumerate(v):
        vec[i] += n
    if model.startswith("?"):
        return cost, unpriced + sum(v)
    return cost + price(model, v), unpriced


def _by_model(models, model, v):
    """Fold one message into `models`, the split the model-mix chart draws:
    {model: {output, total_tokens, cost_usd}}. Output is what it stacks."""
    m = models.setdefault(model.lstrip("?"), {"output": 0, "total_tokens": 0, "cost_usd": 0.0})
    m["output"] += v[OUT]
    m["total_tokens"] += sum(v)
    if not model.startswith("?"):
        m["cost_usd"] += price(model, v)


def _shape(vec, cost, unpriced, models=None):
    """A token vector as the API speaks it."""
    tokens = {"input": vec[IN], "output": vec[OUT], "cache_read": vec[CREAD],
              "cache_write": vec[W5] + vec[W1]}
    return {"tokens": tokens, "total_tokens": sum(vec), "cost_usd": cost,
            "unpriced_tokens": unpriced, "models": models or {}}


def session_files(session_id, root=None):
    """The transcript of `session_id` and its subagents', wherever its
    project folder is. [] for an id that is not a session's."""
    if not re.fullmatch(r"[0-9a-fA-F-]{36}", session_id or ""):
        return []
    root = root or projects_dir()
    out = []
    try:
        folders = [e.path for e in os.scandir(root) if e.is_dir()]
    except OSError:
        return out
    for folder in folders:
        main = os.path.join(folder, session_id + ".jsonl")
        if os.path.isfile(main):
            out.append(main)
        sub = os.path.join(folder, session_id, "subagents")
        try:
            out.extend(os.path.join(sub, f) for f in os.listdir(sub) if f.endswith(".jsonl"))
        except OSError:
            pass
    return out


class _File:
    """One transcript: how far it was read and what that added up to."""

    def __init__(self, session):
        self.session = session
        self.reset()

    def reset(self):
        self.offset = 0
        self.size = 0
        self.mtime = 0.0
        self.head = b""
        self.rest = b""      # a line still being written
        self.cwd = None      # the last working directory seen, for entries without one
        self.lines = 0       # entries parsed, for tests and diagnosis
        self.seen = {}       # message id -> (bucket key, vector it contributed, epoch seconds)
        self.buckets = {}    # (date, project, model) -> vector

    def _add(self, key, vec, sign):
        have = self.buckets.setdefault(key, [0] * 5)
        for i, n in enumerate(vec):
            have[i] += sign * n

    def feed(self, data):
        """Fold complete JSONL lines; keep a trailing partial line for later."""
        data = self.rest + data
        end = data.rfind(b"\n") + 1
        self.rest = data[end:]
        for line in data[:end].split(b"\n"):
            if b'"usage"' not in line and b'"cwd"' not in line:
                continue
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            if isinstance(entry, dict):
                self._entry(entry)

    def _entry(self, entry):
        self.lines += 1
        if isinstance(entry.get("cwd"), str):
            self.cwd = entry["cwd"]
        if entry.get("type") != "assistant":
            return
        msg = entry.get("message")
        usage = msg.get("usage") if isinstance(msg, dict) else None
        if not isinstance(usage, dict) or not msg.get("model") or msg["model"] == "<synthetic>":
            return
        at = _when(entry.get("timestamp"))
        if not at:
            return
        day = at.astimezone().strftime("%Y-%m-%d")
        model, priced = resolve(msg["model"])
        key = (day, project_of(self.cwd), model if priced else "?" + model)
        mid = msg.get("id") or entry.get("uuid") or "line%d" % self.lines
        vec = _vector(usage)
        old = self.seen.get(mid)
        if old:
            self._add(old[0], old[1], -1)
        self.seen[mid] = (key, vec, at.timestamp())
        self._add(key, vec, 1)


class History:
    """Estimated cost per day, project and model, read incrementally.

    Transcripts only ever grow, so each is read once and then from where it
    stopped. A file that shrank (or whose start changed) was replaced and is
    read again whole. Files untouched since before the window are forgotten.
    """

    def __init__(self, root=None):
        self.root = root or projects_dir()
        self._lock = threading.Lock()
        self._files = {}     # path -> _File
        # Transcripts of the sessions open now, kept whatever their age: a
        # session open for weeks is counted over its whole life.
        self._pinned = set()
        self.gen = 0         # bumped whenever a file is read; keys the memo
        self._memo = {}      # query -> (gen, answer)

    def _candidates(self, since):
        """(path, session id, mtime, size) of transcripts touched since `since`."""
        found = []
        try:
            folders = [e for e in os.scandir(self.root) if e.is_dir()]
        except OSError:
            return found
        for folder in folders:
            try:
                entries = list(os.scandir(folder.path))
            except OSError:
                continue
            for e in entries:
                if e.is_file() and e.name.endswith(".jsonl"):
                    found.append((e.path, e.name[:-6]))
                elif e.is_dir():
                    sub = os.path.join(e.path, "subagents")
                    try:
                        for s in os.scandir(sub):
                            if s.is_file() and s.name.endswith(".jsonl"):
                                found.append((s.path, e.name))
                    except OSError:
                        pass
        out = []
        for path, session in found:
            try:
                st = os.stat(path)
            except OSError:
                continue
            if st.st_mtime >= since:
                out.append((path, session, st.st_mtime, st.st_size))
        return out

    def _refresh(self, path, session, mtime, size):
        """Read what `path` gained. Returns True if it was read."""
        state = self._files.get(path)
        if state is None:
            state = self._files[path] = _File(session)
        elif state.size == size and state.mtime == mtime:
            return False
        try:
            with open(path, "rb") as fh:
                if size < state.size or (state.offset and fh.read(HEAD) != state.head[:HEAD]):
                    state.reset()
                fh.seek(state.offset)
                data = fh.read()
                if not state.offset:
                    fh.seek(0)
                    state.head = fh.read(HEAD)
        except OSError:
            return False
        state.offset += len(data)
        state.size, state.mtime = size, mtime
        state.feed(data)
        self.gen += 1
        return True

    def _scan(self, since, budget, prune=True):
        """Bring every transcript touched since `since` up to date, newest
        first, within `budget` seconds. Returns False if the budget ran out.
        With `prune`, files untouched since then are forgotten (pinned ones
        stay). Call with the lock held."""
        started = time.monotonic()
        complete = True
        found = self._candidates(since)
        if prune:
            keep = {path for path, *_ in found} | self._pinned
            for path in [p for p in self._files if p not in keep]:
                del self._files[path]
        # Newest first, so a spent budget leaves the oldest days short.
        for path, session, mtime, size in sorted(found, key=lambda f: -f[2]):
            known = self._files.get(path)
            if known and known.size == size and known.mtime == mtime:
                continue
            if time.monotonic() - started >= budget:
                complete = False
                continue
            self._refresh(path, session, mtime, size)
        return complete

    def _messages(self):
        """(session, model key, vector, epoch) for every message once - the
        session's own transcript before its subagents', as in `_fold`."""
        owners = {}
        for path in sorted(self._files, key=lambda p: (p.count(os.sep), p)):
            state = self._files[path]
            seen = owners.setdefault(state.session, set())
            for mid, (key, vec, at) in state.seen.items():
                if mid not in seen:
                    seen.add(mid)
                    yield state.session, key[2], vec, at

    def totals(self, since, until=None, now=None, budget=3.0):
        """Tokens and estimated cost of every message sent in [since, until),
        across all transcripts on this machine. JSON-able: {tokens: {input,
        output, cache_read, cache_write}, total_tokens, cost_usd,
        unpriced_tokens, models, complete}; `models` is the split by model
        (see `_by_model`)."""
        now = time.time() if now is None else float(now)
        until = now if until is None else float(until)
        with self._lock:
            # Not pruned: a short window must not drop the files a longer
            # one (the cost history, the month) still wants.
            complete = self._scan(since - SLACK, budget, prune=False)
            memo = ("totals", round(since), round(min(until, now) / 60))
            hit = self._memo.get(memo)
            if hit and hit[0] == self.gen:
                return dict(hit[1], complete=complete)
            vec, cost, unpriced, models = [0] * 5, 0.0, 0, {}
            for _, model, v, at in self._messages():
                if since <= at < until:
                    cost, unpriced = _priced(model, v, vec, cost, unpriced)
                    _by_model(models, model, v)
            out = _shape(vec, cost, unpriced, models)
            self._remember(memo, out)
        return dict(out, complete=complete)

    def sessions(self, ids, budget=3.0):
        """({session id: totals}, complete) for each of `ids` over its whole
        life, subagents included. Their transcripts are pinned, so they stay
        counted however old they are. A session with no transcript is left
        out."""
        ids = [i for i in ids if i]
        with self._lock:
            started = time.monotonic()
            complete = True
            pinned = set()
            for sid in ids:
                for path in session_files(sid, self.root):
                    pinned.add(path)
                    try:
                        st = os.stat(path)
                    except OSError:
                        continue
                    if path not in self._files and time.monotonic() - started >= budget:
                        complete = False
                        continue
                    self._refresh(path, sid, st.st_mtime, st.st_size)
            self._pinned = pinned
            memo = ("sessions", tuple(sorted(ids)))
            hit = self._memo.get(memo)
            if hit and hit[0] == self.gen:
                return hit[1], complete
            want = set(ids)
            acc = {}
            for session, model, v, _ in self._messages():
                if session in want:
                    vec, cost, unpriced, models = acc.get(session) or ([0] * 5, 0.0, 0, {})
                    _by_model(models, model, v)
                    acc[session] = (vec,) + _priced(model, v, vec, cost, unpriced) + (models,)
            out = {sid: _shape(*got) for sid, got in acc.items()}
            self._remember(memo, out)
        return out, complete

    def _remember(self, memo, answer):
        for k in [k for k, (g, _) in self._memo.items() if g != self.gen]:
            del self._memo[k]
        self._memo[memo] = (self.gen, answer)

    def summary(self, days=30, now=None, budget=8.0):
        """Cost over the last `days` local days, today included. JSON-able."""
        days = max(1, int(days))
        if isinstance(now, datetime):
            now = now.timestamp()
        now = time.time() if now is None else float(now)
        today = datetime.fromtimestamp(now).date()
        window = [(today - timedelta(days=i)).strftime("%Y-%m-%d")
                  for i in range(days - 1, -1, -1)]
        inside = set(window)
        with self._lock:
            complete = self._scan(now - days * 86400 - SLACK, budget)
            buckets, sessions = self._fold(inside)

        per_day = {d: [0.0, 0] for d in window}
        per_project, per_model = {}, {}
        unpriced = 0
        for (day, project, model), vec in buckets.items():
            tokens = sum(vec)
            if not tokens:
                continue
            if model.startswith("?"):
                unpriced += tokens
                cost, model = 0.0, model[1:]
            else:
                cost = price(model, vec)
            per_day[day][0] += cost
            per_day[day][1] += tokens
            for table, name in ((per_project, project), (per_model, model)):
                row = table.setdefault(name, [0.0, 0])
                row[0] += cost
                row[1] += tokens
        projects = [{"project": name, "cost": row[0], "tokens": row[1],
                     "sessions": len(sessions.get(name, ()))}
                    for name, row in per_project.items()]
        projects.sort(key=lambda r: (-r["cost"], -r["tokens"]))
        models = [{"model": name, "cost": row[0], "tokens": row[1]}
                  for name, row in per_model.items()]
        models.sort(key=lambda r: (-r["cost"], -r["tokens"]))
        return {
            "ok": True,
            "estimated": True,
            "days": [{"date": d, "cost": per_day[d][0], "tokens": per_day[d][1]} for d in window],
            "projects": projects,
            "models": models,
            "total": sum(v[0] for v in per_day.values()),
            "total_tokens": sum(v[1] for v in per_day.values()),
            "unpriced_tokens": unpriced,
            "files": len(self._files),
            "complete": complete,
            "window_days": days,
        }

    def _fold(self, inside):
        """Every file's buckets in the window, a message in two files of one
        session counted once. -> ({bucket key: vector}, {project: {sessions}})."""
        total, sessions = {}, {}
        owners = {}      # session id -> message ids already counted
        # The session's own transcript before its subagents'.
        for path in sorted(self._files, key=lambda p: (p.count(os.sep), p)):
            state = self._files[path]
            seen = owners.setdefault(state.session, set())
            for key, vec in state.buckets.items():
                if key[0] in inside:
                    have = total.setdefault(key, [0] * 5)
                    for i, n in enumerate(vec):
                        have[i] += n
                    if any(vec):
                        sessions.setdefault(key[1], set()).add(state.session)
            for mid, (key, vec, _) in state.seen.items():
                if mid in seen:
                    if key[0] in inside:
                        have = total[key]
                        for i, n in enumerate(vec):
                            have[i] -= n
                else:
                    seen.add(mid)
        return total, sessions


if __name__ == "__main__":
    began = time.monotonic()
    result = History().summary(30)
    print("%d files in %.1fs, complete=%s" % (result["files"], time.monotonic() - began,
                                              result["complete"]))
    print("total $%.2f, %s tokens, %s unpriced" % (
        result["total"], format(result["total_tokens"], ","), format(result["unpriced_tokens"], ",")))
    for row in result["projects"][:5]:
        print("  %-28s $%8.2f  %d sessions" % (row["project"], row["cost"], row["sessions"]))
    for row in result["models"][:5]:
        print("  %-28s $%8.2f" % (row["model"], row["cost"]))
