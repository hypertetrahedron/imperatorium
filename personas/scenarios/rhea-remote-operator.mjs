// Rhea: one long-running session on a remote dev box (a container reached
// over SSH), driven from this machine's window. Nothing runs locally.
import fs from "node:fs";
import path from "node:path";

const PORT = 41000 + Math.floor(Math.random() * 2000);

export default {
  id: "rhea",
  name: "Rhea, remote container operator",
  set: 1,
  config: { remote: { enabled: true, hosts: [{ name: "devbox", label: "devbox", claude: "c", tunnel: { local_port: PORT, remote_port: 8792 } }] } },
  fleet: { workMs: [5000, 5000] },
  expectTiles: 1,
  loadTimeout: 90000,
  async setup({ fleet }) { this.h = fleet.remoteHost("devbox", { sessions: 1, tunnelPort: PORT }); this.sid = [...this.h.sessions.keys()][0]; },

  async run({ d, sb, fleet, page }) {
    const sid = this.sid;
    const tileSel = `#sessions .s`;
    d.steps[0].observations.note = "time from page load; the dispatcher had started a moment before";

    await d.step("open the remote session", "read what the container session has been doing", async () => {
      await d.click(tileSel, { decide: true });
      await d.waitFor("its chat loads over SSH", { fn: () => document.querySelectorAll("#log .turn").length > 0, arg: null }, { timeout: 30000 });
      d.observe("key", await page.locator(tileSel).first().getAttribute("data-k"));
    });

    await d.step("send to the remote session", "give it the next task and see the answer", async () => {
      await d.type("#prompt", "Rebuild the container image and run the smoke tests", { content: true });
      const t0 = Date.now();
      d.observe("sendButtonDisabled", await page.locator("#send").isDisabled());
      d.observe("sendButtonTitle", await page.locator("#send").getAttribute("title"));
      if (await page.locator("#send").isDisabled()) {
        d.note("Send is greyed out for a remote session, with no reason on screen; Enter in the prompt box sends anyway");
        d.op("M"); // working out why, and trying the keyboard
        await d.press("Enter");
      } else await d.click("#send");
      const del = await d.fleetEvent("the prompt reaches the host", e => e.kind === "delivered" && e.remote, { since: t0, timeout: 30000, sits: false });
      d.observe("deliverMs", del.t - t0);
      const sayText = await page.locator("#say").innerText().catch(() => "");
      d.observe("sayAfterSend", sayText);
      const end = await d.fleetEvent("the turn ends on the host", e => e.kind === "turn-end" && e.remote, { since: t0, timeout: 60000, sits: false });
      await d.waitFor("the reply shows in the window", { fn: () => (document.querySelector("#log")?.innerText.match(/CLAUDE/g) || []).length > 0 &&
        /smoke tests/.test(document.querySelector("#log").innerText) && !document.querySelector("#log .turn.pend"), arg: null }, { timeout: 90000 })
        .catch(err => d.note("the reply never replaced the pending prompt: " + err.message));
      d.observe("turnEndMs", end.t - t0);
      d.observe("replyVisibleMs", Date.now() - t0);
      d.observe("replyLagAfterTurnEndMs", Date.now() - end.t);
    });

    await d.step("test the host from Settings", "check the box is healthy from the Settings window", async () => {
      const t0 = Date.now();
      const popup = page.context().waitForEvent("page", { timeout: 5000 }).catch(() => null);
      await d.click("#setbtn", { decide: true });
      const p2 = (await popup) || page;
      d.observe("settingsOpensNewWindow", p2 !== page);
      await p2.getByText("Remote machines", { exact: true }).first().waitFor({ timeout: 15000 });
      d.op("M"); d.op("P"); d.op("B"); d.cur.clicks++; // finding and opening the Remote machines section
      await p2.getByText("Remote machines", { exact: true }).first().click();
      await p2.locator('[data-test="devbox"]').waitFor({ timeout: 15000 });
      await p2.locator('[data-test="devbox"]').scrollIntoViewIfNeeded();
      d.cur.clicks++; d.op("P"); d.op("B");
      const t1 = Date.now();
      await p2.locator('[data-test="devbox"]').click();
      await p2.waitForFunction(() => /ok|reachable|✓|fail|missing|refused/i.test(document.querySelector("[data-test='devbox']")?.closest("div, li, tr")?.innerText || ""), null, { timeout: 30000 }).catch(() => {});
      const ms = Date.now() - t1; d.cur.waitMs += ms; d.cur.waits.push({ label: "test answer", ms });
      d.observe("testMs", ms);
      d.observe("testText", (await p2.locator("[data-test='devbox']").locator("xpath=..").innerText().catch(() => "")).slice(0, 400));
      if (p2 !== page) await p2.close(); else await page.goto(sb.url + "/");
      d.observe("totalMs", Date.now() - t0);
    });

    await d.step("the host drops", "the container restarts; how soon does the window say so", async () => {
      fs.writeFileSync(path.join(sb.bin, "down-devbox"), "");
      const t0 = Date.now();
      const SAYS = /not answering|unreachable|offline|lost|refused|can't reach|cannot reach|not reachable/i;
      // Either it says so, or the session just goes.
      await d.waitFor("the window reacts", { fn: () => /not answering|unreachable|offline|lost|refused|can't reach|cannot reach|not reachable/i.test(document.querySelector("#side").innerText + (document.querySelector("#sub")?.innerText || "")) || !document.querySelector("#sessions .s"), arg: null }, { timeout: 120000 });
      d.observe("reactedAfterMs", Date.now() - t0);
      const said = SAYS.test(await page.locator("#side").innerText());
      d.observe("windowSaysHostUnreachable", said);
      d.observe("sessionStillListed", await page.locator("#sessions .s").count() > 0);
      if (!said) d.note("the host's session vanished from the list with no word that the host is unreachable");
      d.observe("tileText", (await page.locator("#sessions").innerText()).slice(0, 300));
      d.observe("hostLine", (await page.locator("#hostline").innerText().catch(() => "")).slice(0, 200));
      d.observe("sendDisabled", await page.locator("#send").isDisabled());
      d.observe("sendTitle", await page.locator("#send").getAttribute("title"));
      await d.type("#prompt", "are you still there?", { content: true });
      await d.press("Enter");
      await d.pause(2000);
      d.observe("sayAfterSendWhileDown", await page.locator("#say").innerText().catch(() => ""));
    });

    await d.step("the host comes back", "how soon is the session usable again", async () => {
      fs.rmSync(path.join(sb.bin, "down-devbox"));
      const t0 = Date.now();
      const retry = page.locator("#sessions [data-retry], #hostline [data-retry]").first();
      if (await retry.count()) {
        await d.click("#sessions [data-retry], #hostline [data-retry]", { decide: true });
        d.observe("usedRetryNow", true);
      } else d.note("no way in the window to poll the host again now: the wait is the dispatcher's retry schedule");
      await d.waitFor("the session is listed and answering again", { fn: () => !!document.querySelector("#sessions .s") && !document.querySelector("#sessions .w.unr"), arg: null }, { timeout: 240000 });
      d.observe("recoveredAfterMs", Date.now() - t0);
    });
  },
};
