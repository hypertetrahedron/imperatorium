# The native port: Python to one `imperatorium` binary

The port ran from 2026-11-12 to 2026-11-15 on the branch `rust`, and was
tracked in ROADMAP.md ("Native port" rows). It is done: Imperatorium is one
Rust binary, the Python was deleted on 2026-11-15, and the last commit that has
it is the tag `last-python`. This page is the architecture as it is now, and
the record of how it got there. Python file names below are the files at that
tag.

## Why

Imperatorium shipped as a checkout: Python 3.9+, git, a clone, a virtualenv
built on the user's machine, and hooks and scheduled tasks that pointed into
that checkout. Python cost in four places:

- the Agent SDK, used only to read transcripts, is 226 MB and bundles a second
  `claude.exe`;
- every hook event started a Python process (about 95 ms);
- remote hosts needed `python3` and a reader venv, which containers do not
  have;
- the service and the hooks were tied to wherever `pythonw.exe` and the
  checkout happened to be.

The end state is one binary per platform, installed with one command
(`install.ps1` / `install.sh`), that is the dispatcher, the hooks, the remote
reader and the window. Rust rather than Go or Bun: it builds statically for
every target in use (including 32-bit ARM Pis), `hidapi` links in without a
DLL, and whisper-rs covers voice.

## How it was done

Nothing was switched over until it was shown to behave like the Python it
replaced. Each phase ran the same inputs through both implementations (a
differential harness, black-box hook runs, "twin" scripts that drove a Python
dispatcher and the binary side by side, and the Python unit tests ported one by
one). Python stayed the reference until a phase's comparison was clean; then
that phase's Python was deleted.

| Phase | Replaced | Delivered |
|---|---|---|
| 1 | `cc_hook.py`, `cc_statusline.py`, `cc_permission.py`, `policy.py` | `hook`, `statusline`, `permission`; the policy checked on an 86k-case generated corpus. Hooks went from about 97 ms to 5 ms; the release binary was 2.1 MB. |
| 2 | the Agent SDK as transcript reader | `sessions.rs`: chain, info, titles, list, over every transcript on the machine, identical to the SDK. |
| 3 | `dispatcher.py` and the rest of the package | `imperatorium serve`: the whole HTTP API, the window's pages compiled in, `store.rs` writing the state file. Built read-only first, as a shadow beside the Python dispatcher; the cutover was 2026-11-13. |
| 4 | `remote.py`, the remote hook and reader | `remote.rs`: the host runs this same binary (pushed by target triple, SHA-256 checked), so a host needs no `python3`. Containers are hosts whose commands go through `docker exec`. |
| 5 | `voice.py`, `streamdeck_surface.py`, `hotkey.py` | `rust/voice` (`imperatorium-voice`, whisper.cpp, same JSON-lines worker protocol), `streamdeck.rs`, `hotkey.rs`. |
| 6 | `bootstrap.*`, `setup_machine.py`, `service/` | `install`, `uninstall`, `status`, signed releases and `imperatorium update`, the update badge, Vulkan dictation and `voice fetch`. |

Phase details worth keeping:

- **Phase 3 cutover.** The scheduled task `ImperatoriumDispatcher` runs the
  binary under `conhost.exe --headless` (the task is interactive; a console
  program started from it would put a console window on the desktop). The
  binary outlives its conhost, so stopping the task does not stop it.
- **The stream.** `/api/stream` runs until a write fails (the 15 s keepalive
  finds a page that went away). tiny_http cannot close a keep-alive
  connection, so ending the stream after 600 s as Python did left Edge waiting
  on a socket that never spoke again.
- **Remote hosts.** Polling is shell-only (`claude agents --json --all` and
  the session files, one SSH call per host, backoff on failure). Reads go
  through `imperatorium remote-channel` on the host; prompts are injected with
  `imperatorium inbox-send`; hooks come back through an `ssh -R` tunnel to a
  hook-only listener that serves `/hook` and nothing else.
- **Voice.** `rust/voice` is its own workspace because whisper.cpp needs a C++
  compiler and CMake (and CUDA for the GPU build), which the main binary must
  never need. Released Windows builds use Vulkan (57 MB, imports only
  `vulkan-1.dll`); `build-voice.cmd` builds it. The window decodes its own
  recording to 16 kHz mono WAV, since whisper.cpp has no Opus decoder.
- **Releases.** A release is a folder of assets named by target triple,
  `SHA256SUMS`, `manifest.json` and an Ed25519 signature on the manifest
  (`signing.rs`). Stable reads `releases/latest/download`, preview a rolling
  `preview` release. See `docs/releasing.md`. The Windows exe links the C
  runtime statically; the Linux hosts' builds are static musl linked with
  `rust-lld` (`rust/.cargo/config.toml`).

## The binary today

`rust/` is a cargo workspace: `imperatorium` (the binary), `fakecli` (the
stand-in `claude`, `wt` and `ssh` the integration tests put on PATH) and
`release` (build and signing scripts); `voice` is a separate workspace. The
window's pages (`imperatorium/web/`) and `assets/settings_fields.json` are
compiled into the binary (`ui.rs`, `settings.rs`).

Module map of `rust/imperatorium/src/`:

| Area | Modules |
|---|---|
| Entry and command line | `main.rs` (subcommand dispatch), `cli.rs` (the `status`, `send`, `queue`, `history`... commands), `service.rs`, `service_ctl.rs` (scheduled tasks, systemd units, launchd agents), `install.rs`, `update.rs`, `release.rs`, `signing.rs`, `statever.rs` |
| Claude Code hooks and policy | `hooks.rs` (`hook`, `statusline`, `permission`), `http.rs` (the one call hooks make), `policy.rs`, `pending.rs` (decisions waiting for a person), `live.rs` (tool events per session), `usage.rs` |
| The dispatcher | `server/` (`mod.rs`, `route.rs` GET and POST dispatch, `snapshot.rs`, `conv.rs`, `dispatch.rs`, `hook.rs`, `prefs.rs`, `spend.rs`, `alerts.rs`, `views.rs`, `fleet.rs`, `spawn.rs`, `remote_setup.rs`), `store.rs` (state file), `registry.rs` (which sessions exist), `timer.rs`, `timing.rs`, `locks.rs` |
| Transcripts | `transcript.rs`, `sessions.rs`, `conversation.rs`, `sessionstat.rs`, `history.rs`, `timeline.rs`, `activity.rs`, `taskout.rs`, `costs.rs`, `account_spend.rs`, `matrix.rs` |
| Sending to sessions | `inbox.rs`, `slash.rs`, `launch.rs`, `spawn.rs`, `background.rs`, `closing.rs`, `deeplink.rs`, `winfocus.rs`, `opener.rs`, `modlink.rs` (the mod's endpoints) |
| Projects and git | `projects.rs`, `changes.rs`, `overview.rs`, `prs.rs`, `procs.rs`, `proc.rs` |
| Remote machines | `remote.rs`, `remote_cli.rs` |
| Fleet for agents | `fleet.rs`, `mcp.rs` |
| Notifications, voice, devices | `notify.rs`, `voice.rs`, `voice_plan.rs`, `models.rs`, `streamdeck.rs`, `hotkey.rs`, `claude_catalog.rs` |
| Python semantics | `pyish.rs`, `paths.rs` |

## What was kept from Python, and why

The HTTP API, the file formats and the answers had to stay identical so the
window, the Stream Deck, `impctl` and existing state folders needed no change.
So some Python behaviour is reproduced on purpose:

- **`pyish.rs`**: Python truthiness; `str()` and `repr()` of non-strings;
  `json.dumps` bytes (separators, `ensure_ascii`, float repr); Python's `\s`
  and `str.strip()` (which count U+001C..U+001F as whitespace); banker's
  rounding; Python 3.12's compensated float `sum()` (a plain fold drifted by one
  ulp in the cost total).
- **`paths.rs`**: `ntpath.normpath` plus `GetFullPathNameW` plus the lexical
  fallback, and `normcase` through the same `LCMapStringEx` call, because the
  policy's path decisions depend on it.
- **`serde_json` with `preserve_order` and `arbitrary_precision`**: Python
  dicts keep insertion order, which the policy and the byte-for-byte answers
  depend on; transcript numbers pass through exactly as written, so a big
  integer stays exact.
- **Line splitting and times**: `\r\n` and a lone `\r` split lines as Python's
  `read_text` did; `mtime` and `created_at` use Python's float steps.

Deliberate differences, all stricter or fixes, are listed in `policy.rs` and
the goldens' README: `NAME=value cmd` strips only ASCII names; a `safe_bash`
pattern with lookaround or backreferences is an error (so `ask`); `\b` follows
Unicode word characters; hook stdin is read as UTF-8, not cp1252.

## The frozen goldens

Before the Python went, its answers were recorded: each comparison script ran
with `--record DIR` and wrote what Python answered under
`rust/imperatorium/tests/golden/`. Those files are now the spec. The
integration tests in `rust/imperatorium/tests/` replay the same inputs against
the binary (the policy and its 86k-case corpus, the hooks as separate processes,
the transcript readers on a synthetic corpus, the pure checks, install, and
the "twin" dispatcher sessions: hooks, inbox, notify, settings, worktrees,
sessions, remote). The Python unit tests were ported one by one; the ledger
that maps them is archived in `docs/archive/`. See the goldens' own README
(`rust/imperatorium/tests/golden/README.md`) for the layout, the placeholders
and how a golden is changed now.

## Building and testing

```
cd rust
cargo build --release --workspace
cargo test --release --workspace
```

`--workspace` builds `fakecli`. The window's JavaScript tests need node;
`tests/web_js.rs` runs them. Integration tests start dispatchers on free ports
(never 8792, the live dispatcher's) and each test has its own temp home, state
folder and `CLAUDE_CONFIG_DIR`; run the suite one at a time.

Hooks by hand: always set `IMPERATORIUM_ENDPOINT` and
`IMPERATORIUM_PARK_ENDPOINT` to a fake server and `LOCALAPPDATA` to a temp
folder, as the tests do. The park default is the live dispatcher, so a manual
run with parking enabled shows up as an Allow/Deny banner in the real window.

Fast timers: `IMPERATORIUM_TEST_TIME_SCALE` (`timing.rs`) multiplies the
dispatcher's real-time waits. It is read once; unset, empty, zero, negative or
unparsable means 1.0, and at 1.0 every wait is its production value
(`timing::tests::defaults_are_unchanged`). `0.05` turns the 15 s queue sweep
into 0.75 s and the 60 s idle timer into 3 s. Scaled: the queue sweep,
`IDLE_AFTER`, the notifier's per-channel `delay`, a parked permission or
question's hold (clamped to 600 s first), the window streams' intervals, the
update checker's first delay and daily repeat, the PR tracker, and the remote
fleet's poll, backoff and channel retry. Not scaled: timeouts that bound real
work (SSH and HTTP calls, the voice worker's reply) and the voice
`idle_unload` setting.
