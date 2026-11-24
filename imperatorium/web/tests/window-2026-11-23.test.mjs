// The window changes of 2026-11-23 (user request): Settings > Remote machines
// adds a machine and tests a host's connection, so `imperatorium remote add`
// is no longer needed from a terminal.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SETTINGS_SCRIPT, ESC, between, evalJson, cat, assertIn, assertNotIn } from "./load.mjs";

describe("RemoteMachines", () => {
  const body = () => between(SETTINGS_SCRIPT, "// Each host with how its last poll went", "function hostState(");
  const run = (code) => evalJson(cat(ESC, "\n", body(), "\n", code));

  it("lists each host with a Test button, and the add form even with none", () => {
    const [some, none] = run(cat(
      "process.stdout.write(JSON.stringify([",
      " hostsHtml([{name: 'me@pi', label: 'pi', tunnel: true}]),",
      " hostsHtml([])]));"));
    assertIn('data-test="me@pi"', some);
    assertIn('data-res="me@pi"', some);
    assertIn('id="rrefresh"', some);
    assertIn('id="radd-host"', some);
    assertIn("No remote machines yet", none);
    assertIn('id="radd-go"', none);
    assertIn('id="radd-test"', none);
    assertNotIn('id="rrefresh"', none);
  });

  it("a test says what is there and what is missing, escaped", () => {
    const [good, missing, down] = run(cat(
      "process.stdout.write(JSON.stringify([",
      " testHtml({ok: true, reachable: true, rows: [{mark: 'ok', what: 'ssh', detail: 'key login works'}]}),",
      " testHtml({ok: false, reachable: true, configured: false, rows: [",
      "   {mark: '--', what: 'binary', detail: 'not there yet'},",
      "   {mark: 'MISSING', what: 'hooks', detail: '<b>not installed</b>'}]}),",
      " testHtml({ok: false, reachable: false, hint: 'set up key login', rows: [",
      "   {mark: 'MISSING', what: 'ssh', detail: 'Permission denied (publickey)'}]})]));"));
    assertIn("Everything it needs is there", good);
    assertIn('<li class="ok"><b>ok</b> ssh: key login works</li>', good);
    assertIn("something is missing", missing);
    assertIn('<li class="na"><b>not yet</b> binary', missing);
    assertIn("&lt;b&gt;not installed&lt;/b&gt;", missing);
    assertIn("Add sets up what it can", missing);
    assertIn("Cannot log in", down);
    assertIn("set up key login", down);
    assertNotIn("Add sets up", down);
  });

  it("a refused login offers key setup, and the command to run comes escaped", () => {
    const [locked, down, made, had, failed] = run(cat(
      "process.stdout.write(JSON.stringify([",
      " testHtml({ok: false, reachable: false, auth: true, host: 'me@pi', rows: [{mark: 'MISSING', what: 'ssh', detail: 'Permission denied'}]}),",
      " testHtml({ok: false, reachable: false, auth: false, host: 'me@pi', rows: [{mark: 'MISSING', what: 'ssh', detail: 'timed out'}]}),",
      " keyHtml({ok: true, made: true, key: 'C:/u/.ssh/id_ed25519.pub', command: 'type \"k\" | ssh me@pi \"cat >> x\"'}, 'me@pi'),",
      " keyHtml({ok: true, made: false, key: 'k', command: 'c'}, 'me@pi'),",
      " keyHtml({ok: false, error: 'ssh-keygen failed'}, 'me@pi')]));"));
    assertIn('data-keyfor="me@pi"', locked);
    assertNotIn("data-keyfor", down);
    assertIn("Made a key for this computer", made);
    assertIn("me@pi&#39;s password", made.replace(/'/g, "&#39;"));
    assertIn('<pre class="rcmd">type "k" | ssh me@pi "cat &gt;&gt; x"</pre>', made);
    assertIn("data-copy", made);
    assertNotIn("Made a key", had);
    assertIn("ssh-keygen failed", failed);
  });

  it("the add form offers to install Claude Code, off by default", () => {
    const html = run("process.stdout.write(JSON.stringify(hostsHtml([])));");
    assertIn('<input type=checkbox id="radd-claude">', html);
  });
});
