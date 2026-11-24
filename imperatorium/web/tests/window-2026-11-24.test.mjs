// The window changes of 2026-11-24, from the persona runs'
// recommendations (personas/findings/2026-11-24-all.md): the queue, Send on
// remote sessions, n, the Decision Bar's keys, New session's task box and
// empty state, host badges and the Host Line, worktree questions, History's
// preview, closing, and the cache countdown.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SCRIPT, SHARED, find, evalJson, cat, j, assertIn, assertNotIn, renderShell } from "./load.mjs";

/** A top-level `function name(...) {...}` of the window's script. */
const fn = (name) => {
  const i = find(SCRIPT, "function " + name + "(");
  return SCRIPT.slice(i, find(SCRIPT, "\n}\n", i)) + "\n}\n";
};
const STUBS = cat(
  "const key = s => s.sessionId || s.id; const nameOf = s => s.name || s.id;\n",
  "const plural = (n, word) => n + ' ' + word + (n === 1 ? '' : 's');\n");
/** Runs `expr` with SHARED, the stubs and the named functions defined. */
const run = (names, expr) => evalJson(cat(SHARED, STUBS, ...names.map(fn),
  "\nprocess.stdout.write(JSON.stringify(", expr, "));"));

describe("PromptBox", () => {
  it("is cleared after a send only while it still holds what was sent", () => {
    assert.deepEqual(run(["promptStill"], cat(
      "[promptStill('hello ', 'hello'), promptStill('hello and more', 'hello'),",
      " promptStill('', 'hello'), promptStill('  ', 'x')]")), [true, false, null, null]);
    for (const site of ["clearPrompt(key(s), text); say(r.waiting", "if (fromBox && text) clearPrompt(k, text);"])
      assertIn(site, SCRIPT);
  });
});

describe("QueueList", () => {
  const items = [{id: "a", prompt: "first"}, {id: "b", prompt: "<second>"}, {id: "c", prompt: "third", error: "busy"}];

  it("numbers the prompts in sending order, each with earlier, later and cancel", () => {
    const html = run(["queuedHtml"], `queuedHtml(${j(items)})`);
    assertIn("1. first", html);
    assertIn("2. &lt;second&gt;", html);
    assertIn("3. third - last try: busy", html);
    assertIn('data-qmv="a" data-by="-1" title="send it earlier" disabled', html);
    assertIn('data-qmv="c" data-by="1" title="send it later" disabled', html);
    assertNotIn('data-qmv="b" data-by="-1" title="send it earlier" disabled', html);
    assertIn('data-uq="b"', html);
  });

  it("a lone prompt has only cancel", () => {
    const html = run(["queuedHtml"], `queuedHtml(${j(items.slice(0, 1))})`);
    assertNotIn("data-qmv", html);
    assertIn('data-uq="a"', html);
  });

  it("each change redraws at once and a refusal is said", () => {
    assertIn('"/api/queue/move"', SCRIPT);
    assertIn("if (!r.ok) say(r.error || \"refused\", key(s));", SCRIPT);
    assertIn("await drawQueued(current());", SCRIPT);
  });
});

describe("SendButton", () => {
  const st = (s) => run(["sendState"], `sendState(${j(s)})`);

  it("sends to a remote session it can reach", () => {
    assert.deepEqual(st({id: "r", remote: true, host: "gpu1", dispatchable: true}), {off: false, why: ""});
  });

  it("says why it cannot", () => {
    assert.equal(st(null).off, true);
    assertIn("still starting", st({id: "p", provisional: true}).why);
    assertIn("not reachable yet on gpu1", st({id: "r", remote: true, host: "gpu1"}).why);
    assertIn("gpu1 is not answering", st({id: "r", remote: true, host: "gpu1", unreachable: true, dispatchable: true}).why);
  });

  it("a local session is never held back for want of a socket", () => {
    assert.equal(st({id: "l", dispatchable: false}).off, false);
  });
});

describe("NextWaiting", () => {
  it("goes to blocked sessions first, then failed, then finished, oldest first in each", () => {
    const list = [
      {id: "done-old", attention: true, attention_kind: "complete", attention_since: 1},
      {id: "fail", attention: true, attention_kind: "failure", attention_since: 2},
      {id: "blocked-new", attention: true, attention_since: 9},
      {id: "blocked-old", attention: true, attention_since: 5},
      {id: "quiet"}];
    const order = run(["wantRank", "nextWaiting"], cat(
      "(() => { const l = ", j(list), "; const out = []; let k = null;",
      " for (let i = 0; i < 5; i++){ k = nextWaiting(l, k); out.push(k); } return out; })()"));
    assert.deepEqual(order, ["blocked-old", "blocked-new", "fail", "done-old", "blocked-old"]);
  });
});

describe("DecisionKeys", () => {
  it("A allows and D denies the first permission request, only when one is there", () => {
    assert.deepEqual(run(["decisionKey"], cat(
      "[decisionKey('a', true), decisionKey('d', true), decisionKey('x', true), decisionKey('a', false)]")),
      ["allow", "deny", null, null]);
  });

  it("the stream's pending event reads the requests at once", () => {
    assertIn('es.addEventListener("pending"', SCRIPT);
    assertIn("A allow · D deny", SCRIPT);
  });
});

describe("NewSession", () => {
  it("asks for a task when it starts in the background or on a host", () => {
    assert.deepEqual(run(["taskBoxShown", "newTask"], cat(
      "[taskBoxShown(true, false), taskBoxShown(false, true), taskBoxShown(false, false),",
      " newTask('  fix it '), newTask(null)]")), [true, true, false, "fix it", ""]);
    const page = renderShell();
    assertIn("id=ntask", page);
    assertIn('$("#ntask").value = $("#prompt").value.trim();', SCRIPT);
  });

  it("with no project folders it says where to add one", () => {
    const [empty, typed, some] = run(["noProjectsHtml"], cat(
      "[noProjectsHtml('', []), noProjectsHtml('zz', []), noProjectsHtml('', ['C:/code'])]"));
    assertIn("No project folders yet", empty);
    assertIn("data-setroots", empty);
    assertIn("no match", typed);
    assertIn("no match", some);
  });
});

describe("RemoteMachines", () => {
  it("a session whose machine stops answering says so, with Retry now", () => {
    const [state, html] = run(["span", "unreachableHtml"], cat(
      "[stateOf({unreachable: true, status: 'idle'}),",
      " unreachableHtml({host: 'gpu1', unreachable_error: 'ssh: connect refused', next_try: 1090}, 1000)]"));
    assert.deepEqual(state, ["d-done", "not answering"]);
    assertIn("gpu1 is not answering", html);
    assertIn("next try in", html);
    assertIn("data-retry", html);
    assertIn('title="ssh: connect refused"', html);
  });

  it("the Host Line names each machine that is not answering", () => {
    const status = {gpu1: {fails: 2, error: "refused", polled_at: 900, next_attempt: 1020}, pi: {fails: 0}};
    const html = run(["span", "age", "hostLineHtml"], `hostLineHtml({status: ${j(status)}}, 1000)`);
    assertIn("<b>gpu1</b> is not answering", html);
    assertNotIn("<b>pi</b>", html);
    assertIn("data-retry", html);
    assert.equal(run(["span", "age", "hostLineHtml"], `hostLineHtml({status: {pi: {fails: 0}}}, 1000)`), "");
    assertIn("id=hostline", renderShell());
  });

  it("every remote tile names its machine, and the list groups by machine", () => {
    assertIn('`<span class="hb" title="on ${esc(s.host)}">${esc(s.host)}</span>`', SCRIPT);
    assertIn('if (prefs.group === "host") return s.host || HERE_GROUP;', SCRIPT);
    assertIn("<option value=host>by machine</option>", renderShell());
  });
});

describe("Worktrees", () => {
  const ask = (action, info) => run(["worktreeAsk"], `worktreeAsk(${j(action)}, ${j(info)})`);

  it("a discard says what it loses, once", () => {
    const lose = ask("discard", {ahead: 2, dirty: true, base: "main"});
    assertIn("Its 2 commits not in main and uncommitted changes will be lost.", lose.text);
    assert.equal(lose.force, true);
    const clean = ask("discard", {ahead: 0, dirty: false, base: "main"});
    assert.equal(clean.text, "Remove this worktree and delete its branch?");
    assert.equal(clean.force, false);
  });

  it("merge and remove is one question", () => {
    assertIn("then remove the worktree", ask("mergeremove", {base: "main"}).text);
    assertIn('data-wa="mergeremove"', SCRIPT);
    assertIn('data-pja="mergeremove"', SCRIPT);
  });
});

describe("HistoryPreview", () => {
  it("shows what was said, tool calls counted, without resuming", () => {
    const items = [
      {speaker: "you", blocks: [{kind: "text", text: "fix the <login> test"}]},
      {speaker: "tools", blocks: [{kind: "tool", name: "Bash"}, {kind: "tool", name: "Edit"}]},
      {speaker: "claude", blocks: [{kind: "text", text: "done"}]}];
    const html = run(["previewHtml"], `previewHtml(${j(items)})`);
    assertIn("<b>you</b> fix the &lt;login&gt; test", html);
    assertIn("2 tool calls", html);
    assertIn("<b>Claude</b> done", html);
    assert.ok(html.indexOf("2 tool calls") < html.indexOf("<b>Claude</b>"));
    assertIn("nothing said in it", run(["previewHtml"], "previewHtml([])"));
  });

  it("History closes once a resume starts", () => {
    assertIn('if (r && r.ok){ openPopup("history", false); say(spawnOutcome(r)); }', SCRIPT);
  });
});

describe("Closing", () => {
  it("asks only when it would interrupt a turn", () => {
    const [idle, busy] = run(["closeAsk"], cat(
      "[closeAsk({id: 'a', status: 'idle'}), closeAsk({id: 'b', name: 'api', status: 'busy'})]"));
    assert.equal(idle, null);
    assertIn("Close api? It is working right now", busy);
  });
});

describe("PopupsAndPanels", () => {
  it("Esc closes the open pop-up wherever focus is", () => {
    assertIn("function openPopupName()", SCRIPT);
    assertIn("e.preventDefault(); e.stopImmediatePropagation();", SCRIPT);
  });

  it("the Changes tab drops another session's diff on a switch", () => {
    assertIn("if (changesFor !== key(s)){", SCRIPT);
  });

  it("Insights has one range for both cards", () => {
    const page = renderShell();
    for (const id of ["cdays", "days"])
      assertIn(`<option value=90>last 90 days</option>`, page.slice(page.indexOf(`<select id=${id}`)).slice(0, 400));
    assertIn('$("#cdays").value = $("#days").value; loadInsights();', SCRIPT);
  });
});
