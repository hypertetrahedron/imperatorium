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
    "const teammateParts = t => t.startsWith('<teammate') ? [t] : null;\n",
    "const text = t => ({kind: 'text', text: t});\n",
    "let selected = 's1', logFollow = false, draws = 0;\n",
    "let logItems = [{speaker: 'you', blocks: [text('hello')]}, {speaker: 'claude', blocks: []}];\n",
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
      "const you = {speaker: 'you', blocks: [text('x')]}, mate = {speaker: 'you', blocks: [text('<teammate>hi')]};",
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
      "const you = {speaker: 'you', blocks: [text('x')]};",
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

  it("prompts that come back joined in one turn each count, so the last one's note goes too", () => {
    // 2026-11-17: two sends, each stopped with Esc before any reply, then a
    // third: the conversation had the first two as one turn of yours.
    const got = run(cat(
      "['one', 'two', 'three'].forEach(t => { const p = addPending('s1', t); settleSend('s1', p, {ok: true}, 'sent'); });",
      "const joined = {speaker: 'you', blocks: [text('one'), text('two')]};",
      "const third = {speaker: 'you', blocks: [text('three')]};",
      "const marker = {speaker: 'you', blocks: [text('[Request interrupted by user]')]};",
      "const task = {speaker: 'you', note: 'task', blocks: [text('a task finished')]};",
      "const list = pendingSent.s1;",
      "process.stdout.write(JSON.stringify([",
      " settlePending(list, logItems.concat([joined])).map(p => p.text),",
      " settlePending(list, logItems.concat([joined, {speaker: 'claude', blocks: []}, third])).length,",
      " settlePending(list, logItems.concat([marker, task])).length]));"));
    assert.deepEqual(got, [["three"], 0, 3]);
  });

  it("Send and Send As Typed show it; a background send, which starts another session, does not", () => {
    assertIn('const p = mode === "background" ? null : addPending(key(s), text);', SCRIPT);
    assertIn("const p = addPending(key(s), text);\n  let r;\n  try { r = await post(\"/api/typed\"", SCRIPT);
    assertIn("pendingSent[selected] = settlePending(pendingSent[selected] || [], logItems);", SCRIPT);
  });
});

describe("FindWindow", () => {
  it("the header row has one Find button; the box, the filter and the arrows are in its window", async () => {
    const { renderShell } = await import("./load.mjs");
    const html = renderShell();
    const bar = html.slice(html.indexOf("<div id=findbar>"), html.indexOf("<div id=acts>"));
    const btn = bar.slice(0, bar.indexOf("<div id=findpop"));
    const pop = bar.slice(bar.indexOf("<div id=findpop"));
    assertIn("<button id=findbtn", btn);
    assertIn("Find</button>", btn);
    assertIn("hidden>", pop.slice(0, pop.indexOf(">") + 1));
    for (const id of ["id=cfind", "id=cfilter", "id=cfcount", "id=cfprev", "id=cfnext", "id=cfclose"]) {
      assertNotIn(id, btn);
      assertIn(id, pop);
    }
  });

  it("the button and ctrl+F open it; Esc, its close or the button again close it and clear the find", () => {
    assertIn('$("#findbtn").onclick = () => openFind($("#findpop").hidden);', SCRIPT);
    assertIn('e.preventDefault(); openFind(true); return; }', SCRIPT);
    const fn = between(SCRIPT, "function openFind(on){", "\n}");
    assertIn('if (on){ $("#cfind").focus(); $("#cfind").select(); return; }', fn);
    assertIn("resetFind(); drawLog();", fn);
    assertIn('if (t !== "log" && !$("#findpop").hidden) openFind(false);', SCRIPT);
  });

  it("the window floats under the button and fits a phone", () => {
    assertIn("#findpop{position:absolute;right:0;top:calc(100% + 4px);", CSS);
    assertIn("width:min(380px,calc(100vw - 32px))", CSS);
  });
});

describe("FollowClear", () => {
  // Peer request 2026-11-17: a session cleared with /clear opens its new
  // conversation, with the prompt box ready for the first prompt.
  const cleared = (before, list, selected) => evalJson(cat(
    "const key = s => s.sessionId || s.id;\n",
    between(SCRIPT, "function clearedInto(", "// Pure: the title counts"),
    "\nprocess.stdout.write(JSON.stringify(clearedInto(",
    before ? "new Map(" + j(before) + ")" : "null", ", ", j(list), ", ", j(selected), ")));"));

  it("a new id in a process an old one had is that one, cleared", () => {
    assert.deepEqual(cleared([["a", 7], ["b", 9]], [{ sessionId: "b", pid: 9 }, { sessionId: "c", pid: 7 }], "a"),
      { from: "a", to: "c" });
    // the old one left resumable, without its process, counts too
    assert.deepEqual(cleared([["a", 7]], [{ sessionId: "a" }, { sessionId: "c", pid: 7 }], "x"),
      { from: "a", to: "c" });
  });

  it("nothing on the first list, for a new process, a remote row, or an old one still running", () => {
    assert.equal(cleared(null, [{ sessionId: "c", pid: 7 }], "a"), null);
    assert.equal(cleared([["a", 7]], [{ sessionId: "a", pid: 7 }, { sessionId: "c", pid: 8 }], "a"), null);
    assert.equal(cleared([["a", 7]], [{ sessionId: "c", pid: 7, remote: true }], "a"), null);
    assert.equal(cleared([["a", 7]], [{ sessionId: "a", pid: 7 }, { sessionId: "c", pid: 7 }], "a"), null);
  });

  it("the selected one is followed first", () => {
    assert.deepEqual(cleared([["a", 7], ["b", 9]], [{ sessionId: "d", pid: 7 }, { sessionId: "c", pid: 9 }], "b"),
      { from: "b", to: "c" });
  });

  const follow = (selected, boxText, focused) => evalJson(cat(
    "let selected = ", j(selected), ", view = 'session'; const sessions = [{sessionId: 'c'}];\n",
    "const key = s => s.sessionId; const nameOf = s => s.sessionId; const calls = [];\n",
    "const box = {value: ", j(boxText), ", disabled: false, focus(){ calls.push('focus'); }};\n",
    "const say = {textContent: ''}; const $ = q => q === '#prompt' ? box : say;\n",
    "const document = {activeElement: ", focused ? "box" : "null", "};\n",
    "const pick = k => { selected = k; calls.push('pick ' + k); };\n",
    between(SCRIPT, "function catchCleared(", "function select(id)"),
    "\ncatchCleared({from: 'a', to: 'c'});",
    "process.stdout.write(JSON.stringify([selected, calls, say.textContent]));"));

  it("opens the new one and puts the keys in the prompt box", () => {
    assert.deepEqual(follow("a", "", false), ["c", ["pick c", "focus"], ""]);
    // the one you were on follows even with a draft in the box
    assert.deepEqual(follow("a", "half a thought", true), ["c", ["pick c", "focus"], ""]);
  });

  it("another session's clear does not take you from a sentence", () => {
    const got = follow("b", "half a thought", true);
    assert.equal(got[0], "b");
    assertIn("was cleared - not switched while you type", got[2]);
  });

  it("a clear goes before the start-time check, which misses it", () => {
    assertIn("if (cleared) catchCleared(cleared);\n  else catchStarted(", SCRIPT);
  });
});

describe("ShowWindowInView", () => {
  it("Show Window is in the View menu, not a header button", async () => {
    const { renderShell } = await import("./load.mjs");
    const html = renderShell();
    const view = html.slice(html.indexOf("id=viewmenu"), html.indexOf("id=findbar"));
    assertIn("<button id=showbtn data-act=show", view);
    assertIn(">Show Window</button>", view);
    assertNotIn('data-act="show"', SCRIPT);
    assertIn('$("#showbtn").hidden = !(local && s.pid);', SCRIPT);
  });
});

describe("ProjectChanges", () => {
  // Peer request 2026-11-17: every branch and worktree of a project in one
  // pop-up, with the files more than one of them changes first.
  const pure = (code) => evalJson(cat(
    "const key = s => s.sessionId;\n",
    between(SCRIPT, "const normDir =", "function openProject("),
    "\nprocess.stdout.write(JSON.stringify(", code, "));"));

  const LINES = [
    { kind: "main", path: "C:/w/proj", branch: "main" },
    { kind: "worktree", path: "C:/w/proj/.claude/worktrees/x", branch: "wt-x" },
    { kind: "branch", path: null, branch: "side" },
  ];

  it("each session is in the deepest line whose folder holds its own", () => {
    const list = [
      { sessionId: "a", cwd: "C:\\w\\proj" },
      { sessionId: "b", cwd: "C:\\W\\proj\\.claude\\worktrees\\x\\" },
      { sessionId: "c", cwd: "C:\\w\\proj\\sub" },
      { sessionId: "d", cwd: "C:\\w\\other" },
      { sessionId: "e", cwd: "C:\\w\\proj", remote: true },
      { sessionId: "f", cwd: "C:\\w\\project2" },
    ];
    const got = pure("lineSessions(" + j(LINES) + ", " + j(list) + ").map(l => l.map(key))");
    assert.deepEqual(got, [["a", "c"], ["b"], []]);
  });

  it("the projects on offer are this machine's, by name, from the main checkout when a session is there", () => {
    const got = pure("projectChoices(" + j([
      { project: "proj", worktree: "x", cwd: "C:\\w\\proj\\.claude\\worktrees\\x" },
      { project: "proj", cwd: "C:\\w\\proj" },
      { project: "alpha", worktree: "y", cwd: "C:\\w\\alpha\\.claude\\worktrees\\y" },
      { project: "far", cwd: "/srv/far", remote: true },
    ]) + ")");
    assert.deepEqual(got, [
      { name: "alpha", cwd: "C:\\w\\alpha\\.claude\\worktrees\\y" },
      { name: "proj", cwd: "C:\\w\\proj" },
    ]);
  });

  it("each line says what it is", () => {
    assert.deepEqual(pure(j(LINES) + ".map(lineName)"),
      ["main checkout · main", "wt-x · worktree x", "side · branch, no worktree"]);
  });

  it("a pop-up opened from the ☰ menu and the View menu, read when opened and on Refresh", async () => {
    const { renderShell } = await import("./load.mjs");
    const html = renderShell();
    const side = html.slice(html.indexOf("id=sidemenu"), html.indexOf("</details>", html.indexOf("id=sidemenu")));
    assertIn("data-view=project", side);
    const view = html.slice(html.indexOf("id=viewmenu"), html.indexOf("id=findbar"));
    assertIn("data-view=project", view);
    for (const id of ["<div id=projbox class=popup hidden>", "id=pjpick", "id=pjrefresh", "id=projpane", "data-popclose"]) {
      assertIn(id, html.slice(html.indexOf("<div id=projbox")));
    }
    assertIn('project: "#projbox"', SCRIPT);
    assertIn('else if (which === "project"){ $("#pjrefresh").focus(); loadProject(); }', SCRIPT);
    assertIn('get("/api/project_changes?project="', SCRIPT);
    assertIn('get("/api/project_file_diff?project="', SCRIPT);
  });
});
