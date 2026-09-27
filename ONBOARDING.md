# Getting started with Imperatorium

This takes a new machine from nothing to a working window, then adds the other
machines you run Claude Code on. Every step is a script, every script can be
run again safely, and `check` tells you what is still missing at any point.

## 1. This machine

### From nothing

The bootstrap script checks for Python 3.9+, git and Claude Code, offers to
install whatever is missing, clones the repository into `~/imperatorium`, and
starts the setup in step 2.

```
# Windows (PowerShell)
irm https://raw.githubusercontent.com/hypertetrahedron/imperatorium/main/bootstrap.ps1 | iex

# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/hypertetrahedron/imperatorium/main/bootstrap.sh | bash
```

What it installs, and only after asking:

| Missing | Windows | Linux / macOS |
|---|---|---|
| Python 3.9+ | `winget install Python.Python.3.12` | prints the package-manager command (it does not run sudo) |
| git | `winget install Git.Git` | prints the package-manager command |
| Claude Code | Anthropic's `install.ps1` | Anthropic's `install.sh` |

To clone somewhere else: `-Dir D:\code\imperatorium` on Windows (run it as
`& ([scriptblock]::Create((irm <url>))) -Dir ...`), or
`IMPERATORIUM_DIR=~/code/imperatorium` before `bash` elsewhere. Anything else
you pass goes on to `setup_machine.py`.

If Claude Code was just installed, run `claude` once and log in before you go on.

### From a clone

```
git clone https://github.com/hypertetrahedron/imperatorium.git
cd imperatorium
python setup_machine.py
```

## 2. Setup

`setup_machine.py` asks these questions, and `--yes` takes the defaults:

| Question | Default | Flag |
|---|---|---|
| Folder that holds your projects | the folder the checkout sits in (or `~/code`, `~/src`... when that is your home folder) | `--root DIR` (repeatable) |
| Local dictation (F9), on your GPU | no | `--voice` / `--no-voice` |
| Elgato Stream Deck | no | `--streamdeck` / `--no-streamdeck` |
| Auto-approve cheap permission prompts | no | `--auto-approve` / `--no-auto-approve` |
| Statusline (context and rate-limit meter) | yes | `--no-statusline` |
| Tool hooks (live, timed activity) | yes | `--no-tool-hooks` |
| Phone push via ntfy | no | `--ntfy` / `--no-ntfy` |
| Start at logon and keep running | yes | `--no-service` |

Then, in order:

1. builds `.venv` and installs `requirements*.txt` for what you chose;
2. writes `config.json` and `policy.json` from the `*.example.json` files -
   never over ones you already have;
3. with a Stream Deck on Windows, downloads `hidapi.dll` from the official
   libusb release and checks its SHA-256;
4. adds the hooks to `~/.claude/settings.json`, backing it up first and
   touching nothing it did not add;
5. registers the service - scheduled tasks on Windows, a systemd user unit on
   Linux, a launchd agent on macOS - and waits for the dispatcher to answer.

With phone push on, it prints an unguessable ntfy topic: install the ntfy app
and subscribe to it. The switches for push and spoken alerts are in the
window, under **insights**.

**Restart any Claude Code sessions that were already running**, so they load
the hooks. Then open the window:

```
.venv/Scripts/python -m ccontrol.window      # Windows (bin/ elsewhere)
http://127.0.0.1:8792/                       # or any browser
```

On Windows, `ctrl+alt+backquote` raises it from anywhere (the first free key
from `palette.hotkey` in `config.json`).

### Is it working?

```
python setup_machine.py check
```

lists prerequisites, settings files, hooks, statusline, service and whether
the dispatcher answers, with `MISSING` against anything required.

## 3. Other machines

Anything you can `ssh` into that runs Linux or macOS. Nothing is left
listening on the host and no port is opened on either machine.

```
python setup_machine.py remote add devbox              # user@host or an ~/.ssh/config alias
python setup_machine.py remote add pi.local --label pi
```

It walks through:

1. **SSH key login.** If a password-less login fails, it offers to create a
   key and copy it across; you type the host's password once.
2. **Claude Code** 2.1.224+ on the host. If it is missing or too old it
   offers to run Anthropic's installer there (`--install-claude` to skip the
   question). Log in on the host once afterwards: `ssh -t HOST ~/.local/bin/claude`,
   then `/login`.
3. **python3** on the host. If it is missing it tells you the command
   (`sudo apt install python3 python3-venv`) rather than running sudo.
4. **The conversation reader**: the Agent SDK in `~/.claude/ccontrol/venv`
   (~300MB), so the window can show what that host's sessions said.
   `--no-reader` skips it.
5. **The reporting hooks** in the host's `~/.claude/settings.json`, backed up
   first. They reach this machine back through an SSH tunnel the dispatcher
   holds open. `--no-tunnel` skips them; the host is then only polled, and a
   prompt for it always starts a new background session.
6. **config.json**: adds the host with `"tunnel": true` and restarts the dispatcher.

```
python setup_machine.py remote check             # every host, piece by piece
python setup_machine.py remote check devbox
python setup_machine.py remote remove devbox     # hooks off, entry gone
python setup_machine.py remote remove devbox --purge   # and ~/.claude/ccontrol there
```

Sessions already running on the host report in on their next turn;
restarting them is quicker.

A VS Code session running in `bypassPermissions` holds prompts from other
sessions until someone approves them, and VS Code has nowhere to approve
them, so they expire. If you use one, set `"crossSessionInbound": "accept"`
in that host's `~/.claude/settings.json`.

## 4. Keeping it running

```
python setup_machine.py service status
python setup_machine.py service restart
python setup_machine.py service install      # again, e.g. after moving the checkout
python setup_machine.py service uninstall
```

On Linux a user unit stops at logout; `loginctl enable-linger $USER` keeps it running.

## 5. Removing it

```
python setup_machine.py remote remove HOST   # for each host
python setup_machine.py uninstall            # hooks and service on this machine
```

Your `config.json`, `policy.json`, `.venv` and state folder stay behind;
delete the checkout to remove the rest.

## When something is wrong

| Symptom | Look at |
|---|---|
| A session is missing from the sidebar | `python -m ccontrol.registry`; restart the session so it loads the hooks |
| "stale_credential", or a prompt opens a new window instead | the session was resumed; its next turn repairs it |
| A host shows as parked | `python setup_machine.py remote check HOST` |
| The window says it cannot read conversations | `.venv` is missing the Agent SDK: run `python setup_machine.py` again |
| Nothing answers on 8792 | `python setup_machine.py service status` |
| A hook seems silent | set `CCONTROL_DEBUG_LOG` to a file path and watch it |
