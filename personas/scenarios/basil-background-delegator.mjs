// Basil: hands work to background sessions and checks back later; rarely
// sits in a terminal. Reads results, follows up, merges, closes.
import path from "node:path";
import { makeProject } from "../lib/sandbox.mjs";
import { newSession, tileShows, workIn } from "./common.mjs";

export default {
  id: "basil",
  name: "Basil, background delegator",
  set: 1,
  fleet: { workMs: [9000, 14000] },
  projects(sb) { this.ledger = makeProject(sb.projects, "ledger"); this.pipeline = makeProject(sb.projects, "pipeline"); makeProject(sb.projects, "site"); },
  async setup({ fleet }) { this.main = await fleet.addInteractive(this.ledger, { history: 6, name: "ledger-desk" }); },

  async run({ d, fleet, page }) {
    const bg = [];
    await d.step("background session from the New session dialog", "start a background job in another project", async () => {
      const t0 = Date.now();
      await d.menu("#sidemenu", "#new");
      await d.waitFor("the dialog", "#newbox:not([hidden]) #find", { timeout: 5000 });
      await d.type("#find", "pipe");
      await d.waitFor("the project row", "#projects .p", { timeout: 5000 });
      d.observe("rows", await page.locator("#projects .p").count());
      d.observe("hintText", await page.locator("#nbghint").innerText().catch(() => ""));
      await d.click("#nbg", { decide: true });
      d.observe("promptBoxEmpty", (await page.locator("#prompt").inputValue()) === "");
      const TASK = "Profile the nightly import and cut its time in half";
      const taskBox = await page.locator("#ntask:not([hidden])").count();
      d.observe("taskBox", taskBox > 0);
      if (taskBox) await d.type("#ntask", TASK, { content: true });
      await d.click("#nstart");
      const sid = await newSession(d, "the background job starts", t0, { bg: true });
      bg.push(sid);
      d.observe("tileAfterMs", await tileShows(d, sid, t0));
      if (taskBox) {
        const ev = await d.fleetEvent("it starts on its task", e => e.kind === "turn-start" && e.sid === sid, { since: t0, timeout: 30000, sits: false }).catch(() => null);
        d.observe("startedOnTask", !!ev);
      } else {
        d.note("the dialog has no field for the task: it takes the prompt box, so the job started idle and needs a second send");
        await d.selectSession(sid);
        await d.send(sid, TASK, { waitReply: false });
      }
    });

    for (const task of ["Write the reconciliation report job", "Add a CSV export to the ledger API"]) {
      await d.step(`ctrl+Enter: ${task}`, "fire off a background job from the session at hand", async () => {
        await d.selectSession(this.main.sid, { find: false });
        await d.type("#prompt", task, { content: true });
        const t0 = Date.now();
        await d.press("Control+Enter", { shortcut: true });
        const asked = await d.waitFor("a question, if any", "#spawnbox:not([hidden]) [data-sb]", { timeout: 4000 }).then(() => true).catch(() => false);
        d.observe("dialog", asked);
        if (asked) { d.observe("dialogButtons", await page.locator("#sbbar [data-sb]").allInnerTexts()); await d.click("#sbbar [data-sb]", { decide: true }); }
        const sid = await newSession(d, "the job starts", t0, { bg: true });
        bg.push(sid);
        d.observe("tileAfterMs", await tileShows(d, sid, t0));
        d.observe("windowSwitchedToIt", await page.locator(`${d.tile(sid)}.on`).count() > 0);
      });
    }

    await d.step("notice a job finished", "without watching, see that a job is done", async () => {
      const t0 = Date.now();
      const inTree = sid => /worktrees/.test(fleet.sessions.get(sid)?.cwd || "");
      const end = await d.fleetEvent("a job's turn ends", e => e.kind === "turn-end" && bg.includes(e.sid) && inTree(e.sid), { since: 0, timeout: 60000, sits: false });
      const sid = end.sid;
      const shownT = Date.now();
      await d.waitFor("its tile says it is done", { fn: sid => { const t = document.querySelector(`#sessions .s[data-k="${sid}"]`); return t && /done|ready|finished|idle|complete/i.test(t.innerText + t.className) && !/working|busy/i.test(t.querySelector(".m")?.innerText || ""); }, arg: sid }, { timeout: 30000 });
      d.observe("tileUpdatedAfterTurnEndMs", Date.now() - Math.max(end.t, t0 > end.t ? t0 : end.t));
      d.observe("fromStepStartMs", Date.now() - t0);
      this.done = sid;
    });

    await d.step("read the result and follow up", "open the finished job, read it, send a follow-up", async () => {
      await d.selectSession(this.done);
      const lat = await d.send(this.done, "Good. Now add tests for the slow path.", { waitReply: true });
      d.observe("lat", lat);
    });

    await d.step("merge a job's branch", "land the finished job's worktree from Project changes", async () => {
      const s = fleet.sessions.get(this.done);
      if (s && /worktrees/.test(s.cwd)) workIn(s.cwd, ["src/report.py"], "report job");
      await d.menu("#sidemenu", '#sidemenu [data-view="project"]');
      await d.waitFor("the project pane", "#projbox:not([hidden]) #projpane .pjline", { timeout: 20000 });
      d.observe("projectShown", await page.locator("#pjpick").inputValue());
      const merge = page.locator('#projpane [data-pja="merge"]:not([disabled])');
      if (!(await merge.count())) {
        d.note("no enabled Merge in Project changes for the job's worktree");
        d.observe("lineCards", await page.locator("#projpane .pjline").count());
        await d.press("Escape");
        throw new Error("nothing to merge");
      }
      const t0 = Date.now();
      await d.click('#projpane [data-pja="merge"]:not([disabled])', { decide: true });
      await d.waitFor("a result", { fn: () => /merge (done|failed)|merged|conflict/i.test(document.querySelector("#say")?.innerText + document.querySelector("#projpane").innerText), arg: null }, { timeout: 20000 });
      d.observe("mergeMs", Date.now() - t0);
      await d.press("Escape");
      await d.pause(300);
      if (await page.locator("#projbox:not([hidden])").count()) {
        d.note("Esc did not close Project changes after a merge; used its ✕");
        await d.click("#projbox [data-popclose]");
      }
    }, { optional: true });

    await d.step("close a job", "the ✕ on a job no longer needed", async () => {
      const sid = bg.find(x => x !== this.done) || bg[0];
      await d.selectSession(sid);
      const t0 = Date.now();
      await d.click(`${d.tile(sid)} [data-close], ${d.tile(sid)} [data-dismiss]`, { decide: true });
      await d.waitFor("the tile goes", { fn: sid => !document.querySelector(`#sessions .s[data-k="${sid}"]`), arg: sid }, { timeout: 30000 });
      d.observe("closeMs", Date.now() - t0);
      d.observe("stopSent", fleet.events.some(e => e.kind === "launch" && e.args?.[0] === "stop"));
    });
  },
};
