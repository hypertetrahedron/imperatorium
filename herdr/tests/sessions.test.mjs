import { order, meterRows, span, ago } from "../mods/imperatorium-sessions/sessions.js";
import assert from "node:assert/strict";
const rows = order([
  { short_name: "b", status: "idle", pid: 1 },
  { short_name: "a", status: "busy", pid: 2 },
  { short_name: "z", attention: true, attention_since: 20 },
  { short_name: "y", attention: true, attention_since: 10 },
  { short_name: "c" },
]);
assert.deepEqual(rows.map((r) => r.short_name), ["y", "z", "a", "b", "c"]);
const m = meterRows({ limits: { five_hour: { used: 42.4, resets_at: 1000 + 5400 }, seven_day: { used: 5 } } }, 1000);
assert.deepEqual(m.map((x) => [x.label, x.pct, x.text]), [["5h", 42.4, "42% · resets in 1h30"], ["7d", 5, "5%"]]);
assert.deepEqual(meterRows(null, 0), []);
assert.equal(span(59), "1m"); assert.equal(span(90061), "1d1h");
assert.equal(ago(null, 5), ""); assert.equal(ago(100, 130), "now"); assert.equal(ago(100, 400), "5m");
console.log("sessions.js: all ok");
import { tabNames } from "../mods/imperatorium-sessions/sessions.js";
assert.deepEqual(tabNames([{ tabs: [
  { id: "imp-a255c025-5fa", named: false },
  { id: "imp-de8a36cc-a31", named: true },
  { id: "tb123", named: false },
  { id: "imp-ffffffffffff", named: false },
] }], [{ sessionId: "a255c025-5faa-4227", display_name: "fix the parser" },
       { sessionId: "de8a36cc-a31b-49a4", name: "x" }]), [["imp-a255c025-5fa", "fix the parser"]]);
console.log("tabNames: ok");
import { chatCommands, chatTabName } from "../mods/imperatorium-sessions/sessions.js";
{
  const s = { sessionId: "S1", hc: { address: "t7" }, display_name: "fix bug", cwd: "C:\\src\\x" };
  const panes = [{ pane: "p1", terminal: "t7", tab: "tb1" }];
  assert.deepEqual(chatCommands(s, panes, []), [
    ["pane.focus", { terminal: "t7" }],
    ["pane.split", { direction: "right", pane: "p1", mod: "imperatorium-chat", props: { session: "S1" } }]]);
  // Already has its chat: just go to the terminal.
  assert.deepEqual(chatCommands(s, panes.concat([{ pane: "p2", terminal: null, mod: "imperatorium-chat", tab: "tb1" }]), []),
    [["pane.focus", { terminal: "t7" }]]);
  // Not in herdr: a chat tab in its folder's space, then back to it.
  const w = { sessionId: "S2", display_name: "remote one", cwd: "c:/src/x/" };
  const spaces = [{ id: "sp0", folder: "D:/y", tabs: [] }, { id: "sp1", folder: "C:\\src\\x", tabs: [] }];
  assert.deepEqual(chatCommands(w, panes, spaces), [["tab.new",
    { name: "Chat · remote one", mod: "imperatorium-chat", props: { session: "S2" }, space: "sp1" }]]);
  spaces[0].tabs.push({ id: "tb9", name: chatTabName(w) });
  assert.deepEqual(chatCommands(w, panes, spaces), [["space.select", { id: "sp0" }], ["tab.select", { id: "tb9" }]]);
  // A remote session's address is its host's, never a local terminal.
  assert.deepEqual(chatCommands({ ...w, remote: true, hc: { address: "t7" } }, panes, spaces)[0][0], "space.select");
  assert.deepEqual(chatCommands(w, panes, []), []);
}
console.log("sessions.js chat panes: ok");
