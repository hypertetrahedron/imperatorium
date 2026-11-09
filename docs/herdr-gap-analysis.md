# Herdr gap analysis

Date: 2026-11-04. Compared against Imperatorium branch `mods` (ee5639e).

Herdr versions surveyed (shallow clones beside this repo in `github_src/`):

- `herdr` v0.9.5: the Rust terminal multiplexer (TUI).
- `herdr-core` (`hc`) v0.1.18: a headless terminal daemon with a JSON-RPC API.
- `herdr-app` v0.1.13: a GPU desktop client of herdr-core, extended with JavaScript mods.
- About 200 third-party plugins, mods and companion apps found through `gh search`.

## The one difference that drives most of the list

Herdr **owns the terminals**. Every agent runs in a PTY that herdr's background server hosts. Herdr can therefore show the live screen, send any keystroke, read output, keep processes alive when the window closes, and restore the layout after a reboot. It works with about 25 agent CLIs because it reads their screens.

Imperatorium **observes and dispatches**. Claude Code sessions run in the user's own terminals. Imperatorium learns their state from hooks, `claude agents --json`, transcripts and the mod, and delivers prompts through the inbox socket or the mod. That gives it depth on Claude Code that herdr lacks (see "Where Imperatorium is already ahead"). It also means that many herdr features (rows A1–A10, E3, E4 and parts of D) can only be matched by adding a terminal-hosting layer.

The roadmap has already deferred the routes to that layer: WezTerm migration, amux, tiling WMs and third-party multi-session managers. Before treating every row below as required, decide whether matching herdr means **hosting terminals** or **matching the outcomes** (a live view, raw keys, survival across restart) through Claude Code's own surfaces. Rows marked ◆ depend on that decision.

Status column: **Missing**, **Partial** (something exists but falls short of herdr), or **Deferred** (already on the roadmap's Deferred list, with that reason).

---

## A. Terminal ownership and control

| ID | Herdr feature | Imperatorium | Status |
|---|---|---|---|
| A1 ◆ | Hosts each agent in its own PTY (ConPTY on Windows, Ghostty VT engine); shows the live terminal screen | Shows a transcript rendered from JSONL, not the screen | Missing (WezTerm/amux routes Deferred) |
| A2 ◆ | Workspaces › tabs › split panes; zoom, swap, resize, move a running pane to another tab or workspace; `layout.export` / `layout.apply` as declarative trees | No panes; sessions are Windows Terminal tabs. "Resizable layout / side-by-side sessions" is Not Started | Missing |
| A3 ◆ | Detach and reattach: processes keep running when the client closes or SSH drops | A session dies with its terminal tab; only `claude --bg` outlives it | Missing |
| A4 ◆ | Several clients on one server, each with its own view; read-only observers; `--takeover` attach; NDJSON observe/control streams for third-party bridges | One window plus the Matrix, both read-only views of transcripts | Missing |
| A5 | Raw input to any agent: `send-keys` (esc, ctrl+c, shift+tab to cycle modes, arrows/enter to answer a UI), `hc interrupt` | Guarded console typing for slash commands, Send As Typed and `/exit` only; local Windows only; only when idle with nothing parked | Partial |
| A6 | `pane read` / `agent read` (visible, recent, unwrapped, ANSI) and `pane wait-output --regex` | Reads transcripts and task output files; nothing waits for a pattern | Partial |
| A7 | Managed non-agent programs: `hc run` (waits, exit code, timeout, background), `hc exec`, `hc file read/write/edit` locally or `--on` a machine | Only observes shells that Claude Code spawns | Missing |
| A8 ◆ | Kept terminals: respawned as a fresh shell in the last folder after a crash or reboot, with backoff | None | Missing |
| A9 ◆ | Terminal niceties: scrollback, vi copy mode, find in terminal, Ctrl-click links (OSC 8), kitty graphics, OSC 52 clipboard, image paste, kitty keyboard protocol, IME | Find works only in the transcript | Missing (only meaningful with A1) |
| A10 | Popup and scratch terminals; custom key bindings that launch commands (lazygit and so on) in popup/pane/shell | None | Missing |

## B. Agent breadth and state detection

| ID | Herdr feature | Imperatorium | Status |
|---|---|---|---|
| B1 | Supports about 25 agent CLIs: Claude Code, Codex, Copilot CLI, Cursor Agent, OpenCode, Pi, OMP, Droid, Devin, Kimi, Kilo, Hermes, Qoder, Qwen, Letta, MastraCode, Grok, Antigravity, Amp, Kiro, Maki, Gemini CLI, Cline, plus self-reporting Crush, Command Code, Muse, Prime | Claude Code only. "Other agents (Codex, Gemini CLI)" is Not Started, low priority | Missing |
| B2 | Screen-based detection manifests (TOML rules) bundled in the binary, updated remotely without a release, overridable locally; `agent explain` shows why a state was chosen | Detection comes from Claude Code's own signals, which are more accurate for Claude. There is no generic or explainable layer | Missing (needed for B1) |
| B3 | `integration install/uninstall/status --outdated-only` for each agent's hooks; a Settings tab installs integrations for agents found on PATH | `hooks/install.py` for Claude Code only | Partial |
| B4 | An open self-report protocol: `pane report-agent` (state, seq, `resume_argv`) and OSC 7501 "Program Status Protocol" in herdr-core, so any tool can become a first-class agent | `/hook` accepts Claude Code hook payloads only; no documented protocol for other tools | Missing |
| B5 | A "+ menu" / `programs.json` to launch any known agent by kind, with icons for about 100 programs | New/Fork/Resume launch `claude` only | Missing |
| B6 | A distinct **done** state (finished, not yet seen) with seen-tracking per client, shared across devices through labels | A `new` badge and "waiting for you" 60 s after Stop. Seen state is per browser (localStorage), not shared | Partial |
| B7 | Display metadata reported by agents (`report-metadata`: title, state labels, `$tokens` with TTL) rendered in sidebar rows | None | Missing |

## C. Agent-to-agent orchestration

| ID | Herdr feature | Imperatorium | Status |
|---|---|---|---|
| C1 | An agent can spawn another agent and get a handle back (`agent start --kind codex --pane P`, `hc run reviewer "task"`) | The API can dispatch `claude --bg`, but returns no handle to wait on | Partial |
| C2 | Named agents (`[a-z][a-z0-9_-]{0,31}`, unique, renameable) as stable targets | Targets are a session id, prefix, title or fuzzy project match | Partial |
| C3 | Atomic prompt-and-wait (`agent prompt --wait --until idle\|blocked`), `agent wait`, `hc wait --children all\|any` as a join primitive, with stall and `agent_blocked` errors | Fire and forget; the Queue holds a prompt until a turn ends but nothing blocks the caller | Missing |
| C4 | Read results back (`agent read --source recent-unwrapped`) | `/api/conversation` exists but isn't framed for agent-to-agent use | Partial |
| C5 | A parent/child tree: the child's reports are typed into the parent as `<hc from=…>` notes; the parent is told automatically when a child blocks, errors or exits | `ORIGIN_NOTE` tells the receiver **not** to reply; no tree | Missing |
| C6 | Role templates (Markdown plus front matter: command, ready, submit, stop) such as reviewer or implementer | None | Missing |
| C7 | Typed event subscriptions (27 kinds, with filters on `pane.output_matched` and `agent_status_changed`), one-shot `events.wait`, and `events_lost` recovery | `/api/stream` SSE carries the whole session list; no filters or one-shot waits | Partial |
| C8 | An agent skill (`herdr --skill`, `hc --skill`, `skills/*/SKILL.md`) that teaches in-pane agents to drive the tool | None shipped | Missing |
| C9 | Labels (`k=v`) on terminals and a versioned CAS document store (`doc.get/set`) so several clients share pinned/seen/layout state | Prefs live in the dispatcher (pin/archive); no labels or doc store | Partial |
| C10 | `agent.view.set`: an API-defined filter and sort projection for the agents sidebar | None | Missing |
| C11 | Send to several agents at once; a shared work queue | Same | Deferred |

## D. Remote and multi-machine

| ID | Herdr feature | Imperatorium | Status |
|---|---|---|---|
| D1 | **Windows** SSH hosts (and Windows ARM64 by emulation) | Hosts must be Linux or macOS | Missing |
| D2 | Full parity on remote hosts: live terminal, keys, close, attach | Remote sessions are observed and injected only; no close, slash commands, Send As Typed, Changes tab, focus or Stream Deck keys | Partial |
| D3 | Auto-bootstraps the remote binary to an exact version (reuses brew/mise/nix installs; checksum-verified download) and offers a restart on version mismatch | `setup_machine.py remote add` (In Progress, not run against a real host); no version-match check for the copied reader | Partial |
| D4 | Custom transports: `hc machine add --via '<cmd>'` (fly ssh, Vercel Sandbox helper, docker+socat in herdr-mirror) | SSH only | Missing |
| D5 | `--machine <host> <command>` forwards any CLI command to a remote server | `impctl` talks to the local dispatcher only | Missing |
| D6 | Remote image paste: the local clipboard image is staged on the remote and its path pasted; `hc put` | None | Missing |
| D7 | Remote `exec` and `file read/write/edit` | None | Missing |
| D8 | Machine attention states (`! auth`, `! error`) and `hc machine login` for MFA or host-key prompts | Backoff and a parked reason exist; no guided login | Partial |

## E. Persistence and restore

| ID | Herdr feature | Imperatorium | Status |
|---|---|---|---|
| E1 | **Restore everything after a reboot**: the layout comes back and every agent is resumed with its native resume command (`resume_agents_on_restore`, staggered 100 ms apart, without a client attached) | Resume one session at a time from History | Missing |
| E2 | Saves before host shutdown (Linux logind inhibitor, macOS logout hook, Windows shutdown) | Dispatcher state is saved atomically, but there is no "these sessions were open" set to restore | Missing (needed for E1) |
| E3 ◆ | Live handoff: hands running PTYs to a new server across an update (Unix) | N/A without A1 | Missing |
| E4 ◆ | Screen history replay (`experimental.pane_history`) | N/A without A1 | Missing |
| E5 | Up to 48 rolling session snapshots plus backups of unreadable saves | `state.json` plus `config.json.bak`; no rolling snapshots | Partial |
| E6 | Named sessions (`--session`, `HERDR_SESSION`, `HC_SESSION`): fully separate namespaces on one machine | One dispatcher per machine | Missing |

## F. UI and UX

| ID | Herdr feature | Imperatorium | Status |
|---|---|---|---|
| F1 | Named themes (Catppuccin, Tokyo Night, Gruvbox, Dracula, Nord, Solarized, Kanagawa, Rosé Pine, …), custom palettes, auto light/dark switching, user theme files | Follows the system light/dark setting; per-token highlight colours only | Partial |
| F2 | Configurable sidebar rows: tokens, per-token style, conditional rules, per-agent overrides | Fixed tile layout with Full/Compact densities | Partial |
| F3 | Every action rebindable (`keys.json` / `[keys]`, tmux prefix, chords, `config reset-keys`); a command palette listing every command and key | A fixed shortcut set and a Ctrl+K palette | Partial |
| F4 | A responsive narrow/mobile layout (herdr TUI over SSH from a phone) | The window is localhost-only; phone access is Deferred until there is auth | Deferred |
| F5 | A Files side panel (tree, preview, folder watcher, local and remote) | None | Missing |
| F6 | Staging and a commit-message box in the Changes panel; AI commit messages (herdr-sidebar) | Changes is read-only, plus diff comments | Partial |
| F7 | A window-title template, and tab-bar status entries (hostname, clock, periodic shell command output) | Title count only | Partial |
| F8 | Peek at a session over the current view without switching | Selecting a tile switches the main pane | Partial |
| F9 | Docs in Japanese and Simplified Chinese; IME and non-Latin layout handling | English only | Missing (low value for a single user) |

## G. Notifications

| ID | Herdr feature | Imperatorium | Status |
|---|---|---|---|
| G1 | Native OS toasts (Windows Notification Center, notify-send, terminal-notifier) whose **click focuses the exact pane** | Browser notifications; whether a click selects the session wasn't verified | Partial |
| G2 | Sounds: bundled done/request MP3s, custom files, on/off per agent | Spoken alerts (System.Speech) for waiting | Partial |
| G3 | A notification API for agents and plugins (`herdr notification show`, `notify()` in mods) | None exposed | Missing |
| G4 | Telegram: two-way chat per agent, inline approve buttons, time-limited sharing (third-party: herdr-telegram-agents, herdr-remote, ccgram) | None | Missing |
| G5 | Phone push **with approve/deny** (herdr-remote, mobile-relay, Collie, whip) | ntfy push built but never sent live; buttons not built; phone reach Deferred | Partial / Deferred |

## H. Extensibility

| ID | Herdr feature | Imperatorium | Status |
|---|---|---|---|
| H1 | A plugin system: `herdr-plugin.toml` with actions, event hooks, panes, link handlers, startup hooks and build steps; `plugin install owner/repo`; a marketplace auto-indexed from the GitHub topic | None. The HTTP API is the only extension point | Missing |
| H2 | UI mods and layouts (JavaScript over gpui-kit) with a validator (`herdr-app check`) and a screenshot command (`herdr-app shot`); herdr.dev/mods and /layouts indexes | None | Missing |
| H3 | A published, versioned API schema (`herdr api schema`, OpenRPC `hc schema`), additive-only with breaking-change tests, and a capability handshake | Endpoints are documented in prose only | Missing |
| H4 | Shell completions (bash, zsh, fish, PowerShell, elvish) | None for `impctl` | Missing |

## I. Git worktrees

| ID | Herdr feature | Imperatorium | Status |
|---|---|---|---|
| I1 | Create, open or remove a worktree from the sidebar and start an agent in it; worktree groups nest under the repo; `--trust-repository` | Recognises worktrees that `claude --worktree`/`--bg` creates; merge, PR or discard background worktrees. Can't create one on demand for an interactive session | Partial |
| I2 | Ahead/behind upstream shown per workspace | Branch name only | Partial |
| I3 | Worktree setup hooks (copy `.env`, `mise trust`; third-party herdr-worktree-setup) through the `worktree.created` event | None | Missing |

## J. Distribution and operations

| ID | Herdr feature | Imperatorium | Status |
|---|---|---|---|
| J1 | One-line web install (`curl … \| sh`, `irm … \| iex`, an `install.cmd` fallback for locked-down endpoints) | `bootstrap.ps1/.sh` (In Progress, not run on real Linux/macOS) after a clone | Partial |
| J2 | Package managers: Homebrew, mise, Nix flake (plus a Homebrew cask for the app) | None (no PyPI, winget, scoop or brew) | Missing |
| J3 | Self-update with stable/preview channels, SHA-256-verified, an in-app update badge and a What's New screen; running servers keep going | `git pull` by hand | Missing |
| J4 | Signed and notarised binaries (macOS Developer ID, Windows Azure Artifact Signing) | N/A for a Python checkout; matters only if packaged | Missing |
| J5 | `config check` (unknown keys with hints), `--default-config`, hot `reload-config` | The Settings window validates; there's no CLI check or reload | Partial |
| J6 | Rotated logs for each component, plus a "support logs" bundle | `IMPERATORIUM_DEBUG_LOG` only | Partial |
| J7 | A first-run onboarding flow inside the UI (sounds, toasts, integrations) | `ONBOARDING.md` plus the CLI installer | Partial |

## K. Ecosystem features users of herdr now expect (third-party, not in herdr itself)

| ID | Feature (example projects) | Imperatorium | Status |
|---|---|---|---|
| K1 | A phone/tablet PWA with live view and approvals (Collie, Ovis, roamgate, mobile-relay, pairfob) | Same | Deferred (needs auth plus Tailscale) |
| K2 | Native iOS/Android/Watch clients (Heeler, whip, herdrchat, herdr-watch) | None; the roadmap prefers first-party `claude remote-control` | Not Started |
| K3 | A kanban/task board whose cards are prompts dispatched to agents (herdr-board, tsk, beads boards, shepherdr) | Shared work queue Deferred | Deferred |
| K4 | Scheduled prompts (cron) in a fresh worktree per run, with catch-up after sleep (herdr-automations, shepherdr) | None | Missing |
| K5 | A browser pane or dev-server preview (herdr-browser over CDP, shepherdr) | "Localhost preview" Not Started | Not Started |
| K6 | Auto-title sessions and tabs with an LLM (herdr-auto-title) | Uses Claude Code's own titles | Partial |
| K7 | PR/CI tracking that **sends CI failures back** to the agent (herdr-pr-flow), Linear/GitHub issue panes | A PR badge only | Partial |
| K8 | Usage meters for other providers (Codex, Gemini, Grok…) and several Claude accounts (herdr-agent-usage, clauth, herdr-relevo) | Claude only, one account | Missing (follows B1) |
| K9 | A swarm DAG with review gates, and agents in sandboxed VMs (herdr-dagr, agentbox) | None | Missing |
| K10 | Desktop overlay HUD above full-screen apps (herdr-hud) and a menu-bar/tray monitor (Herdi.app, herdi-win) | Stream Deck and the global hotkey fill this role | Partial |

---

## Where Imperatorium is already ahead (keep these when closing gaps)

Herdr has no first-party equivalent of these:

- A risk-tiered auto-approval policy, with requests parked in the window and on the Stream Deck.
- Answering AskUserQuestion from the window.
- Rate-limit meters with a pace forecast, cost history by day, project and model, and budget alerts.
- A full transcript through compactions, with display controls per kind of message.
- A prompt-cache countdown and cold-cache cost.
- Prompts from the window that arrive as the user's own words (the mod).
- A global History search with Resume and Fork.
- Diff comments sent back as a prompt (herdr needs the third-party reviewr for this).
- A Stream Deck surface, local Whisper dictation, the Chat Matrix and error analytics.
- No terminal lock-in: it works with Windows Terminal, VS Code and the desktop app.

## Suggested order, if the goal is functional parity with herdr

1. **The orchestration API on top of today's dispatch**: C1–C8 and B6, plus E1 (restore and resume after a reboot). This is where herdr is most "agent-native", and none of it needs terminal hosting.
2. **G1, G3, F1, F3, H3, H4, J3, J5**: cheap, self-contained polish.
3. **The terminal-hosting decision** (A1–A10, E3, E4, D2): pick one route (own ConPTY host, wrap herdr-core `hc` as a backend, or WezTerm) and record it in the roadmap.
4. **B1–B5**: multi-agent support. A large surface area, and the roadmap rates it low priority.
5. **D1, D4–D7, F4/K1, G4/G5**: remote breadth and phone reach. These need auth first.

---

## Tools, services and subscriptions needed to test the missing features

### Agent CLIs (for B1–B5, C1, K8)

Each one needs an account or API key from its provider. The one marked ★ is the minimum to prove that multi-agent support is genuinely general.

| Agent | What you'd need |
|---|---|
| Codex CLI ★ | A ChatGPT plan that includes Codex, or an OpenAI API key |
| Gemini CLI | A Google account (free tier) or a Gemini API key |
| OpenCode, Crush, Pi, Cline | Free CLIs; they need a model provider key (an Anthropic API key works) |
| GitHub Copilot CLI | A GitHub Copilot subscription |
| Cursor Agent CLI | A Cursor account or subscription |
| Amp | An Amp (Sourcegraph) account with credits |
| Factory Droid | A Factory account |
| Devin CLI | A Devin (Cognition) account |
| Kiro CLI | A Kiro (AWS) account |
| Grok CLI | An xAI API key |
| Antigravity CLI | A Google account |
| Qwen Code | A Qwen account (OAuth) or a DashScope key |
| Kimi Code | A Moonshot (Kimi) account, Kimi ≥ 0.14.0 |
| Letta Code, Hermes, Qoder, MastraCode | Their respective accounts or keys |

A reasonable minimum set: Codex, Gemini CLI (free tier) and OpenCode (on your existing Anthropic key). Together they cover a hook-based agent, a screen-detected agent and a self-reporting plugin agent.

### Machines and operating systems

- **Windows SSH host** (D1): OpenSSH Server enabled on a second Windows machine, or a Hyper-V VM (Hyper-V is available on this Windows 11 Pro machine).
- **macOS machine** (D1 macOS host, J2 Homebrew, J4 notarisation, G1 terminal-notifier, E2 logout hook): any Mac, or a hosted Mac (for example MacStadium or a GitHub Actions macOS runner, for CI only).
- **Linux desktop** (E2 logind inhibitor, G1 notify-send, the Linux service rows already In Progress): a Hyper-V VM running Ubuntu Desktop. WSL can't test logind or a real desktop session.
- **Existing hosts**: `developmenthost1` and `raspberrypi.local` cover Linux remote paths. `servicehost1` needs Claude Code installed before it's useful.
- **Second Windows user or elevated session**: to test elevation boundaries (A4, named-pipe ACLs).

### Remote and phone reach (D4, D6, F4, G4, G5, K1, K2)

- **Tailscale** (free personal plan): the tailnet the deferred phone access assumes.
- **Cloudflare Tunnel**: quick tunnels need no account; a named tunnel needs a free Cloudflare account plus a domain.
- **A Telegram account** and a bot token from @BotFather; a supergroup with Topics enabled for the per-agent-topic design.
- **ntfy**: the app on the phone. Already chosen; needs a live send for the In Progress row.
- **An iPhone and/or Android phone**; an Apple Watch only if you pursue K2.
- **Apple Developer Program** (paid yearly) for TestFlight, if you build a native iOS client.
- **Docker Desktop** for container machines (D4); optionally a **Fly.io** or **Vercel** account to test `--via` transports.

### Terminals and display (A1, A9, G1)

- **WezTerm** (Windows/macOS/Linux): kitty graphics, OSC notifications and the `wezterm cli` route.
- **Ghostty** or **kitty** on the Mac or Linux VM, for kitty graphics and OSC 9/777 notifications. Windows Terminal doesn't support kitty graphics.
- **herdr itself** (free, Apache-2.0) installed locally, to compare behaviour side by side, and herdr-core `hc` if you evaluate it as a terminal backend.
- **A Windows non-Latin keyboard layout or IME** (built into Windows) for F9/A9 input testing.

### Distribution and signing (J1–J4)

- **A PyPI account** (free) if Imperatorium becomes `pip`/`pipx`/`uv tool` installable.
- **winget-pkgs / scoop bucket**: a GitHub account (you have one) for submitting manifests.
- **A Homebrew tap**: a GitHub repo, plus a Mac to test it.
- **Code signing**: Azure Artifact Signing (paid Azure subscription) or an OV/EV certificate for Windows; the Apple Developer Program for macOS notarisation. Only needed if you ship packaged binaries.
- **A static host for update manifests** (GitHub Releases is enough).

### Git and project services (I1–I3, K7)

- `git` and `gh` are already installed. A **GitHub repo with Actions CI** that can fail on purpose, to test CI-failure send-back.
- Optional: a **GitLab** account (`glab`) and a **Linear** account for issue panes.

### Voice and other optional extras

- For realtime cloud voice (herdr-voice style): an OpenAI, xAI or Google API key. Not needed: Imperatorium's local Whisper already covers dictation.
- **Chromium/Edge with CDP** for a browser pane (K5). Edge is already installed.
