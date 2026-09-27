# Imperatorium

One place to see every Claude Code session on this machine and the machines
you reach over SSH, and one way to send work to any of them without hunting
for its window.

(Inside the code it is still `ccontrol` - "Central Control" - and so are the
scheduled tasks, service units and state folders it installs.)

Built for a desk running ~20 concurrent sessions across separate repositories,
spread over several Windows virtual desktops. The premise is that *finding the
window* is a symptom, not the problem: if you can dispatch without visiting a
session, and be told when one actually wants you, most of the hunting stops
being necessary.

## Install

You need Python 3.9+, [Claude Code](https://claude.com/claude-code) 2.1.234+
(2.1.224 on macOS/Linux) and git. `gh`, Windows Terminal and Edge are
optional; each switches a feature on (PR badges, Resume/Fork tabs, a
chromeless window). [ONBOARDING.md](ONBOARDING.md) walks through the whole
thing step by step, including other machines.

**From nothing** - the bootstrap script checks the prerequisites, offers to
install what is missing (winget for Python and git on Windows, Anthropic's
installer for Claude Code), clones this repository into `~/imperatorium` and
runs the setup below:

```
# Windows (PowerShell)
irm https://raw.githubusercontent.com/hypertetrahedron/imperatorium/main/bootstrap.ps1 | iex

# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/hypertetrahedron/imperatorium/main/bootstrap.sh | bash
```

**From a clone:**

```
git clone https://github.com/hypertetrahedron/imperatorium.git
cd imperatorium
python setup_machine.py
```

It asks where your projects live (default: the folder you cloned into) and
which extras you want (dictation, Stream Deck, auto-approval, statusline,
tool hooks, phone push, start at logon). Then it builds `.venv`, writes
`config.json` and `policy.json` from the `*.example.json` files, fetches the
Stream Deck driver and checks its hash, adds the hooks to
`~/.claude/settings.json` (backing it up first) and registers the service
that keeps the dispatcher up: scheduled tasks on Windows, a systemd user unit
on Linux, a launchd agent on macOS. Every step can be run again safely, and
your existing `config.json` and `policy.json` are never overwritten. Restart
running Claude Code sessions afterwards so they pick up the hooks.

```
python setup_machine.py check              # what is present and missing; changes nothing
python setup_machine.py --yes              # all defaults, no questions
python setup_machine.py uninstall          # hooks and the service; your files stay
python setup_machine.py service status     # install | uninstall | restart | status
python setup_machine.py remote add HOST    # onboard another machine (see "Other machines")
python setup_machine.py remote check       # every configured host, piece by piece
```

Hook paths point into this checkout, so if you move it, run the setup again.

## The window

```
python -m ccontrol.window      # chromeless, via Edge --app=
http://127.0.0.1:8792/         # or just open it
```

Sessions down the left, the selected session's conversation in the middle, a
prompt box underneath. Selecting a session shows what it actually said - text,
tool calls, tool results - so you can see what a blocked session is blocked on
instead of guessing from a status dot on another screen. Markdown tables
in what Claude writes are drawn as tables; the rest of its text is shown as
written. Enter sends to that
session; ctrl+Enter hands the work to a fresh background session.

Conversations are read through the Agent SDK's `get_session_messages`, the
supported reader, which works on a session running in another process. The
transcript files are not parsed: the docs say that format is internal and
changes between versions.

Tabs above the conversation show the session's **Changes** (git status and
diff), its **Activity** (every tool call, what failed and why) and the
project's **Worktrees** (the branches background sessions leave, with merge,
PR and discard). The header offers Fork, and Resume for a session that has
finished. **Queue** (alt+Enter) holds a prompt until the session's current
turn ends. The sidebar groups by state or folder, pins, archives and
filters; each tile says what the session is doing now, its context use and
its PR. **history** searches every past session; **insights** tallies tool
failures across projects and switches the phone and spoken alerts on.

## What it does

- **Registry** - every live session, its project, and whether it is idle,
  working, or blocked on you.
- **Attention** - Claude Code hooks push `permission_prompt`, `idle_prompt`
  and `agent_needs_input` here the moment they happen, so the board shows what
  is waiting and for how long.
- **Dispatch** - "project: laser ledger, implement the new feature as
  recommended" is matched to a project and delivered to that project's running
  session. With no session running, it opens one with the prompt pre-filled.
- **Board** - a self-contained page for a spare monitor or a tablet on the LAN.

## Requirements

Python 3.9+, standard library only. Claude Code 2.1.234 or later on native
Windows (2.1.224 on macOS/Linux) - that is when a session's inbox became
reachable. The optional Stream Deck surface has its own dependencies and is
kept separate for that reason.

## Running it

```
python -m ccontrol.dispatcher              # http://127.0.0.1:8792
python hooks/install.py --dry-run          # show what would change
python hooks/install.py                    # install the hooks (backs up first)
python hooks/install.py --statusline       # + usage meter data (context, 5h/7d limits)
python hooks/install.py --tool-hooks       # + live, timed activity (one process per tool call)
python test_core.py && python test_dispatcher.py
python test_policy.py && python test_remote.py && python test_conversation.py
python test_hotkey.py && python test_ui.py        # test_ui needs node; skips without
.venv\Scripts\python.exe test_streamdeck.py        # needs the venv
.venv\Scripts\python.exe test_voice.py
.venv\Scripts\python.exe test_features.py        # the newer tabs, queue, usage, history
python test_setup.py                               # the installer
# 489 tests in total
```

Individual pieces are runnable on their own, which is usually the fastest way
to see what is wrong:

```
python -m ccontrol.registry                            # what is running
python -m ccontrol.projects "project: gem trip, fix it" # what would it match
python -m ccontrol.deeplink Y:/projects-software/gem-trip "fix it"
```

## HTTP surface

| Method | Path | Purpose |
|---|---|---|
| GET | `/` | the window |
| GET | `/api/conversation` | `?session=<id>&limit=N` - what a session said |
| GET | `/api/sessions` | merged registry |
| GET | `/api/projects` | the project catalogue |
| GET | `/api/events` | recent attention events |
| GET | `/api/decisions` | recent auto-approval verdicts |
| GET | `/api/hosts` | configured hosts and their reachability |
| GET | `/api/health` | liveness |
| POST | `/hook` | Claude Code hook events |
| POST | `/api/transcribe` | raw audio in, text out |
| POST | `/api/focus` | `{project}` or `{session_id}` - raise that session, or open one |
| POST | `/api/dispatch` | `{text}` or `{project, prompt}`, plus `mode`, `confirm` |
| GET | `/api/changes` | `?session=` - branch, changed files, diff against HEAD |
| GET | `/api/activity` | `?session=` - tool-call timeline and counts |
| GET | `/api/worktrees` | `?project=` - linked worktrees, ahead/behind/dirty |
| GET | `/api/worktree_diff` | `?project=&path=` - what a worktree's branch would bring |
| POST | `/api/worktree` | `{project, path, action: merge\|pr\|discard, force}` |
| GET | `/api/history` | `?q=` - search past sessions |
| POST | `/api/resume` | `{session_id, where: window\|background, fork, prompt}` |
| GET/POST | `/api/queue`, `/api/unqueue` | prompts held until a session's turn ends |
| GET | `/api/usage` | rate-limit windows and their pace (needs the statusline) |
| GET | `/api/errors` | `?days=` - tool failures by cause, tool and project |
| GET/POST | `/api/prefs` | pinned, archived, grouping |
| GET/POST | `/api/notify` | ntfy and speech channels, on/off |
| GET | `/api/pending` | parked permission requests; `POST /api/decide` answers one |

`mode` is `auto` (inject if a session is live, else launch), `inject`, `launch`,
`background`, or `dry-run`.

`background` runs `claude --bg` in the project instead of opening a window -
work you want done rather than a window you want to sit in. Claude Code
isolates it in a worktree and commits on delete by itself. It still reports
through the same hooks, so it shows on the board and turns its key orange if
it gets blocked. Options for it live under `background` in `config.json`.

Bound to `127.0.0.1`. POSTs must be JSON and must carry no `Origin` header: a
cross-origin `fetch` with a plain-text body is a CORS "simple request", so a
page you merely had open could otherwise drive this.

## How dispatch actually works

Claude Code binds an inbox for each session - a named pipe on Windows, a Unix
socket elsewhere - and exports its path and a per-session token to that
session's hooks and Bash children. Writing two JSON lines to that pipe puts a
message in the session:

```
{"type":"auth","token":"<CLAUDE_CODE_MESSAGING_TOKEN>"}
{"type":"user","message":{"role":"user","content":"..."}}
```

The auth line is mandatory on native Windows; a connection whose first line is
not a valid auth line is closed and delivers nothing.

**A dispatched message is not you.** It arrives as a peer message, and Claude
Code is explicit that such a message is not user consent: it cannot answer a
pending permission prompt, cannot change settings or `CLAUDE.md`, and a slash
command in its text arrives as inert text. What the token buys is delivery
*without an approval dialog*, not authority. So this dispatches work; approving
what that work then asks for stays a human job, through the terminal, Remote
Control, or a `PermissionRequest` hook.

A token is only valid for the inbox it was recorded against. A resumed
session keeps its id but binds a new pipe, so the store hands out the socket
and token together and a token is honoured only while its socket still
matches; a mismatch shows up as `stale_credential` and dispatch falls back to
opening a window instead. Since `SessionStart` fires only once, every hook
event carries the credential, so a session that was already running when the
dispatcher started is repaired by its next `Stop` or `Notification` rather
than staying unreachable until it is resumed.

The token is the constraint that shapes the design. Claude Code hands it to
hooks and nowhere else, which is why `SessionStart` has to be a `command` hook
that reads its own environment, and why the store persists tokens: forget one
and that session is unreachable until it restarts.

## Matching

A wrong match here does not garble text, it fires a prompt at the wrong
repository. So `resolve` reports a confidence and only `certain` is acted on
silently; anything less needs `confirm`. On this machine that correctly lands
`book of errant pages` on `bookoferrantpages` and a slurred `amaranth a` on
`amarantha`, while refusing `home assistant`, which is not a project here.

## The hotkey

`hotkey.py` is all that remains of the old palette: it registers a global
hotkey and raises the window from anywhere, opening one if none is running.
It finds the window by title, so `ui.render_shell`'s `<title>` and
`hotkey.WINDOW_TITLE` have to agree - a test pins that, because otherwise a
press silently opens a second window instead of raising the first.

Conflicts are the norm rather than the exception (`ctrl+alt+space` and `win+k`
were both already taken here), so `palette.hotkey` in `config.json` takes a
list and the first candidate Windows grants wins.

## Dictation

**f9**, or the Dictate button, starts and stops recording; the text lands in
the prompt box. The browser records and the dispatcher transcribes on the
model it already holds, so nothing leaves the machine and the only slow part -
loading the model - happens once at startup.

Voice fills the *prompt*, never the project. Project names here -
`laser-ledger`, `amarantha`, `bookoferrantpages`, `fiducials` - are close to a
worst case for speech recognition, and a misheard name would not garble text,
it would fire a prompt at the wrong repository. Picking from the list costs a
keystroke and removes that failure completely, leaving speech to do what it is
good at: roughly 150 words a minute against 50 typed.

Measured on this machine with `large-v3-turbo` at float16: 3.1s to load the
model, then about 0.09s to transcribe five seconds of speech. The model is
preloaded when the palette starts, so the first dictation is as quick as the
rest; set `voice.preload` to false to trade that for ~2GB of VRAM.

Near-silence returns nothing without ever calling the model - Whisper will
happily invent words for a fumbled keypress.

The microphone is now the browser's, so it follows Windows' default input
device and its own permission prompt rather than `voice.input_device` - that
setting only applies to the headless `Recorder`, which nothing currently uses.
Dictation needs the venv (`faster-whisper`, PyAV); without it the window runs
as before and the button reports that it is unavailable.

## Stream Deck

`streamdeck_surface.py` puts one project on each key, coloured by what that
project's session is doing: orange blocked, blue working, green idle, grey no
session. A blocked key carries a badge counting how long it has been waiting.
Pressing a key raises that session, or opens one if nothing is running.

```
.venv\Scripts\python.exe streamdeck_surface.py
```

It is a separate process with its own dependencies (`streamdeck`, `pillow`),
so the dispatcher stays standard-library only, and it holds no state - killing
or restarting it affects nothing else. Pin the projects you want in fixed
positions under `streamdeck.keys` in `config.json`; anything live that is not
pinned fills the remaining keys, blocked first.

The library's only backend is hidapi, which is not on PyPI, so the official
libusb release DLL is vendored at `vendor/hidapi.dll`. The venv's `python.exe`
is a uv shim, so its directory is not on the DLL search path and the surface
registers the vendor directory with `os.add_dll_directory` at import.

## Layout

```
ccontrol/registry.py    who is running, from `claude agents --json`
ccontrol/projects.py    the catalogue, and turning speech into a project
ccontrol/inbox.py       the named-pipe client
ccontrol/deeplink.py    claude-cli://open URLs
ccontrol/background.py  handing work to `claude --bg`
ccontrol/voice.py       dictation, on the local GPU
ccontrol/remote.py      sessions on other machines: polling, injection, tunnels, reading
ccontrol/store.py       tokens and events that outlive a restart
ccontrol/dispatcher.py  the HTTP server and the routing decisions
ccontrol/ui.py          the window: sidebar, conversation, prompt
ccontrol/conversation.py  session messages, ready to render
ccontrol/window.py      opening it chromeless
ccontrol/policy.py      what may be auto-approved, and what may not
ccontrol/winfocus.py    raising a session's window, by process id
ccontrol/changes.py     git status/diff, and background-session worktrees
ccontrol/activity.py    tool-call timelines, "doing" lines, failure tallies
ccontrol/history.py     searching past sessions; resume and fork
ccontrol/usage.py       statusline readings, rate-limit pace, thresholds
ccontrol/prs.py         the PR for a session's branch, off the request path
ccontrol/notify.py      ntfy pushes and spoken alerts (both off by default)
setup_machine.py        installing on a new machine; check; uninstall;
                        onboarding other machines; the service
bootstrap.ps1/.sh       from nothing: prerequisites, clone, setup
ONBOARDING.md           the step-by-step walkthrough
*.example.json          what config.json and policy.json start from
requirements*.txt       core, voice and Stream Deck dependencies
hooks/cc_statusline.py  the statusline: forwards usage, prints a short line
hotkey.py               the global hotkey, and nothing else
streamdeck_surface.py   the Stream Deck, a separate process
service/tasks.ps1       Windows scheduled tasks that keep all three up
hooks/                  the hook, and an installer for user settings
```

Runtime state lives in `%LOCALAPPDATA%\ccontrol\state.json` on Windows
(`$XDG_STATE_HOME/ccontrol`, or `~/.local/state/ccontrol`, elsewhere), not
beside the source: a checkout on a mapped network drive is common, and
Windows refuses an atomic rename-over on SMB often enough to matter for a
file holding tokens.

## Other machines

Hosts listed under `remote` in `config.json` are polled over SSH with the same
`claude agents --json` that answers locally, so a host needs no agent, no
daemon and no open port - just a key you already use. Their sessions appear on
the board tagged with the host.

```
python -m ccontrol.remote                  # probe the configured hosts
python -m ccontrol.remote somehost         # probe one by name
```

The same SSH call reads the host's `~/.claude/sessions/*.json`, which is where
local rows get their status and inbox path, so a remote row says idle, busy or
waiting just as a local one does.

### Adding a host

```
python setup_machine.py remote add HOST            # user@host or an ~/.ssh/config alias
python setup_machine.py remote add HOST --label pi --no-reader
python setup_machine.py remote check               # what each host has and lacks
python setup_machine.py remote remove HOST [--purge]
```

`remote add` walks through everything a host needs, and each step is safe to
repeat:

1. **SSH key login.** If a password-less login fails it offers to create a
   key (`ssh-keygen`) and copy it across - you type the host's password once.
2. **Claude Code** 2.1.224+ there, found by absolute path. If it is missing or
   old it offers Anthropic's installer (`--install-claude` to skip the
   question). Log in once on the host afterwards.
3. **python3** there, which the hooks and prompt delivery use.
4. **The conversation reader** - the Agent SDK in `~/.claude/ccontrol/venv`,
   ~300MB, so the window can show that host's conversations. `--no-reader`
   skips it.
5. **The reporting hooks**, in the host's `~/.claude/settings.json` (backed
   up first), unless `--no-tunnel`.
6. **config.json**: the host entry with `"tunnel": true`, then a dispatcher
   restart.

The pieces are described below, and can still be done by hand
(`python hooks/install.py --remote HOST [--dry-run|--uninstall]`).

### Making a host as controllable as a local session

Two more things make a remote session reachable, and neither needs a daemon on
the host or an open port on either machine:

- **Hooks, through a reverse tunnel.** The dispatcher holds `ssh -N -R` open
  to each tunnelled host, so `127.0.0.1:8792` on the host leads back here and
  the host runs the same `cc_hook.py`. That is what delivers attention
  (permission prompts, failed turns) and, above all, the session's inbox
  token, which Claude Code hands to hooks and nowhere else.
- **Injection, over SSH.** A prompt for a remote session is piped over SSH to
  a one-line `python3` writer that connects to its Unix socket. The token
  travels on stdin, never on a command line the host's `ps` could show.

The tunnel does **not** land on the dispatcher's own port. It lands on a
hook-only listener, one local port per host (8793 upward), which answers
`/hook` and nothing else - otherwise every process on that host could call
`/api/dispatch` into the sessions on this machine. The host an event came from
is known from which listener it arrived at, never from the payload, and its
records are stored under host-qualified keys, so a host can describe only its
own sessions. The tunnel's `ssh` is bound to the dispatcher with a job object,
so killing the dispatcher cannot orphan it holding the host's port.

With that in place, a remote project dispatches like a local one: into its live
session if one has reported in, otherwise into a fresh `claude --bg` there.
Focus still does not cross the wire - there is no window here to raise.

**Conversations** are read on the host, because that is where the transcript
is. The dispatcher keeps one SSH channel per host open and sends it this
project's own `conversation.py`, so a remote pane is built by exactly the same
code as a local one. The host needs the Agent SDK in a venv, which
`remote add` installs (the interpreter is `reader_python` in `config.json`,
default `$HOME/.claude/ccontrol/venv/bin/python`).

A new connection per 1.5s poll would pay the SSH connect and the SDK's
one-off first read (~1s on a Pi) every time; on a kept channel a read is
~10-20ms. The channel dies with the dispatcher, and a host that fails is not
re-dialled for 30 seconds.

Remote projects are host-qualified - `developmenthost1/bookoferrantpages` -
because that name also exists locally, and firing at the wrong machine is the
same class of mistake as firing at the wrong repository.

Polling runs on its own thread every 20 seconds and never on the request path.
A host that fails is parked with a growing backoff and its real reason, so a
machine that is asleep costs nothing and never hides a working one. `claude` is
called by absolute path: it is not on PATH for a non-interactive SSH session on
Ubuntu, because `.bashrc` returns early.

## Auto-approval

At twenty sessions the bottleneck is not how fast agents work, it is how often
they stop to ask. `hooks/cc_permission.py` answers the cheap asks - reading a
file inside the project, `git status`, `pytest` - and forwards everything else
to you unchanged.

The rule that shapes it: **this removes prompts, never answers.** Anything not
provably cheap becomes `ask`, which is exactly the prompt you would have had,
so you keep the power to approve it. Refusing `git push` and `rm -rf` outright
was the first design and it was wrong - it took away the ability to say yes.

`deny` is reserved for one class: an agent changing the machinery that decides
what it is allowed to do - `settings.json`, `policy.json`, the hook scripts,
`policy.py`, or `--dangerously-skip-permissions`. There is no legitimate
mid-task reason to touch those, and the risk is an agent widening its own
authority. Secrets (`.env`, `id_rsa`, `.aws/`, `*.pem`) are never
auto-approved, but they are asked about rather than refused: you may have good
reason to let Claude read one.

Configure in `policy.json`; `"enabled": false` turns it off without touching
hooks. Per-project keys are `auto_edit`, `allow_reads_outside_cwd`,
`safe_bash` and `never_auto_approve`, and the most specific matching project
path wins. Every verdict lands in `%LOCALAPPDATA%\ccontrol\decisions.jsonl`
and on the board, so it is never a black box.

The hook does no network I/O at all - it is on the critical path of every
prompt, so it must not depend on the dispatcher being up. Silence means "ask",
which is what every failure path does.

**Until `"decision_format": "object"` is set in `policy.json`, none of this
takes effect.** Claude Code reads the reply's `decision` as an object,
`{"behavior": "allow"}`; the hook's original reply sent a bare string, which
is ignored. The documented shape is implemented but switched off, so that
turning auto-approval on is a choice rather than a side effect of a bug fix.
The same switch is what lets an answer from the window's Allow/Deny banner or
the Stream Deck's decision keys reach the session (with `park.enabled`).

## Keep it running

`setup_machine.py` registers this for you; `python setup_machine.py service
install|uninstall|restart|status` does it again on its own.

- **Windows:** three scheduled tasks, per-user, no elevation - the
  dispatcher, the Stream Deck and the hotkey (`CentralControl*`).
- **Linux:** systemd user units, `ccontrol-dispatcher.service` (and
  `ccontrol-streamdeck.service`), `Restart=always`. A user unit stops at
  logout unless you run `loginctl enable-linger $USER`.
- **macOS:** launchd agents, `local.ccontrol.dispatcher` (and
  `.streamdeck`), `KeepAlive`, logging to `~/Library/Logs/ccontrol-*.log`.

The Windows tasks can also be driven directly:

```
powershell -ExecutionPolicy Bypass -File service\tasks.ps1 -Action status
powershell -ExecutionPolicy Bypass -File service\tasks.ps1 -Action install
powershell -ExecutionPolicy Bypass -File service\tasks.ps1 -Action restart
powershell -ExecutionPolicy Bypass -File service\tasks.ps1 -Action uninstall
```

The dispatcher has to outlive any single Claude Code session: started from a
session's shell it is a child of that session and dies with it, taking the
board and the Stream Deck with it.

Two things about Task Scheduler are worth knowing before changing this.
`RestartCount` does not bring back a process that was *killed* - the task
records `0xFFFFFFFF`, goes to Ready, and schedules nothing. And a repetition
attached to the logon trigger never fires again, because that trigger's window
opened at logon. What actually self-heals is a second `-Once` trigger starting
immediately and repeating every minute, with `MultipleInstances=IgnoreNew` so
each repeat is discarded for free while the task is running. Measured
recovery after a force-kill: about 20 seconds.

## Deliberately not built

Worktree isolation and mobile steering are first-party now (`claude --bg`,
`claude remote-control`); what is here is the part they leave to you - seeing
and finishing a background session's worktree, and a push that only fires if
a session is still waiting a minute later. See `ROADMAP.md` for what was
surveyed and rejected, and why.

## License

MIT - see [LICENSE](LICENSE).
