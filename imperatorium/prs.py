"""The pull request a session's branch belongs to, and whether CI likes it.

`gh pr view` in the session's directory answers for its current branch. It
is a network call, so it never runs on the request path: a background thread
refreshes whatever directories the sidebar is showing, and the sidebar reads
the cache. A directory whose branch has no PR is remembered as such, so a
session sitting on `main` costs one call per refresh period, not one per poll.
"""
import json
import os
import subprocess
import sys
import threading
import time

from . import changes

WINDOWS = sys.platform == "win32"
FIELDS = "number,state,url,title,isDraft,reviewDecision,statusCheckRollup,headRefName"
REFRESH = 120.0


def checks_state(rollup):
    """One word for a statusCheckRollup: passing, failing, pending or none.

    Two shapes arrive in the same list - CheckRun (status + conclusion) and
    StatusContext (state) - so each is folded to the same vocabulary first.
    Any failure wins, then anything unfinished, then success.
    """
    if not rollup:
        return "none"
    seen = set()
    for c in rollup:
        if not isinstance(c, dict):
            continue
        if c.get("__typename") == "StatusContext" or ("state" in c and "status" not in c):
            s = (c.get("state") or "").upper()
            seen.add({"SUCCESS": "pass", "FAILURE": "fail", "ERROR": "fail"}
                     .get(s, "pending"))
        else:
            if (c.get("status") or "").upper() != "COMPLETED":
                seen.add("pending")
                continue
            concl = (c.get("conclusion") or "").upper()
            if concl in ("SUCCESS", "NEUTRAL", "SKIPPED"):
                seen.add("pass")
            else:
                seen.add("fail")
    if "fail" in seen:
        return "failing"
    if "pending" in seen:
        return "pending"
    return "passing" if seen else "none"


def summarise(raw):
    """The fields the sidebar badge needs, from `gh pr view --json`."""
    return {
        "number": raw.get("number"),
        "state": (raw.get("state") or "").lower(),  # open | closed | merged
        "draft": bool(raw.get("isDraft")),
        "url": raw.get("url"),
        "title": raw.get("title"),
        "branch": raw.get("headRefName"),
        "review": (raw.get("reviewDecision") or "").lower() or None,
        "checks": checks_state(raw.get("statusCheckRollup")),
    }


def lookup(cwd, timeout=20.0):
    """The PR for `cwd`'s current branch, None when there is none.

    Raises RuntimeError only for a failure worth showing (gh missing).
    """
    try:
        done = subprocess.run(
            ["gh", "pr", "view", "--json", FIELDS], cwd=cwd,
            capture_output=True, text=True, timeout=timeout,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0) if WINDOWS else 0,
        )
    except FileNotFoundError as exc:
        raise RuntimeError("gh is not installed") from exc
    except subprocess.TimeoutExpired:
        return None
    if done.returncode != 0:
        return None
    try:
        return summarise(json.loads(done.stdout))
    except ValueError:
        return None


class Tracker:
    """A cache of {cwd: pr-or-None}, refreshed off the request path."""

    def __init__(self, refresh=REFRESH, fetch=None):
        self.refresh = refresh
        self._fetch = fetch or lookup
        self._lock = threading.Lock()
        self._cache = {}  # norm cwd -> {"at", "pr", "branch"}
        self._wanted = {}  # cwd -> when the sidebar last asked
        self._thread = None
        self.disabled = None

    def get(self, cwd):
        """What is known for `cwd` now; asks for it to be refreshed."""
        if not cwd:
            return None
        key = os.path.normcase(os.path.abspath(cwd))
        with self._lock:
            self._wanted[cwd] = time.time()
            hit = self._cache.get(key)
        return hit["pr"] if hit else None

    def start(self):
        if self._thread is None:
            self._thread = threading.Thread(target=self._loop, name="prs", daemon=True)
            self._thread.start()
        return self

    def _loop(self):
        while True:
            self.poll_once()
            time.sleep(5.0)

    def poll_once(self, now=None):
        """Refresh every wanted directory whose entry is older than `refresh`.

        A branch switch also invalidates: the cache is keyed on the branch it
        was fetched for, so checking out another branch shows its PR on the
        next pass rather than two minutes later.
        """
        now = time.time() if now is None else now
        with self._lock:
            # A session that closed stops being asked about; stop fetching.
            for cwd, asked in list(self._wanted.items()):
                if now - asked > 10 * self.refresh:
                    del self._wanted[cwd]
            wanted = list(self._wanted)
        for cwd in wanted:
            if self.disabled:
                return
            key = os.path.normcase(os.path.abspath(cwd))
            try:
                branch = changes.git(cwd, "branch", "--show-current",
                                     timeout=5.0).strip()
            except changes.GitError:
                continue
            with self._lock:
                hit = self._cache.get(key)
            if hit and hit["branch"] == branch and now - hit["at"] < self.refresh:
                continue
            try:
                pr = self._fetch(cwd)
            except RuntimeError as exc:
                self.disabled = str(exc)
                return
            with self._lock:
                self._cache[key] = {"at": now, "pr": pr, "branch": branch}
