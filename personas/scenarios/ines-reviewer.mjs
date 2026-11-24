// Ines: reads what sessions changed before letting them go on. Changes tab
// on three sessions, line comments sent back as one prompt, the Activity
// panel, a worktree's diff, and Insights' tool failures.
import fs from "node:fs";
import path from "node:path";
import { makeProject } from "../lib/sandbox.mjs";

/** Uncommitted edits, as a session would leave them. */
function edit(dir, files) {
  for (const [f, n] of files) {
    const p = path.join(dir, f);
    fs.mkdirSync(path.dirname(p), { recursive: true });
    const lines = Array.from({ length: n }, (_, i) => `def step_${i}():\n    return ${i}  # changed by the session`).join("\n");
    fs.appendFileSync(p, "\n" + lines + "\n");
  }
}

export default {
  id: "ines",
  name: "Ines, reviewer",
  set: 2,
  fleet: { workMs: [5000, 8000] },
  projects(sb) { this.dirs = ["checkout", "catalog", "pricing"].map(p => makeProject(sb.projects, p, { files: 6, commits: 4 })); },
  async setup({ fleet }) {
    this.s = [];
    for (const dir of this.dirs) this.s.push(await fleet.addInteractive(dir, { history: 8 }));
    edit(this.dirs[0], [["src/mod1.py", 12], ["src/mod2.py", 4]]);
    edit(this.dirs[1], [["src/mod3.py", 30]]);
    edit(this.dirs[2], [["src/mod0.py", 3]]);
  },

  async run({ d, page }) {
    const [a, b, c] = this.s;
    await d.step("read three sessions' changes", "open the Changes tab on each session in turn", async () => {
      const ms = [], stale = [];
      for (const [s, file] of [[a, "mod1.py"], [b, "mod3.py"], [c, "mod0.py"]]) {
        const t0 = Date.now();
        await d.selectSession(s.sid);
        if (s === a) await d.select("#tabpick", "changes"); // the tab stays as she moves on
        const wasStale = await page.evaluate(f => !!document.querySelector("#changes .diff") && !document.querySelector("#changes").innerText.includes(f), file);
        await d.waitFor("this session's diff", { fn: f => (document.querySelector("#changes")?.innerText || "").includes(f) && !!document.querySelector("#changes .diff .dl"), arg: file }, { timeout: 20000 });
        ms.push(Date.now() - t0);
        if (wasStale) stale.push(Date.now() - t0);
      }
      d.observe("changesLoadMs", ms);
      d.observe("previousDiffShownMs", stale);
      if (stale.length) d.note("after a switch the Changes tab shows the previous session's diff under the new title until the new one loads; a click on it does nothing");
      d.observe("tabStaysAcrossSessions", await page.locator("#tabpick").inputValue());
    });

    await d.step("comment on two lines and send them", "leave line comments on the diff and send them to the session as one prompt", async () => {
      await d.selectSession(a.sid, { find: false });
      await d.waitFor("this session's diff", { fn: () => /mod1\.py/.test(document.querySelector("#changes")?.innerText || "") && !!document.querySelector("#changes .diff .dl[data-r]"), arg: null }, { timeout: 20000 });
      const lines = page.locator("#changes .dl.add[data-r], #changes .dl[data-r]");
      d.observe("commentableLines", await lines.count());
      for (const [i, text] of [[2, "This should return early on an empty cart."], [6, "Name it for what it computes."]]) {
        d.op("M"); // finding the line
        await d.click("#changes .dl[data-r]", { nth: i });
        await d.waitFor("the comment box", "#rvtext", { timeout: 5000 });
        await d.type("#rvtext", text, { content: true, clear: false });
        await d.click("#rvadd");
        await d.waitFor("the comment is placed", { fn: n => document.querySelectorAll("#changes .rv").length >= n, arg: i === 2 ? 1 : 2 }, { timeout: 5000 });
      }
      d.observe("reviewBox", (await page.locator("#review").innerText().catch(() => "")).slice(0, 120));
      const t0 = Date.now();
      await d.click("#rvsend", { decide: true });
      const ev = await d.fleetEvent("the review reaches the session", e => e.kind === "delivered" && e.sid === a.sid, { since: t0, timeout: 30000 });
      d.observe("deliverMs", ev.t - t0);
      d.observe("say", await page.locator("#say").innerText().catch(() => ""));
      d.observe("tabAfterSend", await page.locator("#tabpick").inputValue());
      d.note("after sending, the review's answer is on the Conversation tab: she must switch tabs to read it");
    });

    await d.step("watch the Activity panel", "see which tools the session ran and which failed", async () => {
      await d.select("#tabpick", "activity");
      const t0 = Date.now();
      await d.waitFor("the timeline", "#activity .ev", { timeout: 20000 });
      d.observe("activityMs", Date.now() - t0);
      d.observe("events", await page.locator("#activity .ev").count());
      d.observe("totals", (await page.locator("#activity .kv").first().innerText().catch(() => "")).slice(0, 160));
    });

    await d.step("diff a worktree", "see what a worktree's branch would bring before merging", async () => {
      await d.selectSession(b.sid);
      await d.select("#tabpick", "worktrees");
      const t0 = Date.now();
      await d.waitFor("the worktrees panel", { fn: () => /worktree|no linked|none/i.test(document.querySelector("#worktrees")?.innerText || ""), arg: null }, { timeout: 20000 });
      d.observe("worktreesMs", Date.now() - t0);
      const cards = await page.locator("#worktrees .card").count();
      d.observe("cards", cards);
      if (!cards) d.note("no worktree to diff: the session works in the main checkout; the panel says so");
    });

    await d.step("tool failures in Insights", "check which tools fail most across sessions this week", async () => {
      await d.menu("#sidemenu", '#sidemenu [data-view="insights"]');
      const t0 = Date.now();
      await d.waitFor("the failures card", { fn: () => /tool calls/.test(document.querySelector("#errs")?.innerText || ""), arg: null }, { timeout: 30000 });
      d.observe("errorsMs", Date.now() - t0);
      d.observe("summary", (await page.locator("#errs .kv").first().innerText().catch(() => "")).slice(0, 160));
      await d.press("Escape");
      d.observe("closedByEsc", !(await page.locator("#insightbox:not([hidden])").count()));
      if (await page.locator("#insightbox:not([hidden])").count()) { d.note("Esc did not close Insights (focus is not inside it): used its ✕"); await d.click("#insightbox [data-popclose]", { decide: true }); }
    });
  },
};
