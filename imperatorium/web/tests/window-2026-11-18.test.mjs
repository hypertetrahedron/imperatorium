// The window changes of 2026-11-18 (peer requests): the tile's "hook errors"
// chip opens a pop-up saying what went wrong and what to do, and goes once
// the Stop hooks run cleanly again.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SCRIPT, CSS, ESC, between, evalJson, cat, assertIn, assertNotIn, renderShell } from "./load.mjs";

describe("HookErrorsPopup", () => {
  const body = () => between(SCRIPT, "// -- hook errors --", "function hooksRow(){");
  const run = (code) => evalJson(cat(
    ESC, "\n",
    "const clock = t => 't' + t;\n",
    body(), "\n", code));
  const row = (extra) => JSON.stringify(Object.assign({kind: "hooks", errors: [
    {at: 100, event: "Stop", error: "'lint-hook' is not recognized as an internal or external command", hooks: ["lint-hook"]},
    {at: 200, event: "Stop", error: "Failed with exit code 1: boom", hooks: ["lint-hook", "imperatorium"]},
  ]}, extra || {}));

  it("the errors since the hooks last ran cleanly are the current ones", () => {
    const got = run(cat(
      "const h = ", row(), ";\n",
      "process.stdout.write(JSON.stringify([",
      " hookErrorsNow(h).length,",
      " hookErrorsNow(Object.assign({}, h, {ok_at: 150})).map(e => e.at),",
      " hookErrorsNow(Object.assign({}, h, {ok_at: 250})).length,",
      " hookErrorsNow(null).length]));"));
    assert.deepEqual(got, [2, [200], 0, 0]);
  });

  it("says what kind of failure each is, and plain steps to fix it", () => {
    const got = run(cat(
      "const h = ", row(), ";\n",
      "process.stdout.write(JSON.stringify([",
      " hookKind('bash: foo: command not found').what,",
      " hookKind('Hook timed out after 60s').what,",
      " hookKind('connect ECONNREFUSED 127.0.0.1:8792').what,",
      " hookKind('Failed with exit code 2').what,",
      " hookKind('something odd').what,",
      " hookHelpHtml(h, {cwd: 'C:/w/proj'})]));"));
    assert.equal(got[0], "The hook's program could not be found");
    assert.equal(got[1], "The hook took too long");
    assert.equal(got[2], "The hook could not reach a service");
    assert.equal(got[3], "The hook ran and failed");
    assert.equal(got[4], "The hook reported an error");
    const html = got[5];
    assertIn("<b>The hook&#39;s program could not be found.</b>", html.replace(/'/g, "&#39;"));
    assertIn("<code>/hooks</code>", html);
    assertIn("<code>C:/w/proj</code>", html);
    assertIn("<code>~/.claude/settings.json</code>", html);
    assertIn("<li><code>lint-hook</code></li><li><code>imperatorium</code></li>", html);
    // one of them is its own hook
    assertIn("<code>imperatorium status</code>", html);
    assertNotIn('class="old"', html);
  });

  // 2026-11-21 (reports: "python3 command was not found"): a plugin's hook
  // that runs python3 on a machine without it was read as Imperatorium's.
  it("names the hook a missing program came from, and says it is not ours", () => {
    const got = run(cat(
      "process.stdout.write(JSON.stringify([",
      " missingProgram('/usr/bin/bash: line 1: python3: command not found'),",
      " missingProgram('/bin/sh: 1: python3: not found'),",
      " missingProgram(\"'python3' is not recognized as an internal or external command\"),",
      " missingProgram(\"The term 'C:\\\\x\\\\node.exe' is not recognized\"),",
      " missingProgram('Python was not found; run without arguments to install from the Microsoft Store'),",
      " missingProgram('Failed with exit code 1'),",
      " failingHook('python3: command not found', ['\"C:/p/imperatorium.exe\" hook', 'python3 \"${CLAUDE_PLUGIN_ROOT}/hooks/stop.py\"']),",
      " failingHook('Python was not found', ['python \"x.py\"']),",
      " failingHook('boom', ['python3 x.py']),",
      " hookHelpHtml({kind: 'hooks', errors: [{at: 5, event: 'Stop', error: 'line 1: python3: command not found',",
      "   hooks: ['\"C:/p/imperatorium.exe\" hook', 'python3 \"${CLAUDE_PLUGIN_ROOT}/hooks/stop.py\"']}]}, {})]));"));
    assert.deepEqual(got.slice(0, 6), ["python3", "python3", "python3", "node.exe", "python", null]);
    assert.equal(got[6], 'python3 "${CLAUDE_PLUGIN_ROOT}/hooks/stop.py"');
    assert.equal(got[7], 'python "x.py"');
    assert.equal(got[8], null);
    assertIn("not one of Imperatorium's", got[9]);
    assertIn("(not an Imperatorium hook)", got[9]);
    assertNotIn("Imperatorium's own hook", got[9]);
  });
  it("errors from before a clean run are shown as earlier ones; none at all says so", () => {
    const got = run(cat(
      "process.stdout.write(JSON.stringify([",
      " hookHelpHtml(", row({ok_at: 300}), ", {}),",
      " hookHelpHtml({kind: 'hooks', errors: []}, {}),",
      " hookHelpHtml(", JSON.stringify({kind: "hooks", errors: [{at: 1, event: "Stop", error: "x", hooks: ["npx lint"]}]}), ", {})]));"));
    assertIn("ran without an error at t300", got[0]);
    assertIn('<li class="old">', got[0]);
    assertNotIn("class=hkkind", got[0]);
    assertIn("No hook errors recorded", got[1]);
    assertNotIn("imperatorium status", got[2]);
  });

  it("the chip is a button of its own beside the Tasks header, and it and the row open the pop-up", () => {
    assertIn('<button type=button class="st bad hkchip" data-hookerr', SCRIPT);
    assertIn("hookErrorsNow(hooks).length ?", SCRIPT);
    assertIn('if (t.closest("[data-hookerr]")) return openHookHelp();', SCRIPT);
    assertIn('item.dataset.task === "hooks" ? openHookHelp() : openTask(item.dataset.task)', SCRIPT);
    assertIn('hooks: "#hookbox"', SCRIPT);
    assertIn('!$("#projbox").hidden || !$("#hookbox").hidden) return;', SCRIPT);
    const html = renderShell();
    assertIn("<div id=hookbox class=popup hidden>", html);
    assertIn("<div id=hookpane class=panel></div>", html);
    assertIn(".s .tkhr .tkh{flex:1;min-width:0}", CSS);
  });
});
