# Herdr integration plan

Date: 2026-11-09. Follows `docs/herdr-gap-analysis.md` (2026-11-04) and `docs/hc-spike.md` (2026-11-04).

## Goal

One window with:

- herdr's windowing: spaces, tabs, split panes, live terminals that survive the window closing and come back after a reboot;
- Imperatorium's chat view, since it shows a session better than a terminal screen does;
- Imperatorium's extras: dictation, the Chat Matrix, meters and costs, permission parking, AskUserQuestion answers, History, Changes, the Stream Deck.

A rewrite in Rust is allowed, especially if it removes the window problems we kept hitting: redraws that ate clicks, settings and the Matrix opening as separate Edge windows, and no panes.

## What changed since the gap analysis

1. **The spike settled terminal hosting.** Claude Code runs normally inside herdr-core (`hc`). Hooks, the mod, policy, parking, transcripts and meters all kept working, and `hc` added typed input, keys, interrupt, prompt-and-wait, survival and restore. So we don't need to build our own ConPTY host, or move to WezTerm or amux.
2. **herdr-app is the windowing system, and it's built to be extended.** Its window is laid out by *layouts*, and its panels are drawn by *mods*: JavaScript that describes native GPUI elements. Mods can call `fetch` and `exec`, open dialogs and floating windows, post toasts, bind keys, show a live terminal inside a block, and render markdown (`TextView.markdown`) and read-only code with syntax highlighting (`Editor`). Mods **can't** use HTML, CSS or a web view.
3. **herdr-core has a stable, published API.** JSON-RPC over a named pipe, described in `schema/hc.openrpc.json`, with a promise that it only grows. Any client can attach to a terminal over it (framed mode). So "write our own window" is a real option too, not only "fork herdr".

## The split that holds in every option

| Layer | Owner | Notes |
|---|---|---|
| Terminals: PTYs, keep/restore, remote machines | **herdr-core (`hc`)**, unmodified | Proven by the spike |
| Claude Code knowledge: hooks, mod, policy, parking, transcripts, costs, meters, voice worker, Stream Deck, History | **Imperatorium dispatcher** (Python, as today) | This is the asset. Nothing in the goal needs it rewritten |
| The window: layout, chat, Matrix, sidebar | **Being decided** | See the options below |

The dispatcher keeps its HTTP API. Whatever window we choose is a client of both the dispatcher (`127.0.0.1:8792`) and `hc`.

## The window: three options

### Option A: Imperatorium as herdr-app mods (no fork)

herdr-app draws the window. Imperatorium ships a mod pack: a **chat** pane, a **Matrix** pane, an **Imperatorium sidebar** (or extra rows in herdr's own agents list), a **parking/question** dialog and a **meters** strip. Each mod reads from the dispatcher with `fetch`. A session opens as a tab with its terminal and its chat side by side.

- Pros: real herdr windowing, maintained upstream (layouts, splits, peek, keys, palette, themes, remote machines, self-update). The least code we'd own. Native drawing ends the DOM redraw bugs.
- Cons: the chat (about 4,400 lines in `ui.py`, mostly HTML, CSS and JS) is **rewritten** against a preview API with no DOM. Not yet known: whether text can be selected and copied in `TextView`, whether our own highlighting (paths, hashes, diffs, prose marks) can be styled run by run, how a long transcript scrolls, and that there's no streaming `fetch` (we'd long-poll instead of SSE). herdr-app is at 0.1.x with a vendored pre-release GPUI, so the mod API will change under us. Its telemetry is on by default and has to be turned off.
- Dictation: mods can't use the microphone, but they don't need it. The voice worker already runs in the dispatcher. The mod starts and stops it (`fetch`) and puts the text in its input box. The global hotkey keeps working.

### Option B: our own Rust window (Tauri 2 + WebView2) as an `hc` client

A Tauri shell, written in Rust, connects to `hc` in framed mode and relays terminal streams to xterm.js panes. A docking library (dockview) gives tabs, splits and drag-to-rearrange. The existing chat, Matrix, Settings and History pages become pane types, **reused almost unchanged**.

- Pros: keeps the chat exactly as it is. One window instead of several Edge app windows. Rust where it helps (the `hc` bridge, global hotkey, tray, single-binary install). No dependence on herdr-app's mod API.
- Cons: we own the windowing: layout persistence, focus, keys, a palette. xterm.js draws terminals less well than Ghostty's engine does in herdr-app. The window is still a web page, though the redraw bugs are already fixed with `held()` and signature guards.

### Option C: fork herdr-app and add a native Rust chat pane

- Cons that rule it out: we'd carry a fork of a fast-moving 0.1.x app (vendored gpui-pre, a Zig and Ghostty build chain), and we'd still rewrite the chat, this time in Rust GPUI. It has the costs of A and B and the benefits of neither. **Not recommended.**

### Rewriting the dispatcher in Rust

**Not recommended now.** It has about 980 tests, and its hard parts (policy, transcript parsing, costs, remote) gain nothing from Rust. The Rust that's worth writing sits at the boundaries: the `hc` bridge in option B, and possibly a single-binary launcher later. Revisit only if packaging (gap analysis J2–J4) becomes a goal.

## Recommendation

**Go with A if the chat survives as a mod; otherwise B. Never C.** Phase 0 is needed either way, and Phase 1 decides between A and B in a few days with clear pass/fail checks.

## Phases

### Phase 0: sessions hosted in `hc` (needed either way)

From the spike's must-dos, plus restore:

1. **Run the core under supervision.** The scheduled task starts `hc` at logon with a scrubbed environment (no `CLAUDE_*` variables). The dispatcher refuses to use a core whose terminals have `CLAUDE_*` in their environment. File the upstream issue about `hc` not scrubbing inherited variables.
2. **Pair each session with its terminal exactly.** `cc_hook.py` reports `HC_TERMINAL_ID`, with the pid as fallback. Session rows carry the `hc` address.
3. **Launch through `hc`.** New, Fork and Resume start `hc run --keep --cwd … -- claude …`, behind a setting, with Windows Terminal kept as the fallback.
4. **Gate typed input.** `hc send` and `hc keys` count as the person, so they go through the same rule as Send As Typed: window origin plus the per-start token, never `impctl` or peers. Clear the draft first (`ctrl+c`, or check the screen).
5. **Restore after a reboot (E1).** `state.json` keeps the set of open sessions with their `hc` address and session id. At logon, once the kept terminals respawn, the dispatcher types `claude --resume <id>` into each one, 100 ms apart. If `claude` exits inside a kept terminal, the dispatcher offers to resume it.
6. **Verify:** a real reboot with about 5 sessions, then about 20 sessions under `hc` for load.

### Phase 1: decide between A and B (2–3 days)

Build a throwaway herdr-app mod that renders one real transcript from `/api/conversation`, and check it against the following. Each check passes or fails; if any of the first four fails, A fails and we go with B:

1. A 500-turn session scrolls smoothly and opens in under 1 s.
2. Text can be selected and copied across turns.
3. Our highlighting can be expressed: code blocks, diffs, tool-call lines, path and hash marks, and folding by kind (Full/Compact/Hidden).
4. Updates arrive within about 1 s by long-polling, without re-rendering the whole log.
5. An input box with per-session drafts, a Send split button, and dictation start/stop.
6. One parked permission request shown as a dialog and answered.

Record the result in this file and the roadmap.

### Phase 2A, if A wins: the mod pack

1. Dispatcher: add the endpoints mods need (long-poll stream, compact rows) and a mod-pack installer (`setup_machine.py herdr install`). Turn herdr-app telemetry off on install.
2. Mods: chat, Matrix (a grid layout of chat tiles, or one grid mod), an Imperatorium sidebar (or extra rows in herdr's agents list), parking and AskUserQuestion dialogs, meters and costs, History with Resume and Fork, Changes with diff comments.
3. A layout: "Imperatorium" (sidebar | terminal + chat split | Matrix as a tab or floating window). Keys mapped to today's shortcuts.
4. Retire the Edge window once each feature has a mod. Keep `/` served for the phone and fallback.

### Phase 2B, if B wins: the Tauri shell

1. A Rust crate with an `hc` framed client (attach, resize, credit, events) relayed to xterm.js over Tauri IPC.
2. dockview layout with pane types: terminal, chat, Matrix, Settings, History, Changes. Layouts saved in the `hc` `doc` store, so several windows can share them.
3. Refactor `ui.py` so each page mounts as a pane component (one page context, one SSE stream).
4. Tray, global hotkey and toasts move into the shell. `window.py` launches it.

### Phase 3: herdr-parity extras (either path)

Taken from the gap analysis in its suggested order: orchestration (C1–C8: named agents, prompt-and-wait, parent and child sessions, now cheap on top of `hc wait` and `send --wait`), native toasts that focus the pane (G1), themes (F1), remote machines through `hc machine add`, which includes Windows hosts (D1), and multi-agent support (B1–B5) last.

## Risks

- **herdr-app's mod API changes** (A). Pin the herdr-app version during install, and run `herdr-app check` against the mod pack in CI.
- **Two attention systems** showing different answers: herdr's agent status from OSC 7501, Imperatorium's from hooks. In A, pick one sidebar. The dispatcher can also write its state to `hc` labels so herdr shows it.
- **Typed-input power.** Anything that can open hc's pipe can approve prompts. Keep the pipe's ACL to this user, and never forward it to remote callers.
- **No real reboot tested yet.** Phase 0 step 6 is that test.
