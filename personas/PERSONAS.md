# Personas

Who uses Imperatorium, and how. Each persona is one pattern of use, with
the actions it takes in the window. Set 1 is scripted in `scenarios/`; the
later sets are defined here and scripted when they are run.

## Where the patterns come from

**This machine's own transcripts** (`~/.claude/projects`, read-only, 2026-11-23;
temp and test folders left out): 121 sessions in 29 projects over about 60 days.

| Measure | Value |
|---|---|
| Sessions live at once (5-minute samples, last 60 days) | median 4, p90 7, peak 9 |
| Sessions live at once in the *same* project | p90 1, peak 2 |
| Sessions in a worktree | 2 of 121 |
| Prompts per session | median 1, p90 9, max 45 |
| Session shape | 24 no prompt, 37 one prompt, 35 two to five, 22 six to thirty, 3 over thirty |
| Projects with a single session | 16 of 29 |
| Permission mode | bypassPermissions in 76 sessions, acceptEdits 5, auto 1 |
| Sessions that asked the user a question (AskUserQuestion) | 16 (13%) |
| Sessions that compacted | 25 (21%) |
| Slash commands typed | /clear 18; /model, /effort 2 each; others once |
| Entry point | CLI 99, desktop app 21, VS Code 1 |

So the common case is several short sessions spread across projects, not many
sessions in one project. Worktree-heavy and remote use are rarer and riskier,
and they reach the most code. The README's design target, about 20 concurrent
projects plus remote hosts, sets the upper bound.

**Published practice** (web, 2026-11-23): one worktree per parallel session
is the documented pattern, both in the official worktree docs and in the
desktop app's per-session worktrees. Guides put a sustainable ceiling at two
or three parallel sessions, because watching more is costly, and people
report losing track of which session holds which task. Remote work runs
Claude Code in containers or on VPSs, usually inside tmux/systemd so the
session survives SSH drops. Background and headless runs are used for
reviews, refactors and pipelines. Sources:
[Claude Code worktrees](https://code.claude.com/docs/en/worktrees),
[MindStudio on parallel branches](https://www.mindstudio.ai/blog/claude-code-git-worktree-parallel-branches),
[LevelUp: I stopped setting up worktrees](https://levelup.gitconnected.com/i-stopped-setting-up-git-worktrees-for-parallel-claude-code-07023ca5cd1c),
[devcontainer docs](https://docs.anthropic.com/en/docs/claude-code/devcontainer),
[Claude Desktop SSH sessions](https://claude.com/docs/third-party/claude-desktop/ssh-remote-sessions),
[OpenReplay: remote box for agentic coding](https://blog.openreplay.com/remote-box-agentic-coding-setup/),
[MindStudio: Claude Code on a VPS](https://www.mindstudio.ai/blog/run-claude-code-on-vps-always-on-workflows).

## Set 1 (scripted, run first)

| Id | Persona | Pattern | Why it matters |
|---|---|---|---|
| `wren` | Wren, worktree wrangler | 4+ sessions in one repository: new session and fork into worktrees, a background job, switch between them, Worktrees tab, Project changes overlaps, merge, discard, close | Exercises the most code (spawn placement, git, merge) on the path the README designs for |
| `tessa` | Tessa, fleet triage lead | 16 sessions in 10 projects, several mid-turn: permission requests (one, then a burst of three), an AskUserQuestion card, `n` to the waiting one, nudging five sessions in turn, filter and grouping, Chat Matrix, Fleet Ask | The "~20 concurrent projects" design target: attention routing at scale |
| `rhea` | Rhea, remote container operator | A single session on a remote host over SSH, nothing local: open it, send, test the host in Settings, the host drops, the host returns | Remote is its own code path, slow to observe, and failures there are hard to see |
| `basil` | Basil, background delegator | Background jobs from the New session dialog and from ctrl+Enter, noticing a job finished, follow-up, merge from Project changes, close | Work you do not watch: discoverability and notification matter most |
| `sol` | Sol, solo steady | The measured common case: 4 sessions in 4 projects, short exchanges, queue while busy, Stop, /clear, a second project, resume from History | The majority path: small frictions here cost the most in total |
| `kai` | Kai, keyboard navigator | 6 sessions, one with 400 turns: ctrl+K, j/k, alt+1, Earlier back to the start, ctrl+F find, alt+Up prompt jumps, `n` to a waiting session | Keyboard completeness, and how the window scales with long conversations |

## Set 2 (defined, to run next)

| Id | Persona | Actions |
|---|---|---|
| `ines` | Ines, reviewer | Changes tab on 3 sessions, line comments in the Review Box sent back as a prompt, Activity panel, Diff per worktree, Insights tool failures |
| `omar` | Omar, cost watcher | Usage Meter details, Model Mix, Insights cost by day/project/model over 7/30/90 days, Chat Matrix stats tile, a spend watch in Fleet |
| `priya` | Priya, slash-command user | /model picker, /effort, /config form, /resume picker, autocomplete in the prompt box, a mod-run command, Send As Typed |
| `arlo` | Arlo, archaeologist | 500 past sessions in 20 folders: History search, folder filter, sort, time range, open a hit read-only, resume one in a worktree because its folder is taken |
| `nell` | Nell, newcomer | Empty catalogue first run, Settings tour, add a project root, add a remote machine (Test connection fails, then key login), first New session |

## Set 3 (defined; partly needs hardware or the live desktop)

| Id | Persona | Actions | Note |
|---|---|---|---|
| `june` | June, automation integrator | Drives Imperatorium from scripts: `/api/dispatch` with each mode, `impctl`, Fleet watches and proposed actions, confirms from the window | API ergonomics; no browser needed for most of it |
| `max` | Max, interrupt-heavy multitasker | Stops and redirects mid-turn, queue reordering, Send to new session with context mid-turn (print-mode fork), forks of forks | Mid-turn forks run `claude -p`: the stub answers, but timing is not realistic |
| `vera` | Vera, voice and Stream Deck | Hotkey, dictation (F9), Stream Deck keys turning orange on attention | Deferred: needs a microphone, a real hotkey and the HID device |
| `remy` | Remy, many hosts | Five remote hosts, one flapping, sessions on each, a background start on a host | Scales `rhea` up; poll and backoff behaviour across hosts |

## How a persona is measured

See `README.md`. In short: each step records clicks, keys, menus and
dialogs, the time spent waiting on the window, an estimated human time (the
Keystroke-Level Model plus the waits), every HTTP request with its status
and latency, console errors, and notes for friction the script ran into.
