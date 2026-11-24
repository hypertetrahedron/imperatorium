# Persona runs

Friction analysis for the window: scripted personas (`PERSONAS.md`) drive
the real window in headless Chrome against a sandboxed dispatcher and a
simulated fleet of Claude Code sessions. Every step's interactions,
waits, HTTP requests and errors are recorded. These runs are **not** part of
`cargo test` or the JS tests; run them when you want the data.

```
cd personas
npm install                      # playwright; uses the installed Chrome
cargo build -p imperatorium      # in rust/ - the binary under test (or set IMPERATORIUM_TEST_BIN)
node run.mjs                     # set 1;  --set 2, or name personas: node run.mjs wren sol
node report.mjs results/<stamp>  # writes results/<stamp>/report.md
```

`--headed` shows the browser; `--keep` keeps each sandbox folder.

## What is real and what is simulated

- **Real:** the dispatcher binary (production timings, no time scale), the
  window's HTML/JS, git and worktrees, the spawn rules, merges, the remote
  conversation channel (`imperatorium remote-channel`, run locally against
  a fake host home).
- **Simulated** (`lib/sim.mjs`): Claude Code. Each session is a held process,
  a record in `CLAUDE_CONFIG_DIR/sessions`, a transcript, hooks posted to
  `/hook`, and the mod's beat every 2 s. Prompts arrive through the beat as
  they do with the mod installed. A launched session appears 2.5 s after the
  launch, and turns take a few seconds. Times that depend on those are marked
  as simulated in the report.
- **Stubbed** (`stub/simclaude.rs`, compiled once with rustc): `claude`,
  `wt` and `ssh` on the dispatcher's PATH. Nothing reaches a real terminal,
  session or host. The sandbox asserts this before the dispatcher starts, as
  the integration tests do. Deep links, opened files, update checks and
  account spend point at nothing.
- **Not driven:** buttons that reach the desktop (Show Window, Open in
  editor, the Path Menu's Open, Dictate).

## Metrics

Per step: clicks, keys, menu opens, dialogs met, time waiting on the window,
an estimated human time (Keystroke-Level Model: point 1.1 s, click 0.2 s,
key 0.28 s, hand moving between mouse and keyboard 0.4 s, decision 1.35 s,
plus the waits), requests with status and latency, console and page errors,
and step-specific observations (latencies, counts, what a dialog offered).
Per run: dispatcher working set every 5 s, its log's warnings, launches,
the simulated fleet's event timeline.
