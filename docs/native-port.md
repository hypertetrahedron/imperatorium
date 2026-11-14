# Native port: one `imperatorium` binary

Date: 2026-11-12. Branch `rust`. Tracked in ROADMAP.md ("Native port" rows).

## Why

herdr installs with one command because it ships one finished file. Imperatorium
ships a checkout: Python 3.9+, git, a clone, a venv built on the user's machine,
then hooks and scheduled tasks that point into that checkout. Python itself
costs us in four places:

- the Agent SDK, used only to read transcripts, is 226 MB and bundles a second
  `claude.exe`;
- every hook event starts a Python process (~95 ms on this machine);
- remote hosts need `python3` and a reader venv (containers have neither);
- the service and the hooks are tied to wherever `pythonw.exe` and the
  checkout happen to be.

The end state is a single Rust binary per platform, installed by
`irm .../install.ps1 | iex` or `curl .../install.sh | sh`, that is the
dispatcher, the hooks, the remote reader and (later) the window. Rust rather
than Go or Bun: herdr is Rust and the window plan (Option B,
`docs/herdr-integration-plan.md`) is Tauri; `hidapi` builds in statically (no
DLL); whisper-rs covers voice; and every target we use builds, including
32-bit ARM Pis, which Bun does not.

## How: the Python is the spec

Nothing is switched over until it is shown to behave like the Python it
replaces. Each phase adds to `rust/conformance/`, which runs the same inputs
through both implementations, or runs the existing Python tests with the Rust
implementation patched in. Python stays the reference until a phase's
conformance passes; then its Python is deleted.

| Phase | Replaces | Conformance |
|---|---|---|
| 1 | `hooks/cc_hook.py`, `cc_statusline.py`, `cc_permission.py`, `imperatorium/policy.py` | `test_policy.py` against the binary; an 86k-case differential corpus; black-box hook runs against a fake dispatcher, compared byte for byte |
| 2 | the Agent SDK, as the transcript reader under `conversation.py` and `history.py` | `sessions_compare.py`: every transcript here through the SDK and the binary, from the session list to the rendered items |
| 3 | `dispatcher.py` and the rest of the package, `sessionstat.py` and `costs.py` included; UI files compiled into the binary | the HTTP-level tests in `test_dispatcher.py`/`test_features.py`, pointed at the binary; `sessionstat`/`costs` differential on the same transcripts |
| 4 | `remote.py`, remote hooks and reader (the remote `sessionstat` too) | the binary pushed to each host by target triple, SHA-256 checked; `test_remote` |
| 5 | `voice.py` (as `imperatorium-voice`, an optional download), `streamdeck_surface.py`, `hotkey.py` | the voice worker's JSON-lines protocol is kept, so either worker serves the same dispatcher |
| 6 | `bootstrap.*`, `setup_machine.py`, `service/` | install, update, uninstall on clean VMs (never `-Yes` against this machine) |

## Phase 1, as built

```
imperatorium hook                    # was hooks/cc_hook.py
imperatorium statusline              # was hooks/cc_statusline.py
imperatorium permission [--home DIR] # was hooks/cc_permission.py
imperatorium policy-decide           # conformance only: JSON lines in, verdicts out
```

`--home` is where `policy.json` lives (also `IMPERATORIUM_HOME`, else
`%APPDATA%\imperatorium` / `~/.config/imperatorium`); `IMPERATORIUM_POLICY`
still wins. While the checkout exists, `hooks/install.py --native BIN` writes
hooks that pass `--home <checkout>`, so both implementations read the same file.

What had to be reproduced exactly, and where it lives (`src/pyish.rs`,
`src/paths.rs`): Python truthiness; `str()`/`repr()` of non-strings;
`json.dumps`' bytes (separators, `ensure_ascii`, float repr); Python's `\s`
and `str.strip()`, which count U+001C..U+001F as whitespace; banker's
rounding; `ntpath.normpath` + `GetFullPathNameW` + the lexical fallback, and
`normcase` via the same `LCMapStringEx` call; dict insertion order
(`serde_json` with `preserve_order`).

Deliberate differences, all stricter (listed in `src/policy.rs`):

- `NAME=value cmd` strips only ASCII names (bash accepts no others);
- a `safe_bash` pattern using lookaround or backreferences is an error, so `ask`;
- `\b` follows Unicode word characters, which changes only which `ask` reason
  is given for inputs such as `½git push`.

And one fix: Python reads hook stdin as cp1252 on Windows, which garbles
non-ASCII text; the binary reads UTF-8.

Found by the black-box run: `cc_permission.py` called `_note`, which it never
defined, so a failed park (dispatcher 500 or non-JSON) died with NameError
instead of falling through. Fixed in the Python on this branch.

Measured (this machine, release build, `rust/conformance/bench_hooks.py`):

| | Python | Rust |
|---|---|---|
| reporting hook | 97 ms | 5 ms |
| statusline | 94 ms | 5 ms |
| permission (policy answers) | 80 ms | 15 ms |

The release binary is 2.1 MB.

## Phase 2, as built

```
imperatorium sessions chain <id>      # the conversation chain, compactions put back
imperatorium sessions info <id>       # get_session_info, or null
imperatorium sessions titles <id>...  # {id: title}, every tile in one process
imperatorium sessions list            # list_sessions() across every project
```

`src/sessions.rs` ports what Imperatorium used of the SDK's private reader
(0.2.155): finding the file, `_parse_transcript_entries`,
`_build_conversation_chain`, the head/tail metadata scan behind
`get_session_info`/`list_sessions`, plus `conversation.with_compacted`.
`conversation.py` keeps its classification (visible, inbox, queued, notes)
and everything after it, so the window renders exactly what it did.
`imperatorium/native.py` finds the binary (`IMPERATORIUM_BIN`, the installed
copy, then `rust/target/release`) and runs it through `subproc.run` (no
console window, a job object, a timeout). With no binary, or for a
`directory=` read, the SDK path is unchanged; remote hosts use that until
phase 4.

Verified by `rust/conformance/sessions_compare.py` on this machine: 129
sessions, 46,081 chain entries (46 compactions restored, 535 inbox and
queued prompts), 72 listed sessions: identical in the list, every `info`,
every chain and every rendered item. The full Python suite passes with the
binary in place (1,180) and also under a Python with no SDK at all.

Kept faithful: numbers pass through as written (`serde_json`'s
`arbitrary_precision`), so a big integer stays exact as in Python; `\r\n`
and a lone `\r` split lines as Python's `read_text` does; `mtime` and
`created_at` are computed with Python's float steps, so milliseconds agree.

Speed is about the SDK's (the chain goes Rust -> JSON -> Python, which
phase 3 removes). Titles were a regression at first: the dispatcher asks for
every tile's title on each 2 s snapshot, and one process each cost about
15 ms against the SDK's 1-3 ms in-process. `sessions titles` takes them all
in one call (20 titles in about 40 ms).

The SDK stays in `requirements.txt` until the installer ships the binary
(phase 6); with the binary installed it is not imported at all.

`sessionstat.py` and `costs.py` moved to phase 3. They read transcripts
incrementally inside the dispatcher, and calling a binary from Python on
every poll would cost a process per session per snapshot; ported, they run
in-process once the dispatcher is Rust.

## Phase 3, in progress

The dispatcher, endpoint by endpoint. `imperatorium serve [--port 8811]
[--home DIR]` runs beside the Python dispatcher as a **shadow**: it reads
config.json, state.json and the transcripts, writes nothing, and
`rust/conformance/shadow.py` sends both the same GETs and compares the JSON.
The HTTP API stays exactly as it is, so the window, the herdr mods, the
Stream Deck and impctl need no change at the cutover.

Step 1 is the read-only endpoints. Done so far:

- `src/conversation.rs`: the chain's classification (inbox and queued
  prompts, system notes) and `to_items`, `merge_turns`, `tail_blocks`.
  `items_compare.py`: 125 sessions, 25,133 items, identical merged and not.
- `src/server.rs` (tiny_http, a thread per request) with `GET /api/health`
  (`look` ported from settings.look) and `GET /api/conversation` (`limit`,
  `blocks`, `known`; convcache's per-transcript cache and path memo; the
  same SHA-1 signature, so "unchanged" answers match).
- `src/registry.rs`, read-only: `claude agents --json --all`, the session
  files and state.json's hook data, less dismissed and hidden rows - enough
  for `_row`'s name/cwd, with its 15 s / 2 s lookup rule.

`shadow.py` against the live dispatcher: 144 requests over 27 sessions
(tiles and recent History; full, limit, blocks, known, stale known; health,
404, missing session), identical. Answers are as fast or faster than
Python's once warm: the full conversation is spliced in from items
serialised once per change, and bodies carry Content-Length (tiny_http
would otherwise go chunked above 32 KiB).

`GET /api/sessions` followed: `registry.rs` now ports the whole merge
(credential pairing, attention, `moved_on`/`still_running`, `place`,
labels), `procs.rs` the shells under each session (Toolhelp on Windows, `ps`
elsewhere), `sessionstat.rs` the incremental transcript fold
(`stats_compare.py`: 125 sessions x 4 clocks identical, types included),
`costs.rs` `resolve`/`rewrite_cost`, `activity.rs` the doing line; the
server adds the overlay, titles, decorate, dismiss and hide, and sends rows
through `public()` (no tokens). `shadow.py` compares every row the binary
can know: 190 fields over 6 live tiles identical. Left out, and listed in
the comparator: what only the live dispatcher is fed by POSTs (`usage`,
`pr`, `mod`, `mod_pending`, a live hook's `doing`), and on mod-reported rows
the `status`/`dispatchable`/`work.alive` its beat rewrites - step 2.

A cold snapshot costs about 1.4 s, nearly all of it `claude agents` itself
(Claude Code 2.1.323 here, timed alone). Python pays the same; its open
window's `/api/stream` keeps the cache warm so requests rarely see it. The
binary will do the same once `/api/stream` is ported.

Then the window itself: `rust/assets/export_ui.py` writes the three pages
from ui.py (with `_with_meta` switched off) and settings.FIELDS as JSON;
the binary embeds them, adds the token, boot id and `look_head` as
`_with_meta` does, and serves the fonts. A Python test fails when ui.py or
settings.py changes without an export. Also ported: `/api/history` (list,
search, snippets), `/api/stream` (the SSE feed, timing as Python's), and
`/api/events`, `/queue`, `/prefs`, `/decisions`, `/quick`. shadow.py: 165
requests identical - the pages byte for byte bar the per-start token and
boot id.

Then the rest of the read side (2026-11-12/13, overnight): `/api/settings`
and `/api/notify` (settings.describe over the exported schema),
`/api/projects` (projects.discover), `/api/changes`, `/api/worktrees` and
`/api/worktree_diff` (git, bounded as in changes.py), `/api/activity` and
`/api/errors` (timeline, categories, tallies), `/api/costs` and
`/api/session_status` (costs.History: the incremental ledger, every float
added in Python's order - the 30-day total agrees to the last digit, $680
over 1.6 billion tokens), and `/api/awaited` (taskout.py). shadow.py: 360
requests identical.

Two Python behaviours the shadow caught that a port has to copy:
- Python 3.12's `sum()` of floats is Neumaier-compensated (`pyish::
  py_fsum_builtin`); a plain fold drifted by one ulp in the cost total.
- Answers each side caches (History 30 s, errors 5 min) differ by whatever
  live sessions did between the two computations; the comparator matches
  History by session id and compares errors only when both are fresh.

### Step 2: the routes that change things

Not testable against the live dispatcher, so `rust/conformance/twin.py`
runs a throwaway Python dispatcher (`--state` in a temp folder, port 8813,
no config.json) beside the binary (`serve --state ...`, port 8814), sends
both the same POSTs for made-up session ids, and compares the answers, the
state files and the GETs that read them. `store.rs` writes only with
`--state`, and refuses the live state file outright; without `--state`
the binary stays the read-only shadow and refuses POSTs.

Done: `POST /hook` (store.rs, live.rs: SessionStart/End, Notification,
StopFailure, Stop and the rest; tool events), statusline usage (usage.rs:
readings, windows in limits.json, threshold crossings and the budget event,
pace, plan) with `/api/usage`, spend and `/api/matrix`, and the routes that
change only the dispatcher's own state (`/api/prefs`, `/api/notify`,
`/api/unqueue`, `/api/dismiss`, `/api/queue`'s refusals). twin.py: 32 hook
events, 7 refused POSTs and 13 API POSTs identical.

Then, 2026-11-13, the parts that reach sessions, all tested so that none
can reach a real one:

- `rust/conformance/inbox_twin.py` gives each twin a `CLAUDE_CONFIG_DIR`
  of its own: a temporary folder whose `sessions/` holds made-up session
  records, each pointing at a named pipe the script serves and records. A
  real session's inbox is never named and its token never known.
  - Delivery (inbox.rs, the queue in store.rs, enqueue / drain / the
    15-second sweep / Stop's drain, `_deliver`): idle gets it at once,
    busy waits for Stop (one per turn), waiting on you holds it, a gone
    pipe keeps it with the error, a token arriving late is swept, from the
    window is marked as the person's own. Bytes delivered identical.
  - Parked permissions (pending.rs, `/permission/park`, `/api/decide`,
    `/api/pending`, park_wait): 17 steps identical, outcomes checked.
  - The mod link (modlink.rs, `/api/mod/beat|check`, the token beside the
    state, the row's mod fields, delivery through a live mod): 19 steps
    identical, with a temporary copy of the policy (auto-approval on) so
    `mod/check` decides; decisions.jsonl identical.
- `rust/conformance/notify_twin.py`: ntfy pushes (notify.rs; ureq now has
  rustls for https servers) to a fake server on 127.0.0.1, the Python
  twin run from a temporary copy of the package so it can have a
  config.json. Permission prompt, failed turn, usage threshold and the
  idle check a minute after Stop pushed identically; nothing for a prompt
  answered within the delay or with ntfy off. Speech is unit-tested only
  (it would talk out loud). The shadow never pushes.
- `_overlay` now clears settled attention in a store of its own, as Python
  does.

- Saving Settings (settings.rs: check for all 15 field types, apply, the
  atomic save with `.bak`, `json.dumps(indent=2)` byte for byte, restarts;
  applied live only where Python applies it). `settings_compare.py`: 8,424
  cases, every field against Python's `check`, identical (it found
  `py_float` too lenient with underscores). `settings_twin.py`: 8 saves
  into temporary homes, answers / config.json / .bak / the GETs after
  identical. The shadow refuses to save.

- Worktree merge / PR / discard (`changes::finish`, `/api/worktree`).
  `worktree_twin.py`: each twin on its own copy of a throwaway project with
  a local bare origin; `gh` gets an empty config folder and no token, so a
  PR's push lands locally and `gh pr create` stops at "gh auth login". 18
  actions, the repositories left behind and the events identical.
- Resume / fork / new / close (launch.rs, background.rs, closing.rs with
  an `imperatorium type-into` child that types Ctrl+C, `/exit`, Enter).
  `rust/conformance/fakecli` stands in for `claude.exe` and `wt.exe` on the
  twins' PATH, logging what it is asked, and can be a raw console that
  leaves on `/exit` (or a stubborn one that must be killed).
  `session_twin.py`: 17 actions, launches, keystrokes and events
  identical. It found a Python bug, fixed on this branch: `claude --bg`
  inherited the dispatcher's session variables. Then one real smoke test
  (2026-11-13, binary only, user-approved): a new Windows Terminal tab with
  a fresh `claude`, registered in 9 s, closed by `/exit` in 2.8 s, tab gone.
- Release builds unwind rather than abort, so a panic in one thread of
  `serve` costs only that thread; start time unchanged (~6 ms).

- Slash commands and Send As Typed (slash.rs: refusal, unreachable,
  blocked, clean_typed with Python 3.12's Unicode 15 format characters,
  typed_refusal; run_command, send_typed, `/api/typed` only from the
  window, a queued `/command` typed when its turn comes).
  `slash_compare.py`: 127 texts identical. `session_twin.py` now 29
  actions: text cleaned and typed, a command typed, refusals (menus,
  `/exit`, blank, busy, a parked permission), the queue's - identical, and
  the console received exactly what was meant.

- `/api/dispatch` (projects.rs: `_norm`, difflib's `SequenceMatcher.ratio`
  as CPython computes it, resolve, parse_utterance; deeplink.rs; dispatch,
  `_exact_project`, `_session_for`). `projects_compare.py`: 595 cases -
  333 spoken names over this machine's 38 projects and made-up colliding
  ones, norms, utterances, deep links - identical to the rounded score.
  `session_twin.py`, 43 actions now: refusals, a dry run, a misheard and an
  unknown name, a command routed by project name into that project's own
  console, inject to an unreachable inbox, a background start - identical.
  Never tested live: mode `launch` (and `auto` falling back to it), since
  `cmd /c start` resolves from System32 ahead of PATH and would open a real
  Claude window; the URL it opens is covered by the differential. Remote
  projects answer "not ported yet" until phase 4.

- `/api/focus` (winfocus.rs: title normalising and matching, the window
  enumeration, the terminal a session sits in, SetForegroundWindow with the
  AttachThreadInput borrow; `pyish::casefold`). `focus_compare.py`: 1,007
  made-up titles and matches, then the read-only half on this machine's
  real windows (20 windows, their owners, the terminal of each of 5
  running `claude`) - identical. `session_twin.py`, 48 actions: focus's
  refusals. Never run here: an actual raise, which would take the keyboard
  focus from whoever is at the machine - the first press of Focus on a
  binary build is its test.

Not ported yet: herdr-core hosting (hc terminals: refused as not ported),
the herdr mods' `/api/dictate` (it records from the dispatcher's own mic),
remote hosts.

### Step 3: the cutover (2026-11-13)

Closed first, so the switch lost nothing the window uses on this machine:
`prs.rs` (each session's pull request, polled with `gh` off the request
path), `voice.rs` (the dispatcher side of dictation: `/api/transcribe` and
`/api/voice/warm` drive the same `python -m imperatorium.voice --worker`
child over its JSON-lines protocol, from the checkout's venv; preload,
idle unload, the 180 s reply limit; a synthesized WAV came back word for
word, 3.6 s cold and 0.13 s warm on CUDA), `/api/open_window`, `/api/hosts`
(hosts since phase 4), the spend warm-up at start, and `serve --live`: the
live state file (which a store otherwise refuses) on port 8792.

The task `ImperatoriumDispatcher` now runs
`conhost.exe --headless <installed imperatorium.exe> serve --live --home
<checkout>`: the task is interactive, and a console program started from it
would put a console window on the desktop; a headless conhost shows none.
The binary outlives its conhost, so stopping the task does not stop it - end
the `imperatorium.exe` whose command line has `serve --live`.
`%LOCALAPPDATA%\imperatorium\cutover\` holds the Python task definition, the
state file as it was and the phase-2 binary, with `rollback-to-python.ps1`.

Lost at the switch until phase 4: remote hosts and their hook tunnels (both
configured hosts had no sessions at the time).

Fixed after the switch: the window froze about ten minutes after each load.
Python ends `/api/stream` after 600 s and the page reconnects; tiny_http
cannot close a keep-alive connection, so the socket stayed open, Edge waited
on a stream that never spoke again, and never fell back to polling. The
stream now runs until a write fails (the 15 s keepalive finds a page that
went away). A stream held open for 665 s kept its pushes coming.

## Phase 4, as built (2026-11-13)

`remote.rs` ports `remote.py`, and the host's side is this same binary, not
python3:

- **Polling** is unchanged and shell-only: `claude agents --json --all` and
  `~/.claude/sessions/*.json` in one SSH call, merged by `registry::merge`;
  a host on its own thread, parked with a growing backoff when it fails.
- **The binary on the host.** `Deploy` reads `uname -m` and the hash of
  `~/.claude/imperatorium/bin/imperatorium` there; when it differs from this
  machine's build for that triple it is sent over the SSH channel, written
  beside and renamed over, and hashed again. Builds are found beside the
  installed exe in `targets/<triple>/imperatorium` (or a cargo target dir's
  own cross builds). Static musl builds for x86_64 and aarch64, linked with
  the toolchain's `rust-lld` (`rust/.cargo/config.toml`) and built with
  `--no-default-features`: the `tls` feature (ntfy to an https server) is
  the only part needing a C compiler for the target, and a host never sends
  a push.
- **Channels.** `imperatorium remote-channel` on the host answers JSON
  lines: `read` (the phase-2 transcript reader and `to_items`, with the
  session's title) and `stats` (sessionstat's incremental reader). One
  channel for each per host, kept open, serialised, killed on a timeout,
  re-dialled no sooner than 30 s after a failure. No Agent SDK venv is
  needed on the host any more.
- **Injection.** `imperatorium inbox-send SOCKET` on the host, the frames on
  stdin (the token never on a command line).
- **Hooks.** `Tunnel` holds `ssh -N -R 127.0.0.1:8792:127.0.0.1:<local>`;
  `hook_only` is the listener it lands on, `/hook` and nothing else, the
  host known from the listener. `on_hook(payload, host)` keeps a host's
  records under `host/sid`. Every ssh is in one kill-on-close job object, so
  none outlives the dispatcher holding a host's port.
- Remote rows, conversations, Matrix tiles, delivery, the queue, dispatch
  (inject, else `claude --bg` there), the host-qualified catalogue,
  `/api/hosts`, `/api/remote/refresh`, and Python's refusals for what does
  not cross the wire (focus, changes, activity, status, resume, new, close,
  dismiss).
- `hooks/install.py --remote HOST --native host` points a host's hooks at
  that binary, so the host needs no python3 at all.

Verified: `remote_compare.py` against both real hosts (every read and the
stats identical to Python's Reader and Stats under the host's SDK venv);
`remote_hook_compare.sh` on both hosts (the binary's `hook` posts what
today's `cc_hook.py` posts, 5 events); `inbox-send` into a throwaway Unix
socket on both; the live tunnels and hook-only listener from both hosts;
`remote_twin.py` (32 checks, fake ssh) in `run_all.py`.

## Phase 5, as built (2026-11-13)

**Voice.** `rust/voice` is `imperatorium-voice`, its own crate and
workspace (whisper.cpp needs a C++ compiler, CMake and, for the GPU, CUDA,
which the main binary must never need). It speaks the Python worker's
protocol (`--worker SETTINGS_JSON`, `load` / `transcribe` JSON lines), so
`voice.rs` only chooses what to spawn: the exe beside `imperatorium.exe`
when the settings' model is in the models folder
(`%LOCALAPPDATA%\imperatorium\models\ggml-<model>.bin`), else the venv's
Python worker; `IMPERATORIUM_VOICE=python` forces Python.

The same choices as faster-whisper: large-v3-turbo on CUDA, the CPU model
when there is no GPU, greedy at beam_size 1, Silero VAD (v6, as
faster-whisper bundles) with faster-whisper's VadOptions defaults, nothing
for near-silence. The worker reads WAV only: the window now decodes its own
MediaRecorder output (WebM/Opus) with `decodeAudioData`, resamples it to
16 kHz mono in an `OfflineAudioContext` and uploads a WAV (`wav16k` in
`ui.py`); whisper.cpp has no Opus decoder, and faster-whisper reads WAV too.

Models are ggml files from `ggerganov/whisper.cpp` and `ggml-org/whisper-vad`
on Hugging Face, checked against the SHA-256 the repository lists
(phase 6 fetches them when dictation is turned on).

Build: `rust\voice\build-voice.cmd` (VS Build Tools' MSVC, its bundled
CMake and Ninja, CUDA 12.8). bindgen needs `libclang.dll`: set
`LIBCLANG_PATH` (LLVM, VS's Clang tools, or `pip install --target DIR
libclang`). The bindings whisper-rs-sys ships were made on Linux and do not
fit Windows.

Verified: `voice_compare.py` (11 clips through both workers: 10 identical,
the eleventh the native one right where Python misheard; silence, noise and
trailing silence empty or clean; median 0.16 s against Python's 0.26 s);
the window's path in a real browser (MediaRecorder WebM -> `wav16k` ->
`/api/transcribe` -> text); live on the dispatcher (CUDA, 0.1-0.2 s).

**Stream Deck.** `imperatorium streamdeck` (feature `streamdeck`, on by
default, left out of the hosts' builds) ports `streamdeck_surface.py`:
layout, pinned keys, flashing, the three decision keys and their arming,
press to focus. `elgato-streamdeck` over hidapi's `windows-native` backend
(no hidapi.dll), keys drawn with `image` and `ab_glyph` in Segoe UI.
`--preview DIR` writes one frame as PNGs: side by side with Python's
`render_key` the keys match. Task `ImperatoriumStreamDeck` runs it under
`conhost --headless`; its Python definition is in `cutover\`, and
`rollback-to-python.ps1` restores it.

## Phase 6, in progress (2026-11-13)

The user's decisions: build and test everything against a local mock
release server and publish nothing until reviewed; releases use Vulkan for
GPU dictation (the CUDA build needs ~770 MB of cuBLAS DLLs beside it).
Signing is deferred until there is a certificate.

**6a `imperatorium install | uninstall | status`** (`install.rs`): the
binary sets a machine up alone - prerequisites, choices (flags, else asked
at a terminal, else defaults), the binary placed in
`%LOCALAPPDATA%\Programs\imperatorium` or `~/.local/bin`, `config.json` and
`policy.json` from the embedded examples, the mod's files written to the
state folder, settings.json edited exactly as `hooks/install.py --native`
does (`install_compare.py`: 80 cases identical, line endings aside), and the
service: scheduled tasks shaped as `service/tasks.ps1` shapes them (under a
headless conhost), systemd user units, launchd agents. `--dry-run`,
`--task-prefix`, `--port`, `--state` and `--settings` let a test install a
separate copy beside the real one.

**6b `imperatorium hotkey`** (`hotkey.rs`): `hotkey.py` without Python.

**6c releases and self-update** (`update.rs`, `rust/release/`,
`install.ps1`, `install.cmd`, `install.sh`, `.github/workflows/release.yml`).
A release is a folder of assets named by target triple, `SHA256SUMS` and
`manifest.json`; stable reads `releases/latest/download`, preview a rolling
`preview` release; `--from` or `IMPERATORIUM_RELEASES` point anywhere.
`update` checks SHA-256, renames the running exe aside, refreshes the voice
worker and the hosts' builds, and restarts only processes started from its
own exe path. The Windows exe links the C runtime statically. A local
release: `powershell -File rust\release\build.ps1 -Version 0.3.1-preview.1`,
then serve `dist\` over HTTP.

**6d Vulkan dictation and `imperatorium voice fetch | status`**
(`models.rs`, the voice crate's `vulkan` feature). `voice fetch` takes the
worker from the release beside the installed exe and the ggml models from
whisper.cpp's Hugging Face repositories, each kept only if its SHA-256 is
the one embedded here. The released Windows worker is the Vulkan build:
57 MB, imports only `vulkan-1.dll` (every GPU driver installs it; without
it the worker cannot start, which `fetch` and `status` say) besides
system DLLs. Against the CUDA build over the conformance clips
(`voice_compare.py --against CUDA_EXE --native VULKAN_EXE`): 10 of 11
identical (Vulkan heard "poll request" once), 0.11 s a clip against
0.06 s, the CPU fallback works; the very first run fills the driver's
pipeline cache (~4.5 s once). This machine keeps the CUDA build.

Building it: `build-voice.cmd build --release --no-default-features
--features vulkan --target-dir SHORT` with `VULKAN_SDK` set. The target
folder must be under ~35 characters - the shader generator is a CMake
sub-build nesting ~225 below it, and Windows' limit is 260. Both builds
link the C runtime statically: whisper.cpp asks for CMake 3.5, so
`build-voice.cmd` sets `CMAKE_POLICY_DEFAULT_CMP0091=NEW` and
`CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` (whisper-rs-sys passes `CMAKE_*`
through). The worker's `--devices` prints the backend and the GPU ggml
sees; GPU detection asks ggml's device list rather than looking for
`nvcuda.dll`.

**6e the update badge and What's New** (gap J3; `update::Checker`,
`/api/update`, `/api/update/check`, `/api/update/apply`; settings
`updates.check`, `updates.channel`). The dispatcher asks the release three
minutes after start and every six hours; the window shows an `update`
badge by the Settings button while a newer version is out, and Settings >
Updates shows this version, the newest, the release's notes (`notes` in
manifest.json: the tag's annotation in the workflow, `-Notes FILE` in
build.ps1) and an Update button. Only the window may apply (its token); it
runs `imperatorium update` as a detached child, which outlives the
dispatcher it restarts. `update` now finds the tasks to restart by what
they run - this exe - not by name, so a `--task-prefix` install is
restarted and another install's tasks are left alone (it prints which);
systemd units likewise by ExecStart. The dispatcher restarts last.
Tested with a throwaway task running a sandbox dispatcher on 8811 against
a mock release: badge, panel, notes shown as text, Update -> binary
swapped and checked, task restarted on the new version, the live tasks
untouched. The Python dispatcher answers 404, and the window shows no
badge and says it updates by git.

## Building

```
cd rust
cargo build --release
python conformance/run_all.py --release
```

On Windows this machine uses rustup's `x86_64-pc-windows-gnu` toolchain
(no Visual Studio Build Tools installed). Crates that need `dlltool` for
raw-dylib imports (chrono's `windows-link`) do not build there, so the crate
avoids them; with the MSVC toolchain this does not arise. Release builds for
distribution will use MSVC (and code signing) in phase 6.

## Testing hooks by hand

Always set `IMPERATORIUM_ENDPOINT` and `IMPERATORIUM_PARK_ENDPOINT` to a fake
server and `LOCALAPPDATA` to a temp folder, as the conformance scripts do. The
park default is the live dispatcher: a manual run with parking enabled shows up
as an Allow/Deny banner in the real window.
