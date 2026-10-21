r"""impctl - one command line for everything Imperatorium does.

The pieces live in half a dozen places: the service in service\tasks.ps1 (or
systemd/launchd units), setup in setup_machine.py, hooks in hooks\install.py,
remote probing in imperatorium.remote, and everything about sessions behind the
dispatcher's HTTP API. This is the one front door, so nobody has to read
those files to find out what they can do.

    python impctl.py help            every command and its parameters
    python impctl.py help COMMAND    one command in full

Commands that talk to sessions need the dispatcher running; the service and
setup commands do not. Stdlib only, so it runs with whatever Python is there.
"""
import argparse
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
WINDOWS = sys.platform == "win32"
MACOS = sys.platform == "darwin"
BASE = os.environ.get("IMPERATORIUM_URL", "http://127.0.0.1:8792").rstrip("/")

# Component -> (Windows task, command-line pattern of its process). The
# pattern is what tasks.ps1's Stop-Existing matches, split per component.
COMPONENTS = {
    "dispatcher": ("ImperatoriumDispatcher", "imperatorium.dispatcher"),
    "streamdeck": ("ImperatoriumStreamDeck", "streamdeck_surface"),
    "hotkey": ("ImperatoriumHotkey", "hotkey.py"),
}
ORDER = ["dispatcher", "streamdeck", "hotkey"]  # start order; stop is reversed


class Fail(Exception):
    pass


# -- talking to the dispatcher -------------------------------------------------
def api(path, body=None, timeout=15):
    req = urllib.request.Request(
        BASE + path,
        data=None if body is None else json.dumps(body).encode(),
        headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode() or "{}")
    except urllib.error.HTTPError as exc:
        try:
            return json.loads(exc.read().decode() or "{}")
        except ValueError:
            raise Fail("%s answered %d" % (path, exc.code))
    except (urllib.error.URLError, OSError):
        raise Fail("the dispatcher is not responding at %s - start it with "
                   "`python impctl.py start`" % BASE)


def q(value):
    from urllib.parse import quote
    return quote(str(value), safe="")


def find_session(spec):
    """A session by id, id prefix, name or window title (unique substring)."""
    rows = api("/api/sessions").get("sessions") or []
    spec_l = spec.lower()
    exact = [r for r in rows if spec in (r.get("sessionId"), r.get("key"), r.get("name"))]
    if len(exact) == 1:
        return exact[0]
    hits = [r for r in rows
            if (r.get("sessionId") or "").startswith(spec)
            or spec_l in (r.get("name") or "").lower()
            or spec_l in (r.get("window_title") or "").lower()]
    if len(hits) == 1:
        return hits[0]
    if not hits:
        raise Fail("no live session matches %r - see `python impctl.py sessions`" % spec)
    raise Fail("%r matches %d sessions: %s" % (
        spec, len(hits), ", ".join(r.get("name") or r.get("sessionId") for r in hits)))


def find_any_session(spec):
    """A live session, or failing that a past one by id or id prefix - what
    resume and fork need, since a finished session is not on the live list."""
    try:
        return find_session(spec)
    except Fail as live_miss:
        found = {}
        for query in (spec, ""):
            for r in (api("/api/history?q=" + q(query), timeout=60).get("results") or []):
                if r.get("session_id", "").startswith(spec):
                    found[r["session_id"]] = r
            if found:
                break
        if len(found) == 1:
            r = next(iter(found.values()))
            return {"sessionId": r["session_id"], "cwd": r.get("cwd")}
        if len(found) > 1:
            raise Fail("%r matches %d past sessions - give more of the id" % (spec, len(found)))
        raise Fail("%s, nor any past session id (search with `python impctl.py history`)"
                   % str(live_miss).split(" - ")[0])


def sid(row):
    return row.get("sessionId") or row.get("id")


def result(out, args, ok_text=None):
    """Print a POST answer: raw with --json, else one line. Exit 1 on refusal."""
    if args.json:
        print(json.dumps(out, indent=2))
    elif out.get("ok") is False or out.get("error"):
        print("refused: %s" % (out.get("error") or "no reason given"))
    else:
        print(ok_text(out) if ok_text else json.dumps(out, indent=2))
    return 1 if out.get("ok") is False or out.get("error") else 0


def ago(ts):
    if not ts:
        return ""
    s = max(0, time.time() - float(ts))
    for unit, n in (("d", 86400), ("h", 3600), ("m", 60)):
        if s >= n:
            return "%d%s" % (s // n, unit)
    return "%ds" % s


# -- the service ---------------------------------------------------------------
def pick(component):
    return ORDER if component in (None, "all") else [component]


def ps(script):
    return subprocess.run(["powershell", "-NoProfile", "-Command", script],
                          capture_output=True, text=True)


def kill_strays(pattern):
    # A copy started by hand is not the task's and would hold the port (or
    # the hotkey) against it. Same filter as tasks.ps1, one component at a time.
    ps("Get-CimInstance Win32_Process -Filter \"Name='pythonw.exe' or Name='python.exe'\" | "
       "Where-Object { $_.CommandLine -match '%s' } | "
       "ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }"
       % pattern.replace(".", "\\."))


def unit(component):
    sys.path.insert(0, HERE)
    import setup_machine
    for key, path, name in setup_machine.unit_names():
        if key == component:
            return path, name
    return None, None


def svc_stop(names):
    for c in reversed(names):
        if WINDOWS:
            task, pattern = COMPONENTS[c]
            # Disabled, not merely stopped: the task's one-minute heartbeat
            # would otherwise bring it straight back.
            ps("Disable-ScheduledTask -TaskName %s -ErrorAction SilentlyContinue | Out-Null; "
               "Stop-ScheduledTask -TaskName %s -ErrorAction SilentlyContinue" % (task, task))
            kill_strays(pattern)
        elif c == "hotkey":
            continue  # Windows-only
        else:
            path, name = unit(c)
            if not path or not os.path.exists(path):
                continue
            if MACOS:
                subprocess.run(["launchctl", "bootout", "gui/%d" % os.getuid(), path])
            else:
                subprocess.run(["systemctl", "--user", "stop", name])
        print("stopped %s" % c)


def svc_start(names):
    for i, c in enumerate(names):
        if WINDOWS:
            task, _ = COMPONENTS[c]
            got = ps("if (-not (Get-ScheduledTask -TaskName %s -ErrorAction SilentlyContinue)) "
                     "{ 'missing' } else { Enable-ScheduledTask -TaskName %s | Out-Null; "
                     "Start-ScheduledTask -TaskName %s; 'ok' }" % (task, task, task))
            if "missing" in got.stdout:
                print("%s: no task registered - run `python impctl.py service install`" % c)
                continue
            if c == "dispatcher" and i + 1 < len(names):
                time.sleep(2)  # let it bind the port before the others look for it
        elif c == "hotkey":
            continue
        else:
            path, name = unit(c)
            if not path or not os.path.exists(path):
                print("%s: not installed - run `python impctl.py service install`" % c)
                continue
            if MACOS:
                subprocess.run(["launchctl", "bootstrap", "gui/%d" % os.getuid(), path])
            else:
                subprocess.run(["systemctl", "--user", "start", name])
        print("started %s" % c)


def cmd_status(args):
    if WINDOWS:
        for c in ORDER:
            task, pattern = COMPONENTS[c]
            got = ps("$t = Get-ScheduledTask -TaskName %s -ErrorAction SilentlyContinue; "
                     "if (-not $t) { 'not-registered' } else { $t.State }" % task)
            state = got.stdout.strip() or "?"
            procs = ps("(Get-CimInstance Win32_Process -Filter \"Name='pythonw.exe' or "
                       "Name='python.exe'\" | Where-Object { $_.CommandLine -match '%s' }"
                       ").ProcessId -join ','" % pattern.replace(".", "\\.")).stdout.strip()
            print("%-11s task %-15s pids %s" % (c, state, procs or "-"))
    else:
        sys.path.insert(0, HERE)
        import setup_machine
        setup_machine.service_status()
    try:
        health = api("/api/health", timeout=4)
        print("dispatcher responding at %s, pid %s" % (BASE, health.get("pid")))
    except Fail:
        print("dispatcher NOT responding at %s" % BASE)
        return 1
    return 0


def cmd_start(args):
    svc_start(pick(args.component))
    return 0


def cmd_stop(args):
    svc_stop(pick(args.component))
    return 0


def cmd_restart(args):
    names = pick(args.component)
    svc_stop(names)
    time.sleep(1)
    svc_start(names)
    if "dispatcher" in names:
        for _ in range(10):
            time.sleep(1)
            try:
                print("dispatcher back, pid %s" % api("/api/health", timeout=2).get("pid"))
                return 0
            except Fail:
                pass
        print("dispatcher has not answered yet - check `python impctl.py status`")
        return 1
    return 0


def cmd_run(args):
    cmd = [sys.executable, "-m", "imperatorium.dispatcher", "--host", args.host,
           "--port", str(args.port)]
    for r in args.root or ():
        cmd += ["--root", r]
    print("dispatcher in the foreground on %s:%d - ctrl+c stops it. Stop the "
          "service first (`python impctl.py stop dispatcher`) or the port is taken."
          % (args.host, args.port))
    return subprocess.call(cmd, cwd=HERE)


def confirmed(what, rest):
    """Uninstalls take the service and hooks away without asking, so ask here.
    Without a terminal to ask in, only --yes proceeds."""
    if "--yes" in rest or "-y" in rest:
        return True
    if not sys.stdin.isatty():
        print("impctl: %s needs --yes when there is no terminal to confirm in" % what,
              file=sys.stderr)
        return False
    try:
        return input("%s? [y/N] " % what).strip().lower() in ("y", "yes")
    except EOFError:
        print()
        return False


def passthrough(script, *head, confirm=None):
    def run(args):
        rest = [a for a in args.rest if a != "--"]
        if confirm and not confirmed(confirm, rest):
            return 1
        return subprocess.call([sys.executable, os.path.join(HERE, script)]
                               + list(head) + rest, cwd=HERE)
    return run


def cmd_service(args):
    if args.action == "uninstall" and not confirmed(
            "Remove the start-at-logon service", ["--yes"] if args.yes else []):
        return 1
    extra = ["--no-streamdeck"] if args.no_streamdeck else []
    return subprocess.call([sys.executable, os.path.join(HERE, "setup_machine.py"),
                            "service", args.action] + extra, cwd=HERE)


# -- the window ----------------------------------------------------------------
def cmd_open(args):
    sys.path.insert(0, HERE)
    import hotkey
    print(hotkey.raise_or_open())
    return 0


# -- sessions --------------------------------------------------------------------
def cmd_sessions(args):
    rows = api("/api/sessions").get("sessions") or []
    if args.json:
        print(json.dumps(rows, indent=2))
        return 0
    if not rows:
        print("no live sessions")
        return 0
    print("%-8s %-28s %-10s %-6s %s" % ("id", "name", "status", "since", "folder"))
    for r in rows:
        name = r.get("name") or ""
        if r.get("host"):
            name = "%s:%s" % (r["host"], name)
        flag = " *" if r.get("attention") else ""
        print("%-8s %-28s %-10s %-6s %s%s" % (
            (sid(r) or "")[:8], name[:28],
            "bg:%d" % r["background_tasks"] if r.get("background_tasks")
            else r.get("status") or r.get("state") or "",
            ago(r.get("status_changed_at")), r.get("cwd") or "", flag))
    return 0


def cmd_projects(args):
    rows = api("/api/projects").get("projects") or []
    if args.json:
        print(json.dumps(rows, indent=2))
        return 0
    for p in rows:
        print("%-30s %s" % ((p.get("name") or "")[:30], p.get("path") or ""))
    return 0


def cmd_send(args):
    prompt = " ".join(args.prompt)
    if bool(args.session) == bool(args.project):
        raise Fail("name exactly one of --session or --project")
    body = {"prompt": prompt, "mode": "dry-run" if args.dry_run else args.mode}
    if args.session:
        body["session_id"] = sid(find_session(args.session))
    else:
        body["project"] = args.project
    def said(o):
        target = o.get("session") or o.get("project") or o.get("url") or ""
        if isinstance(target, dict):
            target = target.get("path") or target.get("name") or ""
        return "%s -> %s" % (o.get("action") or "sent", target) if target else             (o.get("action") or "sent")
    return result(api("/api/dispatch", body), args, said)


def cmd_queue(args):
    if args.action == "list":
        queued = api("/api/queue").get("queue") or {}
        if args.json:
            print(json.dumps(queued, indent=2))
        elif not queued:
            print("nothing queued")
        for key, items in queued.items():
            print(key)
            for i in items:
                print("   %s  %s" % (i.get("id"), (i.get("prompt") or "")[:100]))
        return 0
    if not args.session:
        raise Fail("queue %s needs a session" % args.action)
    row = find_session(args.session)
    if args.action == "add":
        if not args.text:
            raise Fail("queue add needs the prompt text")
        return result(api("/api/queue", {"session_id": sid(row), "prompt": " ".join(args.text)}),
                      args, lambda o: "delivered now - it was idle" if not o.get("waiting")
                      else "queued (%s waiting)" % o["waiting"])
    if not args.text:
        raise Fail("queue remove needs the item id (see `python impctl.py queue list`)")
    return result(api("/api/unqueue", {"session": row.get("key") or sid(row), "id": args.text[0]}),
                  args, lambda o: "removed")


def cmd_focus(args):
    if not args.project and not args.session:
        raise Fail("name a session or --project")
    body = {"project": args.project} if args.project else {"session_id": sid(find_session(args.session))}
    return result(api("/api/focus", body), args, lambda o: o.get("action") or "focused")


def launch(kind):
    def run(args):
        row = (find_session if kind == "new" else find_any_session)(args.session)
        if kind == "new":
            return result(api("/api/new", {"session_id": sid(row), "cwd": row.get("cwd")}),
                          args, lambda o: "opened a new session in %s" % row.get("cwd"))
        where = "background" if args.bg else "window"
        return result(api("/api/resume", {"session_id": sid(row), "where": where,
                                          "fork": kind == "fork", "prompt": args.prompt,
                                          "cwd": row.get("cwd")}),
                      args, lambda o: "%s: %s" % (o.get("where"), o.get("command")))
    return run


def cmd_history(args):
    got = api("/api/history?q=" + q(" ".join(args.query)), timeout=60)
    if args.json or not got.get("ok", True):
        print(json.dumps(got, indent=2))
        return 0 if got.get("ok", True) else 1
    for r in got.get("results") or []:
        print("%-8s %-5s %-44s %s" % (r["session_id"][:8], "live" if r.get("live") else "",
                                     (r.get("title") or r.get("first_prompt") or "")[:44],
                                     r.get("cwd") or ""))
    print("(resume one with `python impctl.py resume ID`)" if got.get("results") else "no matches")
    return 0


def cmd_pending(args):
    items = api("/api/pending").get("pending") or []
    if args.json:
        print(json.dumps(items, indent=2))
    elif not items:
        print("no permission requests waiting")
    for i in items:
        print("%s  %s  %s" % (i.get("id"), ago(i.get("at")),
                              i.get("summary") or i.get("tool") or json.dumps(i)[:100]))
    return 0


def decide(verdict):
    def run(args):
        out = api("/api/decide", {"id": args.id, "verdict": verdict, "who": "cli"})
        return result(out, args, lambda o: "%s %s" % ("allowed" if verdict == "allow"
                                                      else "denied", args.id))
    return run


def cmd_notify(args):
    if args.channel:
        if args.state is None:
            raise Fail("notify %s needs on or off" % args.channel)
        return result(api("/api/notify", {"channel": args.channel,
                                          "enabled": args.state == "on"}),
                      args, lambda o: "%s %s" % (args.channel, args.state))
    print(json.dumps(api("/api/notify"), indent=2))
    return 0


def get_json(path_fn, needs_session=False):
    """GET commands whose answer is best read as JSON."""
    def run(args):
        if needs_session:
            row = find_session(args.session)
            path = path_fn(args, row)
        else:
            path = path_fn(args)
        print(json.dumps(api(path, timeout=60), indent=2))
        return 0
    return run


def cmd_events(args):
    events = (api("/api/events").get("events") or [])[::-1]  # served newest first
    events = events[-args.n:] if args.n else events
    if args.json:
        print(json.dumps(events, indent=2))
        return 0
    for e in events:
        at = e.get("at") or e.get("ts")
        stamp = time.strftime("%m-%d %H:%M", time.localtime(float(at))) if at else ""
        print("%-11s %-14s %s" % (stamp, e.get("kind") or "",
                                  (e.get("summary") or e.get("project") or e.get("cwd") or "")[:90]))
    return 0


def cmd_test(args):
    import glob
    names = args.suite or [os.path.basename(p)[5:-3] for p in
                           sorted(glob.glob(os.path.join(HERE, "test_*.py")))]
    venv = os.path.join(HERE, ".venv", "Scripts" if WINDOWS else "bin",
                        "python.exe" if WINDOWS else "python")
    # The venv, when there is one: some suites need the Agent SDK in it.
    py = venv if os.path.exists(venv) else sys.executable
    failed = []
    for n in names:
        path = os.path.join(HERE, "test_%s.py" % n)
        if not os.path.exists(path):
            raise Fail("no suite test_%s.py" % n)
        got = subprocess.run([py, path], cwd=HERE, capture_output=True, text=True)
        tail = [line for line in (got.stdout + got.stderr).splitlines()
                if line.startswith(("Ran ", "OK", "FAILED"))]
        print("%-14s %s" % (n, "  ".join(tail[-2:]) or "exit %d" % got.returncode))
        if got.returncode:
            failed.append(n)
    if failed:
        print("failed: %s (run one with `python test_NAME.py` for the detail)" % ", ".join(failed))
    return 1 if failed else 0


# -- the parser --------------------------------------------------------------------
# (group, command) in the order `help` lists them.
SECTIONS = [
    ("Service", ["status", "start", "stop", "restart", "run", "service"]),
    ("Window", ["open"]),
    ("Sessions", ["sessions", "projects", "send", "queue", "focus", "new", "resume",
                  "fork", "history", "changes", "activity"]),
    ("Approvals", ["pending", "allow", "deny"]),
    ("Monitoring", ["events", "usage", "errors", "hosts", "notify", "health"]),
    ("Setup", ["install", "check", "uninstall", "remote", "hooks", "probe", "test"]),
    ("Help", ["help"]),
]


def build():
    ap = argparse.ArgumentParser(prog="impctl", add_help=False,
                                 description="Imperatorium command line. "
                                             "`python impctl.py help` lists every command.")
    ap.add_argument("-h", "--help", action="store_true")
    sub = ap.add_subparsers(dest="command")
    cmds = {}

    def add(name, fn, text, json_flag=True, **kw):
        p = sub.add_parser(name, help=text, description=text, **kw)
        if json_flag:
            p.add_argument("--json", action="store_true", help="print the raw JSON answer")
        p.set_defaults(fn=fn)
        cmds[name] = p
        return p

    comp = dict(nargs="?", choices=["all"] + ORDER, default="all",
                help="which part (default: all)")

    add("status", cmd_status, "Show each service component and whether the dispatcher answers")
    add("start", cmd_start, "Start the service (all, or one component)").add_argument("component", **comp)
    add("stop", cmd_stop,
        "Stop the service (all, or one component). It stays down until `start`: "
        "on Windows the task is disabled so its heartbeat does not revive it"
        ).add_argument("component", **comp)
    add("restart", cmd_restart, "Stop then start; waits for the dispatcher to answer"
        ).add_argument("component", **comp)
    p = add("run", cmd_run, "Run the dispatcher in this terminal, for debugging")
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=8792)
    p.add_argument("--root", action="append", help="a folder of projects (repeatable)")
    p = add("service", cmd_service, "Register or remove the start-at-logon service")
    p.add_argument("action", choices=["install", "uninstall"])
    p.add_argument("--no-streamdeck", action="store_true", help="install: skip the Stream Deck")
    p.add_argument("-y", "--yes", action="store_true", help="uninstall: do not ask")

    add("open", cmd_open, "Raise the Imperatorium window, or open it")

    add("sessions", cmd_sessions, "List live sessions (* = wants attention)")
    add("projects", cmd_projects, "List the projects the dispatcher knows")
    p = add("send", cmd_send, "Send a prompt to a session, or route it to a project")
    p.add_argument("prompt", nargs="+", help="the prompt text")
    p.add_argument("-s", "--session", help="session id, id prefix, or name")
    p.add_argument("-p", "--project", help="project name (launches one if none is running)")
    p.add_argument("--mode", default="auto",
                   choices=["auto", "inject", "launch", "background"],
                   help="how to deliver it (default: auto)")
    p.add_argument("--dry-run", action="store_true", help="say what would happen, do nothing")
    p = add("queue", cmd_queue, "Prompts held until a session's turn ends")
    p.add_argument("action", choices=["list", "add", "remove"])
    p.add_argument("session", nargs="?", help="add/remove: session id, prefix or name")
    p.add_argument("text", nargs="*", help="add: the prompt;  remove: the item id")
    p = add("focus", cmd_focus, "Bring a session's terminal to the front")
    p.add_argument("session", nargs="?", help="session id, prefix or name")
    p.add_argument("-p", "--project", help="a project instead; opens one if none runs")
    add("new", launch("new"), "Start a fresh session in a session's project, in a new tab"
        ).add_argument("session", help="session id, prefix or name")
    for kind, text in (("resume", "Resume a session (a finished one; use fork for a live one)"),
                       ("fork", "Fork a session: a copy, the original untouched")):
        p = add(kind, launch(kind), text)
        p.add_argument("session", help="session id, prefix or name")
        p.add_argument("--bg", action="store_true", help="in the background, not a new tab")
        p.add_argument("--prompt", help="--bg: its first message")
    add("history", cmd_history, "Search past sessions").add_argument("query", nargs="+")
    add("changes", get_json(lambda a, r: "/api/changes?session=" + q(sid(r)), True),
        "Git status and diff for a session's folder").add_argument("session")
    add("activity", get_json(lambda a, r: "/api/activity?session=" + q(sid(r)), True),
        "A session's recent tool calls").add_argument("session")

    add("pending", cmd_pending, "Permission requests waiting for an answer")
    add("allow", decide("allow"), "Approve a pending request").add_argument("id")
    add("deny", decide("deny"), "Deny a pending request").add_argument("id")

    p = add("events", cmd_events, "The event log, newest last")
    p.add_argument("-n", type=int, default=30, help="how many (default 30, 0 = all)")
    add("usage", get_json(lambda a: "/api/usage"), "Context and rate-limit usage")
    add("errors", get_json(lambda a: "/api/errors?days=%s" % a.days),
        "Tool-call failure tallies").add_argument("--days", type=float, default=7)
    add("hosts", get_json(lambda a: "/api/hosts"), "Remote hosts and their state")
    p = add("notify", cmd_notify, "Show notification channels, or turn one on/off")
    p.add_argument("channel", nargs="?", help="e.g. ntfy or speech")
    p.add_argument("state", nargs="?", choices=["on", "off"])
    add("health", get_json(lambda a: "/api/health"), "Is the dispatcher up (and its pid)")

    confirm = {"uninstall": "Remove Imperatorium's hooks and service from this machine"}
    for name, script, head, text in (
            ("install", "setup_machine.py", ["install"],
             "Install on this machine (setup_machine.py install; its flags pass through)"),
            ("check", "setup_machine.py", ["check"], "Check prerequisites and what is installed"),
            ("uninstall", "setup_machine.py", ["uninstall"],
             "Remove hooks and service; asks first unless --yes (config and state are kept)"),
            ("remote", "setup_machine.py", ["remote"],
             "Other machines: remote add|remove|check [HOST] [--label ..] [--purge] ..."),
            ("hooks", os.path.join("hooks", "install.py"), [],
             "Claude Code hooks: [--uninstall] [--dry-run] [--remote HOST] ..."),
            ("probe", os.path.join("imperatorium", "remote.py"), [],
             "Poll remote hosts once and print their sessions: [HOST ...]")):
        p = add(name, passthrough(script, *head, confirm=confirm.get(name)), text,
                json_flag=False, add_help=False)
        p.set_defaults(passthrough=True)
        p.add_argument("rest", nargs=argparse.REMAINDER,
                       help="passed through; `python %s %s-h` lists them"
                       % (script, " ".join(head) + " " if head else ""))
    add("test", cmd_test, "Run the test suites (all, or the ones named)"
        ).add_argument("suite", nargs="*", help="e.g. core features hotkey")
    add("help", None, "This list, or one command in full").add_argument("topic", nargs="?")
    return ap, cmds


def usage_line(p):
    return p.format_usage().replace("usage: ", "").strip().replace("impctl ", "python impctl.py ", 1)


def show_help(cmds, topic=None):
    if topic:
        if topic not in cmds:
            raise Fail("no command %r - `python impctl.py help` lists them" % topic)
        cmds[topic].print_help()
        return 0
    print(__doc__.split("\n\n")[0].strip())
    print("\nUsage: python impctl.py COMMAND [ARGS]    (every command also takes --json)")
    for title, names in SECTIONS:
        print("\n%s" % title)
        for n in names:
            p = cmds[n]
            line = usage_line(p).replace(" [-h]", "").replace(" [--json]", "")
            print("  %s" % " ".join(line.split()).replace(" rest ...", " ..."))
            print("      %s" % p.description)
    print("\nSessions can be named by id, id prefix, or name. `python impctl.py help COMMAND` "
          "shows one in full.\nSet IMPERATORIUM_URL to reach a dispatcher other than %s." % BASE)
    return 0


def main(argv=None):
    ap, cmds = build()
    # The setup commands hand their flags to another script, and argparse will
    # not put a leading "--flag" into a REMAINDER - so those are collected here.
    # Splitting them between REMAINDER and the unknowns loses their order, so
    # a pass-through command gets everything after its name, verbatim.
    argv = sys.argv[1:] if argv is None else list(argv)
    args, extra = ap.parse_known_args(argv)
    if getattr(args, "passthrough", False):
        args.rest = argv[argv.index(args.command) + 1:]
    elif extra:
        ap.error("unrecognized arguments: %s" % " ".join(extra))
    try:
        if args.help or not args.command:
            return show_help(cmds)
        if args.command == "help":
            return show_help(cmds, args.topic)
        return args.fn(args) or 0
    except Fail as exc:
        print("impctl: %s" % exc, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
