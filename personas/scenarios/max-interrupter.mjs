// Max: changes his mind mid-turn. Stops and redirects, queues several
// prompts and wants them in another order, forks a session mid-turn into a
// new one with its context, then forks the fork.
import { makeProject } from "../lib/sandbox.mjs";
import { newSession, tileShows } from "./common.mjs";

export default {
  id: "max",
  name: "Max, interrupt-heavy multitasker",
  set: 3,
  fleet: { workMs: [8000, 12000] },
  projects(sb) { this.game = makeProject(sb.projects, "game-engine", { files: 8, commits: 4 }); makeProject(sb.projects, "tools"); },
  async setup({ fleet }) { this.s = await fleet.addInteractive(this.game, { history: 10, name: "engine" }); },

  async run({ d, fleet, page }) {
    const s = this.s, sid = s.sid;
    await d.step("stop and redirect", "it is going the wrong way: Esc, then the right instruction", async () => {
      await d.selectSession(sid);
      s.workMs = 60000;
      await d.send(sid, "Rewrite the renderer in Vulkan", { waitReply: false });
      await d.waitFor("it is working", "#stop:not([hidden])", { timeout: 15000 });
      await page.locator("#prompt").focus();
      const t0 = Date.now();
      await d.press("Escape", { decide: true });
      const st = await d.fleetEvent("the turn stops", e => e.kind === "turn-end" && e.sid === sid && e.stopped, { since: t0, timeout: 20000 });
      d.observe("escToStoppedMs", st.t - t0);
      s.workMs = 40000; // the redirected turn runs long enough to queue behind
      await d.type("#prompt", "No - only fix the shadow acne in the current renderer", { content: true });
      const t1 = Date.now();
      await d.press("Enter");
      const del = await d.fleetEvent("the new instruction arrives", e => e.kind === "delivered" && e.sid === sid && /shadow/.test(e.text), { since: t1, timeout: 30000 });
      d.observe("redirectDeliveredMs", del.t - t1);
      d.observe("escToRedirectedMs", del.t - t0);
    });

    await d.step("queue three, then reorder", "line up three follow-ups and move the last one first", async () => {
      await d.waitFor("it is working on the redirect", "#stop:not([hidden])", { timeout: 15000 });
      for (const t of ["Then add a test scene", "Then profile the frame time", "Then update the changelog"]) {
        await d.type("#prompt", t, { content: true });
        await d.click("#queue"); await d.waitFor("it says queued", { fn: () => /queued/.test(document.querySelector("#say")?.innerText || "") && !document.querySelector("#prompt").value, arg: null }, { timeout: 8000 }); await d.pause(400);
      }
      await d.waitFor("three queued", { fn: () => document.querySelectorAll("#queued .qi").length >= 3, arg: null }, { timeout: 10000 });
      const ctl = await page.locator("#queued .qi").first().locator("button").allInnerTexts();
      d.observe("controlsPerItem", ctl);
      const canMove = await page.locator("#queued [draggable=true], #queued [data-qup], #queued [data-qmv]").count();
      d.observe("canReorder", canMove > 0);
      if (canMove) {
        // Move "changelog" (third) to the front: Earlier twice.
        for (let i = 0; i < 2; i++) {
          d.op("M");
          const at = await page.locator("#queued .qi span").allInnerTexts().then(a => a.findIndex(x => /changelog/.test(x)));
          await d.click(`#queued .qi:nth-child(${at + 1}) [data-qmv][data-by="-1"]`);
          await d.waitFor("it moved", { fn: n => [...document.querySelectorAll("#queued .qi span")].findIndex(x => /changelog/.test(x.innerText)) === n, arg: at - 1 }, { timeout: 5000 });
        }
      } else {
        d.note("queued prompts cannot be reordered: cancel and queue again");
        // Move "changelog" first: cancel the other two, queue them again after it.
        for (let i = 0; i < 2; i++) { d.op("M"); await d.click("#queued .qi [data-uq]", { nth: 0 }); await d.pause(400); }
        for (const t of ["Then add a test scene", "Then profile the frame time"]) {
          await d.type("#prompt", t, { content: true });
          await d.click("#queue"); await d.pause(600);
        }
      }
      await d.waitFor("three queued again", { fn: () => document.querySelectorAll("#queued .qi").length >= 3, arg: null }, { timeout: 10000 });
      d.observe("order", await page.locator("#queued .qi span").allInnerTexts().then(a => a.map(x => x.replace(/^(queued: |[0-9]+[.] )/, "").slice(0, 32))));
      s.workMs = undefined;
    });

    await d.step("fork mid-turn into a new session", "while it works, start a copy with its context for a side question", async () => {
      await d.waitFor("it is working", "#stop:not([hidden])", { timeout: 60000 });
      await d.type("#prompt", "Side question: why are the normals flipped?", { content: true });
      const t0 = Date.now();
      await d.menu("#sendmore", '#sendmore [data-spawn="fork window"]');
      const asked = await d.waitFor("a question, if any", "#spawnbox:not([hidden]) [data-sb]", { timeout: 8000 }).then(() => true).catch(() => false);
      d.observe("asked", asked);
      if (asked) {
        d.observe("dialogText", (await page.locator("#sbbody").innerText()).slice(0, 300));
        d.observe("dialogButtons", await page.locator("#sbbar [data-sb]").allInnerTexts());
        await d.click('#sbbar [data-sb="same"], #sbbar [data-sb="tree"], #sbbar [data-sb]', { decide: true });
      }
      const fsid = await newSession(d, "the fork starts", t0, { timeout: 60000 });
      this.fork = fsid;
      d.observe("tileAfterMs", await tileShows(d, fsid, t0));
      d.observe("printModeLaunches", fleet.events.filter(e => e.kind === "launch" && e.t >= t0 && e.prog === "claude" && (e.args || []).includes("-p")).length);
    });

    await d.step("fork the fork", "from the side session, fork again", async () => {
      await d.selectSession(this.fork);
      await d.type("#prompt", "And check the tangent space too", { content: true });
      const t0 = Date.now();
      await d.menu("#sendmore", '#sendmore [data-spawn="fork window"]');
      const asked = await d.waitFor("a question, if any", "#spawnbox:not([hidden]) [data-sb]", { timeout: 8000 }).then(() => true).catch(() => false);
      if (asked) await d.click('#sbbar [data-sb="same"], #sbbar [data-sb="tree"], #sbbar [data-sb]', { decide: true });
      const f2 = await newSession(d, "the second fork starts", t0, { timeout: 60000 });
      d.observe("tileAfterMs", await tileShows(d, f2, t0));
      d.observe("tiles", await page.locator("#sessions .s").count());
      d.observe("tileNames", await page.locator("#sessions .s .n").allInnerTexts().then(a => a.map(x => x.trim().slice(0, 40))));
      d.note("check: can the three engine sessions be told apart on the list?");
    });

    await d.step("the queue drains", "the three follow-ups run in order", async () => {
      const t0 = Date.now();
      const seen = [];
      for (let i = 0; i < 3; i++) {
        const ev = await d.fleetEvent(`queued prompt ${i + 1}`, e => e.kind === "delivered" && e.sid === sid && /^Then/.test(e.text) && !seen.includes(e.t), { since: t0 - 120000, timeout: 90000, sits: false });
        seen.push(ev.t); (d.cur.observations.delivered ||= []).push(ev.text.slice(5, 30));
      }
    }, { optional: true });
  },
};
