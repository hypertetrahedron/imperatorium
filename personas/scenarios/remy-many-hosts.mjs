// Remy: five remote machines, sessions on each, one of them flapping.
// Scales rhea up: how the list behaves while a host comes and goes, what
// Refresh now does, and starting background work on a host.
import fs from "node:fs";
import path from "node:path";

const HOSTS = ["gpu1", "gpu2", "build", "staging", "pi"];
const BASE = 43000 + Math.floor(Math.random() * 1500);

export default {
  id: "remy",
  name: "Remy, many hosts",
  set: 3,
  config: { remote: { enabled: true, hosts: HOSTS.map((h, i) => ({ name: h, label: h, claude: "c", tunnel: { local_port: BASE + i, remote_port: 8792 } })) } },
  fleet: { workMs: [5000, 5000] },
  expectTiles: 9,
  loadTimeout: 120000,
  async setup({ fleet }) {
    this.h = HOSTS.map((h, i) => fleet.remoteHost(h, { sessions: i === 4 ? 1 : 2, tunnelPort: BASE + i, cwdBase: "/home/remy/ml-platform" })); // the same checkout on every machine
  },

  async run({ d, sb, fleet, page }) {
    d.steps[0].observations.note = "time from page load until every host's sessions are listed";
    const count = () => page.locator("#sessions .s").count();

    await d.step("group by host", "see which machine each session is on", async () => {
      d.observe("groupOptions", await page.locator("#group option").allInnerTexts());
      const byHost = await page.locator('#group option[value="host"]').count();
      await d.select("#group", byHost ? "host" : "folder");
      await d.pause(300);
      d.observe("groups", await page.locator("#sessions .g, #sessions .fam").allInnerTexts().then(a => a.map(x => x.replace(/\s+/g, " ").trim().slice(0, 40))));
      const lines = await page.locator("#sessions .s").evaluateAll(ts => ts.map(t => t.innerText.replace(/\s+/g, " ").trim().slice(0, 70)));
      d.observe("tileLines", lines);
      d.observe("tilesNamingTheirHost", await page.locator("#sessions .s .hb").count());
      if (!byHost) d.note("no 'by host' grouping: a host shows only on the open tile's Meta Line");
    });

    await d.step("a host flaps", "build drops and returns every 20 s for a minute: watch the list", async () => {
      const down = path.join(sb.bin, "down-build");
      const samples = [];
      const t0 = Date.now();
      let isDown = false, flips = 0;
      while (Date.now() - t0 < 75000) {
        const el = Date.now() - t0;
        const want = Math.floor(el / 20000) % 2 === 0;
        if (want !== isDown && el < 60000) { isDown = want; flips++; if (isDown) fs.writeFileSync(down, ""); else fs.rmSync(down, { force: true }); }
        samples.push({ s: Math.round(el / 1000), tiles: await count(), unreachable: await page.locator("#sessions .w.unr").count(), hostLine: !(await page.locator("#hostline").isHidden()) });
        await d.pause(1000);
      }
      fs.rmSync(down, { force: true });
      const key = x => `${x.tiles}/${x.unreachable}/${x.hostLine}`;
      d.observe("flips", flips);
      d.observe("listChanges", samples.filter((x, i) => i && key(x) !== key(samples[i - 1])).length);
      d.observe("samples", samples.filter((x, i) => !i || key(x) !== key(samples[i - 1])));
      d.observe("tilesNeverDropped", samples.every(x => x.tiles === 9));
      d.observe("windowSaysHostDown", samples.some(x => x.unreachable > 0 || x.hostLine));
    });

    await d.step("Retry now after the host is back", "make the window see build again without waiting", async () => {
      fs.writeFileSync(path.join(sb.bin, "down-build"), "");
      await d.waitFor("build is marked not answering", { fn: () => !!document.querySelector("#sessions .w.unr") || !!document.querySelector("#hostline:not([hidden])") || document.querySelectorAll("#sessions .s").length < 9, arg: null }, { timeout: 120000 });
      fs.rmSync(path.join(sb.bin, "down-build"));
      const t0 = Date.now();
      const inMain = await page.locator("#sessions [data-retry], #hostline [data-retry]").count();
      d.observe("retryInMainWindow", inMain > 0);
      if (inMain) await d.click("#sessions [data-retry], #hostline [data-retry]", { decide: true });
      else {
        const popup = page.context().waitForEvent("page", { timeout: 5000 }).catch(() => null);
        await d.click("#setbtn", { decide: true });
        const p2 = (await popup) || page;
        await p2.getByText("Remote machines", { exact: true }).first().waitFor({ timeout: 15000 });
        await p2.locator("#rrefresh").click();
        if (p2 !== page) await p2.close();
      }
      await d.waitFor("every session listed and answering again", { fn: () => document.querySelectorAll("#sessions .s").length >= 9 && !document.querySelector("#sessions .w.unr"), arg: null }, { timeout: 120000 });
      d.observe("recoveredMs", Date.now() - t0);
    });

    await d.step("send to a session on each host", "a quick nudge to one session per machine", async () => {
      const times = [];
      for (const h of this.h) {
        const s = [...h.sessions.values()][0];
        const tile = page.locator(`#sessions .s[data-k*="${s.sid}"]`);
        if (!(await tile.count())) { d.note(`${h.host}'s session is not listed`); continue; }
        await d.selectSession(await tile.first().getAttribute("data-k"));
        await d.type("#prompt", `status check on ${h.host}`, { content: true });
        const t0 = Date.now();
        await d.press("Enter");
        const ev = await d.fleetEvent(`${h.host} gets it`, e => e.kind === "delivered" && e.remote === h.host, { since: t0, timeout: 30000, sits: false }).catch(() => null);
        times.push({ host: h.host, ms: ev ? ev.t - t0 : null });
      }
      d.observe("deliverMs", times);
    });

    await d.step("background work on a host", "start a background job on gpu1 from the window", async () => {
      await d.menu("#sidemenu", "#new");
      await d.waitFor("the dialog", "#newbox:not([hidden]) #find", { timeout: 5000 });
      await d.type("#find", "gpu1");
      await d.pause(800);
      const rows = await page.locator("#projects .p").allInnerTexts();
      d.observe("rows", rows.map(x => x.slice(0, 60)));
      if (!rows.length) { d.note("the New session dialog lists no project on a remote host"); await d.press("Escape"); throw new Error("no remote project in New session"); }
      d.observe("backgroundBoxDisabled", await page.locator("#nbg").isDisabled());
      if (!(await page.locator("#nbg").isDisabled())) await d.click("#nbg", { decide: true });
      d.observe("remoteNote", await page.locator("#nbnote").innerText().catch(() => ""));
      await d.click("#nstart", { decide: true });
      await d.pause(3000);
      d.observe("tilesAfter", await page.locator("#sessions .s").count());
      d.observe("say", await page.locator("#say").innerText().catch(() => ""));
      d.observe("sshLaunches", fleet.events.filter(e => e.kind === "launch" && e.prog === "ssh" && /--bg|background/.test((e.args || []).join(" "))).length);
    }, { optional: true });
  },
};
