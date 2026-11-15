# Getting started with Imperatorium

This takes a new machine from nothing to a working window, then adds the other
machines you run Claude Code on. Every step can be run again safely, and
`imperatorium check` tells you what is still missing at any point.

Imperatorium is one binary, `imperatorium`. There is nothing to build and no
language runtime to install. (The last Python version is the tag `last-python`.)

## 1. This machine

You need git and Claude Code. If Claude Code is not installed yet, install it
first from [claude.com/claude-code](https://claude.com/claude-code), then run
`claude` once and log in.

### From nothing

The install script downloads the binary for this machine from the latest
release, checks its SHA-256 against the release's manifest, puts it in
`%LOCALAPPDATA%\Programs\imperatorium` (`~/.local/bin` on Linux and macOS),
and starts the setup in step 2. No administrator rights or sudo.

```
# Windows (PowerShell)
irm https://github.com/hypertetrahedron/imperatorium/releases/latest/download/install.ps1 | iex

# Windows, when PowerShell's policy blocks that (cmd.exe)
curl -fsSLo install.cmd https://github.com/hypertetrahedron/imperatorium/releases/latest/download/install.cmd && install.cmd

# Linux / macOS
curl -fsSL https://github.com/hypertetrahedron/imperatorium/releases/latest/download/install.sh | sh
```

`IMPERATORIUM_CHANNEL=preview` picks the preview channel instead of stable, and
`IMPERATORIUM_RELEASES` points at another release folder (a mirror). Anything
you pass the script goes on to `imperatorium install`: save `install.ps1` and
run it with `--dry-run`, or `curl ... | sh -s -- --dry-run`.

### From a checkout

```
git clone https://github.com/hypertetrahedron/imperatorium.git
cd imperatorium/rust
cargo build --release
target/release/imperatorium install
```

## 2. Setup

`imperatorium install` asks these questions, and `--yes` takes the defaults.
A flag answers a question without asking it, and `--no-NAME` says no.

| Question | Default | Flag |
|---|---|---|
| Folder that holds your projects | the folder the checkout sits in (or `~/code`, `~/src`... when that is your home folder) | `--root DIR` (repeatable) |
| Local dictation (F9), on your GPU | no | `--voice` / `--no-voice` |
| Elgato Stream Deck | no | `--streamdeck` / `--no-streamdeck` |
| Auto-approve cheap permission prompts | no | `--auto-approve` / `--no-auto-approve` |
| The Imperatorium mod (Claude Code 2.1.287+) | yes | `--no-mod` |
| Statusline (context and rate-limit meter) | only without the mod | `--statusline` / `--no-statusline` |
| Tool hooks (live, timed activity) | yes | `--no-tool-hooks` |
| Phone push via ntfy | no | `--ntfy` / `--no-ntfy` |
| Global hotkey to raise the window (Windows) | yes | `--no-hotkey` |
| Start at logon and keep running | yes | `--no-service` |

`--home DIR` picks the config folder, and `--dry-run` shows what would change
without changing it.

Then, in order:

1. puts the binary in place;
2. writes `config.json` and `policy.json` into the config folder
   (`%APPDATA%\imperatorium`, or `~/.config/imperatorium`) - never over ones
   you already have;
3. with dictation on, `imperatorium voice fetch` downloads the dictation
   worker and its model;
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
imperatorium open
http://127.0.0.1:8792/                       # or any browser
```

On Windows, `ctrl+alt+backquote` raises it from anywhere (the first free key
from `palette.hotkey` in `config.json`).

### Is it working?

```
imperatorium check
```

lists prerequisites, settings files, hooks, statusline, service and whether
the dispatcher answers, with `MISSING` against anything required, and changes
nothing. `imperatorium status` shows the service's units and whether the
dispatcher answers.

## 3. Other machines

Anything you can `ssh` into that runs Linux or macOS. Nothing is left
listening on the host and no port is opened on either machine, and the host
needs nothing but `sh` and Claude Code: the dispatcher puts a copy of the
binary there.

```
imperatorium remote add devbox              # user@host or an ~/.ssh/config alias
imperatorium remote add pi.local --label pi
```

It walks through:

1. **SSH key login.** If a password-less login fails, it offers to create a
   key and copy it across; you type the host's password once.
2. **Claude Code** 2.1.224+ on the host. If it is missing or too old it
   offers to run Anthropic's installer there (`--install-claude` to skip the
   question). Log in on the host once afterwards: `ssh -t HOST ~/.local/bin/claude`,
   then `/login`.
3. **The binary** at `~/.claude/imperatorium/bin/imperatorium` on the host.
4. **The reporting hooks** in the host's `~/.claude/settings.json`, backed up
   first. They reach this machine back through an SSH tunnel the dispatcher
   holds open. `--no-tunnel` skips them; the host is then only polled, and a
   prompt for it always starts a new background session.
5. **config.json**: adds the host with `"tunnel": true` and restarts the
   dispatcher (`--no-restart` leaves that to you).

```
imperatorium remote check             # every host, piece by piece
imperatorium remote check devbox
imperatorium remote remove devbox     # hooks off, entry gone
imperatorium remote remove devbox --purge   # and ~/.claude/imperatorium there
```

Sessions already running on the host report in on their next turn;
restarting them is quicker.

A VS Code session running in `bypassPermissions` holds prompts from other
sessions until someone approves them, and VS Code has nowhere to approve
them, so they expire. If you use one, set `"crossSessionInbound": "accept"`
in that host's `~/.claude/settings.json`.

## 4. Keeping it running

```
imperatorium service status
imperatorium service restart
imperatorium service install      # again, e.g. after moving the binary
imperatorium service uninstall
```

`imperatorium stop` stays stopped until `imperatorium start`. On Linux a user
unit stops at logout; `loginctl enable-linger $USER` keeps it running.

New releases arrive with `imperatorium update` (`--check` only looks).

## 5. Removing it

```
imperatorium remote remove HOST   # for each host
imperatorium uninstall            # hooks, mod and service on this machine
```

Your `config.json`, `policy.json` and state folder stay behind; delete them
and the binary to remove the rest.

## When something is wrong

| Symptom | Look at |
|---|---|
| A session is missing from the sidebar | `imperatorium sessions`; restart the session so it loads the hooks |
| "stale_credential", or a prompt opens a new window instead | the session was resumed; its next turn repairs it |
| A host shows as parked | `imperatorium remote check HOST`, or `imperatorium hosts` |
| Dictation is unavailable | `imperatorium voice status`, then `imperatorium voice fetch` |
| Nothing answers on 8792 | `imperatorium status`, then `imperatorium service status` |
| A hook seems silent | set `IMPERATORIUM_DEBUG_LOG` to a file path and watch it |
