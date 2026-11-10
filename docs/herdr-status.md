# herdr integration: status

Date: 2026-11-09, overnight (branch `herdr`, from `mods`).
Plan: `docs/herdr-integration-plan.md`. Option A (Imperatorium as herdr-app mods) was chosen; Phase 1 confirmed it works.

## What exists now

| Part | Where | State |
|---|---|---|
| hc client: Claude Code variables stripped, refuses a core that leaks them | `imperatorium/hcore.py` | Built, unit-tested, used live on a test core |
| Sessions started in hc terminals that herdr-app shows: one space per project, one tab per session (named after the session) | `imperatorium/hosting.py`, setting `terminal.host` | Built, verified on a test core; **off by default** |
| Pairing: hooks report `HC_TERMINAL_ID` and `CLAUDE_PID`; the terminal is accepted only if that `claude` is its program or under it with no other `claude` between (so a `claude -p` inside a session can't take its terminal); rows carry `hc {address, core}` | `hooks/cc_hook.py`, `dispatcher._hc_owned`, `hosting.owns_terminal` | Verified live (hooks do see `CLAUDE_PID`) |
| Typing (slash commands, Send As Typed) and Close through hc | `dispatcher.py` `_type_into`, `_close_row` | Close verified live; typing unit-tested |
| Fork through hc: its own tab, labelled and remembered from its first hook (a fork sends no SessionStart) | `hosting.labels(fork=True)`, `on_hook` | Verified live |
| Restore after a reboot: `claude --resume` typed into kept shells (only into a shell, never another program), or new terminals; a session that ended with reason `other` (shutdown, logout) is kept for it | `hosting.Restorer`, `Control.restore_hosted` | Verified with the holder killed as a reboot stand-in; a real reboot not done |
| herdr-core started at logon (by the dispatcher, clean environment) | `Control.restore_hosted` | Unit-tested only |
| Token so the mods' prompts count as yours (like the window's) | `herdr-token` beside the state; `X-Imperatorium-Herdr` | Verified live: with it a prompt arrives as your words, without it as a plugin message |
| Dictation recorded by the dispatcher | `POST /api/dictate` | Real mic + GPU model run; the button works; real speech not tried (night) |
| Chat mod: also Edit diffs (red/green) and a work line (context, cost, cache time, what it waits on) | `herdr/mods/imperatorium-chat` | Verified live (below) |
| Sessions + meters sidebar mod, with a Matrix button | `herdr/mods/imperatorium-sessions` | Verified live |
| Chat Matrix mod (floating window) | `herdr/mods/imperatorium-matrix` | Verified live |
| History dialog: search past sessions, Resume or Fork | `herdr/mods/imperatorium-history` | Search verified live |
| Layout "imperatorium" | `herdr/layouts/imperatorium.json` | Verified live |
| Installer | `python setup_machine.py herdr install / status / uninstall` | Tested on temp homes and the test window |

Tests: 77 in `test_herdr.py`, 8 in `test_herdrsetup.py`, node tests for the mods' pure modules (`herdr/tests`, run by `test_herdr_mods.py`). Full suite: 1,153 tests, all passing (1 skipped, as before).

An independent review (a separate agent reading `git diff mods..herdr`) found nothing that lets a browser page or a no-token client reach typed input, the mic or as-user prompts. It did find ten correctness issues, and the fixes are in commit `bc16802` and the next: pairing could be claimed by a nested `claude`; prompt text starting with `-` could become an hc option; restore could type into a non-shell; a fork took its parent's tab; shutdown would have cancelled restore; and an unlocked read-modify-write, a token compare that raised on non-ASCII input, and a mic that could stay open. Left as is: a hook can still claim any terminal address, as it already could claim any inbox socket. Hooks are unauthenticated local POSTs, and the PID check now limits what that can do.

## Verified in the window (herdr-app 0.1.14, test home, test dispatcher on port 8811)

- The chat follows the focused terminal, and stays put when you click into it.
- It renders a 4,626-block (2.8 MB) conversation in under 2 s, with markdown, inline code, and Claude's line breaks kept.
- Full/Compact/Hidden per kind of block.
- While following, the newest message sits at the bottom. Scrolling up switches to a scrollable history, and "Latest ↓" goes back.
- A parked Write request showed as a banner in the chat; clicking **Allow** let it through ("Allowed by PermissionRequest hook"), and clicking **Deny** refused it. herdr's own agents list said "Needs you" at the same time.
- Typing into the chat box with the keyboard and clicking **Send** worked; the reply appeared.
- Dragging across a reply and pressing Ctrl+C copied the text.
- The sidebar lists every session, waiting first, with the 5h/7d meters.
  - Clicking a herdr session focused its terminal.
  - Clicking one outside herdr (die-reader) pointed the chat at it.
- The Matrix opened from the sidebar and closed with its ✕.
- New (API) started `claude` in an hc terminal, and herdr adopted it as space "e2e-proj".
- Close ended it, and herdr closed its tab.

Screenshots from these runs are in the session scratchpad, not in the repo: they show real conversations.

## Not done, or only half done

- **A real reboot.** Restore was tested by killing the core's holder, which is what the spike used as a stand-in.
- **Selecting across replies.** Selection works within one reply. Each block is its own text view, so a drag can't span two.
- **Highlighting.** The chat has markdown, code spans and tool lines. The window's extra marks (paths that copy on click, hashes, diff colouring of Edit calls), find, the status panel and per-session drafts across reloads are not built yet.
- **Following the end is a workaround.** gpui-kit's `list()` is hard-wired top-aligned (`vendor/gpui-kit/crates/shell/src/materialize/components/list.rs:130`) and ignores scroll handles. The chat draws a clipped tail while following and switches to a list on scroll-back. A bottom-aligned list upstream would let it scroll smoothly from the end.
- **Remote sessions** work in the chat and sidebar as before (read and send). hc's own remote machines are not wired in.
- **Your live setup is untouched:**
  - your dispatcher (8792) still runs yesterday's code;
  - herdr-app is installed per-user but has no Imperatorium mods in its real home;
  - `terminal.host` is still `terminal`.

## How to try it

1. Restart the dispatcher so it runs this branch: `python impctl.py restart dispatcher`. It then writes `herdr-token` and serves `/api/dictate`. Your checkout is already on `herdr`.
2. `python setup_machine.py herdr install` copies the mods and layout into `~/.config/herdr-app` and turns herdr-app telemetry off.
3. Open herdr-app (Start menu: Herdr). Press Ctrl+Shift+P, type "layout", and pick **imperatorium**. Or run `herdr-app do layout.switch --name imperatorium`.
4. To start sessions in herdr: Settings > Terminals > Start sessions in: **hc**. New, Fork and Resume then open in herdr. Sessions already running in Windows Terminal still show in the sidebar and chat.
5. herdr-app offered an update of herdr-core to 0.1.19 in the test window. I didn't take it; the spike and these tests used 0.1.18.

## Decisions for you

1. Restart the live dispatcher on this branch (step 1)? Nothing changes until `terminal.host` is set to hc, apart from the new endpoints and the token file.
2. Make hc the default for new sessions once you've tried it?
3. File the two upstream requests below? They are drafts; nothing was sent.
4. Merge `herdr` into `mods` or `main` after a look?

## Upstream request drafts (not filed)

**gpui-kit / herdr-app: a bottom-aligned `list()`**

> `list()` always builds `ListState::new(.., ListAlignment::Top, ..)` and ignores `track_scroll`. A chat or log view needs to open at its end and stay there as items are added (GPUI's `ListAlignment::Bottom`). Could `list()` take an alignment option, for example `.align("bottom")`, or honour a `VirtualListScrollHandle` with `scroll_to_bottom`? The current workaround is a `justify_end` clipped column while following, which can't scroll.

**herdr-core: scrub agent variables when a client auto-starts the core**

> Any `hc` command auto-starts the core with the caller's environment. Run from inside a Claude Code session, the core inherits `CLAUDE_CODE_MESSAGING_SOCKET`/`_TOKEN`, `CLAUDE_CODE_CHILD_SESSION` and others, and every terminal it starts gets that session's inbox credentials (and transcripts stop being saved). herdr's TUI already strips these markers. Could the core drop `CLAUDE*` (and similar agent markers) from the environment it hands to terminals, or offer `--clean-env`? Also, `hc server status` starts a core too, so there's no side-effect-free way to ask whether one is running.

## Test leftovers (scratchpad, safe to delete)

- herdr-app test home: `%LOCALAPPDATA%\Temp\hapt`.
- Test worktree `imp-test` (registered in this repo's `git worktree list`), test state, `e2e-proj`: all in the session scratchpad.
- hc session `imptest` (`HC_SESSION=imptest hc server stop`).
