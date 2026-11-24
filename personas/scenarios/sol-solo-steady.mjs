// Sol: the pattern this machine's own transcripts show most - a handful of
// sessions in different projects, short exchanges, /clear between tasks,
// History to pick an old thread back up.
import { makeProject } from "../lib/sandbox.mjs";
import { newSession, tileShows } from "./common.mjs";

export default {
  id: "sol",
  name: "Sol, solo steady",
  set: 1,
  fleet: { workMs: [5000, 9000] },
  projects(sb) { this.dirs = ["blog", "budget", "homelab", "scripts"].map(p => makeProject(sb.projects, p)); },
  async setup({ fleet }) {
    this.s = [];
    for (const dir of this.dirs) this.s.push(await fleet.addInteractive(dir, { history: 4 }));
    fleet.pastSessions(this.dirs[0], 25);
    fleet.pastSessions(this.dirs[1], 15);
  },

  async run({ d, fleet, page }) {
    const [blog, budget] = this.s;
    await d.step("send and read the reply", "ask the blog session for a change and read the answer", async () => {
      await d.selectSession(blog.sid);
      const lat = await d.send(blog.sid, "Fix the broken image links in the latest post");
      d.observe("lat", lat);
    });

    await d.step("queue while it works", "type the next instruction while it is busy and queue it", async () => {
      await d.send(blog.sid, "Now regenerate the RSS feed", { waitReply: false });
      await d.type("#prompt", "Then run the link checker", { content: true });
      const t0 = Date.now();
      await d.click("#queue", { decide: true });
      await d.waitFor("it shows as queued", "#queued .qi", { timeout: 8000 });
      const end = await d.fleetEvent("the running turn ends", e => e.kind === "turn-end" && e.sid === blog.sid, { since: t0, timeout: 60000, sits: false });
      const del = await d.fleetEvent("the queued prompt is delivered", e => e.kind === "delivered" && e.sid === blog.sid && /link checker/.test(e.text), { since: t0, timeout: 60000 });
      d.observe("queuedDeliveredAfterTurnEndMs", del.t - end.t);
      await d.fleetEvent("its turn ends", e => e.kind === "turn-end" && e.sid === blog.sid, { since: del.t, timeout: 60000, sits: false });
    });

    await d.step("stop a runaway turn", "it went off in the wrong direction: stop it", async () => {
      blog.workMs = 60000;
      await d.send(blog.sid, "Rewrite every post in a pirate voice", { waitReply: false });
      await d.waitFor("the Stop button", "#stop:not([hidden])", { timeout: 15000 });
      const t0 = Date.now();
      await d.click("#stop", { decide: true });
      const ev = await d.fleetEvent("the session is told to stop", e => e.kind === "stop" && e.sid === blog.sid, { since: t0, timeout: 20000 });
      d.observe("stopReachedSessionMs", ev.t - t0);
      await d.waitFor("the window shows it stopped", { fn: () => document.querySelector("#stop").hidden && /stopped/i.test(document.querySelector("#say")?.innerText || ""), arg: null }, { timeout: 20000 });
      d.observe("stopButtonGoneMs", Date.now() - t0);
      blog.workMs = undefined;
    });

    await d.step("/clear and start fresh", "clear the session for an unrelated task", async () => {
      await d.pause(1000);
      await d.type("#prompt", "/clear", { content: false });
      const t0 = Date.now();
      await d.press("Enter");
      const ev = await d.fleetEvent("the session clears", e => e.kind === "cleared", { since: t0, timeout: 20000 });
      await d.waitFor("the window follows the new session", { fn: sid => !!document.querySelector(`#sessions .s.on[data-k="${sid}"]`), arg: ev.sid }, { timeout: 20000 });
      d.observe("followMs", Date.now() - t0);
      d.observe("tilesAfter", await page.locator("#sessions .s").count());
      this.blogNew = ev.sid;
    });

    await d.step("switch project and ask", "ask the budget session a question", async () => {
      await d.selectSession(budget.sid);
      d.observe("lat", await d.send(budget.sid, "What did we spend on groceries in October?"));
    });

    await d.step("pick up an old thread from History", "find last month's flaky-login work and resume it", async () => {
      await d.menu("#sidemenu", '#sidemenu [data-view="history"]');
      await d.waitFor("History", "#histbox:not([hidden]) #hq", { timeout: 5000 });
      const t0 = Date.now();
      await d.type("#hq", "flaky login");
      await d.waitFor("hits", "#hits .hit", { timeout: 15000 });
      d.observe("searchMs", Date.now() - t0);
      d.observe("hits", await page.locator("#hits .hit").count());
      d.observe("resumeButtons", await page.locator("#hits .hit").first().locator("button").allInnerTexts());
      const t1 = Date.now();
      await d.click('#hits .hit [data-rs]:not([data-w="background"])', { decide: true });
      const asked = await d.waitFor("a question, if any", "#spawnbox:not([hidden]) [data-sb]", { timeout: 3000 }).then(() => true).catch(() => false);
      if (asked) { d.observe("dialogButtons", await page.locator("#sbbar [data-sb]").allInnerTexts()); await d.click("#sbbar [data-sb]", { decide: true }); }
      const sid = await newSession(d, "the resumed session starts", t1);
      d.observe("tileAfterMs", await tileShows(d, sid, t1));
      d.observe("historyStillOpen", await page.locator("#histbox:not([hidden])").count() > 0);
    });
  },
};
