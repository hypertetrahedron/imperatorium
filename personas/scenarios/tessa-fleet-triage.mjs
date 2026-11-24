// Tessa: ~16 sessions across 10 projects, all on the go. Her day is triage:
// approve what is waiting, answer questions, nudge the next session along.
import path from "node:path";
import { makeProject } from "../lib/sandbox.mjs";

const PROJECTS = ["api", "web", "infra", "docs", "ml", "mobile", "billing", "auth", "search", "data"];

export default {
  id: "tessa",
  name: "Tessa, fleet triage lead",
  set: 1,
  fleet: { workMs: [6000, 15000] },
  projects(sb) { this.dirs = PROJECTS.map(p => makeProject(sb.projects, p)); },
  async setup({ fleet }) {
    this.s = [];
    for (let i = 0; i < 16; i++) {
      const dir = this.dirs[i % this.dirs.length];
      this.s.push(await fleet.addInteractive(dir, { name: `${path.basename(dir)}-${Math.floor(i / 10) + 1}`, history: 3 + (i % 5) }));
    }
    // A few are mid-turn as she sits down.
    for (const s of this.s.slice(10, 14)) fleet.runTurn(s, "keep going on the refactor", { workMs: 40000 });
  },

  async run({ d, fleet, page }) {
    const [a, b, c, e, f] = this.s;
    await d.step("a permission request arrives", "notice a request from a session she is not looking at, and allow it", async () => {
      await d.selectSession(this.s[5].sid);
      const t0 = Date.now();
      fleet.park(this.s[9], "Bash", { command: "npm publish --dry-run" });
      await d.waitFor("the decision bar shows it", "#decide:not([hidden]) .dq [data-dv=allow]", { timeout: 20000 });
      d.observe("visibleAfterMs", Date.now() - t0);
      d.observe("tileMarkedWaiting", await page.locator(`${d.tile(this.s[9].sid)}`).innerText().then(t => /wait|permission|needs/i.test(t)));
      await d.click("#decide [data-dv=allow]", { decide: true });
      const ev = await d.fleetEvent("the session gets the answer", ev => ev.kind === "decided" && ev.sid === this.s[9].sid, { since: t0, timeout: 20000 });
      d.observe("verdict", ev.verdict);
      d.observe("parkToVerdictMs", ev.ms);
    });

    await d.step("three requests at once", "clear a burst of three permission requests", async () => {
      const t0 = Date.now();
      for (const s of [a, b, c]) fleet.park(s, "Edit", { file_path: path.join(s.cwd, "src", "mod2.py") });
      await d.waitFor("all three in the bar", { fn: () => document.querySelectorAll("#decide .dq").length >= 3, arg: null }, { timeout: 20000 });
      d.observe("allVisibleAfterMs", Date.now() - t0);
      for (let i = 0; i < 3; i++) {
        await d.click("#decide .dq [data-dv=allow]", { decide: true });
        await d.waitFor("the row goes", { fn: n => document.querySelectorAll("#decide .dq").length < n, arg: 3 - i }, { timeout: 10000 });
      }
      d.observe("decidedAfterMs", Date.now() - t0);
    });

    await d.step("answer a question", "a session asks which approach to take; pick one", async () => {
      const t0 = Date.now();
      fleet.park(e, "AskUserQuestion", { questions: [{ question: "Which cache should the search index use?", header: "Cache",
        multiSelect: false, options: [{ label: "Redis", description: "shared, needs a server" }, { label: "In-process LRU", description: "simple, per instance" }] }] });
      await d.waitFor("the question card", "#decide .dqq .qo", { timeout: 20000 });
      d.observe("visibleAfterMs", Date.now() - t0);
      await d.click("#decide .dqq .qo button", { decide: true, nth: 1 });
      d.observe("submitButtons", await page.locator("#decide .dqq .qa button").allInnerTexts());
      await d.click(`#decide .dqq .qa button:has-text("Submit")`, { decide: false });
      const ev = await d.fleetEvent("the answer reaches it", ev => ev.kind === "decided" && ev.sid === e.sid, { since: t0, timeout: 20000 });
      d.observe("verdict", ev.verdict);
    });

    await d.step("find the waiting one with n", "a session is blocked somewhere in the list; jump to it", async () => {
      const t0 = Date.now();
      const s = this.s[15];
      fleet.park(s, "Bash", { command: "terraform apply" });
      await d.waitFor("it is flagged", "#decide:not([hidden]) .dq", { timeout: 20000 });
      await page.locator("#log").click();
      const visited = [];
      let presses = 0;
      while (presses < 20) {
        await d.press("n", { shortcut: presses === 0 });
        presses++;
        await d.pause(250);
        const on = await page.locator("#sessions .s.on").getAttribute("data-k").catch(() => null);
        visited.push(await page.locator("#sessions .s.on .n").innerText().catch(() => "?"));
        if (on === s.sid) break;
        d.op("M"); // read the tile: not this one
      }
      d.observe("pressesToReachBlocked", presses);
      d.observe("visited", visited);
      d.observe("attentionCount", await page.evaluate(() => document.querySelectorAll("#sessions .s").length));
      if (presses > 1) d.note(`n visits sessions by how long they have waited, not by what they wait for: ${presses} presses to reach the one blocked on a permission request`);
      d.observe("jumpMs", Date.now() - t0);
      await d.click("#decide [data-dv=deny]", { decide: true });
    });

    await d.step("nudge five sessions in turn", "send each of five sessions its next instruction", async () => {
      const per = [];
      for (const s of [a, b, c, f, this.s[6]]) {
        const t = Date.now();
        await d.selectSession(s.sid);
        const lat = await d.send(s.sid, `Next for ${s.name}: tidy the error messages`, { waitReply: false });
        per.push({ ms: Date.now() - t, deliverMs: lat.deliverMs, echoMs: lat.echoMs });
      }
      d.observe("perSession", per);
    });

    await d.step("narrow the list", "filter to the billing sessions, then group by folder", async () => {
      await page.locator("#log").click();
      await d.press("/", { shortcut: true });
      await d.type("#filter", "billing");
      await d.waitFor("the list narrows", { fn: () => document.querySelectorAll("#sessions .s").length <= 2, arg: null }, { timeout: 5000 });
      d.observe("tilesShown", await page.locator("#sessions .s").count());
      await d.type("#filter", "");
      await d.select("#group", "folder");
      await d.waitFor("folder groups", "#sessions .fam, #sessions .g", { timeout: 5000 });
      d.observe("groups", await page.locator("#sessions .fam, #sessions .g").count());
    });

    await d.step("open the chat matrix", "see every session's latest reply at once", async () => {
      const t0 = Date.now();
      const popup = page.context().waitForEvent("page", { timeout: 5000 }).catch(() => null);
      await d.click("#matrixbtn", { decide: true });
      const p2 = await popup;
      const where = p2 || page;
      d.observe("opensNewWindow", !!p2);
      await where.locator("#grid .tile").first().waitFor({ timeout: 15000 });
      const ms = Date.now() - t0; d.cur.waitMs += ms; d.cur.waits.push({ label: "matrix tiles", ms });
      d.observe("matrixTiles", await where.locator("#grid .tile").count());
      d.observe("loadMs", ms);
      if (p2) await p2.close(); else await d.click("#matrixbtn");
    });

    await d.step("ask the fleet", "ask which sessions are stuck, in plain words", async () => {
      await d.click("#fleetrow", { decide: true });
      await d.waitFor("the fleet view", "#fleetview:not([hidden])", { timeout: 5000 });
      await d.type("#prompt", "which sessions are stuck or waiting on me?", { content: true });
      const t0 = Date.now();
      await d.click("#send");
      await d.waitFor("an answer", { fn: () => /simulated answer|on track/i.test(document.querySelector("#flchat")?.innerText || ""), arg: null }, { timeout: 60000 });
      d.observe("answerMs", Date.now() - t0);
    });
  },
};
