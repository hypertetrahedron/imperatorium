// Runs persona scenarios against a sandboxed dispatcher and records metrics.
//
//   node run.mjs                 every persona in set 1
//   node run.mjs --set 2         every persona in set 2
//   node run.mjs wren tessa      just those
//   --headed                     show the browser   --keep  keep sandboxes
//
// Results: results/<timestamp>/<persona>.json, screenshots beside them,
// then `node report.mjs results/<timestamp>` writes report.md.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";
import { Sandbox, sleep } from "./lib/sandbox.mjs";
import { Fleet } from "./lib/sim.mjs";
import { Driver } from "./lib/driver.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const flag = f => args.includes(f);
const setArg = args.includes("--set") ? +args[args.indexOf("--set") + 1] : null;
const names = args.filter((a, i) => !a.startsWith("--") && args[i - 1] !== "--set");

const all = [];
for (const f of fs.readdirSync(path.join(HERE, "scenarios")).filter(f => f.endsWith(".mjs") && f !== "common.mjs").sort()) {
  const m = (await import(pathToFileURL(path.join(HERE, "scenarios", f)))).default;
  all.push(m);
}
const chosen = names.length ? all.filter(p => names.includes(p.id)) : all.filter(p => p.set === (setArg ?? 1));
if (!chosen.length) { console.error("no personas chosen; known:", all.map(p => `${p.id} (set ${p.set})`).join(", ")); process.exit(2); }

const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
const outDir = path.join(HERE, "results", stamp);
fs.mkdirSync(outDir, { recursive: true });
console.log("results:", outDir);

const browser = await chromium.launch({ channel: "chrome", headless: !flag("--headed") });
for (const persona of chosen) {
  const t0 = Date.now();
  console.log(`\n== ${persona.name}`);
  const sb = new Sandbox(persona.id, { config: persona.config || {}, policy: persona.policy, keep: flag("--keep") });
  const result = { persona: persona.id, name: persona.name, set: persona.set, started: new Date().toISOString() };
  let fleet, page, d, mem = [], memTimer;
  try {
    persona.projects?.(sb);
    await sb.start();
    result.dispatcherBootMs = sb.bootMs;
    fleet = new Fleet(sb, persona.fleet || {});
    fleet.watchLaunches();
    await persona.setup?.({ sb, fleet });
    memTimer = setInterval(() => mem.push({ t: Date.now() - t0, ...sb.procStats() }), 5000);
    const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    page = await ctx.newPage();
    d = new Driver(page, { sb, fleet, outDir, persona: persona.id });
    await d.step("open the window", "load the window and see every session", async () => {
      const tl = Date.now();
      await page.goto(sb.url + "/");
      d.op("M");
      const want = persona.expectTiles ?? [...fleet.sessions.values()].filter(s => !s.gone).length;
      await d.waitFor(`all ${want} tiles listed`, { fn: n => document.querySelectorAll("#sessions .s").length >= n, arg: want }, { timeout: persona.loadTimeout || 30000 });
      d.observe("tilesAfterMs", Date.now() - tl);
      d.observe("tiles", await page.locator("#sessions .s").count());
    });
    await persona.run({ d, sb, fleet, page });
    await d.shot("end");
  } catch (e) {
    result.fatal = String(e.stack || e);
    console.error(e);
    if (page) await page.screenshot({ path: path.join(outDir, `${persona.id}-fatal.png`) }).catch(() => {});
  } finally {
    clearInterval(memTimer);
    if (sb.proc) mem.push({ t: Date.now() - t0, ...sb.procStats() });
    result.durationMs = Date.now() - t0;
    result.steps = d?.steps || [];
    result.requests = d?.allRequests || [];
    result.consoleErrors = d?.consoleErrors || [];
    result.fleetEvents = fleet?.events || [];
    result.launches = sb.launches?.() || [];
    result.leaks = sb.outwardLeaks?.() || {};
    result.memory = mem;
    try {
      const log = fs.readFileSync(sb.logFile, "utf8");
      result.serveLogTail = log.split("\n").slice(-60);
      result.serveLogProblems = log.split("\n").filter(l => /panic|error|failed|refused|warn/i.test(l)).slice(0, 80);
    } catch {}
    fs.writeFileSync(path.join(outDir, `${persona.id}.json`), JSON.stringify(result, null, 1));
    for (const s of result.steps) console.log(`  ${s.outcome.padEnd(7)} ${s.name}  clicks ${s.clicks} keys ${s.keys} wait ${(s.waitMs / 1000).toFixed(1)}s human≈${s.humanSec}s${s.errors.length ? "  ! " + s.errors[0] : ""}`);
    if (page) await page.context().close().catch(() => {});
    await fleet?.stop();
    await sb.stop();
    if (sb.keep) console.log("  sandbox kept:", sb.root);
  }
}
await browser.close();
console.log("\nresults:", outDir, "\nreport:  node report.mjs", path.relative(HERE, outDir));
