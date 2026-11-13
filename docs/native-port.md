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
| 2 | `conversation.py`, `history.py`, `sessionstat.py`, `costs.py`, and the Agent SDK | the same transcripts through both readers; `test_conversation`, `test_sessionstat`, `test_costs` |
| 3 | `dispatcher.py` and the rest of the package; UI files compiled into the binary | the HTTP-level tests in `test_dispatcher.py`/`test_features.py`, pointed at the binary |
| 4 | `remote.py`, remote hooks and reader | the binary pushed to each host by target triple, SHA-256 checked; `test_remote` |
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
