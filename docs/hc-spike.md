# Spike: herdr-core (`hc`) as a terminal backend

Date: 2026-11-04. hc 0.1.18 on Windows 11 Pro, Claude Code 2.1.318, mod 0.2.0, live dispatcher on 127.0.0.1:8792.
Follows `docs/herdr-gap-analysis.md`.

The question: can Imperatorium get herdr's terminal-hosting features (live screen, raw keys, survival across window close and reboot) by driving `hc` instead of forking herdr, without losing what it already does?

Method: two throwaway Claude Code sessions (Haiku, `--permission-mode default`) in scratchpad git repos, started with `hc run --bg`. They were driven from both sides and then killed in different ways. No Imperatorium code was changed.

## Results

| Check | Result |
|---|---|
| Install | `irm https://releases.herdr.dev/core/install.ps1 \| iex`. Per-user into `%LOCALAPPDATA%\Programs\herdr-core`, SHA-256 checked, user PATH appended, no admin needed. **Pass** |
| Status without integration | Claude Code emits OSC 7501 by itself. `hc get` shows `{app: claude-code, state: idle\|working\|done\|blocked}`, and blocked carries `kind: permission` and the command text. `turn` counts completed turns. **Pass** |
| Imperatorium still sees the sessions | Listed with the right cwd. `pid` matches `hc get .pid`; mod linked (0.2.0); status agrees with `hc` (blocked = `waiting`, attention true). **Pass** |
| Mod inside `hc` | Status line rendered ("N waiting on you"); `/imperatorium` prints mod version and "connected"; a prompt sent with `impctl send` arrived through the mod and was answered. **Pass** |
| Permission parking | A Bash request from an `hc` session parked in the window and was allowed there (`decisions.jsonl`: "answered in the window"). The policy auto-allowed `hostname`. **Pass** |
| `hc send` = typed input | Transcript records `origin: {kind: human}`, `promptSource: typed`, `turnOrigin: human`, identical to a person typing. The mod's prompt records `origin: plugin`. **Pass**, with security consequences (below) |
| Prompt and wait | `hc send --wait` returns when the turn ends (1.9 s for a one-word reply) with the new `turn` and status. **Pass** |
| Slash commands | `/context` and `/imperatorium` typed through `hc` run as real commands. **Pass** |
| Raw keys | `hc keys down enter` accepted the workspace-trust dialog; `esc` declined a permission prompt. **Pass** |
| Interrupt | `hc interrupt` during a long reply: working → idle in about 1 s (`outcome: stopped`). **Pass** |
| Attached viewer killed | `hc attach` in a separate conhost, then the window was force-killed: the session stayed live with the same pid. **Pass** |
| Planned core restart | `hc server restart`: new core pid, same holder, both sessions live and answering. **Pass** |
| Core force-killed | `Stop-Process -Force` on the core: the next `hc` call auto-started a new core and the holder kept both sessions. **Pass** (see the environment hazard) |
| Holder killed (reboot stand-in) | The kept terminal came back at the same address as a fresh `pwsh`; the one that wasn't kept ended `holder_lost`. Claude itself is lost, as on a real reboot. **Pass** |
| Resume after loss | Typing `claude --resume <id> …` into the respawned shell, or `hc run … claude --resume <id>`, restored the conversation (it recalled its last reply). Imperatorium relinked it under the same session id, with the mod. **Pass** |
| Program crash with `keep` | Killing `claude` inside a kept terminal ends it as `exited`; it does **not** respawn. `keep` covers loss (holder, reboot, server stop), not a normal or forced exit. Expected per docs; on Windows a forced kill counts as an exit. |
| Real reboot | **Not tested.** Nothing starts the `hc` core at logon on Windows (no Run key, task or Startup entry), so terminals return only after the first `hc` call. |

## Findings that shape the design

1. **Start `hc` only from a clean environment.** `hc` does not scrub inherited Claude Code variables. A core started from inside a Claude session passed `CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_CODE_MESSAGING_SOCKET` and `CLAUDE_CODE_MESSAGING_TOKEN` to every new terminal. That turned transcript saving off and handed the spike sessions *this* session's inbox credentials. Any `hc` CLI call auto-starts the core if it's down, so the risk recurs.
   - **Design:** Imperatorium supervises the core (start it at logon from the scheduled task, with a scrubbed environment) and refuses to use a core whose terminals show `CLAUDE_*` in their environment.
   - herdr's TUI already strips these markers; worth an upstream issue for `hc`.
2. **Terminals get the core's environment, not the caller's**, plus `HC_TERMINAL_ID`, `HC_TERMINAL_SOCKET` and `HC_SCOPE`. Hooks inherit `HC_TERMINAL_ID`, so `cc_hook.py` can report it and pair a session with its terminal exactly. `pid` works as a fallback.
3. **Typed input is consent.** `hc send` and `hc keys` count as the person: they can approve permission prompts and spend. That is the capability Send As Typed and console typing were built to gate.
   - **Design:** route them through the same rule (window origin plus per-start token), never through `impctl` or peer callers.
   - Note that anything able to open hc's named pipe already has this power. hc's docs say the socket is a full-trust boundary.
4. **Clear the draft before typing.** After an interrupt, Claude Code puts the interrupted prompt back in the input box, and the next `hc send` appended to it. This happened in the spike: the essay prompt and a later prompt reached Claude as one prompt.
   - **Design:** send `ctrl+c` (or check the screen) before every typed send, as `slash.py` already does.
5. **Restore after reboot is buildable now.** The building blocks are kept terminals plus `claude --resume <id>`. A small "these sessions were open" set in `state.json`, replayed at logon, gives roadmap row E1 for `hc`-hosted sessions. Kept terminals cover the case where the holder or machine goes down; a crashed `claude` still needs the dispatcher to notice and resume it.
6. **Test-harness note.** Git Bash rewrites a leading `/word` argument into a Windows path (`/context` → `C:/Program Files/Git/context`). Call `hc` from Python with an argv list or use `MSYS_NO_PATHCONV=1`. The dispatcher's subprocess calls are unaffected.

## Not covered by this spike

- A real reboot, and starting the `hc` core at logon.
- The JSON-RPC API directly (`events.subscribe`, framed attach). Everything here went through the CLI, which wraps the same methods.
- A live terminal in the Imperatorium window (rendering `hc attach` frames in the browser, for example with xterm.js).
- Remote machines (`hc machine add`), including Windows SSH hosts.
- Several cores or sessions, and performance with about 20 sessions.

## Verdict

The hybrid route holds up. Claude Code runs normally inside `hc`, and everything Imperatorium already does keeps working: hooks, mod, policy, parking, transcripts and meters. On top of that, `hc` provides typed input, keys, interrupt, prompt-and-wait, survival across client and core loss, and a restore path, with no fork and no Imperatorium code changes.

The two must-dos before building on it are clean-environment supervision of the core (finding 1) and gating typed input like Send As Typed (finding 3).
