// Nell: first day. No sessions, no project roots. Looks around Settings,
// adds a project root, tries to add a remote machine (the test fails, then
// key login), and starts her first session.
import fs from "node:fs";
import path from "node:path";
import { makeProject } from "../lib/sandbox.mjs";
import { newSession, tileShows } from "./common.mjs";

export default {
  id: "nell",
  name: "Nell, newcomer",
  set: 2,
  config: { roots: [] },
  expectTiles: 0,
  projects(sb) { this.root = path.join(sb.root, "code"); fs.mkdirSync(this.root); makeProject(this.root, "first-app"); },

  async openSettings(d) {
    const page = d.page;
    const popup = page.context().waitForEvent("page", { timeout: 5000 }).catch(() => null);
    await d.click("#setbtn", { decide: true });
    const p2 = (await popup) || page;
    await p2.locator("#set .f[data-key]").first().waitFor({ timeout: 15000 });
    return p2;
  },

  async run({ d, sb, page }) {
    await d.step("the empty first screen", "what does the window say with nothing running", async () => {
      d.op("M", 2); // reading it
      d.observe("sessionList", (await page.locator("#sessions").innerText()).slice(0, 200));
      d.observe("title", await page.locator("#title").innerText());
      d.observe("logText", (await page.locator("#log").innerText()).slice(0, 300));
      d.observe("promptPlaceholder", await page.locator("#prompt").getAttribute("placeholder"));
      const guides = /new session|get started|no sessions|start/i.test(await page.locator("#main").innerText() + await page.locator("#side").innerText());
      d.observe("pointsToNewSession", guides);
      if (!guides) d.note("an empty window does not say how to start: New session is behind the ☰ menu");
    });

    await d.step("try New session with no roots", "open New session before any project is known", async () => {
      await d.menu("#sidemenu", "#new");
      await d.waitFor("the dialog", "#newbox:not([hidden]) #find", { timeout: 5000 });
      await d.pause(800);
      d.observe("rows", await page.locator("#projects .p").count());
      d.observe("text", (await page.locator("#projects").innerText()).slice(0, 200));
      d.observe("note", await page.locator("#nbnote").innerText().catch(() => ""));
      await d.press("Escape");
    });

    let settings;
    await d.step("a tour of Settings", "open Settings and see what is there", async () => {
      const t0 = Date.now();
      settings = await this.openSettings(d);
      d.observe("opensNewWindow", settings !== page);
      d.observe("loadMs", Date.now() - t0);
      d.observe("settings", await settings.locator("#set .f[data-key]").count());
      d.observe("sections", (await settings.locator("#set h2, #set h3, #set legend").allInnerTexts()).slice(0, 30));
      d.observe("shortcuts", await settings.locator("#keys tr").count());
      d.op("M", 3); // scanning a long form
    });

    await d.step("add a project root", "tell it where her code lives", async () => {
      const f = settings.locator('#set .f[data-key="roots"]');
      await f.waitFor({ timeout: 5000 });
      await f.scrollIntoViewIfNeeded();
      d.op("M"); d.op("P"); d.op("B"); d.cur.clicks++;
      d.observe("help", (await f.innerText()).replace(/\s+/g, " ").slice(0, 200));
      const box = f.locator("textarea, input").first();
      await box.fill(this.root);
      d.op("K", this.root.length); d.cur.keys += this.root.length; // she types (or pastes) the path
      d.op("P"); d.op("B"); d.cur.clicks++;
      const t0 = Date.now();
      await settings.locator("#save").click();
      await settings.waitForFunction(() => /saved|applied|restart/i.test(document.querySelector("#msg")?.innerText || "") || !!document.querySelector("#set .bad"), null, { timeout: 15000 });
      const ms = Date.now() - t0; d.cur.waitMs += ms; d.cur.waits.push({ label: "saved", ms });
      d.observe("saveMs", ms);
      d.observe("msg", await settings.locator("#msg").innerText());
      d.observe("when", (await f.locator(".when").innerText().catch(() => "")).slice(0, 120));
      d.observe("refused", await settings.locator("#set .bad").count());
      d.observe("restartNote", (await settings.locator("#wait").innerText().catch(() => "")).replace(/\s+/g, " ").slice(0, 300));
    });

    await d.step("add a remote machine", "add her cloud box; the first test fails", async () => {
      fs.writeFileSync(path.join(sb.bin, "auth-cloudbox"), "");
      const sec = settings.getByText("Remote machines", { exact: true }).first();
      if (await sec.count()) { await sec.scrollIntoViewIfNeeded(); await sec.click().catch(() => {}); d.op("M"); d.op("P"); d.op("B"); d.cur.clicks++; }
      await settings.locator("#radd-host").waitFor({ timeout: 10000 });
      await settings.locator("#radd-host").fill("cloudbox");
      d.op("P"); d.op("B"); d.op("K", 8); d.cur.keys += 8; d.cur.clicks++;
      d.op("P"); d.op("B"); d.cur.clicks++;
      const t0 = Date.now();
      await settings.locator("#radd-test").click();
      await settings.locator("#radd-res:not([hidden])").waitFor({ timeout: 30000 });
      await settings.waitForFunction(() => (document.querySelector("#radd-res")?.innerText || "").length > 10 && !/testing|checking…/i.test(document.querySelector("#radd-res").innerText), null, { timeout: 30000 }).catch(() => {});
      const ms = Date.now() - t0; d.cur.waitMs += ms; d.cur.waits.push({ label: "the test result", ms });
      d.observe("testMs", ms);
      d.observe("result", (await settings.locator("#radd-res").innerText()).slice(0, 400));
      d.op("M", 2); // reading why it failed
      const key = settings.locator("[data-keyfor]");
      d.observe("offersKeyLogin", await key.count() > 0);
      if (await key.count()) {
        d.op("P"); d.op("B"); d.cur.clicks++;
        const t1 = Date.now();
        await key.first().click();
        await settings.waitForFunction(() => /ssh|copy|key/i.test(document.querySelector("#radd-res")?.innerText || ""), null, { timeout: 20000 }).catch(() => {});
        d.observe("keyMs", Date.now() - t1);
        d.observe("keyResult", (await settings.locator("#radd-res").innerText()).slice(0, 400));
        d.observe("keyMadeInSandbox", fs.existsSync(path.join(sb.userHome, ".ssh")));
      }
      fs.rmSync(path.join(sb.bin, "auth-cloudbox"));
      d.op("M"); // "now run the command, then test again"
      d.op("P"); d.op("B"); d.cur.clicks++;
      const t2 = Date.now();
      await settings.locator("#radd-test").click();
      await settings.waitForFunction(() => /ok|ready|✓|found|claude/i.test(document.querySelector("#radd-res")?.innerText || ""), null, { timeout: 30000 }).catch(() => d.note("the second test did not say the host is ready"));
      d.observe("retestMs", Date.now() - t2);
      d.observe("retest", (await settings.locator("#radd-res").innerText()).slice(0, 400));
      if (settings !== page) await settings.close();
    });

    await d.step("first New session", "start a session in her project", async () => {
      await page.reload();
      await d.menu("#sidemenu", "#new");
      await d.waitFor("the dialog", "#newbox:not([hidden]) #find", { timeout: 5000 });
      const t0 = Date.now();
      await d.waitFor("the project list", "#projects .p", { timeout: 15000 });
      d.observe("rows", await page.locator("#projects .p").allInnerTexts().then(a => a.map(x => x.split("\n")[0])));
      await d.type("#find", "first");
      const listed = await d.waitFor("her project is listed", { fn: () => [...document.querySelectorAll("#projects .p")].some(p => /first-app/.test(p.innerText)), arg: null }, { timeout: 5000 }).then(() => true).catch(() => false);
      d.observe("firstAppListed", listed);
      if (!listed) {
        d.note("the project root she saved is not used until the dispatcher restarts, and the window offers no restart: she has to find the command and run it in a terminal");
        await d.press("Escape");
        d.op("M", 3); d.op("H"); d.op("K", 30); // finding the restart command, switching to a terminal, typing it
        const tr = Date.now();
        await sb.restart();
        await page.reload();
        await page.locator("#sessions").waitFor({ timeout: 30000 });
        const rms = Date.now() - tr; d.cur.waitMs += rms; d.cur.waits.push({ label: "the dispatcher restarts", ms: rms });
        d.observe("restartMs", rms);
        await d.menu("#sidemenu", "#new");
        await d.waitFor("the dialog", "#newbox:not([hidden]) #find", { timeout: 5000 });
        await d.type("#find", "first");
        await d.waitFor("her project is listed now", { fn: () => [...document.querySelectorAll("#projects .p")].some(p => /first-app/.test(p.innerText)), arg: null }, { timeout: 15000 });
      }
      d.observe("listedMs", Date.now() - t0);
      d.observe("modeDefault", await page.locator("#nmode").inputValue());
      d.op("M"); // reading six permission modes
      const t1 = Date.now();
      await d.click("#nstart", { decide: true });
      const sid = await newSession(d, "the session starts", t1);
      d.observe("tileAfterMs", await tileShows(d, sid, t1));
      await d.waitFor("it is selected", { fn: sid => !!document.querySelector(`#sessions .s.on[data-k="${sid}"]`), arg: sid }, { timeout: 15000 })
        .then(() => d.observe("selectedAfterMs", Date.now() - t1)).catch(() => d.note("the new session was not selected"));
    });
  },
};
