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
