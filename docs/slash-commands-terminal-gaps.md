# Slash commands the window cannot run (audit, 2026-11-22)

> **Implemented 2026-11-22** (branch `slash-commands-in-window`). R1 is
> `rust/imperatorium/src/claude_catalog.rs` plus `slash::classify`. R2 is
> mod 0.4.0 `ops` and `Control.run_command`. R3–R8 are in the window
> (`app.js`, "slash commands the window does itself"). The tables in
> `slash.rs` are this audit; keep them in step with it. Not done: `/rewind`
> stays terminal-only, as R7 says. MCP servers in `/status` come from the
> tool list, not a server list.

Claude Code 2.1.330. Every slash command that today makes you leave the
Imperatorium window for the terminal, and what to do about each.

## How this was gathered

- **The command table** comes from the installed `claude.exe` (2.1.330). Each
  built-in carries flags: `type` (`local-jsx` draws in the terminal,
  `local` returns text, `prompt` becomes a model turn), `requires.ink`
  (needs the terminal UI), `supportsNonInteractive`, and
  `thinClientDispatch` (`post-text` or `control-request`, how a remote client
  is meant to run it). Many commands have **two variants**: an interactive
  `local-jsx` one used when you type it bare, and a `local` text one used
  when it gets arguments or runs headless (`/model` bare = picker,
  `/model opus` = text).
- **The list of models, commands and output styles** came from a headless
  `claude -p --input-format stream-json` sent one `initialize` control
  request. That returns, at no API cost: `models` (value, display name,
  description, effort levels, fast mode support), `commands` (the 55 that
  work without a terminal), `available_output_styles`, `agents`,
  `current_permission_mode` and `fast_mode_state`. Verified on this machine.
- **The mod API** (`mod/imperatorium/.claude-plugin/types/claude-code/index.d.ts`)
  already exposes `$.command.list()`, `$.command.run()`, `$.config.list()`,
  `$.config.set()`, `$.session.usage({breakdown})`, `$.session.version()` and
  `$.agent.list()`.
- **Not exercised:** I did not run each command in a real session (that
  would open real panels and change real settings). Entries marked *verify*
  are read off the binary's flags and descriptions.

Today, `rust/imperatorium/src/slash.rs` types `/command` into the session's
console. It refuses `MENU` (22 names), `MENU_BARE` without an argument (8),
and `/exit`/`/quit`. Anything else gets typed, and its output appears only in
the terminal unless Claude Code writes it to the transcript as
`<local-command-stdout>`. `/model opus` does write it there; the one `/context`
in local transcripts left nothing.

## 1. Refused today, and whether the refusal is still right

| Command (aliases) | Refused | Still right? |
|---|---|---|
| `/config` (`/settings`) | always | **Too broad.** `/config key=value` has a text variant ("Set a setting by key"). Only bare `/config` opens the menu. |
| `/permissions` (`/allowed-tools`) | always | Right for bare. `[open\|<description>]` arguments exist; *verify* whether they still open a dialog. |
| `/mcp` | always | **Too broad.** `/mcp reconnect\|enable\|disable [<server>\|all]` has a text variant. Bare opens the menu. |
| `/plugin` (`/plugins`, `/marketplace`) | always | Right (terminal UI only). |
| `/hooks` | always | Right (a read-only panel, `[name]` too). |
| `/memory` | always | Right (panel plus editor). |
| `/agents` | always | **Stale.** Removed in 2.1.330 ("(removed) ... edit .claude/agents/"). Harmless. |
| `/tasks` (`/bashes`) | always | Right (panel). |
| `/status` | always | Right (panel). The window could show the same facts (see R5). |
| `/help` | always | Right (panel). |
| `/doctor` (`/checkup`) | always | **Stale.** It is now a built-in skill listed by `initialize`. It runs as a model turn and can be typed. |
| `/ide` | always | Right. |
| `/login`, `/logout` | always | Right (account and browser flow). |
| `/privacy-settings` | always | Right. |
| `/terminal-setup` | always | Right, and it means nothing in the window anyway. |
| `/theme` | always | Right, and it only changes the terminal's look. |
| `/upgrade` | always | Right (plan and browser flow). |
| `/model` | bare | Right. `/model <name>` works. **The main friction:** you can't type a name you don't know. |
| `/output-style` | bare | **Probably too broad.** The interactive variant is hidden; the text variant "List output styles or switch to one" is the one listed. *verify* |
| `/resume` (`/continue`) | bare | Right. The window's History already does this better. |
| `/rewind` (`/checkpoint`, `/undo`) | bare | **Too narrow.** It has no text variant at all (`supportsNonInteractive: false`), so `/rewind anything` is typed and opens the menu. Refuse it always. *verify* |
| `/export` | bare | Right. The window has its own "Export as Markdown". |
| `/add-dir` | bare | Right. `/add-dir <path>` works. |
| `/exit`, `/quit` | always | Right (use Close). |

## 2. Not refused today, but they still open a terminal UI (gaps)

Today these get typed. Then the terminal shows a picker, wizard or panel that
the window can't see or drive.

**Pickers and choosers.** You must pick something in the terminal.

| Command | Bare | With an argument |
|---|---|---|
| `/effort` | level picker | `/effort high` works (text variant) |
| `/fast` | toggle panel | `/fast on\|off` works |
| `/advisor` | model picker | text variant exists, argument form *verify* |
| `/color` | colour picker | `/color <name>` works (text variant) |
| `/autocompact` | panel | `/autocompact auto\|<tokens>` works |
| `/teleport` (`/tp`) | cloud-session picker | none |
| `/import` | source picker | `/import codex\|gemini\|cursor` |
| `/auto-mode-setup` | wizard | the text variant takes machine arguments, not for people |
| `/remote-env` | environment picker | `[use case]` |
| `/use-case` | picker | `[use case]` |
| `/cloud-plugins` | chooser | none |
| `/rename` | *verify* (bare may prompt) | `/rename <name>` works |
| `/goal` | status panel *verify* | `/goal <condition>` / `clear` works |

**Read-only panels.** They run, but the answer is only on the terminal.

`/usage` (`/cost`, `/stats`), `/context`, `/skills`, `/workflows`,
`/artifacts`, `/diff`, `/release-notes`, `/version`, `/powerup`, `/sandbox`,
`/stickers`, `/plan` once plan mode is on (it shows the plan), `/btw` (the
side answer is drawn over the prompt, not written to the transcript),
`/skill-doctor`, `/access`.

**Account, setup and browser flows.** Rare, and they belong with the account.

`/usage-credits` (`/extra-usage`), `/rate-limit-options`, `/web-setup`,
`/install-github-app`, `/install-slack-app`, `/setup-bedrock`,
`/setup-vertex`, `/design-login`, `/chrome`, `/feedback`, `/bug`
(`/share`).

**Terminal-only features and display.** They mean nothing in the window.

`/tui`, `/scroll-speed`, `/focus`, `/brief`, `/wellbeing` (`/breaks`),
`/keybindings` (opens an editor on this PC), `/voice` (the terminal's mic),
`/radio`, `/copy` (copies in the terminal), `/mobile` (QR code), `/session`
(`/remote`, QR code), `/daemon`, `/remote-control` (`/rc`), `/desktop`
(`/app`).

**Commands that change which session the tile follows.** Typing works, but
the window can lose track of the session.

`/branch`, `/background` (`/bg`), `/stop` (background sessions), `/cd`,
`/restart` (`/update`, restarts the process). *verify* each against the tile
and pid tracking.

## 3. Fine from the window today

`/clear` (`/reset`, `/new`), `/compact`, `/model <name>`, `/effort <level>`,
`/fast on|off`, `/rename <name>`, `/add-dir <path>`, `/export <file>`,
`/resume <id>`, `/recap`, `/goal <cond>`, `/list-agents` (`/peers`),
`/reload-plugins`, `/reload-skills`, `/fork <directive>`, `/project ...`,
`/color <name>`, and every prompt-type command and skill (`/init`,
`/insights`, `/team-onboarding`, `/statusline`, `/doctor`, `/code-review`,
`/security-review`, `/simplify`, `/loop`, `/schedule`, ...).

## Recommendations (most friction removed per unit of work first)

### R1. Get the lists from Claude Code instead of keeping them by hand
`MENU`/`MENU_BARE` is hand-kept and has already drifted: two entries are
stale and 58 terminal-bound commands (some only when bare) are missing. Instead:
- Once per Claude Code version (the dispatcher already knows the version
  from beats), run the headless `initialize` probe. Cache its `models`,
  `commands`, `available_output_styles` and `agents`.
- A built-in command **not** in `initialize.commands` has no text form, so
  it opens a terminal UI. Refuse it when bare, or take it over in the window
  (R3–R6).
- Run the probe with a clean environment: `CLAUDE*` variables stripped, a
  scratch directory, `--setting-sources local`, `--no-session-persistence`.
  That way it fires no hooks at the live dispatcher.

### R2. Run commands through the mod, not the keyboard
The mod already gets a reply on every beat (prompts to submit, Stop). Add
`commands: [{id, command, args}]` to that reply. The mod calls
`$.command.run({command, args})` and posts `{id, text}` back on its next
beat. That gets you:
- **The output in the window.** `text` is what a `local` command printed,
  so `/model opus`, `/effort`, `/mcp reconnect` and `/config k=v` answer
  in the chat.
- **No console typing.** That means no Ctrl+C on a draft, no Windows-only
  limit, no one-line limit, and it works for remote hosts' sessions too.
- **The same rules.** `command.run` queues until the session is idle and
  rejects unknown names.

Keep the current gate: only the window, with its per-start token, can ask
for a command. A command run this way reads `origin: plugin`. The
"messages are not user consent" rule must still hold, so never let an agent
or the Fleet propose a slash command. Typing stays as the fallback for
sessions without the mod.

### R3. Native pickers for the commands that ask you to choose
Feed them from the R1 cache. When you type the bare command in the chat,
the picker opens and then sends `/cmd <value>` the R2 way:
- `/model` lists the models with display name and description (for
  example "Fable 5.2 — For your toughest challenges") and marks the current
  one. The statusline already reports it.
- `/effort` offers only the levels the current model supports
  (`supportedEffortLevels`).
- `/fast` is shown only where `supportsFastMode` is true.
- `/output-style` lists `available_output_styles`.
- `/color` and `/autocompact` are small fixed choices.
- `/advisor` reuses the model list.

The same model and effort pickers belong in the **New session** dialog,
next to Permission mode. `claude --model` and `--effort` are already passed
for background sessions (`background.rs`).

### R4. Slash-command autocomplete in the prompt box
When the box starts with `/`, list `$.command.list()` for that session.
That covers built-ins plus its plugins, skills and MCP prompts, each with
its description and argument hint. Mark each entry by what will happen:
*runs here*, *opens a picker here* (R3), or *terminal only*. You find out
before you press Enter, and you can see the commands without opening `/help`
in the terminal.

### R5. Show the read-only panels in the window
Most of the data is already in the dispatcher or one mod call away:
- `/context` ← `$.session.usage({breakdown: "summary"})`. It returns the
  `get_context_usage` breakdown and is estimated locally, at no cost.
- `/usage` `/cost` `/stats` ← the tile's usage and rate limits plus
  Insights. Open those instead of refusing.
- `/status` and `/version` ← `$.session.version()`, model, cwd, account
  (`$.session.identity()`) and MCP servers.
- `/skills` and `/help` ← the R4 list. `/agents` ← `$.agent.list()`.
- `/diff` ← the existing Changes tab and Project changes pop-up.
- `/tasks` and `/workflows` ← the Activity tab. Extend it if needed.
- `/hooks` and `/permissions` (view only) ← `$.settings.read`.
- `/release-notes` ← static text per version.

### R6. `/config` as a form
`$.config.list()` returns every row the `/config` menu draws, with its kind,
current value, `options` for choice rows and an `isLocked` flag.
`$.config.set()` changes one row and answers `{deny}` when it can't. Draw
it as a settings sheet for the session. That replaces bare `/config`
(`/settings`) and maybe `/theme`. *verify* which rows are present.

### R7. Map the rest onto what the window already does
- `/resume` → History.
- `/export` → Export as Markdown.
- `/exit` → Close.
- `/branch` and `/fork` → Fork (now Send to New Session with Context, in the
  Send button's menu).
- `/add-dir` → the path menu.
- `/copy` → copy the reply.
- `/rewind` stays in the terminal. It has no text form and no mod call.
  Leave it refused with a Show Window button, and later a window-side list
  of your prompts that opens the terminal at the right one.

### R8. Better refusals for what stays in the terminal
- **Put the way out in the note.** The refusal now says "use Focus and run
  it there". Make it a **Show Window** button. Where R3 applies, offer the
  picker in place of the refusal.
- **Say when the terminal won't help either.** For terminal-only display
  commands (`/tui`, `/focus`, `/brief`, `/scroll-speed`, `/theme`), say
  "this changes the terminal's look; it has no effect here".
- **Group account flows.** Put `/login`, `/upgrade`, `/usage-credits` and
  the setup commands under one note: "account and setup run in a terminal,
  once". They're rare, so the friction is acceptable.

### Suggested order
1. R1 and the refusal fixes in section 1. This is small, and it stops the
   list drifting.
2. R2 (the mod route). Everything after it builds on it.
3. R3 for `/model`, `/effort` and `/output-style`. This answers the "what
   models are there?" case directly.
4. R4, R5, R6. Then R7 and R8 as polish.
