"""Set up Imperatorium on this machine, from a fresh clone.

    git clone https://github.com/hypertetrahedron/imperatorium.git
    cd imperatorium
    python setup_machine.py              # asks a few questions, then installs
    python setup_machine.py --yes        # the defaults, no questions
    python setup_machine.py check        # what is present and missing; changes nothing
    python setup_machine.py uninstall    # removes hooks and the service, keeps files

    python setup_machine.py remote add HOST      # onboard another machine over SSH
    python setup_machine.py remote check         # every configured host, step by step
    python setup_machine.py remote remove HOST   # its hooks and its config entry
    python setup_machine.py service status       # install | uninstall | restart | status

What `install` does, in order, and each step is safe to repeat:

 1. checks prerequisites - Python 3.9+, Claude Code, git (gh, Windows
    Terminal and Edge are optional and only switch features on);
 2. makes `.venv` and installs requirements.txt, plus the voice and Stream
    Deck extras if you want them;
 3. writes config.json and policy.json from the *.example.json files - only
    if they do not exist yet, so your own settings are never overwritten;
 4. on Windows with the Stream Deck, downloads hidapi.dll from the official
    libusb/hidapi release and checks it against a pinned SHA-256;
 5. adds the Claude Code hooks (and optionally the statusline and the tool
    hooks) to ~/.claude/settings.json, backing that file up first;
 6. keeps the dispatcher running: scheduled tasks at logon on Windows, a
    systemd user unit on Linux, a launchd agent on macOS;
 7. waits for the dispatcher to answer and tells you where the window is.

`remote add HOST` does the same for a machine you reach over SSH: sets up key
login if it is missing, checks (and optionally installs) Claude Code and
python3 there, installs the conversation reader and the reporting hooks, adds
the host to config.json with a tunnel, and restarts the dispatcher.

Imperatorium is the public name; the code calls itself `ccontrol` (Central
Control), and so do the task, unit and file names it installs.

Standard library only, because it runs before anything else is installed.
"""
import argparse
import hashlib
import io
import json
import os
import re
import secrets
import shlex
import shutil
import subprocess
import sys
import time
import urllib.request
import zipfile
from xml.sax.saxutils import escape as xml_escape

HERE = os.path.dirname(os.path.abspath(__file__))
WINDOWS = sys.platform == "win32"
VENV = os.path.join(HERE, ".venv")
VENV_PY = os.path.join(VENV, "Scripts" if WINDOWS else "bin",
                       "python.exe" if WINDOWS else "python")
STATE_DIR_HINT = ("%LOCALAPPDATA%\\ccontrol" if WINDOWS
                  else "$XDG_STATE_HOME/ccontrol or ~/.local/state/ccontrol")
HEALTH = "http://127.0.0.1:8792/api/health"
WINDOW_URL = "http://127.0.0.1:8792/"

MIN_PYTHON = (3, 9)
# When a session's inbox became reachable - what dispatch depends on.
MIN_CLAUDE = (2, 1, 234) if WINDOWS else (2, 1, 224)

# The official Windows build, pinned. vendor/hidapi.dll on the machine this
# was written on is byte-identical to x64/hidapi.dll in this archive.
HIDAPI_URL = ("https://github.com/libusb/hidapi/releases/download/"
              "hidapi-0.15.0/hidapi-win.zip")
HIDAPI_ZIP_SHA256 = "d18c43ec9506a2f6d7faa9c7e0a342c4b64fbae521b71b5d4ac0777fd24dda93"
HIDAPI_DLL_SHA256 = "d4c05ba2138cb5259a7f796464b5eaa63b9f8f67bb5cb003f96989244ee01583"
HIDAPI_MEMBER = "x64/hidapi.dll"


# -- small helpers -----------------------------------------------------------
def say(msg=""):
    print(msg, flush=True)


def step(title):
    say("\n== %s" % title)


def run(cmd, cwd=None, check=True, capture=False, timeout=None):
    """Run a command, echoing it. Returns CompletedProcess."""
    say("   $ %s" % " ".join('"%s"' % c if " " in str(c) else str(c) for c in cmd))
    return subprocess.run(cmd, cwd=cwd or HERE, check=check, text=True,
                          capture_output=capture, timeout=timeout)


def output(cmd, timeout=20):
    """stdout of a command, or None if it cannot be run."""
    try:
        done = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if done.returncode != 0:
        return None
    return (done.stdout or "").strip()


def ask(question, default, assume_yes):
    """A yes/no question. `assume_yes` takes the default without asking."""
    if assume_yes or not sys.stdin.isatty():
        return default
    hint = "Y/n" if default else "y/N"
    while True:
        got = input("   %s [%s] " % (question, hint)).strip().lower()
        if not got:
            return default
        if got in ("y", "yes"):
            return True
        if got in ("n", "no"):
            return False


def ask_text(question, default, assume_yes):
    if assume_yes or not sys.stdin.isatty():
        return default
    got = input("   %s [%s] " % (question, default)).strip()
    return got or default


# -- prerequisites -------------------------------------------------------------
def parse_version(text):
    """(2, 1, 282) from '2.1.282 (Claude Code)'. None if there is no version."""
    m = re.search(r"(\d+)\.(\d+)\.(\d+)", text or "")
    return tuple(int(x) for x in m.groups()) if m else None


def edge_path():
    if not WINDOWS:
        return shutil.which("microsoft-edge") or shutil.which("msedge")
    for base in (os.environ.get("ProgramFiles(x86)"), os.environ.get("ProgramFiles")):
        if base:
            p = os.path.join(base, "Microsoft", "Edge", "Application", "msedge.exe")
            if os.path.exists(p):
                return p
    return shutil.which("msedge")


def check_prereqs():
    """[(name, ok, detail, required)] - everything install cares about."""
    out = []
    py = sys.version_info[:3]
    out.append(("Python %d.%d+" % MIN_PYTHON, py >= MIN_PYTHON,
                "%d.%d.%d at %s" % (py + (sys.executable,)), True))

    claude = shutil.which("claude")
    ver = parse_version(output(["claude", "--version"]) or "") if claude else None
    out.append(("Claude Code %s+" % ".".join(map(str, MIN_CLAUDE)),
                bool(ver and ver >= MIN_CLAUDE),
                ("%s at %s" % (".".join(map(str, ver)), claude)) if ver else
                ("not on PATH - install it from https://claude.com/claude-code"
                 if not claude else "could not read its version"), True))

    git = shutil.which("git")
    out.append(("git", bool(git), git or "not on PATH", True))

    gh = shutil.which("gh")
    out.append(("gh (GitHub CLI)", bool(gh),
                gh or "optional - PR badges and 'open PR' need it", False))
    if WINDOWS:
        wt = shutil.which("wt")
        out.append(("Windows Terminal (wt)", bool(wt),
                    wt or "optional - Resume and Fork open a tab in it", False))
    edge = edge_path()
    out.append(("Microsoft Edge", bool(edge),
                edge or "optional - opens the window without browser chrome", False))
    uv = shutil.which("uv")
    out.append(("uv", bool(uv), uv or "optional - makes the venv faster to build", False))
    return out


def report(checks):
    ok = True
    for name, good, detail, required in checks:
        mark = "ok " if good else ("MISSING" if required else "-- ")
        say("   %-8s %-28s %s" % (mark, name, detail))
        ok = ok and (good or not required)
    return ok


# -- venv ------------------------------------------------------------------------
def make_venv(extras):
    uv = shutil.which("uv")
    if not os.path.exists(VENV_PY):
        if uv:
            run([uv, "venv", VENV, "--python", sys.executable])
        else:
            run([sys.executable, "-m", "venv", VENV])
    else:
        say("   .venv already exists")
    files = ["requirements.txt"] + ["requirements-%s.txt" % e for e in extras]
    args = []
    for f in files:
        args += ["-r", os.path.join(HERE, f)]
    if uv:
        run([uv, "pip", "install", "--python", VENV_PY] + args)
    else:
        run([VENV_PY, "-m", "pip", "install", "--upgrade", "pip"])
        run([VENV_PY, "-m", "pip", "install"] + args)


# -- config ----------------------------------------------------------------------
# Where people commonly keep repositories, tried in order when this checkout
# sits directly in the home folder (every folder under home is not a project).
COMMON_ROOTS = ("code", "src", "projects", "dev", "repos", "git",
                os.path.join("source", "repos"))


def default_roots(here=HERE, home=None):
    """The folder this checkout sits in: repositories usually live side by side.

    A checkout cloned straight into the home folder is the exception - the
    bootstrap scripts do that - so there a conventional code folder is
    preferred when one exists.
    """
    parent = os.path.dirname(here)
    home = home or os.path.expanduser("~")
    if os.path.normcase(os.path.normpath(parent)) == os.path.normcase(os.path.normpath(home)):
        for name in COMMON_ROOTS:
            if os.path.isdir(os.path.join(home, name)):
                return [os.path.join(home, name)]
    return [parent]


def ntfy_topic():
    """An unguessable ntfy topic: on ntfy.sh the topic name is the only secret."""
    return "imperatorium-" + secrets.token_urlsafe(12)


def build_config(example, roots, voice, streamdeck_keys=None, ntfy=None):
    """config.json content from the example. Pure, so it is tested.

    `ntfy` is a topic name to switch phone pushes on with, or None.
    """
    cfg = json.loads(json.dumps(example))
    cfg["roots"] = list(roots)
    cfg.setdefault("voice", {})["enabled"] = bool(voice)
    if streamdeck_keys is not None:
        cfg.setdefault("streamdeck", {})["keys"] = list(streamdeck_keys)
    if ntfy:
        push = cfg.setdefault("notify", {}).setdefault("ntfy", {})
        push["topic"] = ntfy
        push["enabled"] = True
    return cfg


def build_policy(example, auto_approve):
    pol = json.loads(json.dumps(example))
    pol["enabled"] = bool(auto_approve)
    return pol


def write_if_absent(name, content):
    path = os.path.join(HERE, name)
    if os.path.exists(path):
        say("   %s exists - left as it is" % name)
        return False
    with open(path, "w", encoding="utf-8", newline="") as fh:
        fh.write(json.dumps(content, indent=2) + "\n")
    say("   wrote %s" % name)
    return True


def load_example(name):
    with open(os.path.join(HERE, name), encoding="utf-8") as fh:
        return json.load(fh)


def load_config():
    """This machine's config.json, or None if setup has not written one yet."""
    try:
        with open(os.path.join(HERE, "config.json"), encoding="utf-8") as fh:
            return json.load(fh)
    except FileNotFoundError:
        return None


def save_config(cfg):
    """Write config.json atomically, keeping a copy of what it replaced."""
    path = os.path.join(HERE, "config.json")
    if os.path.exists(path):
        shutil.copy2(path, path + ".bak")
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8", newline="") as fh:
        fh.write(json.dumps(cfg, indent=2) + "\n")
    os.replace(tmp, path)
    say("   updated config.json (previous copy: config.json.bak)")


# -- hidapi ------------------------------------------------------------------------
def extract_hidapi(archive_bytes):
    """The verified x64 DLL from the release zip. Raises ValueError on a mismatch."""
    got = hashlib.sha256(archive_bytes).hexdigest()
    if got != HIDAPI_ZIP_SHA256:
        raise ValueError("hidapi-win.zip SHA-256 is %s, expected %s" % (got, HIDAPI_ZIP_SHA256))
    with zipfile.ZipFile(io.BytesIO(archive_bytes)) as zf:
        dll = zf.read(HIDAPI_MEMBER)
    got = hashlib.sha256(dll).hexdigest()
    if got != HIDAPI_DLL_SHA256:
        raise ValueError("hidapi.dll SHA-256 is %s, expected %s" % (got, HIDAPI_DLL_SHA256))
    return dll


def fetch_hidapi():
    target = os.path.join(HERE, "vendor", "hidapi.dll")
    if os.path.exists(target):
        with open(target, "rb") as fh:
            if hashlib.sha256(fh.read()).hexdigest() == HIDAPI_DLL_SHA256:
                say("   vendor/hidapi.dll already present and verified")
                return
    say("   downloading %s" % HIDAPI_URL)
    with urllib.request.urlopen(HIDAPI_URL, timeout=60) as resp:
        data = resp.read()
    dll = extract_hidapi(data)
    os.makedirs(os.path.dirname(target), exist_ok=True)
    with open(target, "wb") as fh:
        fh.write(dll)
    say("   wrote vendor/hidapi.dll (SHA-256 verified)")


# -- hooks and service -------------------------------------------------------------
def install_hooks(statusline, tool_hooks, uninstall=False):
    cmd = [sys.executable, os.path.join(HERE, "hooks", "install.py")]
    if uninstall:
        cmd.append("--uninstall")
    else:
        if statusline:
            cmd.append("--statusline")
        if tool_hooks:
            cmd.append("--tool-hooks")
    run(cmd)


def pythonw():
    """pythonw.exe beside the interpreter running this, for the scheduled tasks."""
    base = os.path.dirname(sys.executable)
    for name in ("pythonw.exe", "python.exe"):
        p = os.path.join(base, name)
        if os.path.exists(p):
            return p
    return sys.executable


def tasks(action, streamdeck=True):
    cmd = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
           os.path.join(HERE, "service", "tasks.ps1"), "-Action", action,
           "-PythonW", pythonw()]
    if not streamdeck:
        cmd.append("-NoStreamDeck")
    run(cmd, check=False)


# Linux and macOS: the same job the Windows tasks do - start at login, come
# back after a crash - done the way each platform expects. The hotkey is
# Windows-only, so these carry the dispatcher and, optionally, the Stream Deck.
MACOS = sys.platform == "darwin"
UNITS = (("dispatcher", ["-m", "ccontrol.dispatcher"], "the dispatcher and window"),
         ("streamdeck", ["streamdeck_surface.py"], "the Stream Deck surface"))


def service_python():
    return VENV_PY if os.path.exists(VENV_PY) else sys.executable


def systemd_unit(python, args, what, workdir=HERE, path=None):
    """A systemd --user unit. Pure, so it is tested."""
    quoted = " ".join('"%s"' % a.replace('"', '\\"') for a in [python] + list(args))
    env = ('Environment="PATH=%s"\n' % path) if path else ""
    return ("[Unit]\n"
            "Description=Imperatorium - %s\n"
            "After=network-online.target\n\n"
            "[Service]\n"
            "Type=simple\n"
            "WorkingDirectory=%s\n"
            "%s"
            "ExecStart=%s\n"
            "Restart=always\n"
            "RestartSec=5\n\n"
            "[Install]\n"
            "WantedBy=default.target\n") % (what, workdir, env, quoted)


def launchd_plist(label, python, args, workdir=HERE, path=None, log=None):
    """A launchd agent. Pure, so it is tested."""
    items = "".join("<string>%s</string>" % xml_escape(a) for a in [python] + list(args))
    env = ("<key>EnvironmentVariables</key><dict><key>PATH</key><string>%s</string></dict>"
           % xml_escape(path)) if path else ""
    logs = ("<key>StandardOutPath</key><string>%s</string>"
            "<key>StandardErrorPath</key><string>%s</string>"
            % (xml_escape(log), xml_escape(log))) if log else ""
    return ('<?xml version="1.0" encoding="UTF-8"?>\n'
            '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
            '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n'
            '<plist version="1.0"><dict>'
            "<key>Label</key><string>%s</string>"
            "<key>ProgramArguments</key><array>%s</array>"
            "<key>WorkingDirectory</key><string>%s</string>"
            "%s%s"
            "<key>RunAtLoad</key><true/>"
            "<key>KeepAlive</key><true/>"
            "</dict></plist>\n") % (xml_escape(label), items, xml_escape(workdir), env, logs)


def systemd_dir():
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.join(os.path.expanduser("~"), ".config")
    return os.path.join(base, "systemd", "user")


def launchd_dir():
    return os.path.join(os.path.expanduser("~"), "Library", "LaunchAgents")


def unit_names():
    """[(key, unit file path, service name)] for this platform."""
    out = []
    for key, _args, _what in UNITS:
        if MACOS:
            label = "local.ccontrol.%s" % key
            out.append((key, os.path.join(launchd_dir(), label + ".plist"), label))
        else:
            name = "ccontrol-%s.service" % key
            out.append((key, os.path.join(systemd_dir(), name), name))
    return out


def service_install(streamdeck=False):
    if WINDOWS:
        tasks("install", streamdeck=streamdeck)
        return
    python = service_python()
    # A user service starts with a bare PATH, and the dispatcher calls
    # `claude`, `git` and `gh` - so it gets the PATH setup was run with.
    path = os.environ.get("PATH", "")
    specs = {key: (args, what) for key, args, what in UNITS}
    for key, unit_path, name in unit_names():
        if key == "streamdeck" and not streamdeck:
            continue
        args, what = specs[key]
        os.makedirs(os.path.dirname(unit_path), exist_ok=True)
        if MACOS:
            log = os.path.join(os.path.expanduser("~"), "Library", "Logs", "ccontrol-%s.log" % key)
            body = launchd_plist(name, python, args, path=path, log=log)
        else:
            body = systemd_unit(python, args, what, path=path)
        with open(unit_path, "w", encoding="utf-8") as fh:
            fh.write(body)
        say("   wrote %s" % unit_path)
        if MACOS:
            domain = "gui/%d" % os.getuid()
            run(["launchctl", "bootout", domain, unit_path], check=False, capture=True)
            run(["launchctl", "bootstrap", domain, unit_path], check=False)
        else:
            run(["systemctl", "--user", "daemon-reload"], check=False)
            run(["systemctl", "--user", "enable", "--now", name], check=False)
            run(["systemctl", "--user", "restart", name], check=False)
    if not MACOS and shutil.which("loginctl"):
        say("   a user unit stops when you log out; to keep it running without a login:")
        say("     loginctl enable-linger $USER")


def service_uninstall():
    if WINDOWS:
        tasks("uninstall")
        return
    for _key, unit_path, name in unit_names():
        if not os.path.exists(unit_path):
            continue
        if MACOS:
            run(["launchctl", "bootout", "gui/%d" % os.getuid(), unit_path], check=False)
        else:
            run(["systemctl", "--user", "disable", "--now", name], check=False)
        os.remove(unit_path)
        say("   removed %s" % unit_path)
    if not MACOS:
        run(["systemctl", "--user", "daemon-reload"], check=False)


def service_installed():
    """Whether anything that keeps the dispatcher up is registered here."""
    if WINDOWS:
        return output(["schtasks", "/Query", "/TN", "CentralControlDispatcher"]) is not None
    return any(os.path.exists(p) for key, p, _n in unit_names() if key == "dispatcher")


def service_restart():
    if WINDOWS:
        tasks("restart")
        return
    for _key, unit_path, name in unit_names():
        if not os.path.exists(unit_path):
            continue
        if MACOS:
            run(["launchctl", "kickstart", "-k", "gui/%d/%s" % (os.getuid(), name)], check=False)
        else:
            run(["systemctl", "--user", "restart", name], check=False)


def service_status():
    if WINDOWS:
        tasks("status")
        return
    for _key, unit_path, name in unit_names():
        if not os.path.exists(unit_path):
            say("   %-32s not installed" % name)
        elif MACOS:
            say("   %-32s %s" % (name, "loaded" if output(["launchctl", "list", name]) else "not loaded"))
        else:
            say("   %-32s %s" % (name, output(["systemctl", "--user", "is-active", name]) or "inactive"))
    health = wait_for_dispatcher(3)
    say("   dispatcher %s" % ("responding, pid %s" % health.get("pid") if health
                              else "NOT responding on 127.0.0.1:8792"))


def wait_for_dispatcher(seconds=60):
    deadline = time.time() + seconds
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(HEALTH, timeout=2) as resp:
                return json.loads(resp.read().decode())
        except Exception:
            time.sleep(2)
    return None


# -- other machines ------------------------------------------------------------------
# A host is onboarded entirely over SSH: nothing listens there and no port is
# opened on either machine. See README "Other machines" for why each piece exists.
MIN_REMOTE_CLAUDE = (2, 1, 224)
READER_DIR = ".claude/ccontrol/venv"
READER_REQUIREMENT = "claude-agent-sdk>=0.2.155"
CLAUDE_INSTALL = "curl -fsSL https://claude.ai/install.sh | bash"
SSH_OPTS = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=8",
            "-o", "StrictHostKeyChecking=accept-new"]

# One round trip that answers everything `remote add` needs to know. `claude`
# is looked for by absolute path because a non-interactive SSH session on
# Ubuntu never reaches the PATH line in .bashrc.
PROBE = r'''printf 'home=%s\n' "$HOME"
for c in "$(command -v claude 2>/dev/null)" "$HOME/.local/bin/claude" "$HOME/.claude/local/claude" /usr/local/bin/claude /opt/homebrew/bin/claude; do
  if [ -n "$c" ] && [ -x "$c" ]; then printf 'claude=%s\n' "$c"; printf 'version=%s\n' "$("$c" --version 2>/dev/null | head -n 1)"; break; fi
done
if command -v python3 >/dev/null 2>&1; then python3 -c 'import sys; print("python=%d.%d.%d" % sys.version_info[:3])'; fi
if [ -x "$HOME/.claude/ccontrol/venv/bin/python" ]; then "$HOME/.claude/ccontrol/venv/bin/python" -c 'import claude_agent_sdk; print("sdk=ok")' 2>/dev/null; fi
if [ -f "$HOME/.claude/ccontrol/cc_hook.py" ]; then echo hookscript=ok; fi
if [ -f "$HOME/.claude/settings.json" ] && grep -q ccontrol/cc_hook.py "$HOME/.claude/settings.json"; then echo hooks=ok; fi
true'''


def parse_probe(text):
    """{home, claude, version, python, sdk, hooks, hookscript} from PROBE's output."""
    out = {}
    for line in (text or "").splitlines():
        key, sep, value = line.partition("=")
        if sep and key in ("home", "claude", "version", "python", "sdk", "hooks", "hookscript"):
            out.setdefault(key, value.strip())
    out["version"] = parse_version(out.get("version")) if out.get("version") else None
    out["python"] = parse_version(out.get("python")) if out.get("python") else None
    return out


def home_relative(path, home):
    """`$HOME/.local/bin/claude` rather than `/home/me/...`, as config.json writes it."""
    if home and path and (path == home or path.startswith(home.rstrip("/") + "/")):
        return "$HOME" + path[len(home.rstrip("/")):]
    return path


def host_entry(name, claude, label=None, tunnel=True):
    """The config.json entry for a host. Pure, so it is tested."""
    entry = {"name": name, "claude": claude or "$HOME/.local/bin/claude"}
    if label and label != name:
        entry["label"] = label
    if tunnel:
        entry["tunnel"] = True
    return entry


def upsert_host(cfg, entry):
    """cfg with `entry` added, or replacing the host of the same name. Keeps any
    keys the existing entry had that this one does not set (python, ports...)."""
    cfg = json.loads(json.dumps(cfg))
    block = cfg.setdefault("remote", {})
    block["enabled"] = True
    hosts = block.setdefault("hosts", [])
    for i, old in enumerate(hosts):
        old = {"name": old} if isinstance(old, str) else old
        if old.get("name") == entry["name"]:
            merged = dict(old)
            merged.update(entry)
            if not entry.get("tunnel"):
                merged.pop("tunnel", None)
            hosts[i] = merged
            return cfg, "replaced"
    hosts.append(dict(entry))
    return cfg, "added"


def remove_host(cfg, name):
    cfg = json.loads(json.dumps(cfg))
    hosts = (cfg.get("remote") or {}).get("hosts") or []
    kept = [h for h in hosts if (h if isinstance(h, str) else h.get("name")) != name]
    if cfg.get("remote") is not None:
        cfg["remote"]["hosts"] = kept
    return cfg, len(kept) != len(hosts)


def ssh(host, command, data=None, timeout=60, batch=True):
    """(returncode, stdout, stderr). Never raises for a host that is down."""
    argv = ["ssh"] + (SSH_OPTS if batch else ["-o", "StrictHostKeyChecking=accept-new"]) + [host, command]
    try:
        done = subprocess.run(argv, input=data, capture_output=True, text=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired) as exc:
        return 255, "", str(exc)
    return done.returncode, done.stdout or "", (done.stderr or "").strip()


def probe(host):
    rc, out, err = ssh(host, PROBE, timeout=30)
    if rc != 0:
        return None, err or "ssh exited %d" % rc
    return parse_probe(out), None


def public_key():
    ssh_dir = os.path.join(os.path.expanduser("~"), ".ssh")
    for name in ("id_ed25519.pub", "id_ecdsa.pub", "id_rsa.pub"):
        p = os.path.join(ssh_dir, name)
        if os.path.exists(p):
            return p
    return None


def offer_key_login(host, y):
    """Walk through key-based SSH login. Returns True if it may work now."""
    if y or not sys.stdin.isatty():
        say("   set up key login first, then run this again:")
        say("     ssh-keygen -t ed25519         (if you have no key)")
        say("     ssh-copy-id %s                (or append ~/.ssh/id_ed25519.pub to its" % host)
        say("                                    ~/.ssh/authorized_keys by hand)")
        return False
    key = public_key()
    if not key:
        if not ask("You have no SSH key. Create one now (ssh-keygen -t ed25519)?", True, y):
            return False
        subprocess.run(["ssh-keygen", "-t", "ed25519", "-f",
                        os.path.join(os.path.expanduser("~"), ".ssh", "id_ed25519")])
        key = public_key()
        if not key:
            return False
    if not ask("Copy %s to %s? You will be asked for its password once" % (key, host), True, y):
        return False
    with open(key, encoding="utf-8") as fh:
        pub = fh.read().strip() + "\n"
    # ssh-copy-id does not exist on Windows; this is what it does.
    subprocess.run(["ssh", "-o", "StrictHostKeyChecking=accept-new", host,
                    "umask 077; mkdir -p ~/.ssh && touch ~/.ssh/authorized_keys && "
                    "(grep -qxF %s ~/.ssh/authorized_keys || cat >> ~/.ssh/authorized_keys)"
                    % shlex.quote(pub.strip())], input=pub, text=True)
    return True


def install_remote_hooks(host, uninstall=False):
    sys.path.insert(0, os.path.join(HERE, "hooks"))
    import install as hook_install
    try:
        hook_install.install_remote(host, uninstall=uninstall)
        return True
    except SystemExit as exc:
        say("   %s" % exc)
        return False


def restart_dispatcher(y):
    if not service_installed():
        say("   restart the dispatcher yourself so it picks up the change")
        return
    if ask("Restart the dispatcher now so it picks this up?", True, y):
        service_restart()


def remote_add(host, args):
    y = args.yes
    say("Imperatorium - onboarding %s" % host)

    step("1/6 SSH")
    facts, err = probe(host)
    if facts is None:
        say("   cannot log in without a password: %s" % err)
        if offer_key_login(host, y):
            facts, err = probe(host)
        if facts is None:
            say("   still cannot reach %s: %s" % (host, err))
            return 1
    say("   ok  %s, home %s" % (host, facts.get("home")))

    step("2/6 Claude Code on %s" % host)
    ver = facts.get("version")
    if not facts.get("claude") or not ver or ver < MIN_REMOTE_CLAUDE:
        what = ("Claude Code %s is older than %s" % (".".join(map(str, ver)),
                ".".join(map(str, MIN_REMOTE_CLAUDE)))) if ver else "Claude Code is not installed"
        say("   %s" % what)
        if args.install_claude or ask("Install/update it there with the official installer (%s)?"
                                      % CLAUDE_INSTALL, False, y):
            rc, _out, err = ssh(host, CLAUDE_INSTALL, timeout=600)
            if rc != 0:
                say("   the installer failed: %s" % err[-400:])
                return 1
            facts, err = probe(host)
            ver = (facts or {}).get("version")
        if not ver or ver < MIN_REMOTE_CLAUDE:
            say("   install it there, then run this again:  ssh %s '%s'" % (host, CLAUDE_INSTALL))
            return 1
        say("   log in once on %s if you have not:  ssh -t %s %s   (then /login)"
            % (host, host, facts["claude"]))
    say("   ok  %s at %s" % (".".join(map(str, ver)), facts["claude"]))

    step("3/6 python3 on %s" % host)
    if not facts.get("python") or facts["python"] < MIN_PYTHON:
        say("   python3 %d.%d+ is needed there (the hooks and prompt delivery use it)" % MIN_PYTHON)
        say("   e.g.  ssh -t %s 'sudo apt install -y python3 python3-venv'" % host)
        return 1
    say("   ok  python3 %s" % ".".join(map(str, facts["python"])))

    step("4/6 Conversation reader (Agent SDK in ~/%s)" % READER_DIR)
    if facts.get("sdk"):
        say("   ok  already installed")
    elif args.no_reader:
        say("   skipped (--no-reader); that host's conversations will not show in the window")
    elif ask("Install it there? ~300MB; without it the window cannot show that host's conversations",
             True, y):
        cmd = ("python3 -m venv ~/{d} && ~/{d}/bin/pip install -q --upgrade pip && "
               "~/{d}/bin/pip install -q {r}").format(d=READER_DIR, r=shlex.quote(READER_REQUIREMENT))
        say("   $ ssh %s %s" % (host, cmd))
        rc, _out, err = ssh(host, cmd, timeout=900)
        if rc != 0:
            say("   failed: %s" % err[-400:])
            if "ensurepip" in err or "venv" in err:
                say("   on Debian/Ubuntu:  ssh -t %s 'sudo apt install -y python3-venv'" % host)
            say("   carrying on without it; run `remote add %s` again later" % host)
        else:
            say("   installed")

    tunnel = not args.no_tunnel
    step("5/6 Reporting hooks on %s" % host)
    if tunnel:
        if not install_remote_hooks(host):
            return 1
    else:
        say("   skipped (--no-tunnel): the host is polled, but prompts can only start new sessions")

    step("6/6 config.json")
    cfg = load_config()
    if cfg is None:
        say("   no config.json yet - run `python setup_machine.py` first")
        return 1
    entry = host_entry(host, home_relative(facts["claude"], facts.get("home")),
                       args.label, tunnel)
    cfg, how = upsert_host(cfg, entry)
    save_config(cfg)
    say("   %s %s" % (how, json.dumps(entry)))
    restart_dispatcher(y)

    say("\nDone. %s's sessions show in the window within ~20s of the dispatcher starting." % host)
    if tunnel:
        say("Sessions already running there report in on their next turn; restart them to be sure.")
    return 0


def remote_remove(host, args):
    y = args.yes
    step("Hooks on %s" % host)
    install_remote_hooks(host, uninstall=True)
    if args.purge or ask("Also delete ~/.claude/ccontrol (hook script and reader venv) on %s?" % host,
                         False, y):
        rc, _out, err = ssh(host, "rm -rf ~/.claude/ccontrol")
        say("   removed ~/.claude/ccontrol" if rc == 0 else "   could not remove it: %s" % err)
    step("config.json")
    cfg = load_config()
    if cfg is not None:
        cfg, changed = remove_host(cfg, host)
        if changed:
            save_config(cfg)
            restart_dispatcher(y)
        else:
            say("   %s was not in config.json" % host)
    return 0


def remote_check(hosts):
    if not hosts:
        say("   no hosts in config.json. Add one:  python setup_machine.py remote add HOST")
        return 0
    bad = 0
    for entry in hosts:
        name = entry if isinstance(entry, str) else entry.get("name")
        step(name)
        facts, err = probe(name)
        if facts is None:
            say("   MISSING  ssh           %s" % err)
            bad += 1
            continue
        ver = facts.get("version")
        tunnel = isinstance(entry, dict) and entry.get("tunnel")
        rows = [
            ("ssh", True, "key login works"),
            ("claude", bool(ver and ver >= MIN_REMOTE_CLAUDE),
             "%s at %s" % (".".join(map(str, ver)), facts.get("claude")) if ver else "not found"),
            ("python3", bool(facts.get("python")),
             ".".join(map(str, facts["python"])) if facts.get("python") else "not found"),
            ("reader", bool(facts.get("sdk")), "Agent SDK venv" if facts.get("sdk") else
             "optional - conversations need it"),
            ("hooks", bool(facts.get("hooks") and facts.get("hookscript")) or not tunnel,
             "installed" if facts.get("hooks") else
             ("not installed" if tunnel else "not used (no tunnel)")),
        ]
        for what, good, detail in rows:
            say("   %-8s %-12s %s" % ("ok " if good else "-- " if what == "reader" else "MISSING",
                                      what, detail))
            bad += 0 if good or what == "reader" else 1
    if bad:
        say("\nFix a host with:  python setup_machine.py remote add HOST")
    return 1 if bad else 0


def cmd_remote(args):
    action = args.action or "check"
    if action == "check":
        cfg = load_config() or {}
        hosts = (cfg.get("remote") or {}).get("hosts") or []
        if args.host:
            hosts = [h for h in hosts if (h if isinstance(h, str) else h.get("name")) == args.host] \
                or [args.host]
        return remote_check(hosts)
    if action not in ("add", "remove"):
        say("usage: setup_machine.py remote add|remove|check [HOST]")
        return 2
    if not args.host:
        say("usage: setup_machine.py remote %s HOST   (user@host or an ~/.ssh/config alias)" % action)
        return 2
    return (remote_add if action == "add" else remote_remove)(args.host, args)


def cmd_service(args):
    action = args.action or "status"
    if action == "install":
        deck = args.streamdeck
        if deck is None:  # whether the Stream Deck extras were installed
            deck = output([service_python(), "-c", "import StreamDeck"]) is not None
        service_install(streamdeck=deck)
    elif action == "uninstall":
        service_uninstall()
    elif action == "restart":
        service_restart()
    elif action == "status":
        service_status()
    else:
        say("usage: setup_machine.py service install|uninstall|restart|status")
        return 2
    return 0


# -- commands ----------------------------------------------------------------------
def hooks_state():
    """(settings path, [our hook events], statusline is ours) for this machine."""
    sys.path.insert(0, os.path.join(HERE, "hooks"))
    import install as hook_install
    path = hook_install.settings_path()
    try:
        with open(path, encoding="utf-8") as fh:
            settings = json.load(fh)
    except (OSError, ValueError):
        return path, [], False
    events = [e for e, blocks in (settings.get("hooks") or {}).items()
              if isinstance(blocks, list) and any(hook_install.is_ours(b) for b in blocks)]
    return path, events, hook_install.is_our_statusline(settings.get("statusLine"))


def cmd_check(args):
    step("Prerequisites")
    ok = report(check_prereqs())
    step("This checkout")
    for name in ("config.json", "policy.json", ".venv", os.path.join("vendor", "hidapi.dll")):
        say("   %-8s %s" % ("ok " if os.path.exists(os.path.join(HERE, name)) else "-- ", name))
    step("Claude Code hooks")
    path, events, statusline = hooks_state()
    say("   %-8s %s" % ("ok " if events else "MISSING", path))
    say("   %-8s reporting and permission hooks: %s" % (
        "ok " if "SessionStart" in events else "MISSING", ", ".join(events) or "none"))
    say("   %-8s statusline (usage meter)" % ("ok " if statusline else "-- "))
    ok = ok and bool(events)
    step("Service")
    say("   %-8s %s" % ("ok " if service_installed() else "-- ",
                        "scheduled tasks" if WINDOWS else "launchd agent" if MACOS
                        else "systemd user unit"))
    health = wait_for_dispatcher(3)
    say("   %-8s dispatcher on 127.0.0.1:8792%s" % (
        "ok " if health else "-- ", " (pid %s)" % health.get("pid") if health else ""))
    hosts = ((load_config() or {}).get("remote") or {}).get("hosts") or []
    if hosts:
        step("Other machines")
        say("   %d configured; check them with:  python setup_machine.py remote check" % len(hosts))
    if not events:
        say("\nRun `python setup_machine.py` to install what is missing.")
    return 0 if ok else 1


def cmd_install(args):
    y = args.yes
    say("Imperatorium setup - %s" % HERE)
    step("1/7 Prerequisites")
    if not report(check_prereqs()):
        say("\nInstall the missing requirements above, then run this again.")
        return 1

    step("2/7 Choices")
    roots = args.root or [ask_text("Folder that holds your projects", default_roots()[0], y)]
    voice = args.voice if args.voice is not None else ask(
        "Local dictation (F9)? Downloads ~1.6GB of model on first use; a GPU helps", False, y)
    deck = args.streamdeck if args.streamdeck is not None else ask(
        "Elgato Stream Deck surface?", False, y)
    auto = args.auto_approve if args.auto_approve is not None else ask(
        "Auto-approve cheap permission prompts (reads, git status, tests) - see README 'Auto-approval'?",
        False, y)
    statusline = not args.no_statusline and ask(
        "Statusline (usage meter: context %, 5-hour and weekly limits)?", True, y)
    tool_hooks = not args.no_tool_hooks and ask(
        "Tool hooks (live, timed activity; one short process per tool call)?", True, y)
    push = args.ntfy if args.ntfy is not None else ask(
        "Phone push when a session has waited on you for a minute (ntfy.sh, free app)?", False, y)
    service = not args.no_service and ask(
        "Start Imperatorium at logon and keep it running (%s)?" % (
            "scheduled tasks" if WINDOWS else "launchd agent" if MACOS else "systemd user unit"),
        True, y)

    step("3/7 Python environment")
    make_venv([e for e, on in (("voice", voice), ("streamdeck", deck)) if on])

    step("4/7 Settings")
    topic = ntfy_topic() if push else None
    if not write_if_absent("config.json", build_config(load_example("config.example.json"),
                                                       roots, voice, ntfy=topic)) and push:
        cfg = load_config()
        ntfy = cfg.setdefault("notify", {}).setdefault("ntfy", {})
        if ntfy.get("topic"):
            topic = ntfy["topic"]
        else:
            ntfy.update({"topic": topic, "enabled": True})
            save_config(cfg)
    write_if_absent("policy.json", build_policy(load_example("policy.example.json"), auto))
    if topic:
        say("   phone push: install the ntfy app and subscribe to  %s" % topic)
        say("   (or open https://ntfy.sh/%s). Treat that name like a password." % topic)

    if deck and WINDOWS:
        step("4b/7 Stream Deck driver")
        try:
            fetch_hidapi()
        except Exception as exc:
            say("   could not fetch hidapi.dll: %s" % exc)
            say("   get x64/hidapi.dll from %s into vendor/ by hand" % HIDAPI_URL)

    step("5/7 Claude Code hooks")
    install_hooks(statusline, tool_hooks)

    step("6/7 Keep it running")
    if service:
        service_install(streamdeck=deck)
    else:
        say("   skipped - start it with:  %s -m ccontrol.dispatcher" % VENV_PY)
        say("   (or later:  python setup_machine.py service install)")

    step("7/7 Check")
    health = wait_for_dispatcher(60 if service else 3)
    if health:
        say("   dispatcher is up (pid %s)" % health.get("pid"))
        say("\nDone. Open the window:  %s -m ccontrol.window   (or %s)" % (VENV_PY, WINDOW_URL))
    else:
        say("   dispatcher is not answering yet on %s" % HEALTH)
        say("\nInstalled. Start the dispatcher, then open %s" % WINDOW_URL)
    say("Restart running Claude Code sessions so they pick up the hooks.")
    say("Other machines:  python setup_machine.py remote add HOST")
    return 0


def cmd_uninstall(args):
    step("Claude Code hooks")
    install_hooks(False, False, uninstall=True)
    step("Service")
    service_uninstall()
    hosts = ((load_config() or {}).get("remote") or {}).get("hosts") or []
    if hosts:
        say("\nHooks on other machines are left alone; remove each with:")
        for h in hosts:
            say("   python setup_machine.py remote remove %s"
                % (h if isinstance(h, str) else h.get("name")))
    say("\nRemoved. config.json, policy.json, .venv and your state (%s) were left "
        "in place; delete them by hand if you want." % STATE_DIR_HINT)
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("command", nargs="?", default="install",
                    choices=("install", "check", "uninstall", "remote", "service"))
    ap.add_argument("action", nargs="?",
                    help="remote: add | remove | check;  service: install | uninstall | restart | status")
    ap.add_argument("host", nargs="?", help="remote: user@host or an ~/.ssh/config alias")
    ap.add_argument("--yes", "-y", action="store_true", help="accept every default")
    ap.add_argument("--root", action="append",
                    help="a folder of projects (repeatable); default: this checkout's parent")
    ap.add_argument("--voice", dest="voice", action="store_true", default=None)
    ap.add_argument("--no-voice", dest="voice", action="store_false")
    ap.add_argument("--streamdeck", dest="streamdeck", action="store_true", default=None)
    ap.add_argument("--no-streamdeck", dest="streamdeck", action="store_false")
    ap.add_argument("--auto-approve", dest="auto_approve", action="store_true", default=None)
    ap.add_argument("--no-auto-approve", dest="auto_approve", action="store_false")
    ap.add_argument("--ntfy", dest="ntfy", action="store_true", default=None,
                    help="phone pushes through ntfy.sh, on an unguessable topic")
    ap.add_argument("--no-ntfy", dest="ntfy", action="store_false")
    ap.add_argument("--no-statusline", action="store_true")
    ap.add_argument("--no-tool-hooks", action="store_true")
    ap.add_argument("--no-service", action="store_true")
    remote = ap.add_argument_group("remote add / remove")
    remote.add_argument("--label", help="the name the window shows for the host")
    remote.add_argument("--no-tunnel", action="store_true",
                        help="poll only: no hooks there, so no prompts into running sessions")
    remote.add_argument("--no-reader", action="store_true",
                        help="skip the Agent SDK venv that shows the host's conversations")
    remote.add_argument("--install-claude", action="store_true",
                        help="install or update Claude Code there without asking")
    remote.add_argument("--purge", action="store_true",
                        help="remove: also delete ~/.claude/ccontrol on the host")
    args = ap.parse_args(argv)
    return {"install": cmd_install, "check": cmd_check, "uninstall": cmd_uninstall,
            "remote": cmd_remote, "service": cmd_service}[args.command](args)


if __name__ == "__main__":
    sys.exit(main())
