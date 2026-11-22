# Changelog

All notable changes to Imperatorium. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/); see [docs/releasing.md](docs/releasing.md).

## [Unreleased]

The first release of the native build: 0.4.0.

### Added

- Imperatorium is now a single native binary, `imperatorium`. There is no
  Python, no virtualenv and nothing else to install. It does everything the
  Python version did: the dispatcher, the window, the hooks, the command-line
  tools (`status`, `send`, `queue`, `history` and the rest) and setting up
  other machines over SSH (`remote add`), which now copy the binary rather
  than a Python tree.
- One-command install on Windows, Linux and macOS: the install script downloads
  the binary, checks its SHA-256, and runs `imperatorium install`.
- `imperatorium update` and the window's update badge, with two channels:
  stable (the default) and preview.
- Signed releases. Each release's `manifest.json` carries an Ed25519 signature
  (`manifest.json.sig`) that the binary checks before it updates; an unsigned
  or wrongly signed manifest is refused. Release files also carry build
  provenance attestations.
- Code-signed Windows binaries. `imperatorium.exe` and `imperatorium-voice.exe`
  carry an Authenticode signature (Azure Artifact Signing, publisher Lester
  Wilhelm), so Windows and antivirus software can tell where they came from.
- Your prompts stand out in the chat: a tinted band, which Settings >
  Highlighting turns off and Settings > Highlight colours recolours.
- View > Group tool calls (off by default) folds each run of tool calls and
  their results into one line that opens on its own.
- A prompt you send shows in the chat at once, marked as sending, then sent
  and waiting for Claude (or not sent, and why), until the conversation has it.
  Prompts that come back joined in one turn (a stopped one leaves no reply
  between them) each count, so none is left marked as waiting.
- Project changes (☰ menu, or View): every line of work in a project at
  once - the main checkout, each worktree, and each branch ahead of its base
  that no worktree has - with the sessions working in each, what is not
  committed, how far ahead and behind, the PR, and the files each touches.
  Files changed in more than one of them come first, each opening to every
  line's diff of it, so a coming merge conflict shows before the merge. Two
  lines only count what each changed since they parted, so a branch made
  from another's unmerged work does not flag the commits they share.
- A session cleared with `/clear` opens its new conversation in the window,
  with the prompt box ready for the first prompt. Another session's clear does
  not take you away from a prompt you are typing.
- A file opened from a path in the chat, or its folder, comes to the front
  instead of opening behind other windows.
- `imperatorium update --version X.Y.Z` installs a specific release, newer or
  older, and `imperatorium update --rollback` returns to the previous binary.
- `imperatorium uninstall --purge` removes the config and state folders as well
  as the hooks, mod and service.
- `IMPERATORIUM_VERSION` (and `-Version` for `install.ps1`) pins the install
  scripts to one release.
- The state folder is versioned, so an older binary refuses to read state that a
  newer one wrote instead of misreading it, which is what makes a rollback safe.
- Slash commands work from the window. Bare `/model`, `/effort`, `/fast` and
  `/output-style` open pickers fed by Claude Code's own list of models and
  levels. `/context`, `/usage`, `/status`, `/help`, `/skills`, `/hooks` and
  `/permissions` open views, and `/config` opens a form. `/resume`, `/export`,
  `/branch`, `/copy` and `/add-dir` use the window's own features. With mod 0.4.0
  a command runs inside the session and its output comes back to the chat.
  Typing `/` in the prompt box lists the session's commands and says which ones
  start a model turn. Commands only the terminal can do are refused with Show
  Window and the reason. New session lets you choose the model and effort.

### Changed

- Find in the chat is one Find button on the session header. It opens a
  small window with the find box, what to search, the match count and the
  earlier/later arrows, which leaves room on the header row. ctrl+F opens it
  too; Esc, its close or the button again closes it and clears the find.
- Show Window is in the session header's View menu instead of a button of its
  own.
- The line under the session's name no longer repeats its window title (the
  gist of its prompt); the tile shows it, and so does Show Current Prompt.

- The install scripts do their work in one function called on the last line, so
  a download that is cut short runs nothing.
- `install.sh` says that macOS builds are experimental.

### Breaking

- The Python implementation is gone from this line (the last version with it is
  the tag `last-python`). Install the native binary with the install script.

## [0.1.0] - 2026-11-03

The Python implementation, before the Claude Code mods.
