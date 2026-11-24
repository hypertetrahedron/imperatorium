// Omar: keeps an eye on spend. Usage Meter details, Model Mix, Insights'
// cost by day, project and model over 7/30/90 days, the Chat Matrix stats
// tile, and a spend watch in Fleet.
import path from "node:path";
import { makeProject } from "../lib/sandbox.mjs";

export default {
  id: "omar",
  name: "Omar, cost watcher",
  set: 2,
  fleet: { workMs: [6000, 10000] },
  projects(sb) { this.dirs = ["etl", "dashboard", "infra", "notebooks", "website"].map(p => makeProject(sb.projects, p)); },
  async setup({ fleet }) {
    this.s = [];
    for (const [i, dir] of this.dirs.entries()) this.s.push(await fleet.addInteractive(dir, { history: 10 + i * 6 }));
    for (const [i, dir] of this.dirs.entries()) fleet.pastSessions(dir, 12 + i * 4, { daysBack: 90, turns: 6 });
  },

  async run({ d, page }) {
    await d.step("open the meter details", "see this machine's spend and the pace against the limits", async () => {
      const t0 = Date.now();
      await d.waitFor("the meter", "#meter .row", { timeout: 20000 });
      d.observe("meterMs", Date.now() - t0);
      await d.click("#meter", { decide: true });
      await d.waitFor("the details", "#meter.open .extra", { timeout: 5000 });
      d.observe("spendLines", (await page.locator("#meter .spend").allInnerTexts().catch(() => [])).map(t => t.slice(0, 120)));
      d.observe("mixRows", await page.locator("#meter .row.mix").count());
      d.observe("legend", (await page.locator("#meter .legend").first().innerText().catch(() => "")).slice(0, 120));
    });

    for (const days of ["7", "30", "90"]) {
      await d.step(`cost over ${days} days`, "cost by day, project and model in Insights", async () => {
        if (!(await page.locator("#insightbox:not([hidden])").count())) await d.menu("#sidemenu", '#sidemenu [data-view="insights"]');
        const t0 = Date.now();
        const before = await page.locator("#costs").innerText().catch(() => "");
        if ((await page.locator("#cdays").inputValue()) !== days) await d.select("#cdays", days);
        await d.waitFor("the cost chart", { fn: b => !!document.querySelector("#costs svg.chart") && document.querySelector("#costs").innerText !== b, arg: before }, { timeout: 40000 })
          .catch(async e => { if (days !== "30") throw e; }); // 30 is the default: no change to wait for
        if (days === "30") d.note("the failures card's range (#days: 1/7/30 days) is a separate picker from the cost range (7/30/90); 90 days of failures is not offered");
        d.observe("loadMs", Date.now() - t0);
        d.observe("tables", await page.locator("#costs table").count());
        d.observe("firstRows", (await page.locator("#costs table").allInnerTexts().catch(() => [])).map(t => t.split("\n").slice(0, 3).join(" | ").slice(0, 160)));
      });
    }

    await d.step("compare by model", "find which model the money goes to", async () => {
      const txt = await page.locator("#costs").innerText();
      d.op("M", 2); // reading two tables against each other
      d.observe("mentionsModels", /opus|sonnet|haiku/i.test(txt));
      d.observe("modelTable", /model/i.test(txt));
      await d.press("Escape");
      if (await page.locator("#insightbox:not([hidden])").count()) { d.note("Esc did not close Insights (focus is not inside it): used its ✕"); await d.click("#insightbox [data-popclose]", { decide: true }); }
    });

    await d.step("the Matrix stats tile", "the spend for the open sessions at a glance", async () => {
      const popup = page.context().waitForEvent("page", { timeout: 5000 }).catch(() => null);
      await d.click("#matrixbtn", { decide: true });
      const p2 = await popup;
      if (!p2) throw new Error("no Chat Matrix window");
      const t0 = Date.now();
      await p2.locator(".tile.stat .big").first().waitFor({ timeout: 20000 });
      const ms = Date.now() - t0; d.cur.waitMs += ms; d.cur.waits.push({ label: "the stats tile", ms });
      d.observe("statMs", ms);
      d.observe("spendFigure", await p2.locator(".tile.stat .big").first().innerText());
      d.observe("tables", await p2.locator(".tile.stat table").count());
      await p2.close();
    });

    await d.step("a spend watch in Fleet", "be told when the open sessions pass $5", async () => {
      await d.click("#fleetrow", { decide: true });
      await d.waitFor("the fleet view", "#fleetview:not([hidden])", { timeout: 5000 });
      if (!(await page.locator("#flwfold[open]").count())) { await d.click("#flwfold > summary", { decide: true }); }
      await d.select("#wkind", "spend");
      await d.type("#wnum", "5");
      const t0 = Date.now();
      await d.click("#wadd");
      await d.waitFor("the watch is listed", { fn: () => /spend|\$5|5 dollars/i.test(document.querySelector("#flwatch")?.innerText || ""), arg: null }, { timeout: 10000 });
      d.observe("watchMs", Date.now() - t0);
      d.observe("watchText", (await page.locator("#flwatch").innerText()).slice(0, 200));
      d.observe("fleetRowSub", await page.locator("#fleetsub").innerText().catch(() => ""));
    });
  },
};
