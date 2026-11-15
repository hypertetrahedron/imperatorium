// The newer window helpers - unread, attention, keys, warnings, find, diff
// comments, cost chart - run against app.js.
// Ported from test_ui.py: Unread, Attention, HeavyWarnings, FindInChat,
// DiffComments, CostChart.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SHARED, SCRIPT, between, evalJson, j, cat, renderShell, assertIn, assertNotIn, assertLess } from "./load.mjs";

/** The helpers after "what changed since you last looked", with `expr` JSON-written. */
const glanceJs = (expr) => {
  const body = between(SCRIPT, "// -- what changed since you last looked", "let ticks = 0;");
  const kfmt = between(SCRIPT, "function kfmt", "function span");
  return evalJson(cat(
    SHARED, "const key = s => s.sessionId || s.id; const nameOf = s => s.name || s.id;",
    " let selected = null;\n", kfmt, body,
    "\nprocess.stdout.write(JSON.stringify(", expr, "));"));
};

describe("Unread", () => {
  /** A session that finished a turn while you looked elsewhere is "new". */
  it("first sight is seen and a later finish is new", () => {
    const got = glanceJs(cat(
      "(() => { const a = {id: 'a', status: 'idle', status_changed_at: 100},",
      " b = {id: 'b', status: 'busy', status_changed_at: 90};",
      " seenAt = updateSeen({}, [a, b], null, 200); const first = [isUnread(a), isUnread(b)];",
      " b.status = 'idle'; b.status_changed_at = 150; seenAt = updateSeen(seenAt, [a, b], null, 210);",
      " const later = [isUnread(a), isUnread(b)];",
      " selected = 'b'; const whileOpen = isUnread(b);",
      " seenAt = updateSeen(seenAt, [a, b], 'b', 220); selected = 'a';",
      " return [first, later, whileOpen, isUnread(b)]; })()"));
    assert.deepEqual(got, [[false, false], [false, true], false, false]);
  });

  it("gone sessions drop out but an empty list changes nothing", () => {
    const got = glanceJs(cat(
      "[updateSeen({a: 5, b: 6}, [{id: 'a', status: 'busy'}], null, 9),",
      " updateSeen({a: 5}, [], null, 9)]"));
    assert.deepEqual(got, [{ a: 5 }, { a: 5 }]);
  });

  it("tiles and nodes show it", () => {
    for (const needle of ['class="badge new"', "isUnread(s)", "kids.filter(isUnread)"]) {
      assertIn(needle, SCRIPT);
    }
  });
});

describe("Attention", () => {
  it("only sessions that start waiting are announced", () => {
    const got = glanceJs(cat(
      "(() => { const list = [{id: 'a', attention: true}, {id: 'b', attention: true}, {id: 'c'}];",
      " return [newlyWaiting(null, list).length,",
      " newlyWaiting(new Set(['a']), list).map(key)]; })()"));
    assert.deepEqual(got, [0, ["b"]]);
  });

  it("a session that just started is switched to", () => {
    const got = glanceJs(cat(
      "(() => { const now = 2e9, old = {id: 'old', startedAt: (now - 5000) * 1000},",
      " a = {id: 'a', startedAt: (now - 10) * 1000}, b = {id: 'b', startedAt: now - 5},",
      " stale = {id: 'stale', startedAt: (now - 1000) * 1000}, bare = {id: 'bare'};",
      " return [newlyStarted(null, [old, a], now),",
      " newlyStarted(new Set(['old']), [old, a, b], now),",
      " newlyStarted(new Set(['old']), [old, stale, bare], now),",
      " newlyStarted(new Set(['old', 'a']), [old, a], now)]; })()"));
    assert.deepEqual(got, [null, "b", null, null]);
    assertIn("catchStarted(newlyStarted(", SCRIPT);
  });

  it("title counts waiting sessions", () => {
    assert.deepEqual(glanceJs("[titleFor(0), titleFor(3)]"), ["Imperatorium", "(3) Imperatorium"]);
  });

  it("next waiting goes longest wait first and wraps", () => {
    const got = glanceJs(cat(
      "(() => { const l = [{id: 'x'}, {id: 'new', attention: true, attention_since: 50},",
      " {id: 'old', attention: true, attention_since: 10}];",
      " return [nextWaiting(l, 'x'), nextWaiting(l, 'old'), nextWaiting(l, 'new'),",
      " nextWaiting([{id: 'x'}], 'x')]; })()"));
    assert.deepEqual(got, ["old", "new", "old", null]);
  });

  it("the window wires it up", () => {
    const page = renderShell();
    for (const needle of ["id=fav", "id=jumpbox", "id=jq", 'new EventSource("/api/stream")',
                          "openStream();", "gotSessions(", "goWaiting()"]) {
      assertIn(needle, page);
    }
  });
});

describe("HeavyWarnings", () => {
  it("full context and cold large cache warn", () => {
    const got = glanceJs(cat(
      "[heavyWarnings({usage: {context_pct: 91}}, 100).map(h => h.kind),",
      " heavyWarnings({work: {context_tokens: 230000, cache_expires_at: 50}}, 100).map(h => h.short),",
      " heavyWarnings({work: {context_tokens: 230000, cache_expires_at: 500}}, 100).length,",
      " heavyWarnings({work: {context_tokens: 20000, cache_expires_at: 50, cold_tokens: 100000}}, 100).length,",
      " heavyWarnings({work: {context_tokens: 20000, cache_expires_at: 50}}, 100).map(h => h.short),",
      " heavyWarnings({work: {context_tokens: 20000, cache_expires_at: 50, cold_tokens: 0}}, 100).length,",
      " heavyWarnings({work: {cache_expires_at: 50}}, 100).length,",
      " heavyWarnings({usage: {context_pct: 40}}, 100).length]"));
    assert.deepEqual(got, [["context"], ["cold 230k"], 0, 0, ["cold 20k"], 1, 0, 0]);
  });

  it("cold cache is a button in the chat", () => {
    const got = glanceJs(cat(
      "[coldHtml({pid: 7, work: {context_tokens: 230000, cache_expires_at: 50, resume_cost_usd: 1.15}}, 100),",
      " coldHtml({work: {context_tokens: 230000, cache_expires_at: 50}}, 100),",
      " coldHtml({pid: 7, work: {context_tokens: 230000, cache_expires_at: 500}}, 100),",
      " coldHtml({remote: true, work: {context_tokens: 230000, cache_expires_at: 50}}, 100)]"));
    assertIn("230k tokens", got[0]);
    assertIn("$1.15", got[0]);
    assertIn('data-coldact="new"', got[0]);
    assertIn('data-coldact="new-close"', got[0]);
    assertIn("a prompt entered as usual continues this session", got[0]);
    assertNotIn("new-close", got[1]);   // no process to close
    assertIn("cache-write price", got[1]);
    assert.deepEqual(got.slice(2), ["", ""]);
  });

  it("the cold button closes until the next expiry", () => {
    const got = glanceJs(cat(
      "(() => { const s = {id: 'a', pid: 7, work: {context_tokens: 230000, cache_expires_at: 50}};",
      " const open = coldHtml(s, 100, {});",
      " const closed = coldHtml(s, 100, {[coldKey(s)]: true});",
      " const later = coldHtml({...s, work: {...s.work, cache_expires_at: 90}}, 100, {[coldKey(s)]: true});",
      " return [open.includes('data-coldact=\"dismiss\"'), closed, later.length > 0]; })()"));
    assert.deepEqual(got, [true, "", true]);
  });

  it("the cold button sits above show most recent", () => {
    assertLess(SCRIPT.indexOf('<div class="cold" hidden>'), SCRIPT.indexOf('data-newest="1"'));
    assert.ok(SCRIPT.includes('<div class="cold" hidden>'));
  });
});

const ITEMS = `[
 {speaker: 'you', blocks: [{kind: 'text', text: 'please fix the Parser'}]},
 {speaker: 'claude', blocks: [{kind: 'tool', name: 'Bash', summary: 'pytest -q'},
   {kind: 'result', text: 'boom', is_error: true}]},
 {speaker: 'claude', blocks: [{kind: 'thinking', text: 'parser again'},
   {kind: 'tool', name: 'Edit', summary: 'parser.py'}]},
 {speaker: 'claude', blocks: [{kind: 'result', text: 'ok', is_error: false}]}]`;

describe("FindInChat", () => {
  it("text tool and failure searches", () => {
    const got = glanceJs(cat(
      "(() => { const it = ", ITEMS, "; return [findHits(it, 'parser', ''), findHits(it, '', ''),",
      " findHits(it, '', 'tool'), findHits(it, 'edit', 'tool'), findHits(it, '', 'fail'),",
      " findHits(it, 'parser', '', (i, b) => b.kind !== 'thinking')]; })()"));
    assert.deepEqual(got, [[0, 2], [], [1, 2], [2], [1], [0, 2]]);
  });

  it("the window wires it up", () => {
    for (const needle of ["id=cfind", "id=cfilter", "markFinds();", "resetFind();", 'data-i="${i}"']) {
      assertIn(needle, renderShell());
    }
  });
});

const DIFF = [
  "diff --git a/src/app.py b/src/app.py", "index 1..2 100644", "--- a/src/app.py",
  "+++ b/src/app.py", "@@ -10,4 +10,5 @@ def main():", " keep", "-old", "+new", "+newer",
  " tail", "\\\\ No newline at end of file"].join("\\n");

describe("DiffComments", () => {
  it("lines know their file and number", () => {
    const got = glanceJs(`diffRows('${DIFF}').map(r => [r.cls, r.file || null, r.line || null, r.side || null])`);
    assert.deepEqual(got, [
      ["f", null, null, null], ["", null, null, null], ["", null, null, null],
      ["", null, null, null], ["hh", null, null, null],
      ["", "src/app.py", 10, "new"], ["r", "src/app.py", 11, "old"],
      ["a", "src/app.py", 11, "new"], ["a", "src/app.py", 12, "new"],
      ["", "src/app.py", 13, "new"], ["", null, null, null]]);
  });

  it("comments become one prompt", () => {
    const got = glanceJs(cat(
      "reviewPrompt([{file: 'a.py', line: 3, side: 'new', code: '  x = `1`', text: 'why?'},",
      " {file: 'b.py', line: 9, side: 'old', code: '', text: 'keep this\\nplease'}])"));
    assertIn("- `a.py:3` - `x = '1'`\n  why?", got);
    assertIn("- `b.py (removed line 9)`\n  keep this\n  please", got);
    assert.ok(got.startsWith("Review comments on your uncommitted changes"));
  });
});

describe("CostChart", () => {
  it("one bar per day with a hover label", () => {
    const got = glanceJs(cat(
      "(() => { const s = costChart([{date: '2026-10-01', cost: 0, tokens: 0},",
      " {date: '2026-10-02', cost: 12.5, tokens: 3100000}, {date: '2026-10-03', cost: 3, tokens: 9}]);",
      " return [(s.match(/class=\"bar\"/g) || []).length, (s.match(/class=\"hit\"/g) || []).length,",
      " s.includes('$12.50 · 3.1M tokens'), costChart([])]; })()"));
    assert.deepEqual(got, [2, 3, true, ""]);
  });

  it("scale rounds up to a tidy number", () => {
    assert.deepEqual(glanceJs("[niceMax(0.7), niceMax(13), niceMax(240), niceMax(5)]"), [1, 20, 250, 5]);
  });
});
