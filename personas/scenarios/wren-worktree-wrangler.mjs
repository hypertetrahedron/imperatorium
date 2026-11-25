// Wren: many sessions in one repository, each in its own worktree; merges
// and discards branches as the work lands.
import path from "node:path";
import { makeProject } from "../lib/sandbox.mjs";
import { newSession, tileShows, workIn, worktrees } from "./common.mjs";

export default {
  id: "wren",
  name: "Wren, worktree wrangler",
  set: 1,
  projects(sb) { this.atlas = makeProject(sb.projects, "atlas", { files: 8, commits: 5 }); makeProject(sb.projects, "notes"); },
  async setup({ fleet }) { this.main = await fleet.addInteractive(this.atlas, { history: 20, name: "atlas-main" }); },

  async run({ d, fleet }) {
    const s0 = this.main.sid;
    await d.step("open the main session", "find atlas-main and read where it left off", async () => {
      await d.selectSession(s0);
    });

    const spawned = [];
    for (const [label, item] of [["new session in a worktree", "new window"], ["fork with context into a worktree", "fork window"]]) {
      await d.step(label, "start a parallel session on atlas from the Send menu, accepting the worktree", async () => {
        await d.selectSession(s0, { find: false });
        await d.type("#prompt", `Take the ${label} task: build the export endpoint`, { content: true });
        const t0 = Date.now();
        await d.menu("#sendmore", `#sendmore [data-spawn="${item}"]`);
        const asked = await d.waitFor("the worktree question", "#spawnbox:not([hidden]) [data-sb]", { timeout: 8000 }).then(() => true).catch(() => false);
        d.observe("worktreeDialog", asked);
        if (asked) {
          d.observe("dialogButtons", await d.page.locator("#sbbar [data-sb]").allInnerTexts());
          await d.click('#sbbar [data-sb="tree"]', { decide: true });
        }
        const sid = await newSession(d, "the new session starts", t0);
        spawned.push(sid);
        d.observe("tileAfterMs", await tileShows(d, sid, t0));
        await d.waitFor("the window follows it", { fn: sid => !!document.querySelector(`#sessions .s.on[data-k="${sid}"]`), arg: sid }, { timeout: 15000 })
          .then(() => d.observe("followedAfterMs", Date.now() - t0)).catch(() => d.note("the window did not switch to the new session"));
      });
    }

    await d.step("background session via ctrl+Enter", "hand a third task to the background from the prompt box", async () => {
      await d.selectSession(s0, { find: false });
      await d.type("#prompt", "Background: write the migration for the export table", { content: true });
      const t0 = Date.now();
      await d.press("Control+Enter", { shortcut: true });
      const asked = await d.waitFor("a question, if any", "#spawnbox:not([hidden]) [data-sb]", { timeout: 4000 }).then(() => true).catch(() => false);
      if (asked) await d.click('#sbbar [data-sb="tree"]', { decide: true });
      d.observe("worktreeDialog", asked);
      const sid = await newSession(d, "the background session starts", t0, { bg: true });
      spawned.push(sid);
      d.observe("tileAfterMs", await tileShows(d, sid, t0));
    });

    await d.step("switch across the four sessions", "glance at each parallel session in turn", async () => {
      const times = [];
      for (const sid of [s0, ...spawned]) { const t = Date.now(); await d.selectSession(sid); times.push(Date.now() - t); }
      d.observe("switchMs", times);
    });

    // The sessions' work, as it would land: commits in each worktree, two touching the same file.
    const trees = worktrees(this.atlas);
    trees.forEach((w, i) => workIn(w, i < 2 ? ["src/mod1.py", `src/feature${i}.py`] : [`src/feature${i}.py`], `work in tree ${i}`));
    d.steps.at(-1).observations.worktreesOnDisk = trees.length;

    await d.step("review the worktrees tab", "see what each worktree's branch would bring", async () => {
      await d.selectSession(s0, { find: false });
      await d.select("#tabpick", "worktrees");
      await d.waitFor("worktree cards", "#worktrees .card", { timeout: 15000 });
      d.observe("cards", await d.page.locator("#worktrees .card").count());
      d.observe("mergeEnabled", await d.page.locator('#worktrees [data-wa="merge"]:not([disabled])').count());
    });

    await d.step("find overlapping edits", "Project changes: which branches touch the same file", async () => {
      await d.menu("#sidemenu", '#sidemenu [data-view="project"]');
      await d.waitFor("the project pane", "#projbox:not([hidden]) #projpane .pjline", { timeout: 20000 });
      const pick = await d.page.locator("#pjpick").inputValue().catch(() => "");
      d.observe("projectPreselected", pick);
      if (!/atlas/.test(pick)) {
        const opts = await d.page.locator("#pjpick option").allInnerTexts();
        const v = await d.page.locator("#pjpick option", { hasText: "atlas" }).first().getAttribute("value").catch(() => null);
        d.note(`project picker opened on "${pick}", not on the selected session's project (options: ${opts.join(", ")})`);
        if (v !== null) { await d.select("#pjpick", v); await d.waitFor("atlas lines", "#projpane .pjline", { timeout: 20000 }); }
      }
      d.observe("lines", await d.page.locator("#projpane .pjline").count());
      d.observe("overlaps", await d.page.locator("#projpane .pjov").count());
      await d.press("Escape");
    });

    await d.step("merge one worktree back", "merge the first branch into main from the Worktrees tab", async () => {
      await d.select("#tabpick", "worktrees");
      await d.waitFor("a merge button", '#worktrees [data-wa="merge"]:not([disabled])', { timeout: 15000 });
      const t0 = Date.now();
      const before = await d.page.locator("#worktrees .card").count();
      await d.click('#worktrees [data-wa="merge"]:not([disabled])', { decide: true });
      await d.waitFor("the result shows anywhere", { fn: () => /merge (done|failed)|merged into|conflict|refused/i.test((document.querySelector("#say")?.innerText || "") + document.querySelector("#worktrees").innerText), arg: null }, { timeout: 20000 });
      d.observe("mergeMs", Date.now() - t0);
      d.observe("resultWhere", await d.page.evaluate(() => /merge/i.test(document.querySelector("#worktrees").innerText.replace(/Merge into/g, "")) ? "worktrees panel" : "say line by the Send button"));
      d.observe("cardsAfter", await d.page.locator("#worktrees .card").count());
      d.observe("cardsBefore", before);
    });

    await d.step("discard another worktree", "throw away a branch that went nowhere", async () => {
      const n = await d.page.locator('#worktrees [data-wa="discard"]').count();
      if (!n) throw new Error("no discard button");
      const before = await d.page.locator("#worktrees .card").count();
      await d.click('#worktrees [data-wa="discard"]', { decide: true, nth: n - 1 });
      await d.waitFor("the card goes", { fn: n => document.querySelectorAll("#worktrees .card").length < n, arg: before }, { timeout: 20000 });
      await d.pause(500);
    });

    await d.step("close a finished session", "the ✕ on the fork's tile", async () => {
      await d.select("#tabpick", "log");
      const sid = spawned[1];
      const t0 = Date.now();
      // The ✕ shows only on the open tile: open it first.
      d.observe("closeVisibleBeforeOpening", await d.page.locator(`${d.tile(sid)} [data-close]`).count() > 0);
      await d.selectSession(sid);
      await d.click(`${d.tile(sid)} [data-close]`, { decide: true });
      await d.waitFor("the tile goes", { fn: sid => !document.querySelector(`#sessions .s[data-k="${sid}"]`), arg: sid }, { timeout: 30000 });
      d.observe("closeMs", Date.now() - t0);
    });
  },
};
