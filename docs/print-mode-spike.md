# Spike: approvals in print mode (`claude -p`)

2026-11-22, Claude Code 2.1.330. Question: if "Fork here" runs a fork's first
turn in print mode (so `--resume-session-at` can cut the history at the end
of the previous turn), what happens to permission prompts and questions?

## Setup

Throwaway runs in a scratch folder, Haiku, `--permission-mode manual`,
`--setting-sources project` (none of the user's hooks, and not their
`defaultMode: bypassPermissions`), hooks from a `--settings` file. Every
`CLAUDE*`/`OTEL*`/`IMPERATORIUM*` variable stripped. The real-hook runs used
the installed `imperatorium.exe permission` with `IMPERATORIUM_POLICY`
(`enabled: false`, `decision_format: object`, park on),
`IMPERATORIUM_PARK_ENDPOINT` at a fake server on 8823, and LOCALAPPDATA /
APPDATA in the scratch folder. The live dispatcher saw nothing.

Task in each run: `echo hello > out.txt` with Bash, which asks in manual mode.

## Results

| Run | Hook on PermissionRequest | Outcome |
|---|---|---|
| E1 | logs, says nothing | Denied at once (9 s run). The call is in the result's `permission_denials`; the agent says it was blocked and stops. No Notification fired. |
| E2 | allow | Ran; `out.txt` written. |
| E3 | deny with a message | Denied; the agent quotes the hook's message. Listed in `permission_denials`. |
| E4 | sleeps 90 s, then allow | Waited the full 90 s, then ran. Print mode has no timeout of its own on a held hook. |
| E5 | real `imperatorium.exe permission`, park held 30 s then `{"verdict":"allow"}` | Parked (the payload is the same as an interactive session's: `wait` and `wait_background` 604800), held 30 s, ran. The audit has the `ask` and then `answered in the window`. |
| E6 | real hook; prompt asks for AskUserQuestion | **The tool does not exist in print mode.** The agent asks in plain text and ends the turn. No hook fires. |

PreToolUse and Stop hooks fire in print mode as usual. Notification
(`permission_prompt`) does not: with no terminal there is nobody to notify.

## Fork at the end of a turn

Also checked: `--resume <id> --resume-session-at <uuid> --fork-session`
(the flag is hidden and documented as print mode only).

1. Turn 1: "remember APPLE". Turn 2 on the same session: "remember BANANA".
2. Fork, cut at turn 1's last assistant entry: it lists only APPLE. It gets a new session id.
3. Resuming the fork again later (plain `--resume <fork id>`): still only APPLE. The cut is in the fork's own file (no BANANA in it), not something applied at run time.
4. The original still has APPLE and BANANA.

## What it means for "Fork here"

- Approvals work as they do in background sessions, provided the permission
  hook is installed. Without it, anything that would ask is denied, not held.
- The fork's first turn cannot use AskUserQuestion. It falls back to asking
  in plain text, which the user answers with the next message (by then it is
  an ordinary session, resumed in the background).
- Pass `--permission-mode` explicitly (the background setting, `acceptEdits`
  by default). Otherwise the user's own `defaultMode` applies, and here that
  is `bypassPermissions`.
- The cut lasts: after the first turn, the fork is a normal session.

Scripts used (scratch, not kept): a logging hook that answers by
`SPIKE_MODE`, a fake `/permission/park` server, and a runner.
