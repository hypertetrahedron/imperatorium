// The chat matrix page (matrix.js, matrix.css) and the window's side of it.
// Ported from test_ui.py: ChatMatrix.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  SCRIPT, CSS, MATRIX_SCRIPT, MATRIX_CSS, between, matrixJs, evalJson, cat, renderShell, renderMatrix,
  assertIn, assertNotIn, assertLess, assertLessEqual, assertGreaterEqual, assertAlmostEqual,
} from "./load.mjs";

describe("ChatMatrix", () => {
  const layouts = (cases) =>
    matrixJs(`process.stdout.write(JSON.stringify([${cases.map((c) => `layout(${c.join(",")})`).join(",")}]));`);

  it("the matrix script parses", () => {
    new Function(MATRIX_SCRIPT);
  });

  it("tiles keep the ratio and fit", () => {
    const cases = [[1, 1600, 900], [4, 1600, 900], [7, 1600, 900], [12, 1000, 1400], [5, 400, 1600]];
    const got = layouts(cases);
    cases.forEach(([n, W, H], k) => {
      const L = got[k];
      assertAlmostEqual(L.h / L.w, 1.5);
      assertLessEqual(L.cols * L.w + 8 * (L.cols - 1), W + 0.01);
      assertLessEqual(L.rows * L.h + 8 * (L.rows - 1), H + 0.01);
      assertGreaterEqual(L.cols * L.rows, n);
      // Spare cells all fit in the last row.
      assertLess(L.spare, L.cols);
    });
  });

  it("the layout is the biggest possible", () => {
    // Brute force: no other column count gives a bigger tile.
    for (let n = 1; n < 17; n++) {
      for (const [W, H] of [[1600, 900], [900, 1600], [1200, 1200]]) {
        const L = layouts([[n, W, H]])[0];
        for (let cols = 1; cols <= n; cols++) {
          const rows = Math.ceil(n / cols);
          const w = Math.min((W - 8 * (cols - 1)) / cols, (H - 8 * (rows - 1)) / rows / 1.5);
          assertLessEqual(w, L.w + 0.5, JSON.stringify([n, W, H, cols]));
        }
      }
    }
  });

  it("a wide window puts four in a row", () => {
    let L = layouts([[4, 1600, 900]])[0];
    assert.deepEqual([L.cols, L.rows, L.spare], [4, 1, 0]);
    L = layouts([[5, 1600, 900]])[0];
    assert.deepEqual([L.cols, L.rows, L.spare], [5, 1, 0]);
    L = layouts([[7, 1600, 900]])[0];
    assert.equal(L.cols * L.rows - 7, L.spare);
  });

  it("a tile shows replies then the step in flight", () => {
    const html = matrixJs(cat(
      "process.stdout.write(JSON.stringify(tileBody({replies:[{text:'a',turn:1},",
      "{text:'b',turn:2}],current:{kind:'tool',name:'Bash',summary:'<x>',more:2}})));"));
    assertIn('<div class="rp"><div class="txt">a</div></div>', html);
    assertIn('<div class="rp newturn">', html);
    assertIn("<b>Bash</b>", html);
    assertIn("&lt;x&gt;", html);
    assertIn("+2", html);
    assertLess(html.indexOf(">b<"), html.indexOf("Bash"));
  });

  it("a working session says what it is waiting on", () => {
    const got = matrixJs(cat(
      "const w = {agents: 1, shells: 2, monitors: 0, processes: 1};",
      "process.stdout.write(JSON.stringify([",
      " tileBody({status: 'busy', replies: [], work: w}),",
      " tileBody({status: 'busy', replies: [], work: {shells: 0, processes: 0}}),",
      " tileBody({status: 'idle', replies: [], work: w}),",
      " tileBody({status: 'idle', background_tasks: 1, replies: [], work: {shells: 1}}),",
      " tileBody({status: 'busy', replies: [], work: w, current: {kind: 'tool', name: 'Bash'}}),",
      "]));"));
    const [busy, nothing, idle, background, tool] = got;
    assertIn('<div class="cur wait"', busy);
    assertIn('<span class="spin"></span>', busy);
    assertIn("waiting on 1 subagent · 2 background shells · 1 process", busy);
    assertNotIn("waiting on", nothing);
    assertNotIn("waiting on", idle);
    assertIn("waiting on 1 background shell", background);
    assertNotIn("waiting on", tool);
  });

  it("a session in a turn with nothing in flight says it is working", () => {
    // Peer request 2026-10-18: a busy tile between steps showed nothing
    // under its replies, so only the header's dot said it was working.
    const got = matrixJs(cat(
      "process.stdout.write(JSON.stringify([",
      " tileBody({status: 'busy', replies: [], work: {}}),",
      " tileBody({status: 'idle', state: 'working', replies: [], work: {}}),",
      " tileBody({status: 'busy', replies: [], work: {shells: 1}}),",
      " tileBody({status: 'busy', replies: [], work: {}, current: {kind: 'thinking', text: 'x'}}),",
      " tileBody({status: 'idle', background_tasks: 1, replies: [], work: {}}),",
      " tileBody({status: 'idle', replies: [], work: {}}),",
      " tileBody({status: 'busy', attention: true, replies: [], work: {}}),",
      "]));"));
    const [busy, working, shell, thinking, background, idle, attn] = got;
    assertIn('<span class="spin"></span><span class="what">Agent working</span>', busy);
    assertIn("Agent working", working);
    for (const other of [shell, thinking, background, idle, attn]) assertNotIn("Agent working", other);
    assertIn("waiting on 1 background shell", shell);
  });

  it("a working tiles dot pulses", () => {
    // The shared rule, which the matrix page carries (CSS + MATRIX_CSS).
    assertIn(".dot.d-busy,.dot.d-idle,", CSS);
    assertIn(CSS, renderMatrix());
  });

  it("the status tile shows spend", () => {
    const html = matrixJs(cat(
      "process.stdout.write(JSON.stringify(statusTile({cost_usd:12.345,costed:2,sessions:3,",
      "total_tokens:2500000,tokens:{input:1000,output:2000,cache_read:2400000,cache_write:97000},",
      "each:[{name:'a',cost_usd:12.345,tokens:2500000}],limits:{five_hour:{used:42}}})));"));
    for (const needle of ["$12.35", "cost known for 2", "2.5M tokens", "5-hour", "42%", "By session"]) {
      assertIn(needle, html);
    }
  });

  it("the button leads the sidebar header", () => {
    // Peer request 2026-10-18: the SESSIONS label went; the list speaks
    // for itself, and the header starts with its buttons: Settings, then
    // the Matrix Button (peer request 2026-10-18).
    const shell = renderShell();
    assertIn("<h1><span class=ttl><button id=setbtn", shell);
    assertNotIn("sessions <", shell.slice(shell.indexOf("<h1>"), shell.indexOf("id=matrixbtn")));
    const button = shell.indexOf("id=matrixbtn");
    assertLess(shell.indexOf("<h1>"), button);
    assertLess(button, shell.indexOf("id=new"));
    assertIn('aria-label="Chat Matrix"', shell);
    assertIn("<svg", shell.slice(button, shell.indexOf("id=new")));
    for (const needle of ['window.open("/matrix", MATRIX', 'matrixChannel.postMessage("close")',
                          '$("#matrixbtn").onclick = toggleMatrix']) {
      assertIn(needle, SCRIPT);
    }
  });

  it("the page answers close and says it is up", () => {
    for (const needle of ['"imperatorium-matrix"', 'e.data === "close") window.close()',
                          'postMessage("open")', 'postMessage("closed")']) {
      assertIn(needle, MATRIX_SCRIPT);
    }
    assertIn("<title>Chat Matrix</title>", renderMatrix());
  });

  it("tiles stretch to fill once the grid is chosen", () => {
    const script = MATRIX_SCRIPT;
    assertIn("const cw = Math.max(0, (W - GAP * (L.cols - 1)) / L.cols);", script);
    assertIn("const rh = Math.max(0, (H - GAP * (L.rows - 1)) / L.rows);", script);
    assertIn("gridTemplateColumns: `repeat(${L.cols}, ${cw}px)`, gridAutoRows: `${rh}px`", script);
  });

  it("nothing in the matrix scrolls", () => {
    assert.deepEqual(MATRIX_CSS.match(/overflow(?:-[xy])?:\s*(?:auto|scroll)/g) || [], []);
    assertNotIn("scrollTop", MATRIX_SCRIPT);
  });

  it("the attention ring is outside the tile", () => {
    // Inset, the header and the tile's contents painted over parts of it.
    assertNotIn("inset 0 0 0", MATRIX_CSS);
    assertIn(".tile.attn{border-color:var(--warn);box-shadow:0 0 0 1px var(--warn)}", MATRIX_CSS);
  });

  it("a double click shows the session in the main window", () => {
    for (const needle of ['addEventListener("dblclick"', "channel.postMessage({select: sid})",
                          "window.opener.focus()", 'window.open("/?session=" + encodeURIComponent(sid)',
                          "el.dataset.sid = t.sessionId"]) {
      assertIn(needle, MATRIX_SCRIPT);
    }
    for (const needle of ["matrixChannel.postMessage({selected: m.select})",
                          'new URLSearchParams(location.search).get("session")',
                          'window.name = "imperatorium-main"']) {
      assertIn(needle, SCRIPT);
    }
  });

  it("the main window selects what the matrix asks for", () => {
    const body = SCRIPT.slice(SCRIPT.indexOf("if (matrixChannel) matrixChannel.onmessage"),
                              SCRIPT.indexOf("function toggleMatrix"));
    assert.ok(body.length > 0);
    const got = evalJson(cat(
      "let sent = [], picked = [], followed = [], sessions = [{sessionId: 'a'}], matrixSeen = 0;",
      "const key = s => s.sessionId, select = id => picked.push(id),",
      " follow = id => followed.push(id), showMatrixButton = () => {};",
      "const window = {focus(){}}, Date = {now: () => 5};",
      "const matrixChannel = {postMessage: m => sent.push(m)};",
      body,
      "matrixChannel.onmessage({data: {select: 'a'}});",
      "matrixChannel.onmessage({data: {select: 'later'}});",
      "matrixChannel.onmessage({data: 'open'});",
      "process.stdout.write(JSON.stringify([sent, picked, followed, matrixSeen]));"));
    const [sent, picked, followed, seen] = got;
    assert.deepEqual(sent, [{ selected: "a" }, { selected: "later" }]);
    assert.deepEqual([picked, followed, seen], [["a"], ["later"], 5]);
  });

  it("no custom property refers to itself", () => {
    // `--mono:...,var(--mono)` is a cycle; the browser makes the property
    // invalid and every var(--mono) with it.
    for (const [, name, value] of (CSS + MATRIX_CSS).matchAll(/(--[\w-]+)\s*:([^;}]*)/g)) {
      assertNotIn(`var(${name})`, value, name);
    }
  });
});
