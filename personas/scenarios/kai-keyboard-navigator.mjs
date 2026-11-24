// Kai: keyboard only, long-lived sessions with very long conversations.
// Lives in ctrl+K, j/k, find and prompt jumps.
import { makeProject } from "../lib/sandbox.mjs";

export default {
  id: "kai",
  name: "Kai, keyboard navigator",
  set: 1,
  projects(sb) { this.dirs = ["compiler", "runtime", "stdlib", "lsp", "bench", "website"].map(p => makeProject(sb.projects, p)); },
  async setup({ fleet }) {
    this.s = [];
    for (const [i, dir] of this.dirs.entries()) this.s.push(await fleet.addInteractive(dir, { history: i === 0 ? 400 : 15 + i * 5 }));
  },

  async run({ d, fleet, page }) {
    const big = this.s[0];
    await page.locator("#log").click();
    await d.step("ctrl+K to a session", "jump to the stdlib session by name", async () => {
      const t0 = Date.now();
      await d.press("Control+k", { shortcut: true });
      await d.waitFor("the jump box", "#jumpbox:not([hidden]) #jq", { timeout: 3000 });
      await d.type("#jq", "stdl");
      await d.press("Enter");
      await d.waitFor("stdlib is open", { fn: sid => !!document.querySelector(`#sessions .s.on[data-k="${sid}"]`) && document.querySelectorAll("#log .turn").length > 0, arg: this.s[2].sid }, { timeout: 8000 });
      d.observe("ms", Date.now() - t0);
    });

    await d.step("j/k through the list", "step down and up through five sessions", async () => {
      await page.locator("#log").click(); // focus out of any field
      const times = [];
      for (const k of ["j", "j", "j", "k", "k"]) {
        const before = await page.locator("#sessions .s.on").getAttribute("data-k");
        const t = Date.now();
        await d.press(k);
        await d.waitFor("the next one opens", { fn: b => { const o = document.querySelector("#sessions .s.on"); return o && o.dataset.k !== b; }, arg: before }, { timeout: 5000 });
        times.push(Date.now() - t);
      }
      d.observe("stepMs", times);
    });

    await d.step("open the 400-turn session", "open the compiler session's very long conversation", async () => {
      const t0 = Date.now();
      await d.press("Alt+1", { shortcut: true });
      const k1 = await page.locator("#sessions .s").first().getAttribute("data-k");
      if (k1 !== big.sid) { d.note("alt+1 is the first tile in list order, not the compiler session; used ctrl+K"); await d.press("Control+k", { shortcut: true }); await d.type("#jq", "compiler"); await d.press("Enter"); }
      await d.waitFor("its chat shows", { fn: sid => !!document.querySelector(`#sessions .s.on[data-k="${sid}"]`) && document.querySelectorAll("#log .turn").length > 5, arg: big.sid }, { timeout: 30000 });
      d.observe("openMs", Date.now() - t0);
      d.observe("turnsRendered", await page.locator("#log .turn").count());
      d.observe("domNodes", await page.evaluate(() => document.getElementsByTagName("*").length));
    });

    await d.step("reach the start of the conversation", "go back to how it began", async () => {
      let clicks = 0; const t0 = Date.now();
      while (clicks < 40 && await page.locator("#log .earlier").count()) {
        const n = await page.locator("#log .turn").count();
        await d.click("#log .earlier");
        clicks++;
        await d.waitFor("earlier turns load", { fn: n => document.querySelectorAll("#log .turn").length > n || !document.querySelector("#log .earlier"), arg: n }, { timeout: 15000 });
      }
      d.observe("earlierClicks", clicks);
      d.observe("ms", Date.now() - t0);
      d.observe("turnsRendered", await page.locator("#log .turn").count());
      d.observe("reachedTaskOne", /Earlier task 1:/.test(await page.locator("#log").innerText()));
    });

    await d.step("find in a long chat", "find every pytest run, step through three", async () => {
      const t0 = Date.now();
      await d.press("Control+f", { shortcut: true });
      await d.waitFor("the find window", "#findpop:not([hidden]) #cfind", { timeout: 3000 });
      await d.type("#cfind", "pytest");
      await d.waitFor("a count", { fn: () => /\d/.test(document.querySelector("#cfcount")?.innerText || ""), arg: null }, { timeout: 10000 });
      d.observe("countText", await page.locator("#cfcount").innerText());
      d.observe("firstResultMs", Date.now() - t0);
      for (let i = 0; i < 3; i++) await d.press("Enter");
      await d.press("Escape");
    });

    await d.step("jump between prompts", "alt+Up three prompts back", async () => {
      await page.locator("#prompt").click();
      const t0 = Date.now();
      for (let i = 0; i < 3; i++) await d.press("Alt+ArrowUp", { shortcut: i === 0 });
      d.observe("ms", Date.now() - t0);
      d.observe("scrollTop", await page.locator("#log").evaluate(e => e.scrollTop));
    });

    await d.step("n to the one that needs me", "a session wants permission: get there by keyboard", async () => {
      const s = this.s[4];
      fleet.park(s, "Bash", { command: "cargo bench" });
      await d.waitFor("the request shows", "#decide:not([hidden]) .dq", { timeout: 20000 });
      await page.locator("#log").click();
      const t0 = Date.now();
      await d.press("n", { shortcut: true });
      await d.waitFor("it is open", { fn: sid => !!document.querySelector(`#sessions .s.on[data-k="${sid}"]`), arg: s.sid }, { timeout: 8000 });
      d.observe("ms", Date.now() - t0);
      // Allowing it: is there a key?
      const keyHint = await page.locator("#decide").innerText();
      d.observe("decisionKeyHint", /\b(y|a|enter|ctrl)\b/i.test(keyHint));
      await d.click("#decide [data-dv=allow]", { decide: true });
      d.note("permission rows have no key to allow or deny (question cards do: 1-9, Enter); the mouse was needed");
    });
  },
};
