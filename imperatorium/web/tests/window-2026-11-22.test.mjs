// Slash commands the window does itself (2026-11-22,
// docs/slash-commands-terminal-gaps.md): pickers for /model, /effort, /fast
// and /output-style from Claude Code's own lists, views for its panels, the
// prompt box's list of the session's commands, and Show Window on a refusal
// only the terminal can act on.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SCRIPT, CSS, between, evalJson, cat, assertIn, renderShell } from "./load.mjs";

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
    assertIn('post("/api/new", {cwd: p.path, permission_mode: mode, model, effort})', SCRIPT);
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
