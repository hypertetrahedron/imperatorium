# Goldens: the Python reference's answers

Everything here was recorded from the Python implementation (the reference
the binary was ported from) by the conformance scripts in `rust/conformance/`
run with `--record DIR`. The Rust integration tests in `rust/imperatorium/tests/`
replay the same inputs against the binary and compare.

Python is gone from the repository (2026-11-15); the last commit that has it
is tagged `last-python`. These files are now the spec: change one only on
purpose, with the reason in the commit. The script names below (`twin.py`,
`golden.py`, ...) are the scripts at that tag.

## Recording (at the `last-python` tag only)

To record again, check out `last-python` in a separate worktree, with its
venv (`requirements.txt`). Hold the conformance lock (one part at a time; the
twins use fixed ports 8812-8820), build fakecli first
(`cargo build --release -p fakecli`), then:

```
python rust/conformance/<part>.py --record rust/imperatorium/tests/golden/<part>
```

(`run_policy_tests.py` records into `policy_tests/`.) Record twice into two
folders and `diff -r` them: every part here came out byte-identical across
two captures (2026-11-14).

Edited since: `settings_twin/steps.jsonl`'s settings help and restart texts
(2026-11-15) name `imperatorium` subcommands in place of the deleted Python
scripts, to match `assets/settings_fields.json`. Also there: the new
`chat.conversation_cache_mb` field (2026-11-15), inserted after
`chat.matrix_status` in every settings answer, as the schema has it.
The `highlight.prompts` and `highlight.colors.prompt` fields (2026-11-17)
likewise, after `highlight.shape` and `highlight.colors.guide`, and
`prompts` in `/api/health`'s `look.hl`.
`hooks_blackbox/cases.jsonl` (2026-11-15, user decision): a policy whose
`park` sets no `wait_seconds` now holds the request until it is answered,
so the `background_wait_seconds`-only case parks with `wait` 604800 (and
`wait_question` 120) where Python sent 0. Parks also carry the hook's pid
(`_hook_pid`), which hooks_blackbox.rs checks and leaves out of the compare.

Machine-specific strings are placeholders (`golden.py` `Places`; put back by
`tests/common/mod.rs` `Places`): `<repo>` the checkout, `<repo-parent>`,
`<home>` the user's home, `<tmp>` the temp folder the script worked in,
`<state>` a hook run's state folder; each also as `-lc` (lower-cased),
`/` (forward slashes), `j:` (JSON-escaped, inside text that is itself JSON)
and `u:` (percent-encoded, a deep link). Twins add `<tag>` (inbox pipe names'
random part), `<pid>`, `<sid>`, `<side>` (py/rs), `<twin-home>`, `<feat>`,
`<proj>`, `<role>` ids, `<new-uuid>`, `<id>`, `<clock>` - each described below.

## Ported (tests that run in `cargo test`)

| Golden | Script | Test | What is compared |
|---|---|---|---|
| `policy_tests/calls.jsonl` | run_policy_tests.py | policy_tests.rs | every `policy.decide` call test_policy.py made, its exact (verdict, reason, tier) |
| `differential/` | differential.py | differential.rs | 86,197 generated cases, rebuilt from `spec.json` in `cases()` order (checked against `samples.json`); answers as indexes into `answers.json` (`index.txt`: 2*answer + 1 if explained); LOOSER or unexplained differences fail |
| `hooks_blackbox/cases.jsonl` | hooks_blackbox.py | hooks_blackbox.rs | `hook`/`statusline`/`permission`: exit code, stdout, each request to a fake dispatcher, Content-Types, decisions.jsonl minus `at` |
| `sessions_compare/`, `items_compare/`, `stats_compare/` | the three corpus scripts | corpus.rs | over `tests/fixtures/corpus` (synthetic, mtimes pinned): SDK list/info/chain, to_items plain and merged, sessionstat at the clocks in `clocks.json`. No placeholders (a transcript may say "<repo>" itself) |
| `settings_compare/cases.jsonl` | settings_compare.py | pure_checks.rs | settings.check per field and value; `raises` rows skipped |
| `slash_compare/cases.jsonl` | slash_compare.py | pure_checks.rs | parse, refusal, clean_typed, typed_refusal |
| `projects_compare/` | projects_compare.py | pure_checks.rs | _norm, resolve, parse_utterance, deeplink.build; made-up catalogue only (this machine's project list is not recorded) |
| `focus_compare/cases.jsonl` | focus_compare.py | pure_checks.rs | normalise_title, match_title (the pure half; the live window enumeration is machine state and not recorded) |
| `install_compare/cases.jsonl` | install_compare.py | install_compare.rs | settings.json after install-hooks, per start x variant x once/twice; `start_text` is the exact starting file |
| `session_twin/steps.jsonl` | session_twin.py | session_twin.rs | every resume/new/close, typing, dispatch and focus answer (neutral(); typing with id/at as 0), fakecli's log in order, the events, the closed consoles exited, the typed lines; time scale 10 (see below) |
| (none: expectations in the test) | remote_twin.py | remote_twin.rs | the script's 32 checks against fakecli as ssh (fakessh.py's answers), plus: the hook listener is up, only the fake host was called |
| (none: expectations in the test) | remote_cli_twin.py | remote_cli.rs | the script's 27 checks, fakecli as ssh in its sh mode (sandbox HOME); skipped without `sh` |

The local-only full corpus is not here: see "The local corpus" below.

## The twins: how they were recorded and replayed

(Every twin below is now replayed by a test in `rust/imperatorium/tests/`;
the notes are kept as the record of what each replay sets up.)

Each twin's `--record` ran BOTH dispatchers as Python (the "rs" side a
second Python dispatcher) so every comparison in the script still ran,
Python against itself, and wrote the py side's values as `steps.jsonl`: one
JSON object per line, `{"step": kind, ...request..., "want": answer}`, in the
order the script made them. A replay test starts ONE binary dispatcher with
the harness (`Dispatcher::start`, port 0), sets up what the script set up
(below, and the script itself is the exact recipe), sends each step's
request and compares with `want` as the script compared (its normaliser:
`masked`, `scrub`, `neutral`, `same_text`). `config.json` beside a twin's
steps is the config the twin's home got (`twin-<side>`: use any topic; the
ntfy server `http://127.0.0.1:8819/` is the script's fake: use your own
FakeServer's port).

`tests/harness.rs` already replays twin.py's `hook` steps (32 answers).

Two rules for every replay (from `rust`, after python-out/runtime):

- **Time:** start the binary with `IMPERATORIUM_TEST_TIME_SCALE` (e.g.
  `0.05`; see `timing.rs` on `rust`). It scales the queue sweep, the idle
  timer, the notify delay, park/question holds, stream and matrix intervals,
  update checks and remote polls, so inbox_twin's 15 s sweep, notify_twin's
  60 s idle check and the parks' waits take a fraction of their recorded
  time. Scale the test's own waits (the `sleep` steps, the 1.5 s and 1 s
  waits inside `parks()`, the `wait` values in park bodies) by the same
  factor, and still poll with deadlines rather than sleep.
- **/api/hosts:** the binary deliberately has no `python` or
  `reader_python` keys there (it runs on the hosts itself). Python's
  answer is recorded as it is; drop those two keys from the golden before
  comparing. (No golden in this folder records /api/hosts yet; a recording
  that adds one keeps them.)

### twin.py -> `twin/steps.jsonl`

- Setup: no config.json, `--home <repo>`, CLAUDE_CONFIG_DIR an EMPTY folder
  (recording isolated it; the comparison mode reads the machine's real
  sessions, which a replay must not).
- Steps: `hook` (POST /hook `body`, want `[status, answer]`), `refused`
  (POST `path` with exact `raw` (latin-1 text of the bytes, or null: send
  `body` as JSON), `ctype` (null: no Content-Type), `origin` (with `%d`: the
  dispatcher's own port)), `post` (the /api POSTs; `/api/dismiss` of a real
  row is absent: no real rows), `session ids` (sorted non-remote ids of GET
  /api/sessions), `state.json` (the state file, `masked()`: keys in CLOCKS
  holding numbers -> `<clock>`, `dismissed` values -> `<clock>`), `get` (GET
  `path`, masked; /api/usage and /api/matrix were retried up to 3 times 2 s
  apart in case spend moved).
- Compare: first_difference (key order counts).

### inbox_twin.py -> `inbox_twin/` (Windows: named pipes; ~140 s in Python)

- Setup: `records.json` - the made-up session records to write under
  CLAUDE_CONFIG_DIR/sessions (`pid`: the test's own pid, so they are alive;
  `startedAt`: now in ms; `<tag>`: any unique tag, the pipes are
  `\\.\pipe\imp-twin-<tag>-{idle,busy,waits,late}`, served by the test,
  each accepting any number of writers and keeping each connection's bytes;
  `-nobody` is never served). `policy.json` -> IMPERATORIUM_POLICY (fixed;
  the comparison mode copied the live policy).
- Steps: `post` (`path`, `body`, `window`: true = Origin + the page's
  X-Imperatorium-Window token from GET /; `want` `[status, scrub(answer)]`;
  `delivered`: what each pipe received, per session id, UTF-8). `sessions`
  (status, dispatchable, attention per row, sorted). `late hook` then
  `sweep`: the sweep runs every 15 s, so the late session's prompt arrives
  within ~40 s - poll the pipe, do not sleep. `parks`: the whole of
  `parks()` (parked permission requests on threads, /api/pending polled,
  /api/decide, a 1.5 s and a 1 s wait that matter: past `wait`, inside
  `wait_question`/`wait_background`). `mod`: the whole of `mods()` (reads
  `<state dir>/mod-token`; beats, queue, checks, usage), `given` = 2.
  `decisions.jsonl` (scrubbed lines of the state dir's audit log),
  `state.json` (queue, events, sessions; scrubbed), `get` /api/queue.
- scrub: masked, then `id`/`delivered`/`mod_prompt` strings -> `<id>`, an
  `error` naming a pipe -> `<error>`.

### notify_twin.py -> `notify_twin/` (~70 s: the idle check)

- Setup: config.json as given (ntfy at your FakeServer, recording each POST
  as {topic, body, Title, Priority, Tags, Authorization}); five session
  records (status idle, socket `\\.\pipe\imp-notify-twin-nobody`), names
  twin-asks/-answered/-fails/-quiet/-later; IMPERATORIUM_POLICY a missing
  file.
- Steps: `post` and `sleep` in order; `idle push`: wait (poll, up to 100 s)
  until 5 pushes arrived; `get` /api/notify; `pushes` (sorted by JSON text;
  compare as a multiset; the 5-hour reset is `<local HH:MM of 4102444800>`:
  format 4102444800 in local time, %H:%M); `events` ((kind, session_id,
  summary) sorted).
- The /api/notify POST answers were not compared by the script (they carry
  `enabled`); recorded anyway.

### settings_twin.py -> `settings_twin/`

- Setup: notify_twin's start() (config.json as given), saves to POST
  /api/settings, `window: true` as above.
- Steps: `save` (want: same_text(answer): the twin's home -> `<twin-home>`,
  topic -> twin-<side>), `file` (config.json and config.json.bak in the
  home, byte for byte, null when absent), `get` (/api/settings, /api/quick,
  /api/notify via same_text; /api/usage `thresholds`; /api/health `look`).
- The `roots` change names `<tmp>`: the test's own temp root.

### worktree_twin.py -> `worktree_twin/` (~55 s)

- Setup: `project()` in the script: a repo `projects/proj-<side>` with an
  `origins/proj-<side>.git` bare origin, worktrees under .claude/worktrees
  per case (merge, dirty, conflict, pr, clean, unmerged, dirtydiscard,
  mainbusy, detached) plus `byhand-proj-<side>`; main moves on after. GIT
  author/committer env fixed. config `roots` = [<tmp>/projects]; env
  GH_CONFIG_DIR empty folder, GH_TOKEN/GITHUB_TOKEN empty, GH_PROMPT_DISABLED=1
  (gh fails as unauthenticated; never reaches GitHub).
- Steps: `worktree` (POST /api/worktree `body`, `proj-<side>` your project;
  want with proj-* -> proj-<side>), `main checkout` (edit or restore a.txt
  in main between cases), `left` (worktrees, branches, main log, status),
  `pushed` (agent-pr in the origin), `events` (worktree_* (kind, summary)).

### bg_worktree_twin.py -> `bg_worktree_twin/` (Windows)

- Setup: fakecli as claude.exe on the stand-in PATH (FAKE_LOG set by the
  harness); repo `proj-<side>` on main, `proj-<side>-feat` worktree on feat
  one commit ahead; a session record `<sid>` (status idle) whose cwd is the
  feat checkout; config background.model = sonnet.
- Steps: `dispatch` (POST /api/dispatch; `<proj>`/`<feat>` the two
  checkouts; `bg_id` -> `<id>`), `launched` (fakecli's `claude` calls: args,
  cwd), `events` (dispatch_background without at/bg_id), `left` (worktrees
  made: leaf, branch, noted base, has feat's file).

### session_twin.py -> `session_twin/` (Windows; ~50 s)

- Setup: fakecli as claude.exe and wt.exe; five hidden fakecli consoles
  (`--fake-console`, one `--stubborn`) per side whose pids go in the session
  records (roles leaves/stubborn/replaced/typing/busy; ids
  `<role>`), a record `<gone>` without pid; env adds AI_AGENT,
  CLAUDE_CODE_ENTRYPOINT, CLAUDE_PID (must not reach a launch); config as
  given.
- Steps: `post` (resume/new/close, dispatch, focus; `<new-uuid>` for fresh
  ids, `"<cannot open the inbox>"` for the OS's pipe error), `typing`
  (typing_cases in the script: POST /api/typed from the window or not, a
  parked request holding it, /api/queue of /exit and /compact; `id`/`at` as
  0), `launched` (fakecli's log, wt.exe as its basename), `events`. The
  script also checks the closed consoles exited and the typed lines
  (`hello world !`, `/model opus`, `rm -rf build`).
- Never send mode `launch`, nor `auto` with a failing target: either opens
  a real Claude window through the deep link.
- Replay (session_twin.rs): IMPERATORIUM_TEST_TIME_SCALE is 10, not below
  1: the queue sweep must not drain the queued `/compact` (in the recording
  it did not), and the park's `wait` is divided by the scale.
  IMPERATORIUM_TEST_DEEPLINK_LOG sends deep links to a file; the test stops
  at the first one, at a launch fakecli did not log, and before the first
  step unless `wt` and `claude` resolve to the fakes (tests/common/safety.rs).

### remote_twin.py: nothing recorded

It has no Python side: its expectations are written in the script (from
dispatcher.py's remote paths) and it runs only the binary, against
`fakessh.py` (a Python stand-in for ssh). Ported as remote_twin.rs, fakessh as
fakecli copied to ssh.exe (remote_cli_twin.py's sh-running ssh is its
`FAKESSH_MODE=sh`, remote_cli.rs).

## The local corpus (never committed)

The synthetic corpus covers what the readers handle; the full check runs on
a frozen copy of this machine's transcripts, outside the repository. Its
goldens were recorded once, at `last-python`; a new machine can't record
them without that tag's Python:

```
copy  ~/.claude/projects  ->  %LOCALAPPDATA%\imperatorium\conformance-corpus\projects
python rust/conformance/{sessions,items,stats}_compare.py \
    --record %LOCALAPPDATA%\imperatorium\conformance-corpus\golden\<part> \
    --corpus %LOCALAPPDATA%\imperatorium\conformance-corpus --in-place --now 1794614400
```

`corpus.rs` `local_corpus_reads_as_python_reads_it` runs when that folder
(or `$IMPERATORIUM_CORPUS`) has `projects/` and `golden/`, and says it is
skipped otherwise. One `sessions batch` process reads every session; with
the debug binary that takes about a minute (`IMPERATORIUM_TEST_BIN` can
point the tests at a release build).

## Known differences

- `corpus.rs` KNOWN: synthetic sessions 17 and 26 (token counts beyond
  u64/i64): Python keeps exact ints, the binary gives a float or saturates.
- Synthetic session 25: Python's stats fold raises (a string `message`);
  the binary answers. Recorded as `raises`, not compared.
