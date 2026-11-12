import { rowsOf, hardBreaks, answerFor, pendingFor, nextMode, KINDS } from "../mods/imperatorium-chat/rows.js";
import assert from "node:assert/strict";
assert.equal(hardBreaks("1\n2\n3"), "1  \n2  \n3");
assert.equal(hardBreaks("a\n\nb"), "a\n\nb");
assert.equal(hardBreaks("x\n```\ncode\nmore\n```\ny"), "x\n```\ncode\nmore\n```\ny");
assert.equal(hardBreaks("| a |\n| b |"), "| a |\n| b |");
const modes = Object.fromEntries(KINDS.map(([k]) => [k, "full"]));
const items = [
  { speaker: "you", blocks: [{ kind: "text", text: "hi" }] },
  { speaker: "claude", blocks: [{ kind: "text", text: "hello" }, { kind: "tool", name: "Bash", summary: "ls" }] },
  { speaker: "tool", blocks: [{ kind: "result", text: "a\nb", is_error: false }] },
  { speaker: "you", blocks: [{ kind: "text", text: "again" }] },
];
let rows = rowsOf(items, modes, 0);
assert.deepEqual(rows.map((r) => r.kind), ["speaker", "text", "speaker", "text", "tool", "result", "speaker", "text"]);
assert.equal(rows.find((r) => r.kind === "result").lines, 2);
rows = rowsOf(items, modes, 1);
assert.equal(rows.earlier, 1);
assert.deepEqual(rows.map((r) => r.kind), ["speaker", "text"]);
rows = rowsOf(items, { ...modes, tool: "hidden", result: "hidden" }, 0);
assert.ok(!rows.some((r) => r.kind === "tool" || r.kind === "result"));
assert.equal(nextMode("full"), "compact"); assert.equal(nextMode("hidden"), "full");
const q = { questions: [{ question: "Colour?", options: [] }, { question: "Size?", options: [] }] };
assert.equal(answerFor(q, { "Colour?": ["Green"] }), null);
assert.deepEqual(answerFor(q, { "Colour?": ["Green", "Blue"], "Size?": ["L"] }), { "Colour?": "Green, Blue", "Size?": "L" });
assert.deepEqual(pendingFor([{ session_id: "a" }, { session_id: "b" }], "b"), [{ session_id: "b" }]);
console.log("rows.js: all ok");
import { workLine } from "../mods/imperatorium-chat/rows.js";
assert.equal(workLine({ work: { context_tokens: 50014, cache_expires_at: 1000 + 125, shells: 2 }, usage: { cost_usd: 1.5 } }, 1000),
  "50k context · $1.50 · cache 3m · waiting on 2 shells");
assert.equal(workLine({ work: { cache_expires_at: 10 } }, 20), "cache cold");
assert.equal(workLine(null, 0), "");
console.log("workLine: ok");

// A tail keeps the whole conversation's keys, so open rows and drawn
// markdown stay put as the tail moves on.
{
  const whole = rowsOf(items, modes, 0);
  const tail = [{ speaker: "claude", blocks: [items[1].blocks[1]] }, items[2], items[3]];
  const part = rowsOf(tail, modes, 0, { start: 1, first_block: 1 });
  const keyOf = (rs, kind) => rs.find((r) => r.kind === kind).key;
  assert.equal(keyOf(part, "tool"), keyOf(whole, "tool"));
  assert.equal(keyOf(part, "result"), keyOf(whole, "result"));
  assert.equal(part.unloaded, true);
  assert.equal(whole.unloaded, false);
}
console.log("rows.js tail keys: ok");
