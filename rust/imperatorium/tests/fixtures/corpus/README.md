# Synthetic transcript corpus

Every byte here is invented: no real session, path, prompt or reply is in it.
`projects/` was written deterministically by `make_corpus.py` (fixed 2026 timestamps,
ids derived from a counter, no clock), deleted with the rest of the Python on
2026-11-15 and kept at the `last-python` tag; the files are now edited by hand. Point
`CLAUDE_CONFIG_DIR` at this folder (copy it first and set file mtimes if a
check depends on them: `last_modified` and "newest file wins" do).

Session ids are `5e55NNNN-0000-4000-8000-00000000NNNN`, NNNN the hex number below.

| # | Covers |
|---|---|
| 01 | plain turns; text, thinking, Read/Grep/Edit/Write/MultiEdit/TodoWrite/Bash; isMeta first line; turn_duration; progress; system-reminder text; last-prompt summary |
| 02 | spaced JSON (`"k": "v"`), ASCII-escaped (surrogate pairs), +01:00 timestamps, ai-title/custom-title with escapes, tags, summary |
| 03 | two compactions (boundary, isCompactSummary, logicalParentUuid); also copies of this id in two other folders (one empty) |
| 04 | inbox: peer prompts (wrapper, origin note, list content, senderTaskId), queued_command attachments (task-notification, list, empty, sidechain), queue-operation, task-notification origin; a second copy in a later folder |
| 05 | first-prompt skips (command-name fallback, hooks, tick, goal, ide tags, interrupts, blank), local-command stdout/stderr, task-notification with/without summary, bash tags |
| 06 | sidechain and teamName entries, Task with result, two subagent transcripts; 29 is a sidechain-first session (not listed) |
| 07 | background Bash/PowerShell (`backgroundTaskId`), BashOutput/KillShell/TaskOutput, TaskStop, notifications (completed/failed/killed/running), foreground calls still open |
| 08 | Monitor: timeout, persistent, bool/absent timeout, TaskStop, notification end |
| 09 | Task/Agent: async_launched, foreground done/failed, no result, still open |
| 0a | teammates: spawned, idle_notification (message, list, attachment), SendMessage wakes, shutdown/terminated, TaskStop by name, bad bodies |
| 0b, 0c | ScheduleWakeup (replace, stop, zero), CronCreate (recurring, one-shot, past, no fire time), CronDelete |
| 0d | usage: models, repeated message id, 1h and 5m cache, odd types, no id, no timestamp, sidechain usage |
| 0e | mcp_instructions_delta, stop_hook_summary (string/dict/long, trimmed to 10), turn_duration variants, api_error/other system subtypes, branch and version changes |
| 0f | api errors, interrupts, images, odd tool_result content, AskUserQuestion, Edit/MultiEdit edge cases, non-dict blocks, unknown block types, junk timestamp |
| 10 | accents, CJK, emoji, raw U+2028/U+2029/U+0085, exotic whitespace, astral char at the 200-char cut, unicode folder and cwd |
| 11 | CRLF line endings throughout |
| 12 | lone CR between records |
| 13 | truncated last line (no final newline) |
| 14 | empty file |
| 15 | only junk: non-JSON, non-objects, no uuid, BOM, trailing junk |
| 16 | big integers within i64, 1e16, 0.1 |
| 17 | numbers beyond i64 and u64 |
| 18-1a | branching leaves, duplicate uuid, odd parentUuid, cycles, orphan chain, meta/sidechain/team-only leaves |
| 1b, 1c, 1e | compact boundary with dangling, missing and self logicalParentUuid |
| 1f, 20, 21 | summary-only, metadata-only (not listed), command-only first prompt with zero usage |
| 22 | 130 KB: head and tail windows differ, long text/result/command/diff, renamed title and tag in the tail |
| 23 | invalid UTF-8 byte and a multibyte character cut at the end |
| 25 | a `message` that is a string (the Python stats fold raises on it) |
| 26 | token sums that overflow i64 |

Also: `notes.jsonl`, a non-UUID `.jsonl` name and `projects/stray.txt`, which no reader may treat as a session.
