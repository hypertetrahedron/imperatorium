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
import time

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


def labels(session_id, cwd):
    return {hcore.SESSION_LABEL: session_id,
            hcore.SPACE_LABEL: space_id(cwd),
            hcore.TAB_LABEL: tab_id(session_id)}


def launch(core, argv, cwd, session_id):
    """Start `argv` (a `claude ...` command) for `session_id`; its terminal."""
    core.ensure()
    got = core.run(list(argv), cwd=cwd, labels=labels(session_id, cwd), keep=True)
    return {"address": got["address"],
            "core": core.session or "default"}


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


class Restorer:
    """Remembers hosted sessions and brings them back after a reboot.

    `store` holds them under prefs["hosted"] = {session_id: {cwd, address,
    core, at}}; the dispatcher forgets one when its session ends.
    """

    def __init__(self, store, core_for, sleep=time.sleep):
        self.store = store
        self.core_for = core_for  # name -> hcore.Core or None
        self.sleep = sleep

    def hosted(self):
        return dict(self.store.prefs().get("hosted") or {})

    def remember(self, session_id, cwd, terminal):
        got = self.hosted()
        got[session_id] = {"cwd": cwd, "address": terminal["address"],
                           "core": terminal.get("core") or "default", "at": time.time()}
        self.store.set_prefs(hosted=got)

    def forget(self, session_id):
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
            if facts and facts.get("state") == "live" and \
                    (facts.get("labels") or {}).get(hcore.SESSION_LABEL) == sid:
                if is_claude(facts):
                    return {"session_id": sid, "result": "running", "address": addr}
                # A kept terminal, back as a shell: type the resume into it.
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
