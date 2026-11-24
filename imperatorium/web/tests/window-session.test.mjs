// The window's session list, tiles, status panel and page shell (app.js).
// Ported from test_ui.py: Drafts, SessionTree, Shape, WorkAtAGlance,
// StatusPanel, LogWindow, Controls, FollowNewSession.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  SHARED, SCRIPT, CSS, MATRIX_SCRIPT, SETTINGS_SCRIPT, ESC, FONTS,
  between, tail, find, evalJs, evalJson, evalJsLater, j, cat, count, renderShell,
  assertIn, assertNotIn, assertLess,
} from "./load.mjs";

describe("Drafts", () => {
  /** The prompt box keeps one unsent draft per session. */
  const runJs = (steps) => {
    const body = between(SCRIPT, "// -- one unsent draft per session", "let ticks = 0;");
    return evalJson(cat(
      "const box = {value: ''}; const $ = () => box; let selected = null, view = 'session';\n",
      "const store = {}; const localStorage = {getItem: k => store[k] || null,",
      " setItem: (k, v) => { store[k] = v; }};\n", body, "\n", steps));
  };

  it("switching parks and restores", () => {
    const got = runJs(cat(
      "const seen = [];",
      "selected = 'a'; syncDraft(); box.value = 'half a thought'; noteDraft();",
      "selected = 'b'; syncDraft(); seen.push(box.value); box.value = 'other'; noteDraft();",
      "selected = 'a'; syncDraft(); seen.push(box.value);",
      "selected = 'b'; syncDraft(); seen.push(box.value);",
      "process.stdout.write(JSON.stringify(seen));"));
    assert.deepEqual(got, ["", "half a thought", "other"]);
  });

  it("sent draft is gone and late clear spares the new box", () => {
    const got = runJs(cat(
      "selected = 'a'; syncDraft(); box.value = 'go'; noteDraft();",
      "selected = 'b'; syncDraft(); box.value = 'typing in b'; noteDraft();",
      "clearPrompt('a');",  // a's send finished after the switch
      "const inB = box.value; selected = 'a'; syncDraft();",
      "process.stdout.write(JSON.stringify([inB, box.value]));"));
    assert.deepEqual(got, ["typing in b", ""]);
  });
});

describe("SessionTree", () => {
  /** A project with several sessions gathers them under one node. */
  const SESSIONS = [
    { name: "hai-proof-5b", short_name: "5b", cwd: "C:/src/hai-proof", project: "hai-proof" },
    { name: "strata-0b", short_name: "0b", cwd: "C:/src/strata", project: "strata" },
    { name: "fizzy-wondering-lark-c9", display_name: "hai-proof-fizzy-wondering-lark",
      short_name: "fizzy-wondering-lark",
      cwd: "C:/src/hai-proof/.claude/worktrees/fizzy-wondering-lark",
      project: "hai-proof", worktree: "fizzy-wondering-lark", attention: true },
  ];

  const build = (group, sessions, { folded = [], waiting = [], then = null } = {}) => {
    const s = SCRIPT;
    return cat(
      "const store = {'imperatorium.folded': ", j(j(folded)), ", 'imperatorium.waithidden': ", j(j(waiting)), "};\n",
      "const localStorage = {getItem: k => store[k] || null, setItem: (k, v) => { store[k] = v; }};\n",
      "const drawSessions = () => {};\n",
      between(s, "const esc =", "\n\n"), "\n", between(s, "const folder =", "\n"), "\n",
      between(s, "const nameOf =", "\nfunction groupOf"), "\n",
      between(s, "function key(", "\n"), "\n",
      between(s, "function forkOrder(", "\nfunction drawSessions"), "\n",
      "const stateOf = s => [s.attention ? 'd-attn' : 'd-idle', 'x'];\n",
      "const isUnread = s => !!s.unread;\n",
      "const tile = (s, short, d) => '[' + (d ? '>'.repeat(d) : '') + (short && s.short_name || nameOf(s)) + ']';\n",
      "const prefs = {group: ", j(group), "};\n",
      then || "process.stdout.write(tree(" + j(sessions) + "));");
  };
  const runJs = (group, sessions, opts) => evalJs(build(group, sessions, opts));
  const clicks = async (script) => JSON.parse(await evalJsLater(build("state", [], { then: cat(
    script, "; setTimeout(() => process.stdout.write(JSON.stringify(",
    "[[...folded], [...waitHidden]])), DOUBLE_MS + 50);") })));

  it("siblings nest under their project by short name", () => {
    const html = runJs("state", SESSIONS);
    assertIn('data-fam="hai-proof"', html);
    assertIn('<span class="car">▾</span><span>hai-proof</span>', html);
    assertIn('<div class="kids">[5b][fizzy-wondering-lark]</div>', html);
    assert.ok(html.endsWith("[strata-0b]"));
  });

  it("a fork nests under the session it was forked from", () => {
    const row = (id, name, extra) => ({ sessionId: id, name, cwd: "C:/src/strata", project: "strata", ...extra });
    const html = runJs("state", [
      row("c", "child", { work: { forked_from: "p" } }),
      row("o", "other"),
      row("g", "grandchild", { work: { forked_from: "c" } }),
      row("p", "parent"),
      row("x", "orphan", { work: { forked_from: "gone" } }),
      row("l1", "loop-a", { work: { forked_from: "l2" } }),
      row("l2", "loop-b", { work: { forked_from: "l1" } }),
    ]);
    const kids = html.slice(html.indexOf('<div class="kids">') + 18, html.lastIndexOf("</div>"));
    // Roots keep their order; each fork follows its parent, a step in; a
    // parent not on the list leaves the fork at the top; a loop shows once each.
    assert.equal(kids, "[other][parent][>child][>>grandchild][orphan][loop-a][>loop-b]");
  });

  it("the fork marker names the parent", () => {
    const got = evalJs(cat(
      between(SCRIPT, "const esc =", "\n\n"), "\n",
      between(SCRIPT, "function key(", "\n"), "\n",
      between(SCRIPT, "const nameOf =", "\n"), "\n",
      between(SCRIPT, "function forkMark(", "\nfunction tree("), "\n",
      "const all = [{sessionId: 'p', name: 'the <parent>'}];\n",
      "process.stdout.write(JSON.stringify([forkMark({work: {forked_from: 'p'}}, all),",
      " forkMark({work: {forked_from: 'abcdef1234'}}, all), forkMark({}, all)]));"));
    const [named, bare, none] = JSON.parse(got);
    assertIn('title="forked from the &lt;parent&gt;"', named);
    assertIn('title="forked from abcdef12"', bare);
    assert.equal(none, "");
  });

  it("one dot per session on the node", () => {
    const html = runJs("state", SESSIONS);
    const dots = html.slice(html.indexOf('class="dots"'), html.indexOf('<div class="kids">'));
    assert.equal(count(dots, 'class="dot '), 2);
    assertIn("d-attn", dots);
  });

  it("folded node hides everything but its dots", () => {
    const html = runJs("state", SESSIONS, { folded: ["hai-proof"] });
    assertIn('class="fam shut"', html);
    assertNotIn("kids", html);
    assertNotIn("[fizzy-wondering-lark]", html);
    assert.equal(count(html, 'class="dot '), 2);
  });

  it("hiding waiting keeps the others", () => {
    const html = runJs("state", SESSIONS, { waiting: ["hai-proof"] });
    assertIn('<div class="kids">[5b]</div>', html);
    assertIn("waiting hidden", html);
    assert.equal(count(html, 'class="dot '), 2);
  });

  it("folded wins over waiting hidden", () => {
    const html = runJs("state", SESSIONS, { folded: ["hai-proof"], waiting: ["hai-proof"] });
    assertNotIn("kids", html);
    assertNotIn("waiting hidden", html);
  });

  it("a single click folds once the double click window passes", async () => {
    assert.deepEqual(await clicks("famClicked('a')"), [["a"], []]);
  });

  it("a double click hides waiting and does not fold", async () => {
    assert.deepEqual(await clicks("famClicked('a'); famClicked('a')"), [[], ["a"]]);
  });

  it("unfolding shows the waiting ones too", async () => {
    const got = JSON.parse(await evalJsLater(build("state", [], { folded: ["a"], waiting: ["a", "b"], then: cat(
      "famClicked('a'); setTimeout(() => process.stdout.write(JSON.stringify(",
      "[[...folded], [...waitHidden], JSON.parse(store['imperatorium.waithidden'])])), DOUBLE_MS + 50);") })));
    assert.deepEqual(got, [[], ["b"], ["b"]]);
  });

  it("folding leaves the waiting flag for the unfold to clear", async () => {
    const got = await clicks("waitHidden.add('a'); famClicked('a')");
    assert.deepEqual(got, [["a"], ["a"]]);
  });

  it("clicks on two nodes fold both", async () => {
    assert.deepEqual([...(await clicks("famClicked('a'); famClicked('b')"))[0]].sort(), ["a", "b"]);
  });

  it("a lone session is a plain tile", () => {
    assert.equal(runJs("state", SESSIONS.slice(1, 2)), "[strata-0b]");
  });

  it("grouped by folder a lone session still has no node", () => {
    const html = runJs("folder", SESSIONS);
    assertIn('<div class="kids">[5b][fizzy-wondering-lark]</div>', html);
    assertNotIn('data-fam="strata"', html);
    assert.ok(html.endsWith("[strata-0b]"));
  });

  it("a click opens a tile and it carries its pin close and tasks", () => {
    // Opened by a click only (2026-11-14: no longer by the pointer passing).
    assertIn(".s:not(.on) :is(.m,.stl,.x,.tk){display:none}", CSS);
    assertIn(".s.on .why{display:none}", CSS);
    assertNotIn(":not(:hover) :is(.m", CSS);
    assertNotIn(".s:hover .acts", CSS);
    // 2026-11-16: pin and close in the open tile's first line (its top right
    // corner), and on no closed tile.
    assertIn('open && !s.provisional ? `<span class="acts">${acts}</span>` : ""}</div><div class="stl', SCRIPT);
    assertNotIn('prBadge(s.pr)}</span><span class="acts">', SCRIPT);
  });

  it("tile x closes a running local session rather than archiving", () => {
    for (const needle of ['data-close="${esc(key(s))}"', 's.remote || s.provisional || closing.has(key(s)) ? ""',
                          ': s.pid ? `<span data-close=',
                          'post("/api/close"', "closeSession(el.dataset.close)",
                          "famClicked(el.dataset.fam)"]) {
      assertIn(needle, SCRIPT);
    }
    assertNotIn('archived ? "↩" : "✕"', SCRIPT);
  });

  it("a tile with nothing running can be dismissed", () => {
    // Any tile, archived or not (2026-11-12: a finished background job
    // that was not archived had no way off the list). Not one still
    // working before its process is matched (test_finished.Window).
    for (const needle of ['data-dismiss="${esc(key(s))}"', ': s.state === "working" || RUNNING[s.status] ? ""',
                          'post("/api/dismiss"', "dismissSession(el.dataset.dismiss)"]) {
      assertIn(needle, SCRIPT);
    }
  });

  it("notification switches are in settings not insights", () => {
    assertNotIn("id=notif", renderShell());
    assertNotIn('post("/api/notify"', SCRIPT);
    for (const needle of ['sec.id === "notify" ? notifyExtrasHtml(d)', 'id="desk"',
                          "Notification.requestPermission()", '"imperatorium.desknotify"']) {
      assertIn(needle, SETTINGS_SCRIPT);
    }
    // the main window picks up the switch flipped in the Settings window
    assertIn('e.key === "imperatorium.desknotify"', SCRIPT);
  });

  it("vs code tiles carry their own icon", () => {
    assertIn('s.desktop_app ? APP_ICON : s.ide === "vscode" ? IDE_ICON : ""', SCRIPT);
  });

  it("every page reloads for a new dispatcher", () => {
    assertIn("async function checkBoot(onNew)", SHARED);
    for (const script of [SCRIPT, MATRIX_SCRIPT, SETTINGS_SCRIPT]) {
      assertIn("checkBoot(", script.replace(SHARED, ""));
    }
    // the window keeps its session; Settings never drops unsaved edits
    assertIn('"?session=" + encodeURIComponent(selected)', SCRIPT);
    assertIn("Object.keys(changes()).length) say(", SETTINGS_SCRIPT);
  });

  it("worktree tiles carry the icon", () => {
    assertIn("s.worktree ? WT_ICON", SCRIPT);
  });
});

describe("Shape", () => {
  /** Each kind of text shows in full, folded to one line, or not at all. */
  const TURNS = [
    { speaker: "you", blocks: [{ kind: "text", text: "do the thing" }] },
    { speaker: "claude", blocks: [
      { kind: "thinking", text: "\n  first thought\nsecond" },
      { kind: "text", text: "on it" },
      { kind: "tool", name: "Bash", summary: "ls -la", id: "t1" },
      { kind: "result", text: "boom\nmore", is_error: true, id: "t1" }] },
    { speaker: "system", note: "task finished", blocks: [{ kind: "text", text: "build passed" }] },
  ];

  const runJs = (shape, opened = []) => {
    const a = find(SCRIPT, "function renderBlock(b)");
    const body = between(SCRIPT, "const attr =", "// -- end highlighting --") + "\n" +
      SCRIPT.slice(a, find(SCRIPT, "function drawShape"));
    return evalJson(cat(
      "const renderText = t => '[' + t + ']';\n",
      "const teammateParts = () => null;\n",
      "const localStorage = {getItem: () => ", j(j(shape)), "};\n",
      ESC, "\n", body,
      "\nconst opened = new Set(", j(opened), ");",
      "\nprocess.stdout.write(JSON.stringify(", j(TURNS), ".map((it, i) => shapeBlocks(it, i, opened))));"));
  };

  it("full by default is the old rendering", () => {
    const [you, claude, note] = runJs({});
    assert.equal(you, "[do the thing]");
    assertIn('<div class="think">', claude);
    assertIn("[on it]", claude);
    assertIn('<b>Bash</b> <span class="tc">ls</span> <span class="tf">-la</span>', claude);
    assertIn('class="res err"', claude);
    assertNotIn("<details", claude);
    assert.equal(note, "[build passed]");
  });

  it("hidden drops only its kind", () => {
    const [you, claude, note] = runJs({ thinking: "hidden", result: "hidden", system: "hidden" });
    assert.equal(you, "[do the thing]");
    assertNotIn("think", claude);
    assertNotIn("res", claude);
    assertIn("[on it]", claude);
    assert.equal(note, "");
  });

  it("compact is one line that opens", () => {
    const [you, claude] = runJs({ thinking: "compact", tool: "compact", result: "compact", prompt: "compact" },
                                ["1.2"]);
    assertIn('<details class="cmp" data-k="0.0"><summary><span class="ln">do the thing', you);
    assertIn('<span class="k">thinking</span><span class="ln">first thought</span>', claude);
    assertIn('data-k="1.2" open><summary><span class="k">Bash</span><span class="ln">ls -la', claude);
    assertIn('<span class="k err">error</span><span class="ln">boom</span>', claude);
    // Replies were left at Full.
    assertIn("</details>[on it]<details", claude);
  });

  it("unknown mode falls back to full", () => {
    const [you] = runJs({ prompt: "sideways" });
    assert.equal(you, "[do the thing]");
  });
});

describe("DisplayTimes", () => {
  /** View > Display Times: each block says when it was written. */
  const body = () => {
    const a = find(SCRIPT, "function renderBlock(b)");
    return between(SCRIPT, "const attr =", "// -- end highlighting --") + "\n" +
      SCRIPT.slice(a, find(SCRIPT, "let logItems", a));
  };
  // Local noon on 2026-11-15, and the same clock a day earlier.
  const NOON = new Date(2026, 10, 15, 12, 5).toISOString();
  const YESTERDAY = new Date(2026, 10, 14, 9, 30).toISOString();
  const NOW = new Date(2026, 10, 15, 18, 0).getTime();
  const run = (times, code) => evalJson(cat(
    "const renderText = t => '[' + t + ']';\n",
    "const teammateParts = () => null;\n",
    "const localStorage = {getItem: k => k === 'imperatorium.times' ? ", j(times ? "1" : "0"), " : '{}'};\n",
    "let html = '';\n",
    "const $ = () => ({classList: {toggle() {}}, set innerHTML(v) { html = v; }});\n",
    ESC, "\n", body(), "\n", code));

  it("off by default: no stamps, and the menu offers it unticked", () => {
    const got = run(false, cat(
      "drawShape();",
      "process.stdout.write(JSON.stringify([shapeBlocks({speaker: 'you', blocks: [{kind: 'text', text: 'go', at: ",
      j(NOON), "}]}, 0, new Set()), html]));"));
    assert.equal(got[0], "[go]");
    assertIn("Display Times<input type=checkbox data-times>", got[1]);
    // Under the shapes, after the last of them.
    assertLess(got[1].indexOf('data-shape="system"'), got[1].indexOf("Display Times"));
  });

  it("on: each block carries its own time", () => {
    const got = run(true, cat(
      "Date.now = () => ", j(NOW), ";",
      "drawShape();",
      "process.stdout.write(JSON.stringify([shapeBlocks({speaker: 'claude', blocks: [",
      "{kind: 'text', text: 'in 10 minutes', at: ", j(NOON), "},",
      "{kind: 'text', text: 'old', at: ", j(YESTERDAY), "},",
      "{kind: 'text', text: 'no time'}]}, 0, new Set()), html]));"));
    assertIn('<span class=bt title="', got[0]);
    assertIn(">12:05</span>[in 10 minutes]", got[0]);
    assertIn(">Nov 14 09:30</span>[old]", got[0]);
    assertIn("</span>[old][no time]", got[0]);
    assertIn("data-times checked", got[1]);
  });

  it("a hidden block has no stamp either", () => {
    const got = run(true, cat(
      "shape = {thinking: 'hidden'};",
      "process.stdout.write(JSON.stringify(shapeBlocks({speaker: 'claude', blocks: [",
      "{kind: 'thinking', text: 'x', at: ", j(NOON), "}]}, 0, new Set())));"));
    assert.equal(got, "");
  });

  it("times are only as fine as minutes, and a bad time is none", () => {
    const got = run(true, cat(
      "process.stdout.write(JSON.stringify([blockTime(", j(NOON), ", ", j(NOW), ").text,",
      " blockTime('nonsense', 0), blockTime(null, 0)]));"));
    assert.deepEqual(got, ["12:05", null, null]);
  });
});

describe("WorkAtAGlance", () => {
  /** The tile's cache countdown, its signals, and what the open tile adds. */
  const runTileJs = (call) => {
    const kfmt = between(SCRIPT, "function kfmt", "function span");
    let span = tail(SCRIPT, "function span");
    span = span.slice(0, span.indexOf("\n}\n") + 3);
    let usd = tail(SCRIPT, "const usd =");
    usd = usd.slice(0, usd.indexOf("\n") + 1);
    return evalJson(cat(
      ESC, "\n", kfmt, span, usd,
      "const stateOf = s => [s.status === 'busy' ? 'd-busy' : 'd-idle', s.status];\n",
      "const isUnread = s => !!s.unread; const heavyWarnings = () => [];\n",
      "const prBadge = pr => '[PR]'; const age = t => 'AGE';\n",
      between(SCRIPT, "const CACHE_SOON", "function tile("),
      "\nprocess.stdout.write(JSON.stringify(", call, "));"));
  };
  const parts = (work, now = 1000, all = false) =>
    runTileJs(`workParts(${j(work)}, ${now}, ${j(all)}).map(p => p.text)`);
  const details = (row, now = 10000, spend = null) =>
    runTileJs(`tileDetails(${j(row)}, ${now}, ${j(spend)}).map(p => p.text)`);
  const signals = (row, now = 1000, open = false) =>
    runTileJs(`tileSignals(${j(row)}, ${now}, ${j(open)})`);

  it("a closed tile counts down only the last ten minutes", () => {
    const far = { cache_expires_at: 1000 + 58 * 60 - 10 };
    assert.deepEqual(parts(far), []);
    assert.deepEqual(parts(far, 1000, true), ["-58"]);
    assert.deepEqual(parts({ cache_expires_at: 1000 + 4 * 60 - 10 }), ["-4"]);
  });

  it("minutes round up and an expired cache shows nothing", () => {
    assert.deepEqual(parts({ cache_expires_at: 1030 }), ["-1"]);
    assert.deepEqual(parts({ cache_expires_at: 900 }, 1000, true), []);
    assert.deepEqual(parts(null), []);
  });

  it("a scheduled turn says when on any tile", () => {
    const w = { alive: true, wakes_at: 1000 + 20 * 60, wakeups: 1 };
    assert.deepEqual(parts(w), ["wakes 20m"]);
    assert.deepEqual(parts({ ...w, wakes_at: 900 }), []);
    assert.deepEqual(parts({ ...w, alive: false }), []);
  });

  it("the cache countdown drains as a bar of its lifetime", () => {
    // An hour's cache with 15 min left: a quarter of the bar; none without a ttl.
    const got = runTileJs(
      `[workParts({cache_expires_at: 1900, cache_ttl: 3600}, 1000, true)[0].fill,` +
      ` workParts({cache_expires_at: 1900}, 1000, true)[0].fill,` +
      ` workHtml({cache_expires_at: 1900, cache_ttl: 3600}, 1000, "badge work", true)]`);
    assert.equal(got[0], 0.25);
    assert.equal(got[1], null);
    assertIn('class="badge work cache drain" style="--fill:25%"', got[2]);
  });

  it("the four counts are gone from the tile and status line", () => {
    assertIn('workHtml(s.work, now, "badge work", open)', SCRIPT);
    assertNotIn('n.join("/")', SCRIPT);
  });

  it("the open tile says idle time context cost and whats running", () => {
    const got = details({ status: "idle", status_changed_at: 10000 - 720,
                          usage: { context_pct: 45.2 },
                          work: { context_tokens: 230000, alive: true,
                                  agents: 1, shells: 2, monitors: 1, processes: 2 } },
                        10000, { cost_usd: 4.1 });
    assert.deepEqual(got, ["idle 12m", "ctx 230k (45%)", "≈$4.10", "3 running"]);
    const waiting = details({ status: "idle", attention: true, status_changed_at: 10000 - 720 });
    assert.deepEqual(waiting, ["idle 12m"]);
  });

  it("idle time is whole minutes, under one is <1m", () => {
    const idle = (secs) => details({ status: "idle", status_changed_at: 10000 - secs })[0];
    assert.equal(idle(0), "idle <1m");
    assert.equal(idle(59), "idle <1m");
    assert.equal(idle(60), "idle 1m");
    assert.equal(idle(119), "idle 1m");
    assert.equal(idle(3 * 3600 + 5 * 60 + 40), "idle 3h 5m");
  });

  it("a working session says how long and nothing running is left out", () => {
    const got = details({ status: "busy", status_changed_at: 10000 - 180,
                          work: { alive: true, agents: 0, processes: 0 } });
    assert.deepEqual(got, ["working 3m"]);
  });

  it("closed signals appear only when out of the ordinary", () => {
    assert.equal(signals({ usage: { context_pct: 40 },
                           work: { cache_expires_at: 3000 },
                           pr: { state: "open", checks: "passing" } }), "");
    const got = signals({ unread: true, queued: 2, usage: { context_pct: 91 },
                          work: { cache_expires_at: 1200 },
                          attention: true, attention_since: 40,
                          pr: { state: "open", checks: "failing" } });
    const order = [">new<", ">-4<", ">ctx 91%<", ">2 queued<", "[PR]"].map((x) => got.indexOf(x));
    assert.ok(order.every((i) => i >= 0), String(order));
    assert.deepEqual(order, [...order].sort((a, b) => a - b));
    assertNotIn('class="since"', got);   // the open tile's idle time says it
  });

  it("a cold cache is not said on the tile", () => {
    // 2026-11-15 (user request, first asked 2026-11-14): the chat's
    // cold-cache button says it, with the way out; the tile does not.
    assertNotIn('class="badge heavy"', SCRIPT);
    assertNotIn(".badge.heavy", CSS);
    assertNotIn("cold", signals({ work: { context_tokens: 230000, cache_expires_at: 900 } }));
  });

  it("nothing running is said once, in the Tasks header", () => {
    // 2026-11-15 (user request): no "nothing running" row under
    // "Tasks · none running"; with nothing else to list, nothing is under it.
    const items = (l) => evalJson(cat(
      "let idleOpen = false, doneOpen = false;\n",
      "const taskRow = t => '<row ' + t.id + '>';\n",
      between(SCRIPT, "// Pure: the rows under the tile's Tasks header", "function drawTileTasks"),
      "\nprocess.stdout.write(JSON.stringify(taskItems(", j(l), ", 1000)));"));
    const none = { running: [], idle: [], done: [], extra: [] };
    assert.equal(items(none), "");
    assertNotIn("nothing running", SCRIPT);
    assert.equal(items({ ...none, running: [{ id: "a" }] }), "<row a>");
    assertIn("Recently completed (1)", items({ ...none, done: [{ id: "b" }] }));
    assertIn(".s .tk.open .tkl:not(:empty){display:flex}", CSS);
  });

  it("the open tile keeps context and pr for its details line", () => {
    const got = signals({ usage: { context_pct: 91 }, work: { cache_expires_at: 3000 },
                          pr: { state: "open", checks: "failing" } }, 1000, true);
    assertNotIn("ctx", got);
    assertNotIn("[PR]", got);
    assertIn(">-34<", got);
  });
});

describe("StatusPanel", () => {
  /** The panel under the send buttons, rendered from a row and its status. */
  const runJs = (row, status, folded = false) => {
    const body = between(SCRIPT, "function kfmt(", "function drawStatus");
    const work = between(SCRIPT, "const CACHE_SOON", "function tile(");
    return evalJs(cat(
      ESC, "\nconst age = t => 'AGE';\nDate.now = () => 1000 * 1000;\n",
      "const stateOf = s => ['d-busy', s.label];\n", work, body,
      "\nprocess.stdout.write(statusHtml(", j(row), ", ", j(status), ", 1000, ", j(folded), "));"));
  };

  const ROW = { label: "waiting on a background task", queued: 2,
                usage: { context_pct: 17.4, context_size: 1000000, cost_usd: 3.456,
                         model: "Opus 5.5", lines_added: 12, lines_removed: 3 } };
  const STATUS = { ok: true, shell_processes: 1, stat: {
    agents: [], monitors: [{ id: "m", label: "watch <log>", started: 900, expires: 1600 }],
    shells: [{ id: "b", label: "make test" }],
    tokens: { input: 10, output: 2500, cache_read: 1200000, cache_write: 40000 },
    cache: { ttl: 3600, expires_at: 1300, warm: true, hit: 0.97 },
    context_tokens: 174000, prompts: 7, turns: 6, permission_mode: "auto",
    compactions: 2, branch: "main", version: "2.1.296" } };
  const head = (html) => html.slice(0, html.indexOf('<div class="bd">'));

  it("summary line", () => {
    const h = head(runJs(ROW, STATUS));
    for (const want of ["waiting on a background task", "1 shell", "1 monitor", "cache -5m", "$3.46"]) {
      assertIn(want, h);
    }
    assertNotIn("subagent", h);
  });

  it("the summary line has no slash counts", () => {
    const row = { ...ROW, work: { cache_expires_at: 1000 + 300, alive: true,
                                  agents: 1, shells: 2, monitors: 1, processes: 0 } };
    const h = head(runJs(row, STATUS));
    assertIn("cache -5m · $3.46</span>", h);
    assertNotIn("1/2/1/0", h);
  });

  it("detail cells", () => {
    const html = runJs(ROW, STATUS);
    for (const want of ['<b>processes</b><span class="hot">1<', "warm · 5m left · 97% hit", "17% · 174k / 1.0M",
                        "1.2M in · 2.5k out", "1.2M read · 40k written", "7 · 6 turns",
                        "+12 −3", ">auto<", ">main<", ">2<", "ends in 10m", "watch &lt;log&gt;"]) {
      assertIn(want, html);
    }
  });

  it("cold cache and no transcript", () => {
    const status = JSON.parse(JSON.stringify(STATUS));
    status.stat.cache.expires_at = 400;
    assertIn("cold · 10m ago", runJs(ROW, status));
    const bare = runJs({ label: "idle" }, { ok: true, note: "remote sessions report less" });
    assertNotIn("subagents", bare);
    assertIn("remote sessions report less", bare);
  });

  it("shell has the panel under the buttons", () => {
    const html = renderShell();
    assertLess(html.indexOf("id=bar"), html.indexOf("id=status"));
    assertLess(html.indexOf("id=status"), html.indexOf("id=queued"));
  });
});

describe("LogWindow", () => {
  /** A long chat draws its newest turns first; recent chats stay in the page. */
  const runJs = (expr) =>
    evalJson(between(SCRIPT, "const LOG_WINDOW", "function setLog") +
             "\nprocess.stdout.write(JSON.stringify(" + expr + "));");

  it("only the newest turns are drawn until asked", () => {
    assert.deepEqual(runJs(cat("[logStart(387, LOG_WINDOW), logStart(20, LOG_WINDOW),",
                               " logStart(387, LOG_WINDOW + LOG_STEP), logStart(387, 387)]")),
                     [327, 0, 127, 0]);
  });

  it("recent conversations are kept newest last", () => {
    const got = runJs(cat("(() => { for (let i = 0; i < CONV_KEEP + 2; i++) rememberConv('s' + i, [], 'g' + i);",
                          " rememberConv('s3', [1], 'new');",
                          " return [convCache.size, [...convCache.keys()][0],",
                          " [...convCache.keys()].pop(), convCache.get('s3').sig]; })()"));
    assert.deepEqual(got, [8, "s2", "s3", "new"]);
  });

  it("the window wires it up", () => {
    for (const needle of ["showCached()", "data-earlier", "want !== selected", "/api/voice/warm"]) {
      assertIn(needle, SCRIPT);
    }
  });

  it("positions in the log ignore the text zoom", () => {
    // The chat text size zooms each turn, and a zoomed element's offsetTop
    // is in zoomed units: prompt jumps and find overshot by the zoom
    // factor (2026-11-12, thousands of px at 15px text). Positions inside
    // the log come from bounding rects, via logY.
    assertNotIn(".offsetTop", SCRIPT);
    assertIn("const at = x => logY(x) - 8", SCRIPT);
    assertIn("logY(m)", SCRIPT);
    const body = between(SCRIPT, "function logY", "// -- jump between your prompts");
    const got = evalJson(cat(
      "const log = {scrollTop: 5000, clientTop: 0, getBoundingClientRect: () => ({top: 120})};",
      "const $ = () => log;", body,
      "process.stdout.write(JSON.stringify(logY({getBoundingClientRect: () => ({top: 400})})));"));
    assert.equal(got, 5280);
  });
});

describe("Controls", () => {
  it("the whole script parses", () => {
    // The other tests slice pieces out; a typo anywhere else would only
    // show as a blank window.
    new Function(SCRIPT);
  });

  it("nothing is set below 12px but the fold carets", () => {
    // The session timeline is claude-lens's drawing at claude-lens's sizes:
    // its label spacing is reckoned from them (9-11px), so its own rules,
    // all scoped under .tlx, are left as they are.
    const css = CSS.replace(/\.tlx [^{}]*\{[^{}]*\}/g, "");
    const small = [...css.matchAll(/[^;{}]*font(?:-size)?:\s*(\d+(?:\.\d+)?)px[^;}]*/g)]
      .filter((m) => parseFloat(m[1]) < 12).map((m) => m[0]);
    assert.equal(small.length, 2, JSON.stringify(small));
    assert.ok(small.every((x) => x.includes("10px")));
  });

  it("no invalid font shorthand", () => {
    // `font:13px inherit` is not valid CSS; the browser drops the whole
    // declaration, and buttons and the prompt box fell back to its fonts.
    assert.deepEqual(CSS.match(/font:[^;}]*\binherit/g) || [], []);
  });

  it("every face named in the css is bundled", () => {
    // (That no request path reaches outside the folder is the Rust
    // server's `fonts_are_bundled` test.)
    const known = new Set(readdirSync(FONTS));
    for (const [, name] of CSS.matchAll(/url\(\/fonts\/([^)]+)\)/g)) {
      assert.ok(known.has(name), name);
      assert.equal(readFileSync(join(FONTS, name)).subarray(0, 4).toString("latin1"), "wOF2", name);
    }
    const mono = CSS.split("--mono:")[1];
    assertNotIn("Consolas,ui-monospace", mono.slice(mono.indexOf(";") + 1));
  });

  it("each state has its own shape", () => {
    for (const [cls, shape] of [[".d-idle", "box-shadow"], [".d-attn", "rotate(45deg)"],
                                [".d-done", "height:2px"]]) {
      const rule = tail(CSS, cls + "{");
      assertIn(shape, rule.slice(0, rule.indexOf("}")), cls);
    }
    assertIn(".d-fail::after{transform:rotate(-45deg)}", CSS);
  });

  it("shell has the new controls", () => {
    const html = renderShell();
    for (const needle of ["id=curbtn", "Show Current Prompt", "id=jprev", "id=jnext", "id=current", "id=shape"]) {
      assertIn(needle, html);
    }
  });

  it("session controls share the header row", () => {
    // Peer request 2026-10-18: the tabs, the launch actions, the view
    // options, find and prompt jump all on the session header's row, in
    // that order, and nothing left between the chat and the prompt box.
    // The buttons that stay (Resume) sit between find's arrows and prompt
    // jump's, to keep the two pairs apart.
    const html = renderShell();
    const head = html.slice(html.indexOf("id=headrow"), html.indexOf("id=sessionpane"));
    const order = ["id=headtext", "id=tabpick", "id=viewmenu", "id=findbar", "id=acts", "id=jump"]
      .map((n) => head.indexOf(n));
    assert.ok(order.every((i) => i >= 0), String(order));
    assert.deepEqual(order, [...order].sort((a, b) => a - b));
    // The New / Fork menu gave way to the Send button's menu (2026-11-22).
    assertNotIn("id=launch", html);
    for (const act of ["data-act=new-close", "data-act=fork-window", "data-act=fork-background"]) assertNotIn(act, html);
    const view = head.slice(head.indexOf("id=viewmenu"), head.indexOf("id=findbar"));
    for (const needle of ["data-act=show", "data-act=copy", "id=curbtn", "id=shape"]) assertIn(needle, view);
    assertNotIn("id=strip", html);
    assertNotIn("id=tabs", html);
    assertNotIn("#strip", SCRIPT);
    assertNotIn("#tabs", SCRIPT);
  });
});

describe("FollowNewSession", () => {
  /** New selects the session it launched once that shows up on the board. */
  const runJs = (steps) => {
    const body = between(SCRIPT, "let following = null;", "// -- queue");
    return evalJson(cat(
      "let sessions = [], selected = 'old'; const key = s => s.sessionId;",
      " const calls = []; const showCached = () => calls.push('paint');",
      " const drawSessions = () => {}; const refreshTab = () => {};",
      " const loadStatus = () => {}; const loadProcs = () => {}; let view = 'session'; const setView = v => view = v;\n",
      body, "\n", steps));
  };

  it("selected once it appears", () => {
    const got = runJs(cat(
      "follow('new'); sessions = [{sessionId: 'old'}]; catchFollowed();",
      " const before = selected; sessions.push({sessionId: 'new'}); catchFollowed();",
      " const after = selected; selected = 'old'; catchFollowed();",
      " process.stdout.write(JSON.stringify([before, after, selected, calls.length]));"));
    assert.deepEqual(got, ["old", "new", "old", 1]);
  });

  it("given up after the deadline", () => {
    const got = runJs(cat(
      "follow('new'); following.until = Date.now() - 1;",
      " sessions = [{sessionId: 'new'}]; catchFollowed();",
      " process.stdout.write(JSON.stringify([selected, following]));"));
    assert.deepEqual(got, ["old", null]);
  });

  it("the header offers the buttons", () => {
    for (const needle of ["follow(r.session_id)", "catchFollowed();"]) assertIn(needle, SCRIPT);
    // Show Window is in the View menu since 2026-11-17.
    assertIn(">Show Window</button>", renderShell());
    // New+close is gone since 2026-11-22 (/new does it); new sessions start
    // from the Send button's menu.
    for (const needle of ["data-act=new-close", ">New+close<"]) assertNotIn(needle, renderShell());
    assertIn('data-spawn="new window"', renderShell());
  });
});
