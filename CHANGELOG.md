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

### Changed

- The install scripts do their work in one function called on the last line, so
  a download that is cut short runs nothing.
- `install.sh` says that macOS builds are experimental.

### Breaking

- The Python implementation is gone from this line (the last version with it is
  the tag `last-python`). Install the native binary with the install script.

## [0.1.0] - 2026-11-03

The Python implementation, before the Claude Code mods.
