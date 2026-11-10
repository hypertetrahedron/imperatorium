"""Install the Imperatorium mods and layout into herdr-app.

    python setup_machine.py herdr install    [--home DIR] [--url URL]
    python setup_machine.py herdr status     [--home DIR]
    python setup_machine.py herdr uninstall  [--home DIR]

herdr-app reads its mods, layouts and settings from its home
(`~/.config/herdr-app`, or HERDR_APP_HOME) and reloads whatever changes
there. `install`:

- copies `herdr/mods/imperatorium-*` to `<home>/mods/` (only those folders,
  replaced whole) and `herdr/layouts/imperatorium.json` to `<home>/layouts/`;
- writes `<home>/imperatorium.json`: the dispatcher's address, and the file
  the dispatcher writes its per-start herdr token to;
- sets `"telemetry": false` in `<home>/settings.json` (herdr-app sends a
  daily usage report unless told not to), keeping every other setting and a
  `.bak` of the file.

`uninstall` removes only what `install` put there; the telemetry setting
stays off. Nothing here touches herdr-core, running terminals, or herdr's own
mods and layouts.

Standard library only: setup_machine.py runs it before the venv exists.
"""
import json
import os
import shutil

PREFIX = "imperatorium-"
LAYOUT = "imperatorium.json"
CONFIG = "imperatorium.json"
DEFAULT_URL = "http://127.0.0.1:8792"


def default_home(env=None):
    env = os.environ if env is None else env
    if env.get("HERDR_APP_HOME"):
        return env["HERDR_APP_HOME"]
    return os.path.join(os.path.expanduser("~"), ".config", "herdr-app")


def token_path(env=None):
    """Where the dispatcher writes its herdr token: beside its state, as
    dispatcher.herdr_token_path(default_state_path()) decides."""
    env = os.environ if env is None else env
    base = (env.get("LOCALAPPDATA") or env.get("XDG_STATE_HOME")
            or os.path.join(os.path.expanduser("~"), ".local", "state"))
    new, old = os.path.join(base, "imperatorium"), os.path.join(base, "ccontrol")
    use = old if not os.path.exists(new) and os.path.isdir(old) else new
    return os.path.join(use, "herdr-token")


def source_dir(repo):
    return os.path.join(repo, "herdr")


def our_mods(repo):
    root = os.path.join(source_dir(repo), "mods")
    return sorted(n for n in os.listdir(root)
                  if n.startswith(PREFIX) and os.path.isdir(os.path.join(root, n)))


def install(repo, home=None, url=DEFAULT_URL, token_file=None, say=print):
    home = home or default_home()
    os.makedirs(os.path.join(home, "mods"), exist_ok=True)
    os.makedirs(os.path.join(home, "layouts"), exist_ok=True)
    done = []
    for name in our_mods(repo):
        src = os.path.join(source_dir(repo), "mods", name)
        dst = os.path.join(home, "mods", name)
        if os.path.isdir(dst):
            shutil.rmtree(dst)
        shutil.copytree(src, dst)
        done.append("mods/" + name)
    shutil.copyfile(os.path.join(source_dir(repo), "layouts", LAYOUT),
                    os.path.join(home, "layouts", LAYOUT))
    done.append("layouts/" + LAYOUT)
    cfg = {"url": url.rstrip("/"), "token_file": (token_file or token_path()).replace("\\", "/")}
    _write_json(os.path.join(home, CONFIG), cfg)
    done.append(CONFIG)
    if set_telemetry_off(home):
        done.append("settings.json: telemetry off")
    for d in done:
        say("  " + d)
    return done


def uninstall(home=None, say=print):
    home = home or default_home()
    gone = []
    mods = os.path.join(home, "mods")
    if os.path.isdir(mods):
        for name in sorted(os.listdir(mods)):
            if name.startswith(PREFIX):
                shutil.rmtree(os.path.join(mods, name), ignore_errors=True)
                gone.append("mods/" + name)
    for rel in (os.path.join("layouts", LAYOUT), CONFIG):
        p = os.path.join(home, rel)
        if os.path.isfile(p):
            os.remove(p)
            gone.append(rel.replace("\\", "/"))
    for g in gone:
        say("  removed " + g)
    return gone


def status(repo, home=None):
    home = home or default_home()
    out = {"home": home, "exists": os.path.isdir(home), "mods": {}, "layout": False,
           "config": None, "telemetry_off": False}
    for name in our_mods(repo):
        dst = os.path.join(home, "mods", name)
        out["mods"][name] = ("current" if os.path.isdir(dst) and _same_tree(
            os.path.join(source_dir(repo), "mods", name), dst)
            else "stale" if os.path.isdir(dst) else "missing")
    out["layout"] = os.path.isfile(os.path.join(home, "layouts", LAYOUT))
    out["config"] = _read_json(os.path.join(home, CONFIG))
    out["telemetry_off"] = (_read_json(os.path.join(home, "settings.json")) or {}).get("telemetry") is False
    return out


def set_telemetry_off(home):
    """True when settings.json was changed."""
    path = os.path.join(home, "settings.json")
    current = _read_json(path)
    if current is None and os.path.exists(path):
        raise ValueError("%s is not JSON; fix it or set telemetry off in herdr's "
                         "Settings > Privacy" % path)
    current = current or {}
    if current.get("telemetry") is False:
        return False
    if os.path.exists(path):
        shutil.copyfile(path, path + ".bak")
    current["telemetry"] = False
    _write_json(path, current)
    return True


def _same_tree(a, b):
    for root, _dirs, files in os.walk(a):
        for f in files:
            pa = os.path.join(root, f)
            pb = os.path.join(b, os.path.relpath(pa, a))
            try:
                with open(pa, "rb") as x, open(pb, "rb") as y:
                    if x.read() != y.read():
                        return False
            except OSError:
                return False
    return True


def _read_json(path):
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except (OSError, ValueError):
        return None


def _write_json(path, data):
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as fh:
        json.dump(data, fh, indent=2)
        fh.write("\n")
    os.replace(tmp, path)
