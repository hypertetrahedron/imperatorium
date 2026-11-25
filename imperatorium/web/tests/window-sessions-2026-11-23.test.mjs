// The window changes of 2026-11-23 (user request) to starting, closing and
// finding sessions: a tile's ✕ always ends the session and removes the tile;
// a provisional tile and chat note stand in while a session starts; History
// filters by folder, sorts and narrows by time, and /resume starts on the
// session's own folder.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SCRIPT, ESC, between, evalJson, cat, j, assertIn, assertNotIn, renderShell } from "./load.mjs";

const DIRS = cat(
  between(SCRIPT, "const folder = cwd =>", "\n// What a tile calls a session."), "\n",
  between(SCRIPT, "const normDir = p =>", "\n"), "\n");

describe("ProvisionalSessions", () => {
  const body = () => cat(
    "function key(s){ return s.sessionId || s.id; }\n",
    between(SCRIPT, "// Pure: whether row `s` is the session provisional `p` became", "function provGone("));
  const run = (code) => evalJson(cat(body(), "\n", code));

  it("a provisional becomes the session with its id, or a background one by its short id", () => {
    const got = run(cat(
      "const p = {match: {id: 'abc-1', bg: null}}, q = {match: {id: null, bg: '1f2e3d4c'}}, n = {match: null};\n",
      "process.stdout.write(JSON.stringify([",
      " provMatches(p, {sessionId: 'abc-1'}), provMatches(p, {sessionId: 'abc-2'}),",
      " provMatches(q, {id: '1f2e3d4c', sessionId: 'zzz'}), provMatches(q, {sessionId: '1f2e3d4c-aaaa'}),",
      " provMatches(q, {sessionId: 'ffff'}), provMatches(n, {sessionId: 'abc-1'}),",
      " provMatches(p, {sessionId: 'abc-1', provisional: true})]));"));
    assert.deepEqual(got, [true, false, true, true, false, false, false]);
  });

  it("each waits, gives way to its session, or is given up on once late", () => {
    const got = run(cat(
      "const a = {sessionId: 'starting-1', match: {id: 's1'}, until: 1000},",
      "      b = {sessionId: 'starting-2', match: {id: 's2'}, until: 1000},",
      "      c = {sessionId: 'starting-3', match: null, until: 0};\n",
      "const r = provSort([a, b, c], [{sessionId: 's1'}], 2000), w = provSort([b], [], 500);\n",
      "process.stdout.write(JSON.stringify([r.keep.map(key), r.found.map(([p, s]) => [key(p), key(s)]),",
      " r.late.map(key), w.keep.map(key)]));"));
    // c is still being started (no answer yet), so it never times out.
    assert.deepEqual(got, [["starting-3"], [["starting-1", "s1"]], ["starting-2"], ["starting-2"]]);
  });

  it("starting a session shows a tile and a loading note, and a failure a dialog", () => {
    const spawn = between(SCRIPT, "async function spawnFrom(", "// A session launched from here is selected");
    for (const needle of ["provAdd({name:", "it is loading.", "provFailed(p, r.error", 'notice("Could not start it"',
                          "p.match = {id: r.session_id || null, bg: r.bg_id || null};", "addNote(sid, loading)"])
      assertIn(needle, spawn);
    // A background session no longer needs a task first.
    assertNotIn("type what the background session should do first", SCRIPT);
    // Only a remote host's start (through /api/dispatch) still has a greeting.
    assertNotIn("Ready when you are.", between(SCRIPT, "const model = $(\"#nmodel\").value", "async function openNew("));
    assertNotIn("Ready when you are.", spawn);
    // The board's list keeps the ones still starting, and drops late ones.
    assertIn("sessions = list.concat(provSweep(list));", SCRIPT);
    assertIn("else if (provisional.some(p => p.until && Date.now() > p.until))", SCRIPT);
    // Nothing is asked of the dispatcher about a session that is not there yet.
    assertIn("if ((current() || {}).provisional){ lastSig = \"\"; logItems = []; drawLog(force); return; }", SCRIPT);
    assertIn("if (!selected || (current() || {}).provisional){ $(\"#status\").hidden = true; return; }", SCRIPT);
  });
});

describe("ClosingSessions", () => {
  it("a closed tile stays off a list fetched before the close, for a while", () => {
    const got = evalJson(cat(
      "function key(s){ return s.sessionId || s.id; }\n",
      between(SCRIPT, "function withoutClosed(", "\n}\n"), "\n}\n",
      "const closed = new Map([['a', 1000]]), list = [{sessionId: 'a'}, {sessionId: 'b'}];\n",
      "process.stdout.write(JSON.stringify([withoutClosed(list, closed, 5000, 15000).map(key),",
      " withoutClosed(list, closed, 20000, 15000).map(key)]));"));
    assert.deepEqual(got, [["b"], ["a", "b"]]);
  });

  it("the ✕ says what it is doing in the chat and removes the tile when done", () => {
    const close = between(SCRIPT, "async function closeSession(", "// A tile's ✕ when nothing is running");
    for (const needle of ['addNote(k, bg ? "Stopping this background session…"', "closing.add(k)",
                          'setNote(n, "Could not close it: "', "removeTile(k,", 'r.how === "stopped" ? "stopped "'])
      assertIn(needle, close);
    assertIn("removeTile(k, \"removed \"", between(SCRIPT, "async function dismissSession(", "const CLOSED_HOLD"));
    // While it closes the tile says so and offers no second ✕.
    assertIn(': closing.has(key(s)) ? ["d-busy", "closing…"] : stateOf(s);', SCRIPT);
    assertIn("const x = (s.remote && !remoteOff) || s.provisional || closing.has(key(s)) ? \"\"", SCRIPT);
    // A host's session gets a dismiss only once nothing of it runs there (2026-11-24).
    assertIn('const remoteOff = s.remote && !s.pid && ["stopped", "done", "failed"].includes(s.state);', SCRIPT);
  });

  it("a note is drawn at the end of the chat, escaped", () => {
    const got = evalJson(cat(ESC, "\nconst renderText = t => '<div class=\"txt\">' + esc(t) + '</div>';\n",
      between(SCRIPT, "function noteHtml(", "\n}\n"), "\n}\n",
      "process.stdout.write(JSON.stringify([noteHtml({id: 3, text: 'a <b>', state: 'working'}),",
      " noteHtml({id: 4, text: 'no', state: 'failed'})]));"));
    assertIn('class="turn sys wnote working" data-note="3"', got[0]);
    assertIn("working…", got[0]);
    assertIn("a &lt;b&gt;", got[0]);
    assertNotIn("working…", got[1]);
    assertIn("(chatNotes[selected] || []).forEach((n, j) => {", SCRIPT);
  });
});

describe("HistoryFilters", () => {
  const body = () => cat(ESC, "\n", DIRS, between(SCRIPT, "// Pure: the query string for the History search.", "// /resume (and History"));
  const run = (code) => evalJson(cat(body(), "\n", code));

  it("asks for the folder, order and time only when chosen", () => {
    const got = run(cat("process.stdout.write(JSON.stringify([historyQuery('pi tunnel', '', 'modified', ''),",
      " historyQuery('', 'C:\\\\w\\\\app', 'created', '7')]));"));
    assert.deepEqual(got, ["/api/history?q=pi%20tunnel", "/api/history?q=&folder=C%3A%5Cw%5Capp&sort=created&days=7"]);
  });

  it("lists every folder with its count, keeps the chosen one, escaped", () => {
    const got = run(cat("process.stdout.write(JSON.stringify([",
      " folderOptions([{folder: 'C:\\\\w\\\\App', count: 3}, {folder: 'C:\\\\w\\\\<x>', count: 1}], 'c:/w/app/'),",
      " folderOptions([], 'C:\\\\w\\\\new')]));"));
    assertIn('<option value="">all folders</option>', got[0]);
    assertIn(' selected title="C:\\w\\App">App (3)</option>', got[0]);
    assertIn("&lt;x&gt; (1)", got[0]);
    assertIn('value="C:\\w\\new" selected', got[1]);
    assertIn("new (0)", got[1]);
  });

  it("says when a session was active and, on another day, when it started", () => {
    const got = run(cat("const d = new Date(2026, 10, 23, 10, 0).getTime() / 1000;\n",
      "process.stdout.write(JSON.stringify([histWhen({modified: d, created: d - 3600}),",
      " histWhen({modified: d, created: d - 3 * 86400}), histWhen({modified: d})]));"));
    assertNotIn("started", got[0]);
    assertIn(" · started ", got[1]);
    assert.ok(got[2].startsWith("active "));
  });

  it("has the controls, and /resume opens it on the session's folder", () => {
    const page = renderShell();
    for (const id of ["id=hfolder", "id=hsort", "id=hdays", "id=histbias"]) assertIn(id, page);
    for (const v of ["modified", "created", "oldest", "title", "size"]) assertIn(`<option value=${v}>`, page);
    assertIn('if (what === "history") return historyFor(s);', SCRIPT);
    const forS = between(SCRIPT, "function historyFor(", "let histBiased");
    assertIn("(w ? w.main : s.cwd)", forS);   // a worktree session: its project's folder
    assertIn("Show all folders", forS);
    // From the menu it starts on every folder.
    assertIn('if (!histBiased){ $("#hfolder").value = ""; $("#histbias").hidden = true; }', SCRIPT);
    assertIn('["#hfolder", "#hsort", "#hdays"].forEach(sel => $(sel).addEventListener("change", search));', SCRIPT);
  });
});
