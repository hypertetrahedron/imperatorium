// Slash commands the window does itself (2026-11-22,
// docs/slash-commands-terminal-gaps.md): pickers for /model, /effort, /fast
// and /output-style from Claude Code's own lists, views for its panels, the
// prompt box's list of the session's commands, and Show Window on a refusal
// only the terminal can act on.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SHARED, SCRIPT, CSS, between, evalJson, cat, assertIn, assertNotIn, renderShell } from "./load.mjs";

const PURE = between(SCRIPT, "// Pure: the commands whose name or alias starts", "\n// The pop-up the pickers and views draw in.");
const KEYS = between(SCRIPT, "// Pure: whether a key in the prompt box belongs to the list", "\n\nasync function send(mode){");
const run = expr => evalJson(cat(PURE, "\n", KEYS, "\nprocess.stdout.write(JSON.stringify(", expr, "));"));

const CAT = `{models: [
  {value: "default", displayName: "Default (recommended)", resolvedModel: "claude-opus-5-5", supportedEffortLevels: ["low", "high"]},
  {value: "opus", displayName: "Opus 5.5", resolvedModel: "claude-opus-5-5", supportedEffortLevels: ["low", "high", "max"]},
  {value: "haiku", displayName: "Haiku 5.5", resolvedModel: "claude-haiku-5-5", supportedEffortLevels: ["low", "medium"]}]}`;

describe("SlashList", () => {
  const L = `[{name: "compact"}, {name: "config", aliases: ["settings"]}, {name: "context"}, {name: "co"}, {name: "model"}]`;

  it("matches by name or alias, the exact one first", () => {
    assert.deepEqual(run(`slashMatches(${L}, "co").map(c => c.name)`), ["co", "compact", "config", "context"]);
    assert.deepEqual(run(`slashMatches(${L}, "set").map(c => c.name)`), ["config"]);
    assert.deepEqual(run(`slashMatches(${L}, "", 2).length`), 2);
  });

  it("Tab completes; Enter completes only what is not a whole command yet", () => {
    assert.deepEqual(run(`[slashKey(true, "/com", ["compact"], "Tab"), slashKey(true, "/com", ["compact"], "Enter"),
      slashKey(true, "/compact", ["compact"], "Enter"), slashKey(false, "/com", ["compact"], "Tab"),
      slashKey(true, "/c", [], "Escape"), slashKey(true, "/c", [], "ArrowDown"), slashKey(true, "/c", [], "a")]`),
      [true, true, false, false, "close", "move", false]);
  });

  it("each command says what it does from here", () => {
    assert.deepEqual(run(`["run", "picker", "terminal"].map(how => howTag({how}))`),
      ["runs here", "picker here", "terminal only"]);
  });

  it("a command that prompts the model says it starts a turn", () => {
    assert.equal(run(`howTag({how: "run", turn: true})`), "starts a turn");
  });

  it("is under the prompt box, and completes on a click", () => {
    assertIn("<div id=ask><div id=slashlist role=listbox hidden></div><textarea id=prompt", renderShell());
    assertIn('const o = e.target.closest("[data-sli]");', SCRIPT);
  });
});

describe("SlashPickers", () => {
  it("finds the session's model by id, alias or name", () => {
    assert.deepEqual(run(`[catalogModel(${CAT}, "claude-haiku-5-5").value, catalogModel(${CAT}, "opus").value,
      catalogModel(${CAT}, "Opus 5.5").value, catalogModel(${CAT}, "nonesuch"), catalogModel(null, "opus")]`),
      ["haiku", "opus", "opus", null, null]);
  });

  it("offers the effort levels the current model takes, else every one", () => {
    assert.deepEqual(run(`[effortLevels(${CAT}, "claude-haiku-5-5"), effortLevels(${CAT}, ""), effortLevels(null, "")]`),
      [["low", "medium"], ["low", "high", "max", "medium"], ["low", "medium", "high", "xhigh", "max"]]);
  });

  it("a picker or a view the dispatcher names opens here instead of a refusal", () => {
    assertIn('if (r.how && r.how !== "terminal"){\n    dropPending(p.id);', SCRIPT);
    // After the caller clears the prompt box, so /add-dir can fill it in.
    assertIn('say("", key(s));\n    queueMicrotask(() => slashHere(s, r.how, r.what, r.name));', SCRIPT);
    assertIn('if (how === "picker") return openPicker(s, what);', SCRIPT);
    assertIn('sendSlash(s, "/" + what + " " + v)', SCRIPT);
  });

  it("New session asks for the model and effort too", () => {
    const page = renderShell();
    assertIn("<label for=nmodel>Model</label><select id=nmodel>", page);
    assertIn("<select id=neffort", page);
    assertIn('spawnFrom("new", bg ? "background" : "window", {session_id: null, cwd: p.path,', SCRIPT);
    assertIn('prompt: bg ? text || "Ready when you are." : "", permission_mode: mode, model, effort});', SCRIPT);
  });
});

describe("SlashViews", () => {
  it("reads through the session's mod, window only, and offers Show Window without one", () => {
    assertIn('post("/api/session/op", {session_id: key(s), op, args: args || {}}', SCRIPT);
    assertIn('(r.no_mod ? `<button class=sm data-slshow>Show Window</button>` : "")', SCRIPT);
  });

  it("/config changes one row at a time, as the menu would", () => {
    assertIn('op: "config_set", args: {key: r.key, value}', SCRIPT);
    assertIn('const dis = r.isLocked ? " disabled" : "";', SCRIPT);
  });

  it("a refusal only the terminal can act on carries Show Window", () => {
    assertIn('p.state === "failed" && p.show ? `<button class="ghost tiny" data-showwin', SCRIPT);
    assertIn('if (e.target.closest && e.target.closest("[data-showwin]")){ act("show"); return; }', SCRIPT);
    assertIn("<div id=slashbox class=popup hidden>", renderShell());
    assertIn("#slashlist[hidden]{display:none}", CSS);
  });
});

// Send to a new session (2026-11-22): the Send button's four options, the
// dialog before a worktree is made, and the warnings it carries.
describe("SendToNewSession", () => {
  const ASK = between(SCRIPT, "const SPAWN_TITLE = {", "// The dialog: resolves");
  const OUT = between(SCRIPT, "// Pure: the status line once it started.", "// kind: new | fork | resume;");
  const ask = (plan, confirm) => evalJson(cat(SHARED, "\n", ASK, "\n", OUT,
    "\nconst a = spawnAsk(", JSON.stringify(plan), ", ", String(confirm), ");",
    "\nprocess.stdout.write(JSON.stringify(a && {body: a.body, keys: a.buttons.map(b => b[0]), foot: a.foot}));"));
  const base = {placement: "worktree", folder: "C:/p", warning: "", dirty_count: 0, dirty: [], kind: "new",
    where: "window", occupied: true, parent_live: false, mid_turn: false};

  it("asks before a worktree, unless Settings says not to", () => {
    const a = ask(base, true);
    assert.deepEqual(a.keys, ["tree", "here", "cancel"]);
    assertIn("git worktree", a.body);
    assertIn("Settings > Worktrees", a.foot);
    assert.equal(ask(base, false), null);
    assert.equal(ask({...base, placement: "here"}, true), null);
  });

  it("a fork from uncommitted work warns even with the question off", () => {
    const plan = {...base, kind: "fork", warning: "dirty", dirty_count: 14, parent_live: true,
      dirty: ["a.rs", "b.rs"]};
    const a = ask(plan, false);
    assert.deepEqual(a.keys, ["copy", "nocontext", "here", "cancel"]);
    assertIn("14 uncommitted file(s)", a.body);
    assertIn("and 12 more", a.body);
    assertIn("you resolve that yourself", a.body);
    assert.equal(a.foot, "");
  });

  it("resume into a worktree says its edits stay behind; not git says the folder is shared", () => {
    const b = ask({...base, kind: "resume", warning: "behind", dirty_count: 1, dirty: ["x"]}, false);
    assert.deepEqual(b.keys, ["tree", "here", "cancel"]);
    assertIn("stay there", b.body);
    const s = ask({...base, placement: "here", warning: "shared"}, true);
    assert.deepEqual(s.keys, ["here", "cancel"]);
    assertIn("not a git repository", s.body);
  });

  it("mid-turn, it says where the context ends", () => {
    const a = ask({...base, kind: "fork", mid_turn: true, parent_live: true}, true);
    assertIn("last finished turn", a.body);
    assertIn("the tab opens when that turn ends", a.body);
  });

  it("the status line says where it went", () => {
    const run = r => evalJson(cat(ASK, "\n", OUT, "\nprocess.stdout.write(JSON.stringify(spawnOutcome(", JSON.stringify(r), ")));"));
    assert.equal(run({where: "background", placement: "worktree", branch: "worktree-x", bg_id: "1f2e"}),
      "started in the background in a new worktree (worktree-x) · 1f2e");
    assertIn("the tab opens when that turn ends", run({where: "window", first_turn: "headless", placement: "here"}));
    assertIn("opened a new session", run({where: "window", kind: "new", placement: "here"}));
  });

  it("the Send menu has the four options; Background and New / Fork are gone", () => {
    const page = renderShell();
    const menu = page.slice(page.indexOf("id=sendmore"), page.indexOf("</details>", page.indexOf("id=sendmore")));
    for (const k of ["new window", "fork window", "new background", "fork background"]) assertIn(`data-spawn="${k}"`, menu);
    for (const label of [">Send to New Session<", ">Send to New Session with Context<",
      ">Send to New Background Session<", ">Send to New Background Session with Context<"]) assertIn(label, menu);
    assertNotIn("id=bg ", page);
    assertNotIn("id=launch", page);
    assertIn("<input type=checkbox id=nbg>", page);
    assertIn("<div id=spawnbox class=popup hidden>", page);
    assertIn('spawnFrom(e.shiftKey ? "fork" : "new", "background")', SCRIPT);
    assertNotIn('send("background")', SCRIPT);
    assertIn("#spawnbox .sbwarn", CSS);
  });
});

// Worktree visibility (2026-11-22): which worktree a session is in, from
// the View menu, the Worktrees tab and History.
describe("WorktreeVisibility", () => {
  const HELP = between(SCRIPT, "const folder = cwd =>", "// What a tile calls a session.");
  const NORM = between(SCRIPT, "// Pure: a folder as the window compares them.", "// Pure: the projects there are sessions");
  const WHO = between(SCRIPT, "// Who a worktree card says works in it:", "async function finishWorktree(");
  const run = expr => evalJson(cat(SHARED, "\n", HELP, "\n", NORM, "\n", WHO,
    "\nconst key = s => s.sessionId; const nameOf = s => s.name;",
    "\nprocess.stdout.write(JSON.stringify(", expr, "));"));

  // Windows paths, quoted for the evaluated code.
  const W = JSON.stringify(String.raw`C:\src\app\.claude\worktrees\fix-it`);
  const X = JSON.stringify(String.raw`C:\r\.claude\worktrees\x`);

  it("knows a managed worktree and names it under its project", () => {
    assert.deepEqual(run(`[managedWorktree(${W}), managedWorktree("C:/src/app"), placeName(${W}), placeName("C:/src/app/"),
      sameDir(${JSON.stringify(String.raw`C:\A\b`)}, "c:/a/B/")]`),
      [{main: "C:/src/app", name: "fix-it"}, null, "app \u00b7 worktree fix-it", "app", true]);
  });

  it("a worktree card names the sessions working in it", () => {
    const got = run(`(globalThis.sessions = [{sessionId: "s1", name: "parser", cwd: ${X}},
      {sessionId: "s2", name: "old", cwd: "C:/r/.claude/worktrees/x", resumable: true}],
      [worktreeWho({agent: true, path: "C:/r/.claude/worktrees/x"}), worktreeWho({agent: true, path: "C:/r/.claude/worktrees/y"}),
       worktreeWho({agent: false, path: "C:/elsewhere"})])`);
    assertIn('data-wpick="s1"', got[0]);
    assertIn(">parser<", got[0]);
    assertNotIn("old", got[0]);
    assert.equal(got[1], "no session running in it");
    assert.equal(got[2], "made by hand - not managed here");
  });

  it("the View menu opens the folder and merges a worktree back; History shows the project", () => {
    const page = renderShell();
    const view = page.slice(page.indexOf("id=viewmenu"), page.indexOf("</details>", page.indexOf("id=viewmenu")));
    assertIn("<button id=editbtn data-act=editor", view);
    assertIn("<button id=mergebtn data-act=mergeback", view);
    assertIn('$("#mergebtn").hidden = !(local && managedWorktree(s.cwd));', SCRIPT);
    assertIn('post("/api/open_path", {path: s.cwd, session_id: key(s), editor: true})', SCRIPT);
    assertIn('if (what === "mergeback") return finishWorktree(s, "merge", s.cwd);', SCRIPT);
    assertIn("${esc(placeName(r.cwd))}", SCRIPT);
    assertNotIn('"background session" : "made by hand', SCRIPT);
  });
});
