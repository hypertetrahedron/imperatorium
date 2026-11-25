// Priya: lives in slash commands. /model and /effort pickers, the /config
// view, the /resume picker, the prompt box's autocomplete, a skill the mod
// runs, and Send As Typed.
import { makeProject } from "../lib/sandbox.mjs";

export default {
  id: "priya",
  name: "Priya, slash-command user",
  set: 2,
  config: { chat: { send_as_typed: true } },
  fleet: { workMs: [4000, 6000] },
  projects(sb) { this.app = makeProject(sb.projects, "storefront"); this.lib = makeProject(sb.projects, "ui-kit"); },
  async setup({ fleet }) {
    this.s = await fleet.addInteractive(this.app, { history: 12 });
    await fleet.addInteractive(this.lib, { history: 3 });
    fleet.pastSessions(this.app, 20);
    fleet.pastSessions(this.lib, 10);
  },

  /** Types a slash command and presses Enter; returns what the window opened. */
  async slash(d, text) {
    await d.type("#prompt", text, { content: false });
    await d.press("Escape").catch(() => {}); // close the autocomplete list if it opened
    await d.page.locator("#prompt").focus();
    const t0 = Date.now();
    await d.press("Enter");
    return t0;
  },

  async run({ d, fleet, page }) {
    const sid = this.s.sid;
    await d.step("autocomplete a command", "type /re and pick a command from the list", async () => {
      await d.selectSession(sid);
      await d.type("#prompt", "/re", { content: false });
      const t0 = Date.now();
      await d.waitFor("the command list", "#slashlist:not([hidden]) .slopt", { timeout: 8000 });
      d.observe("listMs", Date.now() - t0);
      d.observe("options", await page.locator("#slashlist .slopt").allInnerTexts().then(a => a.map(x => x.replace(/\s+/g, " ").slice(0, 70))));
      await d.press("ArrowDown");
      await d.press("Tab");
      d.observe("completedTo", await page.locator("#prompt").inputValue());
      await page.locator("#prompt").fill("");
    });

    await d.step("/model picker", "switch the session's model from the window", async () => {
      let t0 = await this.slash(d, "/model");
      const first = await d.waitFor("the picker", "#slashbox:not([hidden]) [data-slval]", { timeout: 8000 }).then(() => true).catch(() => false);
      if (!first) {
        d.observe("firstOpen", (await page.locator("#slashbox").innerText()).replace(/\s+/g, " ").slice(0, 160));
        d.note("the first /model after the dispatcher starts shows 'nothing listed' (the model list is not loaded yet) and does not fill in when it arrives: close it and type /model again");
        await d.press("Escape");
        d.op("M");
        t0 = await this.slash(d, "/model");
        await d.waitFor("the picker, second try", "#slashbox:not([hidden]) [data-slval]", { timeout: 15000 });
      }
      d.observe("pickerMs", Date.now() - t0);
      d.observe("choices", await page.locator("#slashpane [data-slval]").allInnerTexts().then(a => a.map(x => x.replace(/\s+/g, " ").slice(0, 50))));
      const t1 = Date.now();
      await d.click('#slashpane [data-slval="sonnet"]', { decide: true });
      const ev = await d.fleetEvent("the session runs /model", e => e.kind === "op" && e.sid === sid, { since: t1, timeout: 20000 });
      d.observe("op", ev.command || ev.op);
      d.observe("reachedSessionMs", ev.t - t1);
      await d.pause(800);
      d.observe("sayAfter", await page.locator("#say").innerText().catch(() => ""));
      d.observe("pickerStillOpen", await page.locator("#slashbox:not([hidden])").count() > 0);
      if (await page.locator("#slashbox:not([hidden])").count()) await d.press("Escape");
    });

    await d.step("/effort picker", "raise the effort for a hard task", async () => {
      const t0 = await this.slash(d, "/effort");
      await d.waitFor("the picker", "#slashbox:not([hidden]) [data-slval]", { timeout: 15000 });
      d.observe("pickerMs", Date.now() - t0);
      d.observe("choices", await page.locator("#slashpane [data-slval]").allInnerTexts().then(a => a.map(x => x.trim().slice(0, 30))));
      const t1 = Date.now();
      await d.click('#slashpane [data-slval="high"]', { decide: true });
      const ev = await d.fleetEvent("the session runs /effort", e => e.kind === "op" && e.sid === sid, { since: t1, timeout: 20000 });
      d.observe("reachedSessionMs", ev.t - t1);
      if (await page.locator("#slashbox:not([hidden])").count()) await d.press("Escape");
    });

    await d.step("/config view", "check the session's settings", async () => {
      const t0 = await this.slash(d, "/config");
      await d.waitFor("the settings view", { fn: () => { const b = document.querySelector("#slashbox"); return b && !b.hidden && (document.querySelector("#slashpane")?.innerText || "").length > 20; }, arg: null }, { timeout: 15000 });
      d.observe("viewMs", Date.now() - t0);
      d.observe("title", await page.locator("#sltitle").innerText());
      d.observe("controls", await page.locator("#slashpane select, #slashpane input, #slashpane button").count());
      await d.press("Escape");
    });

    await d.step("/resume picker", "pick up an earlier session in this folder", async () => {
      const t0 = await this.slash(d, "/resume");
      await d.waitFor("History on this folder", "#histbox:not([hidden]) #hits .hit", { timeout: 15000 });
      d.observe("historyMs", Date.now() - t0);
      d.observe("folderNote", (await page.locator("#histbias").innerText().catch(() => "")).slice(0, 120));
      d.observe("hits", await page.locator("#hits .hit").count());
      d.observe("folderFilter", await page.locator("#hfolder").inputValue());
      await d.press("Escape");
    });

    await d.step("a skill the mod runs", "run /changelog-draft 2.0 in the session", async () => {
      const t0 = await this.slash(d, "/changelog-draft 2.0");
      const ev = await d.fleetEvent("it reaches the session", e => (e.kind === "op" || e.kind === "delivered") && e.sid === sid, { since: t0, timeout: 20000 });
      d.observe("via", ev.kind);
      d.observe("reachedSessionMs", ev.t - t0);
      await d.pause(600);
      d.observe("say", await page.locator("#say").innerText().catch(() => ""));
    });

    await d.step("Send As Typed", "send a line as if typed at the terminal", async () => {
      await d.fleetEvent("the session is idle", e => e.kind === "turn-end" && e.sid === sid, { since: 0, timeout: 1 }).catch(() => {});
      await d.type("#prompt", "Summarise the diff in one line", { content: true });
      const t0 = Date.now();
      await d.menu("#sendmore", "#sendtyped");
      const ev = await d.fleetEvent("it reaches the session", e => e.kind === "delivered" && e.sid === sid, { since: t0, timeout: 30000 });
      d.observe("deliverMs", ev.t - t0);
      d.observe("asUser", ev.as_user);
      d.observe("say", await page.locator("#say").innerText().catch(() => ""));
    });
  },
};
