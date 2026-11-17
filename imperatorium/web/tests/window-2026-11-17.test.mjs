// The window changes of 2026-11-17 (user requests): your prompts highlighted
// (Settings > Highlighting, with a colour of their own), View > Group tool
// calls, and a sent prompt shown in the chat at once.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SCRIPT, CSS, ESC, between, evalJson, j, cat, find, assertIn, assertNotIn, assertLess } from "./load.mjs";

describe("PromptHighlight", () => {
  it("your turns get a band in the prompt colour, which both themes define", () => {
    assertIn("html.hl-prompts .turn.you-turn{background:color-mix(in srgb,var(--hl-prompt) 10%,transparent);", CSS);
    assertIn("box-shadow:inset 3px 0 var(--hl-prompt)", CSS);
    assert.equal((CSS.match(/--hl-prompt:#[0-9a-f]{6}/g) || []).length, 2);
  });

  it("a prompt just sent is drawn as one of your turns, so it is banded too", () => {
    assertIn('<div class="turn you-turn pend ${p.state}"', SCRIPT);
  });
});

describe("GroupToolCalls", () => {
  const body = () => {
    const a = find(SCRIPT, "function renderBlock(b)");
    return between(SCRIPT, "const attr =", "// -- end highlighting --") + "\n" +
      SCRIPT.slice(a, find(SCRIPT, "let logItems", a));
  };
  const TURN = { speaker: "claude", blocks: [
    { kind: "text", text: "on it" },
    { kind: "tool", name: "Bash", summary: "ls", id: "t1" },
    { kind: "result", text: "a", id: "t1" },
    { kind: "thinking", text: "hmm" },
    { kind: "tool", name: "Bash", summary: "pwd", id: "t2" },
    { kind: "result", text: "boom", is_error: true, id: "t2" },
    { kind: "tool", name: "Read", summary: "x.rs", id: "t3" },
    { kind: "result", text: "fn", id: "t3" },
    { kind: "text", text: "done" },
    { kind: "tool", name: "AskUserQuestion", summary: "q", id: "t4", questions: [] },
  ] };
  const run = (group, shape, opened, code) => evalJson(cat(
    "const renderText = t => '[' + t + ']';\n",
    "const teammateParts = () => null;\n",
    "const localStorage = {getItem: k => k === 'imperatorium.group' ? ", j(group ? "1" : "0"),
    " : k === 'imperatorium.shape' ? ", j(j(shape)), " : null};\n",
    "let html = '';\n",
    "const $ = () => ({classList: {toggle() {}}, set innerHTML(v) { html = v; }});\n",
    ESC, "\n", body(), "\n",
    code || cat("process.stdout.write(JSON.stringify(shapeBlocks(", j(TURN), ", 1, new Set(", j(opened || []), "))));")));

  it("off by default: no groups, and the View menu offers it unticked under the tool rows", () => {
    const got = run(false, {}, [], "drawShape(); process.stdout.write(JSON.stringify([shapeBlocks(" + j(TURN) + ", 1, new Set()), html]));");
    assertNotIn("tgroup", got[0]);
    assertIn("Group tool calls<input type=checkbox data-group>", got[1]);
    assertLess(got[1].indexOf('data-shape="result"'), got[1].indexOf("Group tool calls"));
    assertLess(got[1].indexOf("Group tool calls"), got[1].indexOf('data-shape="system"'));
  });

  it("on: each run of calls and results is one collapsed group", () => {
    const html = run(true, {});
    const groups = html.match(/<details class="tgroup"[^>]*>/g) || [];
    // Thinking between calls ends the run; the reply after them does too.
    assert.deepEqual(groups, ['<details class="tgroup" data-k="g1.1">', '<details class="tgroup" data-k="g1.4">']);
    assertIn('<span class="k">1 tool call</span><span class="ln">Bash</span></summary>', html);
    assertIn('<span class="k">2 tool calls</span><span class="ln">Bash, Read</span><span class="gf">1 failed</span>', html);
    assertLess(html.indexOf("[on it]"), html.indexOf("tgroup"));
    assertLess(html.lastIndexOf("</details>"), html.indexOf("[done]"));
  });

  it("a question card is never folded away", () => {
    const html = run(true, {});
    const after = html.slice(html.indexOf("[done]") + 6);
    assert.ok(after.length > 0, "the question card is drawn after the reply");
    assertNotIn("tgroup", after);
  });

  it("something hidden between calls does not end the run, and a group opened stays open", () => {
    const html = run(true, { thinking: "hidden" }, ["g1.1"]);
    const groups = html.match(/<details class="tgroup"[^>]*>/g) || [];
    assert.deepEqual(groups, ['<details class="tgroup" data-k="g1.1" open>']);
    assertIn('<span class="k">3 tool calls</span><span class="ln">Bash ×2, Read</span>', html);
  });

  it("inside a group each block keeps its own shape", () => {
    const html = run(true, { tool: "compact" });
    assertIn('<div class="gb"><details class="cmp" data-k="1.1">', html);
  });

  it("calls and results all hidden leave no group behind", () => {
    const html = run(true, { tool: "hidden", result: "hidden" });
    assertNotIn("tgroup", html);
  });

  it("opening a group is remembered and a search opens the one it is in", () => {
    assertIn('d.matches("details.cmp,details.tgroup")', SCRIPT);
    assertIn('details.cmp:not([open]),details.tgroup:not([open])', SCRIPT);
  });
});

describe("SentPromptShown", () => {
  const body = () => between(SCRIPT, "// -- a prompt just sent", "// -- following the end of the conversation");
  const run = (code) => evalJson(cat(
    ESC, "\n",
    "const SPEAKER = {you: 'you'};\n",
    "const renderText = t => '[' + t + ']';\n",
    "const mateTurn = it => !!it.mate;\n",
    "let selected = 's1', logFollow = false, draws = 0;\n",
    "let logItems = [{speaker: 'you', blocks: []}, {speaker: 'claude', blocks: []}];\n",
    "const drawLog = () => { draws++; };\n",
    body(), "\n", code));

  it("shows at once as sending, then as sent and waiting for Claude", () => {
    const got = run(cat(
      "const p = addPending('s1', 'fix it');",
      "const a = pendingHtml(p);",
      "settleSend('s1', p, {ok: true}, 'sent');",
      "process.stdout.write(JSON.stringify([a, pendingHtml(p), draws, logFollow, p.base]));"));
    assertIn('<div class="turn you-turn pend sending" data-pend="1"><div class="who you">you<span class="pnote">sending…</span></div>[fix it]</div>', got[0]);
    assertIn('<span class="pnote">sent - waiting for Claude to respond</span>', got[1]);
    assertIn('data-unpend="1"', got[1]);
    assert.equal(got[2], 2);
    assert.equal(got[3], true);
    assert.equal(got[4], 1);
  });

  it("goes once the conversation has one more turn of yours", () => {
    const got = run(cat(
      "const p = addPending('s1', 'one'); settleSend('s1', p, {ok: true}, 'sent');",
      "const q = addPending('s1', 'two'); settleSend('s1', q, {ok: true}, 'sent');",
      "const you = {speaker: 'you', blocks: []}, mate = {speaker: 'you', mate: true, blocks: []};",
      "const list = pendingSent.s1;",
      "process.stdout.write(JSON.stringify([",
      " settlePending(list, logItems).length,",
      " settlePending(list, logItems.concat([mate])).length,",
      " settlePending(list, logItems.concat([you])).map(p => p.text),",
      " settlePending(list, logItems.concat([you, you])).length]));"));
    assert.deepEqual(got, [2, 2, ["two"], 0]);
  });

  it("one not sent says why and stays until dismissed, and no longer counts", () => {
    const got = run(cat(
      "const p = addPending('s1', 'one'); const q = addPending('s1', 'two');",
      "settleSend('s1', p, {ok: false, error: 'no such session'}, 'sent');",
      "settleSend('s1', q, {ok: true}, 'sent');",
      "const you = {speaker: 'you', blocks: []};",
      "const left = settlePending(pendingSent.s1, logItems.concat([you]));",
      "const h = pendingHtml(p);",
      "dropPending(p.id);",
      "process.stdout.write(JSON.stringify([q.base, left.map(x => x.text), h, pendingSent.s1.length]));"));
    assert.equal(got[0], 1);
    assert.deepEqual(got[1], ["one"]);
    assertIn('pend failed', got[2]);
    assertIn("not sent - no such session", got[2]);
    assert.equal(got[3], 1);
  });

  it("Send and Send As Typed show it; a background send, which starts another session, does not", () => {
    assertIn('const p = mode === "background" ? null : addPending(key(s), text);', SCRIPT);
    assertIn("const p = addPending(key(s), text);\n  let r;\n  try { r = await post(\"/api/typed\"", SCRIPT);
    assertIn("pendingSent[selected] = settlePending(pendingSent[selected] || [], logItems);", SCRIPT);
  });
});
