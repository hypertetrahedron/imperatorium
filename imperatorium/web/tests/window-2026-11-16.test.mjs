// The window changes of 2026-11-16 (user requests): the question card one
// question at a time, the chat's record of it likewise, paths that open,
// the branch and worktree in the Session Subtitle, and the sidebar's ☰ menu
// with its New Session dialog.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  SCRIPT, SHARED, CSS, ESC, between, evalJs, evalJson, j, cat, renderShell, assertIn, assertNotIn, assertLess,
} from "./load.mjs";

/** questionHtml for request `i`. */
const card = (i) => evalJs(cat(ESC, "\n",
  "const folder = c => String(c).split(/[\\\\/]/).pop();\n",
  "const hostTag = () => '';\n",
  between(SCRIPT, "function questionHtml(", "// Pure: what each question of a card"),
  "\nprocess.stdout.write(questionHtml(", j(i), "));"));

const Q = (header, question, labels, multi) =>
  ({ header, question, multiSelect: !!multi, options: labels.map((label) => ({ label, description: "" })) });

describe("QuestionCardPages", () => {
  it("several questions show one at a time, with a tab each and a Submit tab", () => {
    const html = card({ id: "p1", cwd: "C:\\w\\proj", questions: [
      Q("Auth", "Which auth?", ["OAuth", "JWT"]), Q("DB", "Which database?", ["Postgres", "SQLite"], true)] });
    assertIn('data-qtab="0"', html);
    assertIn('data-qtab="1"', html);
    assertIn('class="qtab qsubmit" role="tab" data-qtab="2"', html);
    // The first page shows; the second and the review wait.
    assertIn('data-k="0" data-multi=""\n      data-q="Which auth?">', html);
    assertIn('data-q="Which database?" hidden>', html);
    assertIn('<div class="qrev" data-k="2" hidden>', html);
    // Numbered choices for one answer, boxes for any number.
    assertIn('<span class="qn">1.</span>OAuth', html);
    assertIn('<span class="qn">&#9744;</span>Postgres', html);
    assertIn("Submit answers</button>", html);
    assertLess(html.indexOf('data-qstep="-1"'), html.indexOf('data-qtab="0"'));
  });

  it("one question has no tabs", () => {
    const html = card({ id: "p2", cwd: "/w/proj", questions: [Q("Go", "Ship it?", ["Yes", "No"])] });
    assertNotIn("qtabs", html);
    assertNotIn("qrev", html);
    assertIn('<span class="chip">Go</span>', html);
    assertIn("Submit answer</button>", html);
  });

  it("a redraw keeps the page and the choices", () => {
    assertIn("keepQuestions();\n  $(\"#decide\").innerHTML", SCRIPT);
    assertIn("show(kept ? kept.at : 0);", SCRIPT);
  });
});

describe("AskRecordPages", () => {
  /** askHtml for block `b`, answered with `result` (or not yet). */
  const record = (b, result) => evalJs(cat(ESC, "\n",
    "let pendingNow = [];\n",
    "const toolResults = new Map(", j(result == null ? [] : [[b.id, result]]), ");\n",
    between(SHARED, "function answersOf(", "function toolHtml("),
    "\nprocess.stdout.write(askHtml(", j(b), "));"));

  it("several questions page by tab, the answered ones ticked", () => {
    const html = record({ id: "toolu_1", session: "s", questions: [
      Q("Auth", "Which auth?", ["OAuth", "JWT"]), Q("DB", "Which database?", ["Postgres"])] },
      '"Which auth?"="JWT"');
    assertIn('<div class="askq many">', html);
    assertIn('name="aq-toolu_1" id="aq-toolu_1-0" checked>', html);
    assertIn('<label for="aq-toolu_1-0" title="Which auth?">&#9746; Auth</label>', html);
    assertIn('<label for="aq-toolu_1-1" title="Which database?">&#9744; DB</label>', html);
    assert.equal(html.split('<section class="askp">').length - 1, 2);
    assertIn(".askq.many .askp{display:none}", CSS);
    assertIn(".askq input.ar:nth-of-type(4):checked~.askp:nth-of-type(4)", CSS);
  });

  it("one question is shown whole, as before", () => {
    const html = record({ id: "t2", session: "s", questions: [Q("Go", "Ship it?", ["Yes"])] });
    assertIn('<div class="askq">', html);
    assertNotIn("asktabs", html);
  });
});

describe("HeadSubtitle", () => {
  const sub = (s, branch) => evalJs(cat(ESC, "\n",
    between(SCRIPT, "const WT_ICON", "\n"), "\n",
    between(SCRIPT, "function headSub(", "function drawHead("),
    "\nprocess.stdout.write(headSub(", j(s), ", ", j(branch), "));"));

  it("leads with the branch and the worktree", () => {
    const html = sub({ worktree: "fix-login", cwd: "C:\\w\\proj\\.claude\\worktrees\\fix-login" }, "fix/login");
    assertIn('<span class="hbr" title="git branch">&#9095; fix/login</span>', html);
    assertIn("worktree fix-login</span>", html);
    assertLess(html.indexOf("hbr"), html.indexOf("C:\\w\\proj"));
  });

  it("the main checkout says so; nothing known, only the folder", () => {
    assertIn("main checkout", sub({ cwd: "C:\\w\\proj" }, "main"));
    assert.equal(sub({ cwd: "C:\\w\\proj" }, ""), "C:\\w\\proj");
  });
});

describe("SidebarMenu", () => {
  it("+ new, history and insights are one ☰ menu", () => {
    const page = renderShell();
    assertIn("<details id=sidemenu class=menu>", page);
    const menu = page.slice(page.indexOf("id=sidemenu"), page.indexOf("</details>", page.indexOf("id=sidemenu")));
    for (const want of ["id=new", "data-view=history", "data-view=insights"]) assertIn(want, menu);
    assertNotIn("id=picker", page);
  });

  it("the New Session dialog asks for the project and the permission mode", () => {
    const page = renderShell();
    assertIn("<div id=newbox hidden>", page);
    for (const m of ["default", "acceptEdits", "plan", "auto", "dontAsk", "bypassPermissions"])
      assertIn(`<option value=${m}>`, page);
    assertIn('post("/api/new", {cwd: p.path, permission_mode: mode})', SCRIPT);
  });

  it("projects are found by name or folder", () => {
    const got = evalJson(cat(between(SCRIPT, "function projectHits(", "function drawProjects("),
      "\nconst L = [{name: 'laser-ledger', path: 'C:/w/laser-ledger'}, {name: 'gem-trip', path: 'Y:/p/gem-trip'}];",
      "\nprocess.stdout.write(JSON.stringify([projectHits(L, 'laser').map(p => p.name),",
      " projectHits(L, 'y:/p').map(p => p.name), projectHits(L, '').length, projectHits(L, 'gemtrip').length]));"));
    assert.deepEqual(got, [["laser-ledger"], ["gem-trip"], 2, 1]);
  });
});

describe("PathMenu", () => {
  it("a click on a path offers open, its folder, or copy", () => {
    assertIn("<div id=pathmenu class=pathmenu role=menu hidden></div>", renderShell());
    assertIn('post("/api/open_path", {path, session_id: session, reveal: how === "reveal"})', SCRIPT);
    for (const a of ['data-pact="open"', 'data-pact="reveal"', 'data-pact="copy"']) assertIn(a, SCRIPT);
  });
});

describe("StopButton", () => {
  it("shows only for a local session in the middle of a turn", () => {
    const got = evalJson(cat(
      "const RUNNING = {busy: 'working', shell: 'running a command'};\n",
      between(SCRIPT, "const canStop =", "\n"),
      "\nprocess.stdout.write(JSON.stringify([canStop({status: 'busy'}), canStop({state: 'working'}),",
      " canStop({status: 'shell'}), canStop({status: 'idle'}), canStop({status: 'busy', remote: true}), canStop(null)]));"));
    assert.deepEqual(got, [true, true, true, false, false, false]);
    assertIn("<button id=stop class=\"ghost danger\" hidden", renderShell());
  });

  it("Esc in the prompt box stops a working turn, and is never sent twice at once", () => {
    assertIn('if (view === "session" && canStop(current())){ e.preventDefault(); stopTurn(); }', SCRIPT);
    assertIn("if (Date.now() - stopAt < 3000) return true;", SCRIPT);
    assertIn('post("/api/stop", {session_id: k})', SCRIPT);
  });
});

describe("HistoryInsightsPopups", () => {
  it("History and Insights are pop-ups over the window, not in its place", () => {
    const page = renderShell();
    const main = page.slice(page.indexOf("<div id=main>"), page.indexOf("<div id=histbox"));
    assertNotIn("id=historypane", main);
    assertNotIn("id=insightpane", main);
    for (const box of ["histbox", "insightbox"]) {
      const part = page.slice(page.indexOf(`<div id=${box}`));
      assertIn(`<div id=${box} class=popup hidden>`, page);
      assertIn("data-popclose", part.slice(0, 600));
    }
    // Nothing hides the chat any more.
    assertNotIn('$("#sessionpane").hidden = v !== "session"', SCRIPT);
    assertNotIn('$("#ask").hidden = v !== "session"', SCRIPT);
  });

  /** openPopup against stub boxes: which are shown after `calls`. */
  const shown = (calls) => evalJson(cat(
    "const els = {'#histbox': {hidden: true}, '#insightbox': {hidden: true}, '#hq': {focus() {}}};\n",
    "const $ = s => els[s]; const document = {querySelectorAll: () => []};\n",
    "let searched = 0, loaded = 0; const search = () => searched++, loadInsights = () => loaded++, closeMenus = () => {};\n",
    between(SCRIPT, "const POPUPS =", "\n\nfunction setTab("), "\n", calls,
    "\nprocess.stdout.write(JSON.stringify([!els['#histbox'].hidden, !els['#insightbox'].hidden, searched, loaded]));"));

  it("one at a time, and the session view closes them", () => {
    assert.deepEqual(shown("openPopup('history', true);"), [true, false, 1, 0]);
    assert.deepEqual(shown("openPopup('history', true); openPopup('insights', true);"), [false, true, 1, 1]);
    assert.deepEqual(shown("openPopup('insights', true); setView('session');"), [false, false, 0, 1]);
  });
});
