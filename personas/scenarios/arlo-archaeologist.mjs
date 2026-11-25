// Arlo: digs through a long history. 500 past sessions in 20 folders:
// History search, folder filter, sort, time range, reading a hit, and
// resuming one whose folder is taken (so it goes to a worktree).
import { makeProject } from "../lib/sandbox.mjs";
import { newSession, tileShows } from "./common.mjs";

const FOLDERS = ["api", "web", "cli", "infra", "docs", "billing", "search", "auth", "mobile", "ml",
  "etl", "admin", "sdk", "bots", "site", "ops", "data", "notify", "media", "legacy"];

export default {
  id: "arlo",
  name: "Arlo, archaeologist",
  set: 2,
  loadTimeout: 60000,
  projects(sb) { this.dirs = FOLDERS.map(p => makeProject(sb.projects, p)); },
  async setup({ fleet }) {
    for (const [i, dir] of this.dirs.entries()) fleet.pastSessions(dir, 25, { daysBack: 120, turns: 3 + (i % 4) });
    this.live = await fleet.addInteractive(this.dirs[1], { history: 4, name: "web-today" });
  },

  /** Does something that searches History again, and waits for the answer to be drawn. */
  async settle(d, label, fn) {
    const page = d.page, t0 = Date.now();
    const resp = page.waitForResponse(r => r.url().includes("/api/history"), { timeout: 15000 }).catch(() => null);
    await fn();
    if (!(await resp)) d.note(`${label}: no new search was made`);
    await d.pause(150); // drawn
    const ms = Date.now() - t0;
    d.cur.waitMs += ms; d.cur.waits.push({ label, ms });
    return ms;
  },

  async run({ d, page }) {
    await d.step("search everything", "find past work on the flaky login test", async () => {
      await d.menu("#sidemenu", '#sidemenu [data-view="history"]');
      await d.waitFor("History", "#histbox:not([hidden]) #hq", { timeout: 5000 });
      const t0 = Date.now();
      await this.settle(d, "the first answer", () => d.type("#hq", "flaky login"));
      d.observe("firstAnswer", (await page.locator("#hits").innerText()).slice(0, 160));
      await d.waitFor("hits", "#hits .hit", { timeout: 60000 });
      d.observe("searchMs", Date.now() - t0);
      d.observe("hits", await page.locator("#hits .hit").count());
      d.observe("folders", await page.locator("#hfolder option").count());
    });

    await d.step("narrow to one folder", "only the web folder's sessions", async () => {
      const web = await page.locator("#hfolder option").evaluateAll(os => (os.find(o => /[\\/]web$/.test(o.value)) || {}).value);
      if (!web) throw new Error("no web folder in the filter");
      d.observe("folderOptions", await page.locator("#hfolder option").allInnerTexts().then(a => a.slice(0, 4)));
      d.op("M"); // scanning 20 folders for the right one
      d.observe("filterMs", await this.settle(d, "the folder's hits", () => d.select("#hfolder", web)));
      d.observe("hits", await page.locator("#hits .hit").count());
    });

    await d.step("sort and limit to 30 days", "the longest sessions of the last month", async () => {
      d.observe("sortMs", await this.settle(d, "re-sorted", () => d.select("#hsort", "size")));
      d.observe("hitsSorted", await page.locator("#hits .hit").count());
      d.observe("daysMs", await this.settle(d, "limited to 30 days", () => d.select("#hdays", "30")));
      d.observe("hits", await page.locator("#hits .hit").count());
    });

    await d.step("clear the search, browse the folder", "with no words, does the folder list everything", async () => {
      d.op("K", 11); d.cur.keys += 11; // deleting the words
      d.observe("ms", await this.settle(d, "the folder's whole list", async () => { await page.locator("#hq").fill(""); await page.locator("#hq").dispatchEvent("input"); }));
      d.observe("hits", await page.locator("#hits .hit").count());
      d.observe("text", (await page.locator("#hits").innerText()).slice(0, 200));
    });

    await d.step("read a hit without resuming", "look inside an old session before deciding", async () => {
      const hit = page.locator("#hits .hit:not(:has([data-open]))").first();
      d.observe("hitText", (await hit.innerText()).replace(/\s+/g, " ").slice(0, 200));
      d.observe("buttons", await hit.locator("button").allInnerTexts());
      const readable = await hit.locator("a, [data-open], [data-read]").count();
      d.observe("hasReadOnlyOpen", readable > 0);
      if (!readable) d.note("a hit cannot be read without resuming it: only Resume / Resume in bg / Fork / Copy cmd");
      d.op("M", 2); // reading the snippet instead
    });

    await d.step("resume one whose folder is taken", "resume an old web session while web-today runs there", async () => {
      const t1 = Date.now();
      await d.click('#hits .hit [data-rs][data-w="window"]:not([data-fork])', { decide: true });
      const asked = await d.waitFor("the worktree question", "#spawnbox:not([hidden]) [data-sb]", { timeout: 6000 }).then(() => true).catch(() => false);
      d.observe("asked", asked);
      if (asked) { d.observe("dialogButtons", await page.locator("#sbbar [data-sb]").allInnerTexts()); await d.click('#sbbar [data-sb="tree"]', { decide: true }); }
      const sid = await newSession(d, "the resumed session starts", t1);
      d.observe("tileAfterMs", await tileShows(d, sid, t1));
      d.observe("historyStillOpen", await page.locator("#histbox:not([hidden])").count() > 0);
    });
  },
};
