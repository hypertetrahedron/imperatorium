"""Add (or remove) the Imperatorium hooks in ~/.claude/settings.json.

User scope on purpose: one edit reaches every project, instead of copying hook
configuration into twenty repositories.

Every hook this installs is `async: true`, so Claude Code does not wait on it,
and cc_hook.py swallows its own failures - a dispatcher that is not running
must never be something a person notices mid-session.

    python hooks/install.py --dry-run     # show the diff, change nothing
    python hooks/install.py               # install, backing up settings first
    python hooks/install.py --uninstall   # remove only what this installed
    python hooks/install.py --remote HOST [--dry-run|--uninstall]
                                          # the reporting hooks, on HOST

Two opt-in extras, off unless asked for:

    --tool-hooks   PreToolUse / PostToolUse / PostToolUseFailure, async. Live,
                   timestamped activity and "running X for 40s" in the window.
                   Costs one short Python process per tool call per session.
    --statusline   the statusline, which is the only place Claude Code hands
                   out context use and the 5-hour / weekly rate limits. It
                   prints a compact line of its own. Refuses to replace a
                   statusline that is not ours.
"""
import argparse
import json
import os
import shutil
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
HOOK_SCRIPT = os.path.join(HERE, "cc_hook.py")
PERMISSION_SCRIPT = os.path.join(HERE, "cc_permission.py")
STATUSLINE_SCRIPT = os.path.join(HERE, "cc_statusline.py")

# Stamped onto every entry so uninstall can find exactly what it added and
# leave anyone else's hooks alone.
MARK = "ccontrol"

# event -> (matcher, script, async)
#
# Reporting hooks are async: Claude Code must never wait on them, and their
# output is ignored. PermissionRequest is the exception and must stay
# synchronous - its whole purpose is the decision it prints, and an async hook
# is not waited on, so the reply would arrive after the prompt it answers.
EVENTS = {
    "SessionStart": ("startup|resume|clear", HOOK_SCRIPT, True),
    "SessionEnd": (None, HOOK_SCRIPT, True),
    "Notification": (
        "permission_prompt|idle_prompt|agent_needs_input"
        "|elicitation_dialog|elicitation_url_dialog",
        HOOK_SCRIPT,
        True,
    ),
    "Stop": (None, HOOK_SCRIPT, True),
    "StopFailure": (None, HOOK_SCRIPT, True),
    "PermissionRequest": (None, PERMISSION_SCRIPT, False),
}


# Opt-in: one hook process per tool call is a real cost across twenty
# sessions, so these are only installed when asked for (--tool-hooks).
TOOL_EVENTS = {
    "PreToolUse": (None, HOOK_SCRIPT, True),
    "PostToolUse": (None, HOOK_SCRIPT, True),
    "PostToolUseFailure": (None, HOOK_SCRIPT, True),
}


def statusline_command(python=None):
    return '"%s" "%s"' % (python or sys.executable, STATUSLINE_SCRIPT)


def is_our_statusline(value):
    return isinstance(value, dict) and "cc_statusline.py" in str(value.get("command"))


def apply_statusline(settings, uninstall=False, python=None):
    """Install or remove our statusline. Returns [description of the change]."""
    cur = settings.get("statusLine")
    if uninstall:
        if is_our_statusline(cur):
            settings.pop("statusLine", None)
            return ["remove statusLine"]
        return []
    if cur and not is_our_statusline(cur):
        return ["SKIP statusLine: one is already configured and it is not ours "
                "(%s)" % str(cur.get("command") if isinstance(cur, dict) else cur)[:80]]
    settings["statusLine"] = {"type": "command",
                              "command": statusline_command(python),
                              "padding": 0}
    return ["replace statusLine" if cur else "add statusLine"]


def settings_path():
    home = os.environ.get("CLAUDE_CONFIG_DIR") or os.path.join(
        os.path.expanduser("~"), ".claude"
    )
    return os.path.join(home, "settings.json")


def entry(matcher, script, is_async, command=None):
    hook = {
        "type": "command",
        "command": command or sys.executable,
        "args": [script],
        # The reporters are fire-and-forget. The permission hook may block
        # while the window is offered the decision, so its budget has to
        # exceed policy.json's park wait - otherwise Claude Code gives up
        # first and an answer from the window arrives too late to be used.
        "timeout": 5 if is_async else permission_timeout(),
        "statusMessage": MARK,
    }
    if is_async:
        hook["async"] = True
    block = {"hooks": [hook]}
    if matcher:
        block["matcher"] = matcher
    return block


def permission_timeout():
    """Long enough to outlast a parked decision, with slack for the round trip.

    With parking off this stays small: the hook returns immediately and a long
    budget would only delay the terminal prompt when something hangs.
    """
    path = os.path.join(os.path.dirname(HERE), "policy.json")
    try:
        with open(path, encoding="utf-8") as fh:
            park = (json.load(fh) or {}).get("park") or {}
    except (OSError, ValueError):
        park = {}
    if not park.get("enabled"):
        return 8
    longest = max(float(park.get("wait_seconds") or 0),
                  float(park.get("background_wait_seconds") or 0))
    return int(longest) + 20


def is_ours(block):
    for hook in block.get("hooks") or []:
        if hook.get("statusMessage") == MARK:
            return True
        for arg in hook.get("args") or []:
            if str(arg).lower() in (HOOK_SCRIPT.lower(), PERMISSION_SCRIPT.lower()):
                return True
    return False


def load(path):
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except FileNotFoundError:
        return {}
    except ValueError as exc:
        raise SystemExit(
            "%s is not valid JSON (%s). Fix it by hand before installing." % (path, exc)
        )


def apply(settings, uninstall=False, events=None, command=None):
    """Return (settings, [description of each change])."""
    changes = []
    hooks = settings.setdefault("hooks", {})
    for event, (matcher, script, is_async) in (events or EVENTS).items():
        blocks = hooks.get(event) or []
        if not isinstance(blocks, list):
            changes.append("SKIP %s: existing value is not a list" % event)
            continue
        kept = [b for b in blocks if not is_ours(b)]
        removed = len(blocks) - len(kept)
        if uninstall:
            if removed:
                changes.append("remove %s (%d entry)" % (event, removed))
            if kept:
                hooks[event] = kept
            else:
                hooks.pop(event, None)
            continue
        kept.append(entry(matcher, script, is_async, command))
        hooks[event] = kept
        changes.append(
            ("replace %s" % event) if removed else ("add %s" % event)
        )
    if not hooks:
        settings.pop("hooks", None)
    return settings, changes


# The reporting hooks only. PermissionRequest stays local: its policy file and
# decision log live on this machine, and a host's prompts are answered there.
REMOTE_EVENTS = [e for e in EVENTS if e != "PermissionRequest"]
SSH = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8"]


def _ssh(host, command, data=None):
    import subprocess

    done = subprocess.run(SSH + [host, command], input=data,
                          capture_output=True, timeout=60)
    if done.returncode != 0:
        raise SystemExit("%s: %s" % (host, (done.stderr or done.stdout or b"")
                                     .decode("utf-8", "replace").strip()
                                     or "ssh exited %d" % done.returncode))
    return done.stdout.decode("utf-8", "replace")


def install_remote(host, uninstall=False, dry_run=False, python="python3"):
    """The same hooks, on another machine, reporting through its tunnel.

    cc_hook.py is copied to ~/.claude/ccontrol/ there and posts to
    127.0.0.1:8792, which the dispatcher's reverse tunnel carries back here.
    Paths are absolute because a hook runs with no shell to expand `~`.
    """
    home = _ssh(host, 'printf %s "$HOME"').strip()
    if not home.startswith("/"):
        raise SystemExit("%s: could not read $HOME (got %r)" % (host, home))
    script = home + "/.claude/ccontrol/cc_hook.py"
    path = home + "/.claude/settings.json"
    raw = _ssh(host, "cat %s 2>/dev/null || true" % path)
    try:
        settings = json.loads(raw) if raw.strip() else {}
    except ValueError as exc:
        raise SystemExit("%s:%s is not valid JSON (%s). Fix it by hand first."
                         % (host, path, exc))
    events = {e: (EVENTS[e][0], script, EVENTS[e][2]) for e in REMOTE_EVENTS}
    updated, changes = apply(settings, uninstall=uninstall, events=events,
                             command=python)

    print("settings : %s:%s" % (host, path))
    print("hooks    : %s %s" % (python, script))
    for line in changes or ["(nothing to do)"]:
        print("  %s" % line)
    if dry_run:
        print("\n--dry-run: nothing written. Resulting hooks block:")
        print(json.dumps(updated.get("hooks", {}), indent=2))
        return 0

    if not uninstall:
        with open(HOOK_SCRIPT, "rb") as fh:
            _ssh(host, "mkdir -p %s/.claude/ccontrol && cat > %s"
                 % (home, script), fh.read())
    stamp = time.strftime("%Y%m%d-%H%M%S")
    body = (json.dumps(updated, indent=2) + "\n").encode("utf-8")
    _ssh(host, '[ -f {p} ] && cp {p} {p}.ccontrol-{t}.bak; '
               'cat > {p}.ccontrol.tmp && mv {p}.ccontrol.tmp {p}'
         .format(p=path, t=stamp), body)
    print("backup   : %s.ccontrol-%s.bak (if there was a file)" % (path, stamp))
    print("written. Restart a session there for the hooks to take effect.")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--uninstall", action="store_true")
    ap.add_argument("--settings", default=None)
    ap.add_argument("--remote", metavar="HOST",
                    help="install on HOST over SSH instead of here")
    ap.add_argument("--python", default="python3",
                    help="interpreter on the remote host (default python3)")
    ap.add_argument("--tool-hooks", action="store_true",
                    help="also install the per-tool-call activity hooks")
    ap.add_argument("--statusline", action="store_true",
                    help="also install the usage-reporting statusline")
    args = ap.parse_args()

    if args.remote:
        return install_remote(args.remote, args.uninstall, args.dry_run,
                              args.python)

    path = args.settings or settings_path()
    for script in (HOOK_SCRIPT, PERMISSION_SCRIPT):
        if not os.path.exists(script):
            raise SystemExit("hook script is missing: %s" % script)

    settings = load(path)
    events = dict(EVENTS)
    # Uninstall removes everything this ever added, asked for or not.
    if args.tool_hooks or args.uninstall:
        events.update(TOOL_EVENTS)
    updated, changes = apply(settings, uninstall=args.uninstall, events=events)
    if args.statusline or args.uninstall:
        changes += apply_statusline(updated, uninstall=args.uninstall)

    print("settings : %s" % path)
    print("hooks    : %s" % sys.executable)
    print("           %s" % HOOK_SCRIPT)
    print("           %s  (PermissionRequest, synchronous)" % PERMISSION_SCRIPT)
    for line in changes or ["(nothing to do)"]:
        print("  %s" % line)

    if args.dry_run:
        print("\n--dry-run: nothing written. Resulting hooks block:")
        print(json.dumps(updated.get("hooks", {}), indent=2))
        if "statusLine" in updated:
            print("statusLine: %s" % json.dumps(updated["statusLine"]))
        return 0

    os.makedirs(os.path.dirname(path), exist_ok=True)
    if os.path.exists(path):
        backup = "%s.ccontrol-%s.bak" % (path, time.strftime("%Y%m%d-%H%M%S"))
        shutil.copy2(path, backup)
        print("backup   : %s" % backup)

    tmp = path + ".ccontrol.tmp"
    with open(tmp, "w", encoding="utf-8") as fh:
        json.dump(updated, fh, indent=2)
        fh.write("\n")
    os.replace(tmp, path)
    print("written. Restart a session for the hooks to take effect.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
