// June: drives Imperatorium from scripts - /api/dispatch in each mode, the
// CLI, Fleet watches and actions proposed by a script - and confirms what
// needs confirming from the window. Measures API ergonomics: what each call
// answers, how long it takes, and what a script is refused.
import { spawnSync } from "node:child_process";
import { makeProject, binary } from "../lib/sandbox.mjs";

export default {
  id: "june",
  name: "June, automation integrator",
  set: 3,
  fleet: { workMs: [4000, 6000] },
  projects(sb) { this.relay = makeProject(sb.projects, "relay"); this.batch = makeProject(sb.projects, "batch-jobs"); },
  async setup({ fleet }) { this.s = await fleet.addInteractive(this.relay, { history: 5, name: "relay-main" }); },

  /** A script's call: no window Origin, no window token. */
  async api(d, method, path, body) {
    const t0 = Date.now();
    const r = await fetch(d.sb.url + path, { method, headers: { "Content-Type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
    const text = await r.text();
    let j; try { j = JSON.parse(text); } catch { j = text.slice(0, 200); }
    const rec = { path, status: r.status, ms: Date.now() - t0, ok: j?.ok, error: j?.error, action: j?.action, keys: j && typeof j === "object" ? Object.keys(j).slice(0, 14) : [] };
    (d.cur.observations.calls ||= []).push(rec);
    return { r, j, rec };
  },

  cli(d, args) {
    const t0 = Date.now();
    const env = { ...d.sb.env(), IMPERATORIUM_URL: d.sb.url };
    const r = spawnSync(binary(), args, { env, encoding: "utf8", timeout: 30000, windowsHide: true });
    const rec = { args: args.join(" "), code: r.status, ms: Date.now() - t0, out: (r.stdout || "").slice(0, 300), err: (r.stderr || "").slice(0, 200) };
    (d.cur.observations.cli ||= []).push(rec);
    return rec;
  },

  async run({ d, fleet, page }) {
    const sid = this.s.sid;
    await d.step("dry-run, then inject", "route a prompt to the live session from a script", async () => {
      const dry = await this.api(d, "POST", "/api/dispatch", { project: "relay", prompt: "Rotate the API keys", mode: "dry-run" });
      d.observe("dryRun", JSON.stringify(dry.j).slice(0, 300));
      const t0 = Date.now();
      const inj = await this.api(d, "POST", "/api/dispatch", { project: "relay", prompt: "Rotate the API keys", mode: "auto" });
      d.observe("auto", JSON.stringify(inj.j).slice(0, 300));
      if (inj.j?.ok) {
        const ev = await d.fleetEvent("the session gets it", e => e.kind === "delivered" && e.sid === sid, { since: t0, timeout: 30000, sits: false }).catch(() => null);
        d.observe("deliveredMs", ev ? ev.t - t0 : null);
        d.observe("deliveredAsUser", ev?.as_user ?? null);
        if (!ev) d.note("a script's inject was accepted but never reached the session");
      } else d.note("a script's inject was refused: " + (inj.j?.error || inj.r.status));
    });

    await d.step("background and launch modes", "start work in another project from a script", async () => {
      const t0 = Date.now();
      const bg = await this.api(d, "POST", "/api/dispatch", { project: "batch-jobs", prompt: "Run the nightly reconciliation", mode: "background" });
      d.observe("background", JSON.stringify(bg.j).slice(0, 300));
      const up = await d.fleetEvent("the job starts", e => e.kind === "session-up" && e.bg, { since: t0, timeout: 30000, sits: false }).catch(() => null);
      d.observe("bgUpMs", up ? up.t - t0 : null);
      const t1 = Date.now();
      const ln = await this.api(d, "POST", "/api/dispatch", { project: "batch-jobs", prompt: "Look at the failed rows", mode: "launch" });
      d.observe("launch", JSON.stringify(ln.j).slice(0, 300));
      const up2 = await d.fleetEvent("the window session starts", e => e.kind === "session-up" && !e.bg && e.t >= t1, { since: t1, timeout: 30000, sits: false }).catch(() => null);
      d.observe("launchUpMs", up2 ? up2.t - t1 : null);
    });

    await d.step("a slash command and a bad request", "what a script gets back for /compact and for mistakes", async () => {
      await this.api(d, "POST", "/api/dispatch", { session_id: sid, prompt: "/compact", mode: "inject" });
      await this.api(d, "POST", "/api/dispatch", { project: "no-such-project", prompt: "hello" });
      await this.api(d, "POST", "/api/dispatch", { prompt: "" });
      await this.api(d, "POST", "/api/dispatch", { project: "relay", prompt: "x", mode: "sideways" });
      await this.api(d, "POST", "/api/dispatch", "not an object");
    });

    await d.step("what a script may not do", "window-only endpoints, called from a script", async () => {
      for (const [p, b] of [["/api/stop", { session_id: sid }], ["/api/spawn", { kind: "new", where: "background", cwd: this.relay }],
        ["/api/typed", { session_id: sid, prompt: "hi" }], ["/api/ask", { question: "who is stuck?" }]])
        await this.api(d, "POST", p, b);
      const refused = d.cur.observations.calls.filter(c => c.status === 403 || c.ok === false).length;
      d.observe("refused", refused);
    });

    await d.step("the CLI", "the same from a shell script", async () => {
      this.cli(d, ["sessions", "--json"]);
      this.cli(d, ["projects"]);
      this.cli(d, ["send", "Check the queue depth", "-s", sid, "--dry-run"]);
      this.cli(d, ["queue", "list"]);
      this.cli(d, ["pending"]);
      this.cli(d, ["history", "rotate"]);
      d.observe("failed", d.cur.observations.cli.filter(c => c.code !== 0).length);
    });

    await d.step("propose a watch, confirm it in the window", "a script asks to be told when relay-main waits", async () => {
      const t0 = Date.now();
      const w = await this.api(d, "POST", "/api/fleet/watch", { op: "add", kind: "waiting", session: "relay-main", minutes: 5 });
      d.observe("proposed", JSON.stringify(w.j).slice(0, 200));
      await d.waitFor("the window shows it to confirm", { fn: () => /to confirm/.test(document.querySelector("#fleetsum")?.innerText || "") || !document.querySelector("#fleetbtn")?.hidden, arg: null }, { timeout: 20000 });
      d.observe("visibleAfterMs", Date.now() - t0);
      await d.click("#fleetrow", { decide: true });
      await d.waitFor("the proposal", '#flprop [data-fl^="watch:confirm"]', { timeout: 10000 });
      await d.click('#flprop [data-fl^="watch:confirm"]', { decide: true });
      await d.waitFor("it is active", { fn: () => /watch on/.test(document.querySelector("#fleetsub")?.innerText || ""), arg: null }, { timeout: 10000 });
      const con = await this.api(d, "POST", "/api/fleet/watch", { op: "confirm", id: w.j?.watch?.id || w.j?.id || "x" });
      d.observe("scriptCannotConfirm", con.j?.ok === false);
    });

    await d.step("propose an action, confirm it", "a script proposes a prompt for relay-main; June runs it", async () => {
      const t0 = Date.now();
      const a = await this.api(d, "POST", "/api/fleet/action", { op: "propose", action: "queue_prompt", session: "relay-main", prompt: "Re-run the failed deploy step", reason: "the deploy failed at 03:10" });
      d.observe("proposed", JSON.stringify(a.j).slice(0, 200));
      await d.waitFor("the proposal", '#flprop [data-fl^="action:confirm"]', { timeout: 20000 });
      d.observe("visibleAfterMs", Date.now() - t0);
      d.observe("proposalText", (await page.locator("#flprop").innerText()).slice(0, 200));
      const t1 = Date.now();
      await d.click('#flprop [data-fl^="action:confirm"]', { decide: true });
      const ev = await d.fleetEvent("it reaches the session", e => e.kind === "delivered" && e.sid === sid && /deploy/.test(e.text), { since: t1, timeout: 60000, sits: false }).catch(() => null);
      d.observe("deliveredMs", ev ? ev.t - t1 : null);
      d.observe("flnote", await page.locator("#flnote").innerText().catch(() => ""));
    });
  },
};
