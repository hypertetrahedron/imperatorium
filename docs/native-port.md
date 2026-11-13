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

Not ported yet: slash commands typed into a terminal and Send As Typed
(keystrokes into a console: `/api/typed`, a queued `/command`, both
refused as "not ported yet"), close / resume / new / fork (processes and
terminals), `/api/dispatch` (starts sessions), worktree merge/PR/discard,
focus (Win32), herdr-core hosting, dictation, remote hosts. All of these
start, stop or type into real processes, so testing them needs throwaway
`claude` sessions (or terminals) of their own - a decision for the user.

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
