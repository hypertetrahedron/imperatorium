// The window's press-hold redraw guards, task rows, awaited panel, reviewed
// logic, log diff and teammate messages (app.js).
// Ported from test_ui.py: SendLine, HeldRedraws, RedrawGuards, TileTasks,
// AwaitedPanel, ReviewedTests, PermissionCardPlacementTests, WorkPulseTests,
// LogDiff, TeammateMessages.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  SCRIPT, ESC, between, tail, find, evalJs, evalJson, j, cat, renderShell, renderMatrix,
  runRenderer as run, assertIn, assertNotIn, assertLess,
} from "./load.mjs";

/** The press-hold helpers, say/showSaid, with a stub DOM; `expr` after them. */
const pressesJs = (expr) => {
  const body = between(SCRIPT, "const pressedIn", "\nfunction drawSessions");
  return evalJson(cat(
    "const line = {textContent: ''}; const $ = () => line;",
    " let selected = 'a'; const timers = []; const setTimeout = f => timers.push(f);\n", body,
    "\nprocess.stdout.write(JSON.stringify(", expr, "));"));
};

describe("SendLine", () => {
  /** The line under the prompt box belongs to the session it is about. */
  it("switching hides it and switching back shows it", () => {
    const got = pressesJs(cat(
      "(() => { say('refused: no inbox'); const a = line.textContent;",
      " selected = 'b'; showSaid(); const b = line.textContent;",
      " selected = 'a'; showSaid(); return [a, b, line.textContent]; })()"));
    assert.deepEqual(got, ["refused: no inbox", "", "refused: no inbox"]);
  });

  it("a reply after a switch is kept for its session", () => {
    const got = pressesJs(cat(
      "(() => { say('sending…', 'a'); selected = 'b'; showSaid();",
      " say('refused', 'a'); const onB = line.textContent;",
      " selected = 'a'; showSaid(); return [onB, line.textContent]; })()"));
    assert.deepEqual(got, ["", "refused"]);
  });
});

describe("HeldRedraws", () => {
  /** A redraw between press and release would eat the click. */
  it("a redraw waits for the release and then runs once", () => {
    const got = pressesJs(cat(
      "(() => { let n = 0; const draw = () => n++;",
      " pressedIn.add('#sessions');",
      " const a = held('#sessions', draw), b = held('#sessions', draw), other = held('#status', draw);",
      " releasePresses(); const queued = timers.length; timers.forEach(f => f());",
      " return [a, b, other, queued, n, held('#sessions', draw)]; })()"));
    // held twice, once queued; another panel is not held; after the
    // release the draw runs once, and nothing is held any more.
    assert.deepEqual(got, [true, true, false, 1, 1, false]);
  });

  it("set text leaves equal text alone", () => {
    const got = pressesJs(cat(
      "(() => { let writes = 0; const el = {_t: 'x', get textContent(){ return this._t; },",
      " set textContent(v){ writes++; this._t = v; }};",
      " setText(el, 'x'); setText(el, 'y'); return [writes, el.textContent]; })()"));
    assert.deepEqual(got, [1, "y"]);
  });
});

describe("RedrawGuards", () => {
  /** Panels redrawn on a timer replace their content only when it changed. */
  it("timer panels are held while pressed", () => {
    const js = SCRIPT;
    for (const [fn, sel] of [["drawSessions", "#sessions"], ["drawStatus", "#status"], ["drawProcs", "#procs"]]) {
      const start = find(js, `function ${fn}(){`);
      assertIn(`held("${sel}", ${fn})`, js.slice(start, start + 200), fn);
    }
    const start = find(js, "function drawLog(force){");
    assertIn('held("#log", () => drawLog(force))', js.slice(start, start + 200));
    assertIn('["#sessions", "#status", "#procs", "#log"].forEach(holdPresses)', js);
    assertIn('window.addEventListener("pointerup", releasePresses, true)', js);
  });

  it("timer panels compare against what they drew", () => {
    const js = SCRIPT;
    for (const el of ["#sessions", "#proctabs", "#meter", "#activity", "#curbody"]) {
      assertIn(`if ($("${el}").dataset.sig !== `, js, el);
    }
    assertIn("if (box.dataset.sig !== html){ box.innerHTML = html; box.dataset.sig = html; }", js);
    // innerHTML comes back in the browser's own quoting, so it is never
    // what was drawn: comparing against it redrew every time.
    assertNotIn(".innerHTML !== ", js);
  });
});

describe("TileTasks", () => {
  /** The folded list of what a session has going, at the foot of its tile. */
  const runJs = (code) =>
    evalJson(cat(
      "const RUNNING = {busy: 1, shell: 1};\n",
      "function age(t){ return t ? Math.round(Date.now()/1000 - t) + 's' : ''; }\n",
      between(SCRIPT, "const TASK_MARK", "function foldTasks("), "\n", code));

  it("the list and its summary", () => {
    const got = runJs(cat(
      "const s = {status: 'busy', doing: 'Bash pytest', doing_since: 1, work: {processes: 2}};",
      "const l = taskList(s, [{id: 'b1', kind: 'shell', label: 'npm run dev'},",
      "                      {id: 'a1', kind: 'agent', label: 'explore'},",
      "                      {id: 'b0', kind: 'shell', label: 'old', done: true, ended: 1},",
      "                      {id: 'mcp', kind: 'mcp', label: 'chrome'}]);",
      "process.stdout.write(JSON.stringify([l.running.map(t => [t.id, t.kind]),",
      " l.done.map(t => t.id), l.extra.map(t => t.id),",
      " taskSummary(l.running), taskSummary([])]));"));
    const [running, done, extra, summary, empty] = got;
    assert.deepEqual(running, [["tool", "tool"], ["b1", "shell"], ["a1", "agent"], ["processes", "process"]]);
    // What ended stays out of the running list, under its own row.
    assert.deepEqual(done, ["b0"]);
    assert.deepEqual(extra, ["mcp"]);
    // Every running row is counted, the processes row by how many it is.
    assert.equal(summary, "1 tool call · 1 shell · 1 subagent · 2 processes");
    assert.equal(empty, "none running");
  });

  it("only processes is not none running", () => {
    const got = runJs(cat(
      "const l = taskList({status: 'idle', work: {processes: 1}}, []);",
      "process.stdout.write(JSON.stringify(taskSummary(l.running, l.idle)));"));
    assert.equal(got, "1 process");
  });

  it("idle teammates are listed not counted as running", () => {
    const got = runJs(cat(
      "const l = taskList({status: 'idle'}, [{id: 'w1', kind: 'agent', label: 'wait', idle: true},",
      "                                    {id: 'w2', kind: 'agent', label: 'wait'}]);",
      "process.stdout.write(JSON.stringify([l.running.map(t => t.id), l.idle.map(t => t.id),",
      " taskSummary(l.running, l.idle)]));"));
    assert.deepEqual(got, [["w2"], ["w1"], "1 subagent · 1 idle subagent"]);
  });

  it("an idle session has no tool call and times read right", () => {
    const got = runJs(cat(
      "const now = Date.now()/1000;",
      "const l = taskList({status: 'idle', doing: 'Bash ls'}, []);",
      "process.stdout.write(JSON.stringify([l.running.length,",
      " taskWhen({kind: 'wakeup', fires_at: now + 600}, now),",
      " taskWhen({kind: 'wakeup', fires_at: now - 5}, now),",
      " taskWhen({kind: 'shell', done: true, ended: now - 30}, now)]));"));
    assert.deepEqual(got, [0, "in 600s", "due", "30s ago"]);
  });
});

describe("AwaitedPanel", () => {
  /** The tabs under the chat for what the session is waiting on. */
  const view = (tasks, pick = "auto") =>
    evalJson(between(SCRIPT, "function procView(", "function procWanted(") +
             `\nprocess.stdout.write(JSON.stringify(procView(${j(tasks)}, ${j(pick)})));`);
  const A = { id: "ba", kind: "shell", label: "npm test", started: 1, mtime: 50, text: "a" };
  const B = { id: "bb", kind: "monitor", label: "watch log", started: 2, mtime: 40, text: "b" };

  it("nothing awaited shows nothing", () => {
    assert.equal(view([]).show, null);
  });

  it("one task has no auto tab", () => {
    const got = view([A]);
    assert.deepEqual(got.tabs.map((t) => t.id), ["ba"]);
    assert.equal(got.show.id, "ba");
    assert.equal(got.tabs[0].on, true);
  });

  it("several put auto first and follow the latest output", () => {
    const got = view([A, B]);
    assert.deepEqual(got.tabs.map((t) => t.id), ["auto", "ba", "bb"]);
    assert.equal(got.show.id, "ba");
    assert.equal(got.tabs[1].live, true);
    const later = { ...B, mtime: 60 };
    assert.equal(view([A, later]).show.id, "bb");
  });

  it("a chosen tab sticks until its task ends", () => {
    assert.equal(view([A, B], "bb").show.id, "bb");
    const got = view([A, B], "gone");
    assert.deepEqual([got.pick, got.show.id], ["auto", "ba"]);
  });

  it("panel sits directly under the chat", () => {
    const page = renderShell();
    assertIn(cat("<div id=log></div><div id=taskview hidden><div id=tvhead></div><pre id=tvtext></pre></div>",
                 "<div id=procs hidden>"), page);
  });
});

/** The window's reviewed logic, with the page around it stubbed: `steps` is
 * run against it; the session ids posted to /api/reviewed. */
const reviewedRun = (steps) => {
  const sec = between(SCRIPT, "// -- reviewed --", "function showNewest");
  return evalJson(cat(`
let posted = [], now = 1e6;
Date.now = () => now;
globalThis.setTimeout = f => f();
const document = {hidden: false, hasFocus: () => false};
let view = "session", tab = "log", atEnd = true, pendingNow = [];
let row = {sessionId: "a", attention: true, attention_since: 1, status: "idle"};
const sessions = [row];
const current = () => row, key = s => s.sessionId, $ = () => ({});
const logAtEnd = () => atEnd, drawSessions = () => {};
const post = (u, b) => { posted.push(b.session_id); return Promise.resolve({ok: false}); };
`, sec, "\nlogFresh = 'a';\n", steps, "\nprocess.stdout.write(JSON.stringify(posted));"));
};

describe("ReviewedTests", () => {
  /** Reviewed takes something you did at the end of a finished session. */
  it("following the end alone is not reviewing", () => {
    // The log scrolled itself to the end, the window came back: no input.
    assert.deepEqual(reviewedRun("maybeReviewed(); maybeReviewed();"), []);
  });

  it("input at the end reviews", () => {
    assert.deepEqual(reviewedRun("touched();"), ["a"]);
  });

  it("input away from the end does not", () => {
    assert.deepEqual(reviewedRun("atEnd = false; touched();"), []);
  });

  it("input long ago does not count", () => {
    assert.deepEqual(reviewedRun("atEnd = false; touched(); now += 5000; atEnd = true; maybeReviewed();"), []);
  });

  it("scrolling to the end with your wheel reviews", () => {
    assert.deepEqual(reviewedRun("atEnd = false; touched(); now += 300; atEnd = true; maybeReviewed();"), ["a"]);
  });

  it("work in progress is not reviewed", () => {
    for (const busy of ['row.status = "busy"', 'row.status = "shell"', 'row.status = "waiting"',
                        "row.waiting_tasks = 2", 'row.state = "blocked"',
                        'pendingNow = [{session_id: "a"}]']) {
      assert.deepEqual(reviewedRun(busy + "; touched();"), [], busy);
    }
  });
});

describe("PermissionCardPlacementTests", () => {
  it("parked requests sit at the foot of the chat", () => {
    const page = renderShell();
    assertLess(page.indexOf("id=log"), page.indexOf("id=decide"));
    assertLess(page.indexOf("id=decide"), page.indexOf("id=ask"));
  });
});

describe("WorkPulseTests", () => {
  /** Everything that stands for work in progress pulses, in both windows. */
  const RULE = ".dot.d-busy,.dot.d-idle,.s .tki.run .k,.s .tki .live{animation:pulse";

  it("both pages pulse work in progress", () => {
    for (const page of [renderShell(), renderMatrix()]) {
      assertIn(RULE, page);
      assertIn("@keyframes pulse", page);
      assertIn("prefers-reduced-motion", page);
    }
  });

  it("running task rows are marked but not wakeups or finished ones", () => {
    const start = find(SCRIPT, "function taskRow(");
    const fn = SCRIPT.slice(start, find(SCRIPT, "function drawTileTasks", start));
    const got = evalJson(cat(ESC, `
let taskView = null;
const TASK_MARK = {}, STATUS_WORD = {}, taskWhen = () => "";
`, fn, `
const row = (t, running) => /class="tki([^"]*)"/.exec(taskRow(t, 0, false, running))[1];
process.stdout.write(JSON.stringify([
  row({id: "a", kind: "shell"}, true), row({id: "b", kind: "wakeup"}, true),
  row({id: "c", kind: "shell", done: true}), row({id: "d", kind: "mcp"})]));`));
    assert.deepEqual(got, [" run", "", " done", ""]);
  });
});

describe("LogDiff", () => {
  /** The log keeps the elements of turns that did not change (logDiff says
   * which did); patchLog's DOM work is exercised in the browser. */
  const diff = (have, want) =>
    evalJson(between(SCRIPT, "function logDiff", "// What #log holds") +
             `\nprocess.stdout.write(JSON.stringify(logDiff(${j(have)}, ${j(want)})));`);
  const turns = (...pairs) => pairs.map(([i, sig]) => ({ i, sig }));

  it("nothing changed", () => {
    const t = turns([0, "a"], [1, "b"]);
    assert.deepEqual(diff(t, t), { gone: [], fresh: [], changed: [] });
  });

  it("a new turn and a finished one", () => {
    const have = turns([0, "a"], [1, "b"]);
    const want = turns([0, "a"], [1, "b2"], [2, "c"]);
    assert.deepEqual(diff(have, want), { gone: [], fresh: [2], changed: [1] });
  });

  it("showing earlier adds at the front and keeps the rest", () => {
    const have = turns([5, "e"], [6, "f"]);
    const want = turns([3, "c"], [4, "d"], [5, "e"], [6, "f"]);
    assert.deepEqual(diff(have, want), { gone: [], fresh: [3, 4], changed: [] });
  });

  it("a window that slides drops the oldest", () => {
    const have = turns([0, "a"], [1, "b"], [2, "c"]);
    const want = turns([1, "b"], [2, "c"], [3, "d"]);
    assert.deepEqual(diff(have, want), { gone: [0], fresh: [3], changed: [] });
  });

  it("a turn hidden by the display settings is gone", () => {
    const have = turns([0, "a"], [1, "b"]);
    assert.deepEqual(diff(have, turns([1, "b"])), { gone: [0], fresh: [], changed: [] });
  });

  it("the search changes only the turns it marks", () => {
    const have = turns([0, "a"], [1, "b|old"]);
    const want = turns([0, "a"], [1, "b|new"]);
    assert.deepEqual(diff(have, want), { gone: [], fresh: [], changed: [1] });
  });
});

describe("TeammateMessages", () => {
  /** A teammate's report, shown as the terminal shows it. */
  const WRAP = cat("Another Claude session sent a message:\n%s\n\nThis came from another Claude session ",
                   "— not typed by your user, but very likely working on their behalf.");
  const wrap = (inner) => WRAP.replace("%s", () => inner);

  it("an idle notification reads as finished", () => {
    const text = wrap(cat('<teammate-message teammate_id="waiter-1" color="blue">\n',
      '{"type":"idle_notification","from":"waiter-1","timestamp":"2026-11-15T02:03:00.024Z",',
      '"idleReason":"available","result":"The wait finished and I sent `OK`."}\n',
      "</teammate-message>"));
    const [parts, line, html] = run(
      `[teammateParts(${j(text)}), teammateLine(teammateParts(${j(text)})), teammateHtml(teammateParts(${j(text)}))]`);
    assert.deepEqual(parts.map((p) => [p.name, p.verb, p.text]),
                     [["waiter-1", "finished", "The wait finished and I sent `OK`."]]);
    assert.equal(line, "@waiter-1 finished: The wait finished and I sent `OK`.");
    assertIn("Teammate <span class=at>@waiter-1</span> finished", html);
    assertIn("<code>OK</code>", html);
    assertNotIn("idle_notification", html);
    assertNotIn("This came from", html);
  });

  it("plain messages and several in one turn", () => {
    const text = wrap(cat('<teammate-message teammate_id="a" color="green" summary="Status update">\n',
      "Two of three done.\n</teammate-message>\n\n",
      '<teammate-message teammate_id="b" color="yellow">\n',
      '{"type":"idle_notification","result":"OK"}\n</teammate-message>'));
    const parts = run(`teammateParts(${j(text)})`);
    assert.deepEqual(parts.map((p) => [p.name, p.summary, p.verb, p.text]),
                     [["a", "Status update", "", "Two of three done."], ["b", "", "finished", "OK"]]);
  });

  it("other text is left alone", () => {
    assert.equal(run(`teammateParts(${j("hello")})`), null);
    const mixed = 'Please look at this: <teammate-message teammate_id="x">hi</teammate-message>';
    assert.equal(run(`teammateParts(${j(mixed)})`), null);
  });
});
