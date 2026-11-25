// Drives the real window (headless Chrome via Playwright) as a persona
// would, and measures each step:
//
// - interactions: clicks, key presses, menu opens, dialogs met, text typed
// - system wait: time spent waiting for the window to show the outcome
// - estimated human time: Keystroke-Level Model operators (Card, Moran &
//   Newell): P point 1.1 s, B button 0.2 s, K key 0.28 s, H home hands
//   between mouse and keyboard 0.4 s, M mental preparation 1.35 s (charged
//   for each decision: a menu choice, a dialog, finding something) - plus
//   the system wait, which the person sits through. Content the persona
//   types (a prompt's words) is counted apart, as it is not friction.
// - HTTP: every request the page made in the step, with status and time
// - errors: console errors, page errors, failed requests, timeouts
import fs from "node:fs";
import path from "node:path";

const KLM = { P: 1.1, B: 0.2, K: 0.28, H: 0.4, M: 1.35 };
const FORBIDDEN = ["#showbtn", "#editbtn", "[data-pact]", "#mic"]; // would reach the desktop

export class Driver {
  constructor(page, { sb, fleet, outDir, persona }) {
    Object.assign(this, { page, sb, fleet, outDir, persona });
    this.steps = [];
    this.cur = null;
    this.hand = "mouse";
    this.allRequests = [];
    this.consoleErrors = [];
    const inflight = new Map();
    page.on("request", r => inflight.set(r, Date.now()));
    const done = (r, failed) => {
      const t0 = inflight.get(r); inflight.delete(r);
      const u = new URL(r.url());
      if (!u.pathname.startsWith("/api") && u.pathname !== "/hook") return;
      const rec = { path: u.pathname, method: r.method(), ms: t0 ? Date.now() - t0 : null, failed, step: this.cur?.name ?? "(between)" };
      r.response().then(async resp => {
        rec.status = resp?.status();
        // A refusal the window may or may not show: {ok: false, error}.
        if (r.method() === "POST" && resp) {
          const j = await resp.json().catch(() => null);
          if (j && j.ok === false) { rec.refused = String(j.error || j.reason || "ok: false").slice(0, 200); }
        }
      }).catch(() => {}).finally(() => {
        if (rec.refused && this.cur) this.cur.notes.push(`refused by the dispatcher: ${rec.method} ${rec.path}: ${rec.refused}`);
        this.allRequests.push(rec);
        if (this.cur) this.cur.requests.push(rec);
        if ((rec.status >= 400 || failed) && this.cur && u.pathname !== "/api/stream") this.cur.errors.push(`HTTP ${rec.status || "failed"} ${rec.method} ${rec.path}`);
      });
    };
    page.on("requestfinished", r => done(r, false));
    page.on("requestfailed", r => { if (!/stream/.test(r.url())) done(r, true); });
    page.on("console", m => { if (m.type() === "error") { this.consoleErrors.push(m.text()); this.cur?.errors.push("console: " + m.text().slice(0, 200)); } });
    page.on("pageerror", e => { this.consoleErrors.push(String(e)); this.cur?.errors.push("pageerror: " + String(e).slice(0, 200)); });
    page.on("dialog", async dlg => {
      if (this.cur) { this.cur.dialogs.push(`${dlg.type()}: ${dlg.message().slice(0, 140)}`); this.op("M"); this.op("P"); this.op("B"); }
      await dlg.accept().catch(() => {});
    });
  }

  op(o, n = 1) { if (this.cur) { this.cur.klm += KLM[o] * n; this.cur.ops[o] = (this.cur.ops[o] || 0) + n; } }
  toMouse() { if (this.hand !== "mouse") { this.op("H"); this.hand = "mouse"; } }
  toKeys() { if (this.hand !== "keys") { this.op("H"); this.hand = "keys"; } }

  async step(name, goal, fn, { optional = false } = {}) {
    const s = this.cur = { name, goal, t0: Date.now(), clicks: 0, keys: 0, typed: 0, menus: 0, dialogs: [], klm: 0, ops: {},
      waitMs: 0, waits: [], requests: [], errors: [], notes: [], observations: {}, outcome: "ok" };
    try { await fn(); }
    catch (e) {
      s.outcome = optional ? "skipped" : "failed";
      s.errors.push("step: " + String(e.message || e).split("\n")[0].slice(0, 300));
      const shot = path.join(this.outDir, `${this.persona}-${this.steps.length + 1}-fail.png`);
      await this.page.screenshot({ path: shot }).catch(() => {});
      for (const [i, pg] of this.page.context().pages().entries()) if (pg !== this.page) await pg.screenshot({ path: shot.replace(".png", `-popup${i}.png`) }).catch(() => {});
      s.screenshot = path.basename(shot);
      await this.page.keyboard.press("Escape").catch(() => {});
    }
    s.wallMs = Date.now() - s.t0;
    s.humanSec = +(s.klm + s.waitMs / 1000).toFixed(2);
    delete s.t0;
    this.steps.push(s);
    this.cur = null;
    return s;
  }

  note(text) { this.cur?.notes.push(text); }
  observe(k, v) { if (this.cur) this.cur.observations[k] = v; }

  guard(sel) { if (FORBIDDEN.some(f => sel.includes(f))) throw new Error(`refusing ${sel}: it would reach the real desktop`); }

  /** A click on something the persona has to find on screen. `decide`: it is a choice (M). */
  async click(sel, { decide = false, nth, timeout = 8000 } = {}) {
    this.guard(sel);
    this.toMouse();
    if (decide) this.op("M");
    this.op("P"); this.op("B");
    this.cur && this.cur.clicks++;
    const loc = nth !== undefined ? this.page.locator(sel).nth(nth) : this.page.locator(sel).first();
    await loc.click({ timeout });
  }

  /** Opens a <details> menu and picks an item: two clicks, one decision. */
  async menu(summarySel, itemSel) {
    this.cur && this.cur.menus++;
    await this.click(summarySel + " > summary", { decide: true });
    await this.click(itemSel);
  }

  async press(keys, { decide = false, shortcut = false } = {}) {
    this.toKeys();
    if (decide || shortcut) this.op("M");
    const n = keys.split("+").length;
    this.op("K", n);
    this.cur && (this.cur.keys += n);
    await this.page.keyboard.press(keys);
  }

  /** Types into a field. Content (a prompt) is counted apart; anything else is K per char. */
  async type(sel, text, { content = false, clear = true } = {}) {
    this.toKeys();
    const loc = this.page.locator(sel).first();
    if (clear) await loc.fill("");
    await loc.click();
    await loc.pressSequentially(text, { delay: 0 });
    if (this.cur) {
      if (content) this.cur.typed += text.length;
      else { this.op("K", text.length); this.cur.keys += text.length; }
    }
  }

  async select(sel, value) {
    this.toMouse();
    this.op("M"); this.op("P", 2); this.op("B", 2);
    this.cur && (this.cur.clicks += 2);
    await this.page.selectOption(sel, value);
  }

  /** Waits for the window to show something; the time is system wait. */
  async waitFor(label, what, { timeout = 30000 } = {}) {
    const t0 = Date.now();
    try {
      if (typeof what === "string") await this.page.locator(what).first().waitFor({ state: "visible", timeout });
      else await this.page.waitForFunction(what.fn, what.arg, { timeout, polling: 100 });
    } catch (e) {
      const ms = Date.now() - t0;
      if (this.cur) { this.cur.waitMs += ms; this.cur.waits.push({ label, ms, timedOut: true }); }
      throw new Error(`timed out after ${ms} ms waiting for: ${label}`);
    }
    const ms = Date.now() - t0;
    if (this.cur) { this.cur.waitMs += ms; this.cur.waits.push({ label, ms }); }
    return ms;
  }

  /** Waits for the simulated fleet to see something (no human cost unless `sits`). */
  async fleetEvent(label, pred, { since = 0, timeout = 30000, sits = true } = {}) {
    const t0 = Date.now();
    while (Date.now() - t0 < timeout) {
      const e = this.fleet.events.find(e => e.t >= since && pred(e));
      if (e) {
        const ms = Date.now() - t0;
        if (sits && this.cur) { this.cur.waitMs += ms; this.cur.waits.push({ label, ms }); }
        return e;
      }
      await new Promise(r => setTimeout(r, 50));
    }
    if (this.cur) this.cur.waits.push({ label, ms: timeout, timedOut: true });
    throw new Error(`timed out after ${timeout} ms waiting for: ${label}`);
  }

  async pause(ms) { await this.page.waitForTimeout(ms); }

  tile(sid) { return `#sessions .s[data-k="${sid}"]`; }

  async selectSession(sid, { find = true } = {}) {
    if (find) this.op("M"); // scanning the list for it
    await this.click(this.tile(sid));
    await this.waitFor("its tile is open and its chat loaded", { fn: sid => !!document.querySelector(`#sessions .s.on[data-k="${sid}"]`) &&
      (document.querySelector("#log")?.offsetParent === null || !/loading/i.test(document.querySelector("#log")?.innerText.slice(0, 40) || "")), arg: sid }, { timeout: 8000 });
  }

  /** Types a prompt and sends it to the selected session; measures each stage of the round trip. */
  async send(sid, text, { via = "button", waitReply = true } = {}) {
    await this.type("#prompt", text, { content: true });
    const t0 = Date.now();
    if (via === "enter") await this.press("Enter"); else await this.click("#send");
    const lat = {};
    const d = await this.fleetEvent("prompt reaches the session", e => e.kind === "delivered" && (!sid || e.sid === sid), { since: t0, timeout: 30000, sits: false });
    lat.deliverMs = d.t - t0;
    await this.waitFor("prompt shows in the chat as sent", { fn: t => [...document.querySelectorAll("#log .turn.you-turn, #log .turn")].some(x => !x.classList.contains("pend") && x.innerText.includes(t)), arg: text.slice(0, 30) }, { timeout: 20000 })
      .then(() => { lat.echoMs = Date.now() - t0; }).catch(e => this.note("the sent prompt never left the 'sending' state: " + e.message));
    if (waitReply) {
      const end = await this.fleetEvent("the turn ends", e => e.kind === "turn-end" && (!sid || e.sid === sid), { since: t0, timeout: 60000, sits: false });
      lat.turnEndMs = end.t - t0;
      const before = Date.now();
      await this.waitFor("the reply shows", { fn: () => !document.querySelector("#log .turn.pend") && /CLAUDE|keeps now/.test(document.querySelector("#log")?.innerText.slice(-600) || ""), arg: null }, { timeout: 20000 })
        .catch(e => this.note(e.message));
      lat.replyShownAfterTurnEndMs = Date.now() - Math.max(before, end.t);
    }
    for (const [k, v] of Object.entries(lat)) this.observe(k, v);
    return lat;
  }

  async shot(name) {
    const p = path.join(this.outDir, `${this.persona}-${name}.png`);
    await this.page.screenshot({ path: p }).catch(() => {});
    return path.basename(p);
  }
}
