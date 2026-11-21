// The window keeps going when a request never answers (2026-11-21): across a
// dispatcher restart one hung fetch froze the refresh loop, Send and the
// new-dispatcher check, and hung requests filled the browser's six
// connections to the host so even F5 queued. Every request now times out,
// and a stream that goes quiet past its pings is opened again.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SHARED, SCRIPT, between, evalJson, evalJsLater, cat, assertIn } from "./load.mjs";

const TIMED = between(SHARED, "const GET_MS =", "\n\n// Which dispatcher served this page.");
// A fetch that answers only by being aborted, and a minimal AbortController.
const STUB = cat(
  "class AbortController { constructor(){ const s = this.signal = {aborted: false, fns: []};",
  " s.addEventListener = (k, f) => s.fns.push(f); } abort(){ this.signal.aborted = true; this.signal.fns.forEach(f => f()); } }\n",
  "let calls = [];\n",
  "const fetch = (url, opts) => { calls.push([url, !!opts.signal]); return new Promise((res, rej) => opts.signal.addEventListener('abort', () => rej(new Error('aborted')))); };\n");

describe("RequestTimeouts", () => {
  it("a GET that never answers gives up, marked as the dispatcher not answering", async () => {
    const got = await evalJsLater(cat(STUB, TIMED, "\nconst document = {querySelector: () => null};\n",
      "get('/api/sessions', 30).then(() => process.stdout.write('answered'),",
      " e => process.stdout.write(JSON.stringify({msg: e.message, down: !!e.down, calls})));"));
    assert.deepEqual(JSON.parse(got), {msg: "the dispatcher did not answer", down: true, calls: [["/api/sessions", true]]});
  });

  it("so does a POST, and Ask waits longer than the rest", async () => {
    const got = await evalJsLater(cat(STUB, TIMED, "\nconst document = {querySelector: () => null};\n",
      "post('/api/dispatch', {}, undefined, 30).catch(e => process.stdout.write(String(e.down)));"));
    assert.equal(got, "true");
    assertIn("const GET_MS = 15000, POST_MS = 60000;", SHARED);
    assertIn('post("/api/ask", {question: q}, undefined, 200000)', SCRIPT, "Ask may take up to 180 s");
  });

  it("a send that fails frees Send, and says so", () => {
    assertIn("} catch (e) { r = {ok: false, error: \"the dispatcher did not answer\"}; }\n  busy = false;", SCRIPT);
  });
});

describe("StreamWatchdog", () => {
  it("gives up on a stream not heard from for three pings", () => {
    const got = evalJson(cat(between(SCRIPT, "const STREAM_QUIET =", "\nfunction openStream("),
      between(SCRIPT, "// Pure: whether a stream last heard from", "\nfunction watchStream("),
      "\nprocess.stdout.write(JSON.stringify([streamStale(true, 0, 44000), streamStale(true, 0, 46000), streamStale(false, 0, 99999)]));"));
    assert.deepEqual(got, [false, true, false]);
  });

  it("hears the dispatcher's ping, and is checked every tick", () => {
    assertIn('es.addEventListener("ping", heard);', SCRIPT);
    assertIn("async function tick(){\n  watchStream();", SCRIPT);
    assertIn("if (e && e.down) noteDown(true);", SCRIPT, "says when the dispatcher is not answering");
  });
});
