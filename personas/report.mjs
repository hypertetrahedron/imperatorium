// Aggregates a run's results/<stamp>/*.json into report.md beside them.
//   node report.mjs results/<stamp>
import fs from "node:fs";
import path from "node:path";

const dir = process.argv[2];
if (!dir) { console.error("usage: node report.mjs results/<stamp>"); process.exit(2); }
const runs = fs.readdirSync(dir).filter(f => f.endsWith(".json")).map(f => JSON.parse(fs.readFileSync(path.join(dir, f), "utf8")));
const q = (a, p) => { if (!a.length) return null; const b = [...a].sort((x, y) => x - y); return b[Math.min(b.length - 1, Math.floor(p * b.length))]; };
const sec = ms => (ms / 1000).toFixed(1);
const sum = (a, f) => a.reduce((t, x) => t + (f(x) || 0), 0);
const out = [];
const P = (...l) => out.push(...l);

P(`# Persona run ${path.basename(dir)}`, "");
P("Estimated human time = Keystroke-Level Model operators (point 1.1 s, click 0.2 s, key 0.28 s, hand move 0.4 s, decision 1.35 s) plus the time spent waiting on the window. Prompt text typed is not counted.", "");
P("## Summary", "", "| Persona | Steps ok / failed | Clicks | Keys | Menus | Dialogs | Waiting (s) | Est. human (s) | API calls | Errors | Peak MB |", "|---|---|---|---|---|---|---|---|---|---|---|");
for (const r of runs) {
  const st = r.steps;
  const peak = Math.max(0, ...r.memory.map(m => m.workingSetMB || 0));
  P(`| ${r.name} | ${st.filter(s => s.outcome === "ok").length} / ${st.filter(s => s.outcome === "failed").length}${r.fatal ? " (fatal)" : ""} | ${sum(st, s => s.clicks)} | ${sum(st, s => s.keys)} | ${sum(st, s => s.menus)} | ${sum(st, s => s.dialogs.length)} | ${sec(sum(st, s => s.waitMs))} | ${sum(st, s => s.humanSec).toFixed(1)} | ${r.requests.length} | ${sum(st, s => s.errors.length)} | ${peak || "?"} |`);
}

const all = runs.flatMap(r => r.steps.map(s => ({ ...s, persona: r.persona })));
P("", "## Costliest steps (estimated human time)", "", "| Persona | Step | Human (s) | of which waiting (s) | Clicks | Keys | Menus | Dialogs | Outcome |", "|---|---|---|---|---|---|---|---|---|");
for (const s of [...all].sort((a, b) => b.humanSec - a.humanSec).slice(0, 20))
  P(`| ${s.persona} | ${s.name} | ${s.humanSec} | ${sec(s.waitMs)} | ${s.clicks} | ${s.keys} | ${s.menus} | ${s.dialogs.length} | ${s.outcome} |`);

P("", "## Longest waits on the window", "", "| Persona | Step | Waiting for | ms |", "|---|---|---|---|");
for (const w of all.flatMap(s => s.waits.map(w => ({ ...w, step: s.name, persona: s.persona }))).sort((a, b) => b.ms - a.ms).slice(0, 20))
  P(`| ${w.persona} | ${w.step} | ${w.label}${w.timedOut ? " (timed out)" : ""} | ${w.ms} |`);

P("", "## Failures, notes and dialogs", "");
for (const s of all) {
  const bits = [...s.errors.map(e => "error: " + e), ...s.notes.map(n => "note: " + n), ...s.dialogs.map(d => "dialog: " + d)];
  if (!bits.length) continue;
  P(`- **${s.persona} / ${s.name}** (${s.outcome})${s.screenshot ? ` [${s.screenshot}]` : ""}`);
  for (const b of [...new Set(bits)].slice(0, 6)) P(`  - ${b.replace(/\n/g, " ")}`);
}

P("", "## Observations per step", "");
for (const r of runs) {
  P(`### ${r.name}`, "");
  for (const s of r.steps) if (Object.keys(s.observations).length) P(`- ${s.name}: \`${JSON.stringify(s.observations).slice(0, 600)}\``);
  P("");
}

P("## API latency (all personas)", "", "| Endpoint | Calls | p50 ms | p95 ms | max ms | Non-2xx |", "|---|---|---|---|---|---|");
const by = {};
for (const r of runs) for (const x of r.requests) if (x.path !== "/api/stream") (by[x.method + " " + x.path] ||= []).push(x);
for (const [k, xs] of Object.entries(by).sort((a, b) => b[1].length - a[1].length)) {
  const ms = xs.map(x => x.ms).filter(x => x != null);
  P(`| ${k} | ${xs.length} | ${q(ms, .5)} | ${q(ms, .95)} | ${Math.max(...ms)} | ${xs.filter(x => x.failed || x.status >= 300).length} |`);
}

P("", "## Request volume", "", "| Persona | Minutes | API calls | per minute | Top caller |", "|---|---|---|---|---|");
for (const r of runs) {
  const c = {}; for (const x of r.requests) c[x.path] = (c[x.path] || 0) + 1;
  const top = Object.entries(c).sort((a, b) => b[1] - a[1])[0] || ["-", 0];
  const min = r.durationMs / 60000;
  P(`| ${r.persona} | ${min.toFixed(1)} | ${r.requests.length} | ${(r.requests.length / min).toFixed(0)} | ${top[0]} (${top[1]}) |`);
}

P("", "## Dispatcher log problems", "");
for (const r of runs) {
  const p = r.serveLogProblems || [];
  P(`- ${r.persona}: ${p.length} line(s)` + (p.length ? "" : ""));
  for (const l of [...new Set(p.map(x => x.replace(/\d{2}:\d{2}:\d{2}(\.\d+)?/g, "").slice(0, 200)))].slice(0, 6)) P(`  - \`${l}\``);
}
P("", "## Safety", "");
for (const r of runs) P(`- ${r.persona}: deep links ${r.leaks?.deepLinks?.length ?? "?"}, files/folders opened ${r.leaks?.opened?.length ?? "?"}, launches seen ${r.launches.length}${r.fatal ? `; FATAL: ${r.fatal.split("\n")[0]}` : ""}`);
fs.writeFileSync(path.join(dir, "report.md"), out.join("\n") + "\n");
console.log(path.join(dir, "report.md"));
