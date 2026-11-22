// Fleet (2026-11-20; docs/research-super-agent.md): Ask about every session,
// the watches the dispatcher checks, and the proposals an agent or Ask
// makes, which only this page confirms. Since 2026-11-21 it is the first row
// of the session list, shown where the chat goes, with the watches folded at
// the top and the prompt box asking.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SCRIPT, CSS, ESC, between, evalJson, cat, assertIn, assertNotIn, renderShell } from "./load.mjs";

const FLEET = between(SCRIPT, "function sessionByKey(", "async function loadFleet(");
const STUBS = cat(ESC, "\nconst attr = s => esc(s).replace(/\"/g, \"&quot;\");\n",
  "const pkey = s => s.key || s.sessionId, key = s => s.sessionId, nameOf = s => s.display_name || s.name;\n",
  "const age = t => t ? '3m' : '';\n",
  "const SPEAKER = {you: 'you'};\n",
  "let sessions = [{key: 'k1', sessionId: 'k1', name: 'gem-trip-4', project: 'gem-trip'}];\n",
  "let fleetBook = null, fleetFired = null, deskNotify = false, view = 'fleet';\n",
  "const WATCH_NUM = {waiting: ['minutes', '10'], spend: ['dollars', '5']};\n");

describe("FleetView", () => {
  it("is the first row of the session list, not a pop-up in the menu", () => {
    const html = renderShell();
    assertIn("<div id=fleetrow role=button tabindex=0 ", html);
    assert.ok(html.indexOf("id=fleetrow") < html.indexOf("<div id=sessions>"), "above the sessions");
    assertNotIn("data-view=fleet", html);
    assertNotIn("id=fleetbox", html);
    assertNotIn('fleet: "#fleetbox"', SCRIPT);
    assertIn('$("#fleetrow").onclick = openFleetView;', SCRIPT);
    assertIn("#fleetrow.on{", CSS);
  });

  it("shows where the chat goes: watches folded at the top, then what waits, then Ask", () => {
    const html = renderShell();
    const v = html.slice(html.indexOf("<div id=fleetview hidden>"));
    for (const id of ["flwfold", "flwatch", "wtarget", "wkind", "wnum", "wadd", "flprop", "flfired", "flchat"])
      assertIn("id=" + id, v);
    assert.ok(v.indexOf("<details id=flwfold>") < v.indexOf("id=flprop"), "watches first, collapsible");
    assert.ok(v.indexOf("id=flprop") < v.indexOf("id=flchat"));
    assert.ok(html.indexOf("<div id=sessionpane>") < html.indexOf("<div id=fleetview hidden>"));
    assertNotIn("id=askq", html);
  });

  it("asks with the normal prompt box", () => {
    assertIn('if (view === "fleet"){ if (mode !== "background") askFleet(); return; }', SCRIPT);
    assertIn('const q = $("#prompt").value.trim();\n  if (!q || fleetAsking) return;', SCRIPT);
    assertIn('post("/api/ask", {question: q}, undefined, 200000)', SCRIPT);
    assertIn('const k = view === "fleet" ? "fleet" : selected;', SCRIPT, "Fleet keeps its own draft");
    assertIn("#app.fleet #queue,", CSS, "Queue and Background do not apply");
  });

  it("leaves Fleet for a session through select()", () => {
    assertIn("  closeTask();\n  select(s.dataset.k);\n});", SCRIPT);
    assertIn('if (v === "fleet") openFleetView(); else leaveFleet();', SCRIPT);
  });

  it("says on its row what waits for you and how many watches are on", () => {
    const got = evalJson(cat(STUBS, FLEET, "\nprocess.stdout.write(JSON.stringify([fleetRowText(null),",
      "fleetRowText({proposed: 2, watches: [{status: 'active'}, {status: 'paused'}, {status: 'proposed'}]}),",
      "fleetRowText({proposed: 0, watches: [{status: 'active'}, {status: 'active'}]})]));"));
    assert.deepEqual(got[0], {sum: "", sub: "ask about every session", attn: false});
    assert.deepEqual(got[1], {sum: "2 to confirm", sub: "1 watch on", attn: true});
    assert.deepEqual(got[2], {sum: "", sub: "2 watches on", attn: false});
  });

  it("shows an Ask exchange as two chat turns", () => {
    const got = evalJson(cat(STUBS, FLEET, "\nprocess.stdout.write(JSON.stringify([askTurnHtml({q: '<who>?', html: ''}), askTurnHtml({q: 'x', html: '<b>ok</b>'})]));"));
    assertIn('<div class="turn you-turn"><div class="who you">you</div><div class=txt>&lt;who&gt;?</div></div>', got[0]);
    assertIn("asking - a model reads a snapshot", got[0]);
    assertIn('<div class="who claude">Ask</div><b>ok</b>', got[1]);
  });

  it("turns Ask's [[session:KEY]] into links that open the session", () => {
    const got = evalJson(cat(STUBS, FLEET,
      "\nprocess.stdout.write(JSON.stringify([citeHtml('see [[session:k1]] and [[session:zzzzzzzzzz]]'), citeHtml('[[session:gem-trip-4]]')]));"));
    assert.equal(got[0], 'see <a href="#" class=cite data-cite="k1">gem-trip-4</a> and zzzzzzzzzz', "an unknown one is plain text, whole");
    assert.equal(got[1], '<a href="#" class=cite data-cite="k1">gem-trip-4</a>', "cited by name, it still opens the session");
  });

  it("shows a proposal with its prompt, its source, and Confirm / Decline", () => {
    const got = evalJson(cat(STUBS, FLEET,
      "\nprocess.stdout.write(JSON.stringify([",
      "proposalHtml({id: 'a1', text: 'queue a prompt for gem-trip-4', prompt: '<b>commit</b>', source: 'session 1234 (MCP)', reason: 'tests pass'}, 'action'),",
      "proposalHtml({id: 'w1', text: 'when gem-trip-4 finishes its work', source: 'ask'}, 'watch')]));"));
    assertIn('data-fl="action:confirm:a1">Run it', got[0]);
    assertIn('data-fl="action:decline:a1">Decline', got[0]);
    assertIn("&lt;b&gt;commit&lt;/b&gt;", got[0], "the prompt is shown, escaped");
    assertIn("proposed by session 1234 (MCP)", got[0]);
    assertIn("tests pass", got[0]);
    assertIn('data-fl="watch:confirm:w1">Watch', got[1]);
    assertIn('data-fl="watch:remove:w1">Decline', got[1], "declining a proposed watch removes it");
  });

  it("offers every session, every project, or one of each as a watch's target", () => {
    const got = evalJson(cat(STUBS, FLEET, "\nprocess.stdout.write(JSON.stringify(fleetTargets()));"));
    assertIn('<option value="">every session</option>', got);
    assertIn('<option value="p:gem-trip">project gem-trip</option>', got);
    assertIn('<option value="s:k1">gem-trip-4</option>', got);
  });

  it("is polled with the tick", () => {
    assertIn("if (ticks % 2 === 1) loadFleet();", SCRIPT);
  });
});
