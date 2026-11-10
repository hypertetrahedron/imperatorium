"""Sessions hosted in herdr-core terminals, shown in herdr-app.

With `terminal.host` set to "hc", New, Fork and Resume start `claude` in a
herdr-core terminal instead of a Windows Terminal tab. Each terminal is
labelled so herdr-app shows it: one space per project folder, one tab per
session. herdr-app adopts ids it doesn't know as new spaces and tabs, named
after the folder (verified with herdr-app 0.1.14).

The terminal is `--keep`: after a reboot, a crash of the core's holder or a
logout, herdr-core brings it back as a shell in the same folder, at the same
address. It never re-runs `claude` itself, so `Restorer` does: every session
started here is remembered until it ends, and at start the dispatcher types
`claude --resume <id>` into each kept terminal that came back as a bare
shell (or starts a new terminal if it didn't come back at all).
"""
import hashlib
import os
import re
import threading
import time
import uuid

from . import hcore

HOSTS = ("terminal", "hc")

# Stagger restores, as herdr does, so twenty `claude` starts don't land at once.
RESTORE_GAP = 0.1


def space_id(cwd):
    """One herdr space per project folder: stable, readable, unique."""
    norm = os.path.normcase(os.path.abspath(cwd or "")).rstrip("\\/")
    base = re.sub(r"[^a-z0-9]+", "-", os.path.basename(norm).lower()).strip("-")[:24] or "root"
    return "imp-%s-%s" % (base, hashlib.sha1(norm.encode("utf-8")).hexdigest()[:6])


def tab_id(session_id):
    return "imp-" + (session_id or "")[:12]


def labels(session_id, cwd, fork=False):
    """A fork gets its own id from Claude Code only when it starts, so its
    terminal is labelled then (SessionStart); until then it has a tab of
    its own rather than its parent's."""
    if fork:
        return {hcore.SPACE_LABEL: space_id(cwd), hcore.TAB_LABEL: "imp-" + uuid.uuid4().hex[:12]}
    return {hcore.SESSION_LABEL: session_id,
            hcore.SPACE_LABEL: space_id(cwd),
            hcore.TAB_LABEL: tab_id(session_id)}


def launch(core, argv, cwd, session_id, fork=False):
    """Start `argv` (a `claude ...` command) for `session_id`; its terminal."""
    core.ensure()
    got = core.run(list(argv), cwd=cwd, labels=labels(session_id, cwd, fork), keep=True)
    return {"address": got["address"],
            "core": core.session or "default"}


# What a kept terminal comes back as after a loss, and what a person starts
# `claude` from. Restore types only into one of these: never into an editor,
# a REPL or any other program that happens to hold the terminal.
SHELLS = {"pwsh", "powershell", "cmd", "bash", "sh", "zsh", "fish", "nu", "dash"}


def _exe_name(name):
    base = os.path.basename(str(name or "")).lower()
    return base[:-4] if base.endswith(".exe") else base


def is_shell(facts):
    fg = (facts or {}).get("foreground") or {}
    name = fg.get("name") or ((fg.get("argv") or (facts or {}).get("argv") or [""])[0])
    return _exe_name(name) in SHELLS


def owns_terminal(table, claude_pid, terminal_pid):
    """Does the `claude` with `claude_pid` run in the terminal whose program
    is `terminal_pid` - as that program, or under it with no other `claude`
    in between? A `claude -p` run from a Bash call inside a hosted session
    inherits the terminal's HC_TERMINAL_ID but sits under the outer claude.
    `table`: procs.process_table() rows (pid, ppid, exe, started)."""
    try:
        claude_pid, terminal_pid = int(claude_pid), int(terminal_pid)
    except (TypeError, ValueError):
        return False
    if claude_pid == terminal_pid:
        return True
    parent = {row[0]: (row[1], row[2]) for row in table or ()}
    pid, seen = claude_pid, set()
    while pid in parent and pid not in seen:
        seen.add(pid)
        ppid, _ = parent[pid]
        if ppid == terminal_pid:
            return True
        if ppid not in parent or _exe_name(parent[ppid][1]).startswith("claude"):
            return False
        pid = ppid
    return False


def is_claude(facts):
    """Is `claude` what runs in this terminal now (not a shell left behind)?"""
    fg = (facts or {}).get("foreground") or {}
    name = os.path.basename(str(fg.get("name") or "")).lower()
    if name.startswith("claude"):
        return True
    argv = fg.get("argv") or (facts or {}).get("argv") or []
    # `claude` started through node or a .cmd shim.
    return any(os.path.basename(str(a)).lower().startswith("claude") for a in argv[:3]) \
        and not name.startswith(("pwsh", "powershell", "cmd", "bash", "zsh", "sh", "fish"))


def close(core, address, wait=8.0, sleep=time.sleep):
    """End the `claude` in an hc terminal the way a person would.

    Ctrl+C (clears a draft or stops a turn), then `/exit`; a terminal
    started for the session (its program was `claude`) then finishes, and
    herdr-app closes its pane. Killed only if `claude` is still there after
    `wait` seconds. A shell the person started it from is left open.
    Returns closing.close's shape: {ok, how, tab_closed}.
    """
    try:
        core.keys(address, "ctrl+c")
        core.send(address, "/exit", force=True)
    except hcore.HcError as exc:
        return {"ok": False, "error": str(exc)[:280]}
    facts = None
    end = time.time() + wait
    while time.time() < end:
        try:
            facts = core.get(address)
        except hcore.HcError:
            facts = None
        if not facts or facts.get("state") != "live" or not is_claude(facts):
            return {"ok": True, "how": "exit",
                    "tab_closed": not facts or facts.get("state") != "live"}
        sleep(0.5)
    try:
        core.kill(address)
    except hcore.HcError as exc:
        return {"ok": False, "error": "it would not exit: %s" % str(exc)[:200]}
    return {"ok": True, "how": "killed", "tab_closed": True}


def _same_terminal(facts, sid, rec):
    """Is the terminal at the remembered address still this session's?

    Ours carry the session label. One the person started by hand (in
    herdr-app, say) has none; its address came from the session's own hook,
    and hc keeps a kept terminal's address across a loss, so the same
    folder is enough. A label naming another session never is.
    """
    label = (facts.get("labels") or {}).get(hcore.SESSION_LABEL)
    if label:
        return label == sid
    a = os.path.normcase(os.path.normpath(str(facts.get("cwd") or "")))
    b = os.path.normcase(os.path.normpath(str(rec.get("cwd") or "")))
    return bool(a) and a == b


class Restorer:
    """Remembers hosted sessions and brings them back after a reboot.

    `store` holds them under prefs["hosted"] = {session_id: {cwd, address,
    core, at}}; the dispatcher forgets one when its session ends.
    """

    def __init__(self, store, core_for, sleep=time.sleep):
        self.store = store
        self.core_for = core_for  # name -> hcore.Core or None
        self.sleep = sleep
        # remember/forget read, change and write the whole record; the hook
        # thread and the opener can do so at once.
        self._lock = threading.Lock()

    def hosted(self):
        return dict(self.store.prefs().get("hosted") or {})

    def remember(self, session_id, cwd, terminal):
        with self._lock:
            got = self.hosted()
            got[session_id] = {"cwd": cwd, "address": terminal["address"],
                               "core": terminal.get("core") or "default", "at": time.time()}
            self.store.set_prefs(hosted=got)

    def forget(self, session_id):
        with self._lock:
            got = self.hosted()
            if got.pop(session_id, None) is not None:
                self.store.set_prefs(hosted=got)

    def restore(self, running_ids=()):
        """Resume every remembered session that isn't running. A list of
        what was done, one entry per session, for the event log."""
        out = []
        running = set(running_ids)
        for sid, rec in sorted(self.hosted().items(), key=lambda kv: kv[1].get("at") or 0):
            if sid in running:
                continue
            core = self.core_for(rec.get("core"))
            if core is None:
                out.append({"session_id": sid, "result": "no hc"})
                continue
            out.append(self._one(core, sid, rec))
            self.sleep(RESTORE_GAP)
        return out

    def _one(self, core, sid, rec):
        cmd = ["claude", "--resume", sid]
        addr = rec.get("address")
        try:
            core.ensure()
            facts = None
            if addr:
                try:
                    facts = core.get(addr)
                except hcore.HcError:
                    facts = None
            if facts and facts.get("state") == "live" and _same_terminal(facts, sid, rec):
                if is_claude(facts):
                    return {"session_id": sid, "result": "running", "address": addr}
                if not is_shell(facts):
                    return {"session_id": sid, "result": "terminal busy",
                            "address": addr, "error": "something other than a shell runs there"}
                # A kept terminal, back as a shell: type the resume into it.
                if (facts.get("labels") or {}).get(hcore.SESSION_LABEL) != sid:
                    core.label(addr, **{hcore.SESSION_LABEL: sid})
                core.send(addr, " ".join(cmd), force=False)
                return {"session_id": sid, "result": "resumed in place", "address": addr}
            cwd = rec.get("cwd")
            if not cwd or not os.path.isdir(cwd):
                return {"session_id": sid, "result": "folder gone", "cwd": cwd}
            t = launch(core, cmd, cwd, sid)
            self.remember(sid, cwd, t)
            return {"session_id": sid, "result": "resumed in a new terminal",
                    "address": t["address"]}
        except hcore.HcError as exc:
            return {"session_id": sid, "result": "failed", "error": str(exc)[:280]}
