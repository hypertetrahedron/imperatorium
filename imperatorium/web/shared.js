
// This browser's own state was kept under "ccontrol.*" before the rename;
// carried over once, so folds, drafts and what was seen survive it.
try {
  for (const k of Object.keys(localStorage))
    if (k.startsWith("ccontrol.")) {
      const to = "imperatorium." + k.slice(9);
      if (localStorage.getItem(to) === null) localStorage.setItem(to, localStorage.getItem(k));
      localStorage.removeItem(k);
    }
} catch (e) {}
const $ = s => document.querySelector(s);

// Every request gives up in the end (2026-11-21). With none, one request
// that never answered - across a dispatcher restart - held the refresh loop,
// Send and the new-dispatcher check for good, and a few of them filled the
// browser's six connections to this host, so even F5 queued behind them.
// A request that times out or cannot connect throws an error marked `down`.
const GET_MS = 15000, POST_MS = 60000;
async function timed(url, opts, ms){
  const ctl = typeof AbortController === "function" ? new AbortController() : null;
  const t = ctl ? setTimeout(() => ctl.abort(), ms) : null;
  try { return await fetch(url, ctl ? Object.assign({signal: ctl.signal}, opts) : opts); }
  catch (e) { throw Object.assign(new Error("the dispatcher did not answer"), {down: true}); }
  finally { if (t) clearTimeout(t); }
}

// GET a JSON endpoint; anything but a 2xx throws, so a caller's catch is the
// "dispatcher not answering" path.
async function get(url, ms){
  const r = await timed(url, {cache: "no-store"}, ms || GET_MS);
  if (!r.ok) throw new Error(r.status);
  return r.json();
}

// Served with the page and never stored: what Send As Typed and every other
// state-changing request vouch with.
const windowToken = () => (document.querySelector("meta[name=imp-window]") || {}).content || "";
// POST `body` as JSON and return the JSON answer whatever its status (a
// refusal carries its reason there). `headers` adds to the usual ones.
async function post(url, body, headers, ms){
  const r = await timed(url, {method: "POST", cache: "no-store",
    headers: Object.assign({"Content-Type": "application/json", "X-Imperatorium-Window": windowToken()}, headers),
    body: JSON.stringify(body)}, ms || POST_MS);
  return r.json();
}

// Which dispatcher served this page. A restarted dispatcher may serve
// different code, and an open page keeps the old until reloaded, so each
// page asks now and then and calls `onNew` once the answer changes.
const BOOT = typeof document === "undefined" ? ""
  : (document.querySelector("meta[name=imp-boot]") || {}).content || "";
// What Settings says the pages draw with (settings.look): text sizes as
// zoom factors over the sizes the CSS is written at (ui.FONT_BASE), which
// highlights show (html.hl-*), and colours set in place of the theme's.
// Served in the page (ui.look_head) and on /api/health, so an open page
// follows a change without a reload. Not on the Settings page, whose colour
// pickers show the theme's own colours.
const FONT_BASE = {chat: 14, matrix: 13};
function applyLook(l){
  const head = document.getElementById("lookvars");
  if (!l || !head) return;
  const root = document.documentElement, f = l.fonts || {}, c = l.colors || {};
  for (const k in FONT_BASE){
    const n = Number(f[k]);
    if (n > 0) root.style.setProperty("--" + k + "z", String(n / FONT_BASE[k]));
  }
  for (const [k, on] of Object.entries(l.hl || {})) root.classList.toggle("hl-" + k, !!on);
  for (const prop of [...root.style].filter(x => x.startsWith("--hl-")))
    if (!c[prop.slice(5)]) root.style.removeProperty(prop);
  for (const [k, v] of Object.entries(c))
    if (/^#[0-9a-f]{6}$/i.test(v)) root.style.setProperty("--hl-" + k, v);
  head.textContent = "";   // the page's own style now says it all
}
let bootChecking = false, bootOnNew = null;
// Not asked while the page is hidden (nobody is looking, and a reload would
// only be wasted): the question is asked the moment it shows again.
async function checkBoot(onNew){
  bootOnNew = onNew;
  if (!BOOT || bootChecking || (typeof document !== "undefined" && document.hidden)) return;
  bootChecking = true;
  try {
    const h = await get("/api/health");
    if (h && h.look) applyLook(h.look);
    if (h && h.boot && h.boot !== BOOT) onNew();
  } catch (e) { /* down or restarting: ask again next time */ }
  bootChecking = false;
}

if (typeof document !== "undefined")
  document.addEventListener("visibilitychange", () => { if (!document.hidden && bootOnNew) checkBoot(bootOnNew); });

// `fn` every `ms` while the page is showing; paused while it is hidden (a
// minimised or covered window, another tab), and run once at once when it
// shows again. `now` also runs it once straight away.
function whileVisible(ms, fn, now){
  let t = null;
  const start = () => { if (t === null) t = setInterval(fn, ms); };
  const stop = () => { if (t !== null) { clearInterval(t); t = null; } };
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) stop(); else { fn(); start(); }
  });
  if (!document.hidden) { start(); if (now) fn(); }
}

const esc = s => String(s == null ? "" : s)
  .replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;");

// A key event as a combination like "ctrl+space": modifiers in the order
// settings.normal_key writes them, then the physical key's name, so a
// layout or Shift does not change what a key is called. "" for a bare
// modifier press.
function comboOf(e){
  const c = e.code || "";
  if (/^(Control|Alt|Shift|Meta|OS)/.test(c)) return "";
  const key = /^Key[A-Z]$/.test(c) ? c.slice(3).toLowerCase()
            : /^Digit\d$/.test(c) ? c.slice(5)
            : c.toLowerCase();
  if (!key) return "";
  return [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.metaKey && "win", key]
    .filter(Boolean).join("+");
}
// "ctrl+space" as people write it: "Ctrl+Space".
const prettyKey = k => String(k || "").split("+")
  .map(p => p.charAt(0).toUpperCase() + p.slice(1)).join("+");

// The caption now arrives as `attention_reason`, worded by the dispatcher
// from Claude Code's own table, so there is one vocabulary rather than one
// per surface. This map is only the fallback for a row recorded before that.
const WAITING = {
  permission_prompt: "needs permission",
  idle_prompt: "waiting for you",
  agent_needs_input: "needs input",
  elicitation_dialog: "needs an answer",
  elicitation_url_dialog: "needs a URL",
};

// The full set is ["busy","shell","idle","waiting"]; `shell` is a session
// running a command. Anything unhandled used to fall through to a grey dot
// captioned with its own enum name.
const RUNNING = {busy: "working", shell: "running a command"};

// The wire role says "user" for every tool result, because that is how a
// result is handed back to the model - not who wrote it. The server sends a
// `speaker` derived from content; this only has to name it.
// `system` is Claude Code itself (a task finishing, a command's output); the
// item's `note` says which.
const SPEAKER = {you: "you", claude: "Claude", tool: "tool result", system: "Claude Code"};

// -- spend ------------------------------------------------------------------
// Global is everything sent from this machine in the account's windows (5h
// and 7d on a subscription, the month otherwise); current is the sessions
// open now, each over its whole life. Both are estimates at API list prices.
const SPAN_NAMES = {five_hour: "5h", seven_day: "7d", month: "month"};
const SPAN_TITLES = {five_hour: "5-hour window", seven_day: "7-day window", month: "This month"};
const usd = v => typeof v !== "number" ? "" : "$" + (v >= 100 ? Math.round(v) : v.toFixed(2));
function tok(n){
  n = n || 0;
  if (n >= 1e9) return (n / 1e9).toFixed(1) + "B";
  if (n >= 1e6) return (n / 1e6).toFixed(n >= 1e7 ? 0 : 1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(n >= 1e4 ? 0 : 1) + "k";
  return String(Math.round(n));
}
const hhmm = t => new Date(t * 1000).toLocaleTimeString([], {hour: "2-digit", minute: "2-digit"});
// One block of spend, spelled out for a tooltip.
function spendDetail(x){
  const t = x.tokens || {};
  return `${tok(x.total_tokens)} tokens: in ${tok(t.input)}, out ${tok(t.output)}, ` +
    `cache read ${tok(t.cache_read)}, cache write ${tok(t.cache_write)} · ≈${usd(x.cost_usd || 0)}` +
    (x.unpriced_tokens ? ` (${tok(x.unpriced_tokens)} tokens on models with no known price)` : "");
}
function windowTitle(w){
  const since = w.since ? (w.name === "month" ? "" : ` since ${hhmm(w.since)}`) : "";
  return (SPAN_TITLES[w.name] || w.name) + (w.resets_at ? `${since}, resets ${hhmm(w.resets_at)}`
    : w.name === "month" ? "" : " (the last " + (w.name === "five_hour" ? "5 hours" : "7 days") + ", reset time not known yet)");
}
const SPEND_NOTE = "Estimated at API list prices from the transcripts on this machine; " +
  "usage elsewhere (claude.ai, other machines) shows only in the meters' percentages.";

// -- usage meter ---------------------------------------------------------
const clock = t => new Date(t * 1000).toLocaleTimeString([], {hour:"2-digit", minute:"2-digit"});
const LIMIT_NAMES = {five_hour: "5h", seven_day: "7d", spend_limit: "spend",
                     seven_day_opus: "7d opus", seven_day_sonnet: "7d sonnet"};
const LIMIT_TITLES = {five_hour: "5-hour window", seven_day: "7-day window", spend_limit: "Spend limit",
                      seven_day_opus: "7-day Opus window", seven_day_sonnet: "7-day Sonnet window"};
const LIMIT_SECONDS = {five_hour: 5 * 3600, seven_day: 7 * 86400};
// A reset that isn't today names its weekday too: "Tue 09:00".
const resetLabel = t => {
  const d = new Date(t * 1000);
  return (d.toDateString() === new Date().toDateString() ? ""
          : d.toLocaleDateString([], {weekday: "short"}) + " ") + clock(t);
};
// The tick: how far through the window the clock is, so the fill can be read
// against time - fill ahead of the tick means using faster than the window allows.
const timeTick = (name, resets) => {
  const len = LIMIT_SECONDS[name];
  if (!len || !resets) return "";
  const done = Math.max(0, Math.min(1, 1 - (resets - Date.now() / 1000) / len));
  return `<b style="left:${(done * 100).toFixed(1)}%"></b>`;
};
// "2h 5m", "3d 4h": how long until a reset, for the Meter Tip.
const until = t => {
  const m = Math.max(0, Math.round((t - Date.now() / 1000) / 60));
  if (m < 60) return m + "m";
  const h = Math.floor(m / 60);
  return h < 24 ? h + "h" + (m % 60 ? " " + (m % 60) + "m" : "") : Math.floor(h / 24) + "d " + (h % 24) + "h";
};
// The Meter Tip: what a row's numbers mean and when its window resets.
const meterTip = (name, w, spent) => {
  if (w.waiting) {
    // The window reset and no session has reported the new one yet.
    const lines = [esc(LIMIT_TITLES[name] || name), "reset at " + esc(resetLabel(w.reset_at)) +
      ", waiting for a new reading", "shows 0% until a session reports the new window"];
    if (spent) lines.push("from this machine: " + tok(spent.total_tokens) + " tokens · ≈" + usd(spent.cost_usd || 0));
    return `<div class="tip">${lines.map(l => `<div>${l}</div>`).join("")}</div>`;
  }
  if (name === "spend_limit")
    return `<div class="tip">${spendLines(w, spent).map(l => `<div>${l}</div>`).join("")}</div>`;
  const lines = [esc(LIMIT_TITLES[name] || name), Math.round(w.used) + "% used" +
    // Kept across restarts, so it can be from before this dispatcher started.
    (w.at && Date.now() / 1000 - w.at > 600 ? ` (reading from ${esc(resetLabel(w.at))})` : "")];
  if (w.resets_at) lines.push("resets " + esc(resetLabel(w.resets_at)) + " (in " + until(w.resets_at) + ")");
  const len = LIMIT_SECONDS[name];
  if (len && w.resets_at)
    lines.push(Math.round(Math.max(0, Math.min(1, 1 - (w.resets_at - Date.now() / 1000) / len)) * 100)
               + "% of the window elapsed (the tick)");
  if (spent) lines.push("from this machine: " + tok(spent.total_tokens) + " tokens · ≈" + usd(spent.cost_usd || 0));
  return `<div class="tip">${lines.map(l => `<div>${l}</div>`).join("")}</div>`;
};

// The spend limit's Meter Tip: what is left of a fixed allocation - an admin's
// monthly budget, or a Claude apps gateway's spend limit, e.g. an Enterprise
// seat's $500 a month. Dollars when they are known (a gateway sends the
// percentage first), else the percentage left. It adds roughly how many
// tokens that buys at this machine's own recent mix (`spent`: a spend window
// with total_tokens and cost_usd), which is a guide, not a promise: the mix
// of models and caching sets the price. The bar itself is the used fraction;
// what is left is its inverse, so the tip carries it rather than a second bar.
const PERIOD_WORDS = {daily: "a day", weekly: "a week", monthly: "a month"};
function spendLines(w, spent){
  const dollars = typeof w.limit_usd === "number" && w.limit_usd > 0;
  const usedUsd = typeof w.used_usd === "number" ? w.used_usd : (dollars ? w.limit_usd * w.used / 100 : null);
  const leftPct = Math.max(0, Math.min(100, 100 - w.used));
  const leftUsd = dollars ? Math.max(0, w.limit_usd - usedUsd) : null;
  const lines = [LIMIT_TITLES.spend_limit, dollars
    ? `${usd(leftUsd)} of ${usd(w.limit_usd)} left` + (PERIOD_WORDS[w.period] ? ` (${PERIOD_WORDS[w.period]})` : "")
    : Math.round(leftPct) + "% left (the dollars have not come yet)"];
  if (dollars) lines.push(`${usd(usedUsd)} used (${Math.round(w.used)}%)`);
  // Kept across restarts, so it can be from before this dispatcher started.
  if (w.at && Date.now() / 1000 - w.at > 600) lines.push(`reading from ${esc(resetLabel(w.at))}`);
  if (w.resets_at) lines.push("refills " + esc(resetLabel(w.resets_at)) + " (in " + until(w.resets_at) + ")");
  if (dollars && spent && spent.cost_usd > 0 && spent.total_tokens > 0)
    lines.push(`≈${tok(leftUsd * spent.total_tokens / spent.cost_usd)} tokens at this machine's ` +
               `recent mix (${tok(spent.total_tokens / spent.cost_usd)} per $)`);
  lines.push("an estimate from token counts, not a bill");
  return lines;
}

// Global and Open lines under the bars.
function spendHtml(sp){
  if (!sp || !sp.global) return "";
  const wins = sp.global.windows || [];
  const more = sp.complete === false ? " · counting…" : "";
  const g = wins.map(w => `${SPAN_NAMES[w.name] || w.name} ≈${usd(w.cost_usd || 0)}`).join(" · ");
  const gt = wins.map(w => windowTitle(w) + ": " + spendDetail(w)).join("\n") + "\n\n" + SPEND_NOTE;
  const c = sp.current || {};
  const ct = "The sessions open in Imperatorium, each over its whole life, subagents included:\n" +
    (c.each || []).map(e => `${e.name || e.sessionId}: ${tok(e.total_tokens)} tokens · ≈${usd(e.cost_usd || 0)}`).join("\n") +
    "\n\nTotal " + spendDetail(c) + "\n\n" + SPEND_NOTE;
  return `<div class="spend extra">` +
    (wins.length ? `<span class="k">global</span><span class="v" title="${esc(gt)}">${esc(g + more)}</span>` : "") +
    `<span class="k">open</span><span class="v" title="${esc(ct)}">${
      c.sessions || 0} session${c.sessions === 1 ? "" : "s"} · ≈${usd(c.cost_usd || 0)} · ${tok(c.total_tokens)} tokens</span></div>`;
}

// -- model mix ---------------------------------------------------------
// After claude-lens's "Model mix" chart: model families in cost order,
// cheapest first, which is both the stacking order (left to right here) and
// the rank on the ironbow ramp, so a bar reads as a cost gradient. It stacks
// output tokens. "other" is a model with no known price, which is not the
// same as an expensive one, so it is grey. "other" catches all: keep it last.
const FAMILIES = [
  ["haiku",  m => m.includes("haiku"),  "i1"],
  ["sonnet", m => m.includes("sonnet"), "i2"],
  ["opus",   m => m.includes("opus"),   "i3"],
  ["fable",  m => m.includes("fable") || m.includes("mythos"), "i4"],
  ["other",  () => true, "io"],
];
const famOf = model => FAMILIES.find(([, test]) => test(String(model || "").toLowerCase()))[0];
// {family: {output, total_tokens, cost_usd}} from the server's split by model.
function byFamily(models){
  const out = {};
  for (const [name, m] of Object.entries(models || {})) {
    const k = famOf(name), f = out[k] || (out[k] = {output: 0, total_tokens: 0, cost_usd: 0});
    f.output += m.output || 0; f.total_tokens += m.total_tokens || 0; f.cost_usd += m.cost_usd || 0;
  }
  return out;
}
function mixRow(label, title, models){
  const fams = byFamily(models);
  const have = FAMILIES.filter(([n]) => fams[n]);
  const total = have.reduce((a, [n]) => a + fams[n].output, 0);
  const segs = have.filter(([n]) => fams[n].output > 0).map(([n, , slot]) =>
    `<i class="${slot}" style="width:${(100 * fams[n].output / total).toFixed(2)}%"></i>`).join("");
  const lines = [esc(title) + ": output tokens by model"].concat(have.map(([n]) => {
    const f = fams[n];
    return `${n}: ${tok(f.output)} out (${total ? Math.round(100 * f.output / total) : 0}%) · ` +
      `${tok(f.total_tokens)} tokens in all · ≈${usd(f.cost_usd)}`;
  }));
  if (!have.length) lines.push("nothing sent");
  return `<div class="row mix extra"><span>${esc(label)}</span><span class="stack">${segs}</span>` +
    `<span class="pct">${tok(total)}</span><div class="tip">${lines.map(l => `<div>${l}</div>`).join("")}</div></div>`;
}
// One row per global window, then the open sessions; a legend of the
// families that appear.
function mixHtml(sp){
  if (!sp || !sp.global) return "";
  const wins = sp.global.windows || [], cur = sp.current;
  const rows = wins.map(w => mixRow(SPAN_NAMES[w.name] || w.name, SPAN_TITLES[w.name] || w.name, w.models));
  if (cur) rows.push(mixRow("open", "Open sessions", cur.models));
  if (!rows.length) return "";
  const seen = new Set();
  for (const m of wins.map(w => w.models).concat(cur ? [cur.models] : []))
    for (const n of Object.keys(m || {})) seen.add(famOf(n));
  const legend = FAMILIES.filter(([n]) => seen.has(n))
    .map(([n, , slot]) => `<span><i class="${slot}"></i>${n}</span>`).join("");
  return `<div class="cap extra">model mix · output tokens</div>` + rows.join("") +
    (legend ? `<div class="legend extra">${legend}</div>` : "");
}


// No windows at all is normal off a Pro or Max plan: Bedrock, Vertex, an API
// key, or a Team or Enterprise seat has no 5h/7d limits to show, so the
// space names the plan instead of sitting empty like a broken meter.
function planHtml(plan){
  if (!plan || !plan.label) return "";
  return plan.alongside
    ? `<div class="plan also" title="sessions on these have no rate-limit windows">also ${esc(plan.label)} - no limits</div>`
    : `<div class="plan" title="no rate-limit windows on this plan">${esc(plan.label)}</div>`;
}

// The meters: a row per window the account reports - a verified Ultra
// account has no 5h one, an API key or Bedrock none, a gateway's spend limit
// only "spend", whose tip says what is left - then (marked extra, so the sidebar shows
// them only when opened) the pace lines, the plan, the spend lines and the
// model mix. `lines: false` leaves out the spend lines, for the matrix's
// status tile, which spells the spend out below.
function meterHtml(u, opts){
  const lines = !opts || opts.lines !== false;
  const rows = Object.entries(u.limits || {});
  const spent = {};
  const wins = ((u.spend || {}).global || {}).windows || [];
  for (const w of wins) spent[w.name] = w;
  const bars = rows.map(([name, w]) => {
    const pct = Math.max(0, Math.min(100, w.used));
    const cls = w.used >= 95 ? "over" : w.used >= 80 ? "hi" : "";
    const p = w.pace;
    let pace = "";
    if (p && p.full_at && p.before_reset)
      pace = `<div class="pace bad extra">at this pace: full by ${clock(p.full_at)} (${Math.round(p.per_hour)}%/h)</div>`;
    else if (p && p.per_hour > 0)
      pace = `<div class="pace extra">${Math.round(p.per_hour)}%/h - lasts until reset</div>`;
    return `<div class="row${w.waiting ? " waiting" : ""}"><span>${esc(LIMIT_NAMES[name] || name)}</span>
      <span class="bar"><i class="${cls}" style="width:${pct}%"></i>${timeTick(name, w.resets_at)}</span>
      <span class="pct">${Math.round(w.used)}%</span>${meterTip(name, w, name === "spend_limit" ? wins[wins.length - 1] : spent[name])}</div>${pace}`;
  }).join("");
  const rest = (lines ? spendHtml(u.spend) : "") + mixHtml(u.spend);
  if (!bars && !rest) return planHtml(u.plan);
  return `<div class="rows">` + bars + planHtml(u.plan) + rest + `</div>`;
}

// What an idle session is still waiting on, in words: "waiting on 1 shell ·
// 1 monitor". `tasks` is the dispatcher's count by kind.
const TASK_WORDS = {shells: ["shell", "shells"], monitors: ["monitor", "monitors"],
                    agents: ["subagent", "subagents"], wakeups: ["wakeup", "wakeups"]};
function tasksLabel(s){
  const t = s.tasks || (s.background_tasks ? {shells: s.background_tasks} : {});
  const parts = Object.keys(TASK_WORDS).filter(k => t[k] > 0)
    .map(k => t[k] + " " + TASK_WORDS[k][t[k] === 1 ? 0 : 1]);
  return parts.length ? "waiting on " + parts.join(" \u00b7 ") : "waiting on tasks";
}
// The dispatcher decides (registry.classify): an idle session is waiting on
// tasks, complete (its turn ended with nothing pending: attention of kind
// "complete"), or ready (never given work). Needs you is a session blocked
// on you - a permission prompt, a question.
function stateOf(s){
  if (s.attention){
    if (s.attention_kind === "failure") return ["d-fail", s.attention_reason || "failed"];
    if (s.attention_kind === "complete") return ["d-comp", "complete"];
    return ["d-attn", s.attention_reason || s.waitingFor ||
      WAITING[s.last_notification] || s.last_notification || "waiting for you"];
  }
  // Wanted you, but you have already looked (its chat read to the end).
  if (s.reviewed) return [s.attention_kind === "complete" ? "d-revc" : "d-rev", "reviewed"];
  // `done` is a background job that ran to completion: nothing to wait for.
  if (s.state === "done") return ["d-done", "finished"];
  if (s.state && s.state !== "working") return ["d-done", s.state];
  if (RUNNING[s.status] || s.state === "working")
    return ["d-busy", RUNNING[s.status] || "working"];
  // Idle between turns but something it started is still going: it will
  // come back by itself, so it does not need you.
  if (s.status === "idle" && (s.waiting_tasks || s.background_tasks)) return ["d-idle", tasksLabel(s)];
  if (s.status === "idle") return ["d-ready", "ready"];
  return ["d-done", s.status || "unknown"];
}

// Markdown tables, the GFM shape Claude writes: a header row, a delimiter row
// of dashes (colons set alignment), then body rows until a line without a
// pipe. Nothing inside a ``` fence is touched - a table in a code sample is
// meant to be read as source.
const TABLE_SEP = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

function cells(line){
  let s = line.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
  return s.split(/(?<!\\)\|/).map(c => c.trim().replace(/\\\|/g, "|"));
}

// Inline Markdown: `code`, [links](https://...), bare URLs, **bold**,
// *italic*, ~~strike~~. Code and links are set aside first so nothing inside
// them is read as emphasis; underscores only count at word edges, so
// snake_case names and paths come through untouched. Only http(s) links are
// made clickable - a session's output is not trusted to pick a scheme.
const attr = s => esc(s).replace(/"/g, "&quot;");
const link = (href, text) => `<a href="${attr(href)}" target="_blank" rel="noopener">${text}</a>`;

// -- highlighting -------------------------------------------------------------
// Every mark is always drawn; the html.hl-<name> classes (Settings >
// Highlighting, applyLook) decide which show, so a switch needs no redraw.
// Marks are spans around the text and never change it, so find, copy and
// the line counts read the same as before.

// Text set aside while the rest is marked: placeholders of private-use
// characters, which no \w, \d or \s in the patterns below can match.
function holder(){
  const kept = [];
  const hold = h => "\uE000" + [...(kept.push(h) - 1).toString(16)]
    .map(c => String.fromCharCode(0xE010 + parseInt(c, 16))).join("") + "\uE001";
  const HELD = /\uE000([\uE010-\uE01F]+)\uE001/g;
  const back = s => {
    for (let n = 0; n < 8 && HELD.test(s); n++)
      s = s.replace(HELD, (_, d) => kept[parseInt([...d].map(c => (c.charCodeAt(0) - 0xE010).toString(16)).join(""), 16)]);
    return s;
  };
  return {hold, back};
}

// File paths: rooted (C:\, ~/, ./, ../, /), relative with a file extension,
// or a bare name with a known one; each may end in :line or :line:col.
const PATH_EXTS = "py|pyi|js|mjs|cjs|ts|tsx|jsx|json|jsonc|md|txt|yml|yaml|toml|ini|cfg|conf|ps1|psm1|sh|bash|bat|cmd|" +
  "html|htm|css|scss|go|rs|java|kt|c|h|cc|cpp|hpp|cs|rb|php|sql|lock|log|xml|csv|tsv|env|vue|svelte|swift|lua|r|ipynb";
const PATH_SRC = /(?:[A-Za-z]:[\\/]|~[\\/]|\.{1,2}[\\/]|\/(?=[\w.]))(?:[\w.@+-]+[\\/])*[\w.@+-]*\w/.source + "|" +
  /(?:[\w.@+-]+[\\/])+[\w@+-]*\.[A-Za-z0-9]{1,8}/.source + "|" +
  "[\\w-]+(?:\\.[\\w-]+)*\\.(?:" + PATH_EXTS + ")";
const LINE_SRC = /(?::\d+(?:[:-]\d+)?)?/.source;
const PATH_RE = new RegExp(/(?<![\w\/\\.:@-])/.source + "(?:" + PATH_SRC + ")" + LINE_SRC + /(?![\w\/\\])/.source, "g");
const PATH_ONLY = new RegExp("^(?:" + PATH_SRC + ")" + LINE_SRC + "$");
// Commit hashes (7-40 hex with a digit and a letter) and #123.
const REF_RE = /(?<![\w-])(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{7,40}(?![\w-])|(?<![\w&#])#\d{1,6}(?!\w)/g;
// Amounts: $ and numbers with a unit - tokens, durations, sizes, %.
const QTY_RE = new RegExp(/\$\d[\d,]*(?:\.\d+)?(?:\s?[kKMB](?!\w))?/.source + "|" +
  /(?<![\w.$])\d[\d,]*(?:\.\d+)?(?:\s?[kKM](?=\s?tokens?\b))?\s?/.source +
  "(?:tokens?|ms|secs?|seconds?|mins?|minutes?|hrs?|hours?|days?|weeks?|[kKMGT]i?B|bytes?|px|%|s|h|m)(?!\\w)", "g");
// What a test run or a log says went wrong, went right, or wants a look.
const SIGNALS = [
  ["err", /\b\d+ (?:failed|failures?|errors?)\b|\b(?:FAILED|FAIL|FAILURES?|ERROR|Error|Errors|Traceback|Exception|Fatal|fatal|failed|failure|failing|panic(?:ked)?)\b|\b[A-Z]\w*(?:Error|Exception)\b/g],
  ["pass", /\bRan \d+ tests?\b|\b\d+ (?:passed|tests? passed|succeeded)\b|\b(?:PASSED|PASS|OK|passed|passing|succeeded|successful(?:ly)?)\b/g],
  ["warn", /\b(?:WARNING|WARN|Warning|warning|warnings|deprecated|[A-Z]\w*Warning)\b/g],
];
// A code span that is a command: a known program first, or a cmdlet.
const CMD_RE = new RegExp("^(?:\\$\\s+)?(?:\\S*[\\\\/])?(?:git|gh|python3?|py|pip3?|uv|pytest|npm|npx|node|yarn|pnpm|" +
  "deno|bun|cargo|go|make|cmake|docker|kubectl|helm|terraform|cd|ls|dir|cat|grep|rg|find|sed|awk|curl|wget|ssh|" +
  "scp|rsync|rm|mv|cp|mkdir|touch|chmod|chown|echo|export|source|sudo|winget|choco|scoop|claude|impctl|bash|sh|" +
  "zsh|pwsh|powershell|code|setx|tar|unzip|java|dotnet|ruby|gem|bundle|php|brew|apt|apt-get|systemctl|journalctl)" +
  "(?:\\.exe)?(?:\\s|$)|^[A-Z][a-z]+-[A-Z]\\w+");

const pathSpan = (p, extra) =>
  `<span class="pth${extra ? " " + extra : ""}" data-copy="${attr(p)}" title="click to open, show in its folder or copy">${esc(p)}</span>`;
function markWith(s, hold, sigs){
  s = s.replace(PATH_RE, m => hold(pathSpan(m)));
  s = s.replace(REF_RE, m => hold(`<span class="ref">${esc(m)}</span>`));
  for (const [cls, re] of SIGNALS) s = s.replace(re, m => hold(`<span class="sg-${cls}">${esc(m)}</span>`));
  if (!sigs) s = s.replace(QTY_RE, m => hold(`<span class="qty">${esc(m)}</span>`));
  return s;
}
// Plain text (a tool result) with its paths, references and signals marked.
function markPlain(text){
  const {hold, back} = holder();
  return back(esc(markWith(String(text == null ? "" : text), hold, true)));
}

// Code: one tokenizer per family, each an ordered list of [class, pattern];
// a bare word is a keyword from the family's list, a function when "(" is
// next, or plain. Patterns may not hold capturing groups.
const NUM = /\b(?:0[xX][\da-fA-F_]+|0[bBoO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)[a-zA-Z]{0,2}\b/.source;
const DQ = /"(?:\\.|[^"\\\n])*"/.source, SQ = /'(?:\\.|[^'\\\n])*'/.source;
const C_KW = "abstract as async await break case catch class const continue debugger default defer delete do else " +
  "enum export extends false final finally fn for from func function go goto if impl implements import in " +
  "instanceof interface let loop match mod mut namespace new nil null of package private protected pub public " +
  "readonly return self Self static struct super switch this throw throws true try type typeof undefined use " +
  "using var void where while with yield int char bool float double long short unsigned string";
const LANGS = {
  py: {kw: "and as assert async await break case class continue def del elif else except False finally for from " +
      "global if import in is lambda match None nonlocal not or pass raise return self True try while with yield",
    rules: [["com", /#.*/.source],
            ["str", /[rRbBfFuU]{0,2}(?:'{3}[\s\S]*?'{3}|"{3}[\s\S]*?"{3})/.source + "|" + /[rRbBfFuU]{0,2}/.source + "(?:" + DQ + "|" + SQ + ")"],
            ["fn", /@[\w.]+/.source], ["num", NUM], ["id", /[A-Za-z_]\w*/.source]]},
  c: {kw: C_KW,
    rules: [["com", /\/\/.*|\/\*[\s\S]*?\*\//.source],
            ["str", DQ + "|" + SQ + "|" + /`(?:\\[\s\S]|[^`\\])*`/.source],
            ["fn", /@\w+/.source], ["num", NUM], ["id", /[A-Za-z_$][\w$]*/.source]]},
  sh: {kw: "if then else elif fi for in do done while until case esac function return local export readonly " +
      "unset shift exit source true false time select", cmd: true,
    rules: [["com", /(?<![\w$#{])#.*/.source], ["str", /"(?:\\[\s\S]|[^"\\])*"|'[^']*'/.source],
            ["var", /\$\{[^}\n]*\}|\$[A-Za-z_]\w*|\$[@#?*!$0-9]/.source],
            ["num", /(?<=\s)--?[A-Za-z][\w-]*/.source + "|" + NUM], ["id", /[A-Za-z_][\w.-]*/.source]]},
  ps: {kw: "begin break catch class continue data do dynamicparam else elseif end exit filter finally for foreach " +
      "from function if in param process return switch throw trap try until using var while", ci: true,
    rules: [["com", /<#[\s\S]*?#>|#.*/.source], ["str", /"(?:`[\s\S]|""|[^"`])*"|'(?:''|[^'])*'/.source],
            ["var", /\$\{[^}\n]*\}|\$(?:env:|script:|global:)?[\w]+/.source],
            ["fn", /\b[A-Za-z]+-[A-Za-z]+\b/.source],
            ["num", /(?<=\s)-[A-Za-z]\w*/.source + "|" + NUM], ["id", /[A-Za-z_]\w*/.source]]},
  json: {kw: "true false null",
    rules: [["fn", DQ + /(?=\s*:)/.source], ["str", DQ], ["num", /-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/.source],
            ["id", /[A-Za-z_]\w*/.source]]},
  yaml: {kw: "true false null yes no on off True False Null", flags: "m",
    rules: [["com", /(?<!\S)#.*/.source], ["kw", /^[ \t]*\[[^\]\n]+\]/.source],
            ["fn", /(?<=^[ \t]*(?:- +)?)[\w.$-][\w .$\/-]*?(?=[ \t]*:(?:\s|$))/.source],
            ["str", DQ + "|" + /'(?:''|[^'\n])*'/.source], ["num", NUM], ["id", /[A-Za-z_]\w*/.source]]},
  html: {kw: "",
    rules: [["com", /<!--[\s\S]*?-->/.source], ["kw", /<\/?[A-Za-z][\w:-]*|\/?>/.source],
            ["fn", /(?<=\s)[\w:@.-]+(?==)/.source], ["str", /"[^"]*"|'[^']*'/.source]]},
  sql: {kw: "select from where and or not insert into values update set delete create table index view drop alter " +
      "add join left right inner outer full cross on as group by order having limit offset union all distinct case " +
      "when then else end null is in like between exists primary key foreign references default begin commit " +
      "rollback with returning asc desc count sum avg min max", ci: true,
    rules: [["com", /--.*|\/\*[\s\S]*?\*\//.source], ["str", /'(?:''|[^'])*'/.source], ["num", NUM],
            ["id", /[A-Za-z_]\w*/.source]]},
};
const LANG_OF = {python: "py", py: "py", python3: "py", py3: "py", pyi: "py",
  js: "c", javascript: "c", jsx: "c", mjs: "c", ts: "c", typescript: "c", tsx: "c", java: "c", c: "c", h: "c",
  cpp: "c", "c++": "c", cc: "c", cs: "c", csharp: "c", go: "c", golang: "c", rust: "c", rs: "c", kotlin: "c",
  kt: "c", swift: "c", php: "c", css: "c", scss: "c", dart: "c", scala: "c",
  sh: "sh", bash: "sh", shell: "sh", zsh: "sh", console: "sh", terminal: "sh", shellsession: "sh",
  ps: "ps", ps1: "ps", powershell: "ps", pwsh: "ps",
  json: "json", jsonc: "json", json5: "json", yaml: "yaml", yml: "yaml", toml: "yaml", ini: "yaml",
  html: "html", xml: "html", svg: "html", vue: "html", sql: "sql", diff: "diff", patch: "diff", udiff: "diff"};

// No language named: a guess from the text, or none.
function guessLang(src){
  const s = src.trim();
  if (/^(?:diff --git|--- \S|\+\+\+ \S|@@ )/m.test(s) && /^[-+]/m.test(s)) return "diff";
  if (/^[\[{]/.test(s)) { try { JSON.parse(s); return "json"; } catch (e) {} }
  if (/^\s*(?:def \w+\(|class \w+[(:]|import \w+$|from [\w.]+ import |if __name__)/m.test(s)) return "py";
  if (/\b(?:Get|Set|New|Remove|Start|Stop|Write|Invoke|Test)-[A-Z]\w+|\$env:/.test(s)) return "ps";
  if (/^\s*(?:const|let|var|function|export|import .* from|interface|fn|func|package|#include|public class) /m.test(s)
      || /\) =>|=> \{/.test(s)) return "c";
  if (/^\s*(?:\$ |#!\/|git |cd |python|npm |pip |ls |sudo |curl |\.\/)/m.test(s)) return "sh";
  if (/^\s*<[A-Za-z!?]/.test(s)) return "html";
  if (/^\s*(?:SELECT|INSERT|UPDATE|CREATE|WITH)\s/i.test(s) && /;\s*$|\bFROM\b/i.test(s)) return "sql";
  if (/^[\w.-]+:(?:\s|$)/m.test(s) && !/[;{}()]/.test(s)) return "yaml";
  return "";
}

function tokenize(src, lang){
  const L = LANGS[lang];
  if (!L.re){
    L.re = new RegExp(L.rules.map(r => "(" + r[1] + ")").join("|"), "g" + (L.flags || ""));
    L.kwset = new Set(L.kw.split(" ").filter(Boolean).map(w => L.ci ? w.toLowerCase() : w));
  }
  const out = [];
  let at = 0;
  for (const m of src.matchAll(L.re)){
    if (!m[0]) continue;
    if (m.index > at) out.push(["", src.slice(at, m.index)]);
    let cls = L.rules[m.slice(1).findIndex(x => x !== undefined)][0];
    if (cls === "id"){
      const end = m.index + m[0].length;
      if (L.kwset.has(L.ci ? m[0].toLowerCase() : m[0])) cls = "kw";
      else if (L.cmd && /(?:^|[|;&(]|\$ )\s*(?:sudo\s+)?$/.test(src.slice(src.lastIndexOf("\n", m.index - 1) + 1, m.index))) cls = "fn";
      else if (/^\s*\(/.test(src.slice(end, end + 20))) cls = "fn";
      else cls = "";
    }
    out.push([cls, m[0]]);
    at = m.index + m[0].length;
  }
  if (at < src.length) out.push(["", src.slice(at)]);
  return out;
}

// Indent guides: each indent level's leading whitespace as its own span,
// which html.hl-guides draws a faint line at the start of.
function indentUnit(src){
  let u = 0;
  for (const m of src.matchAll(/^( +)\S/gm)){ const n = m[1].length; if (n >= 2 && (!u || n < u)) u = n; }
  return Math.min(u || 4, 8);
}
function guides(ws, unit){
  if (ws.includes("\t")) return ws.replace(/\t/g, '<span class="ig">\t</span>');
  const n = Math.floor(ws.length / unit);
  return `<span class="ig">${" ".repeat(unit)}</span>`.repeat(n) + ws.slice(n * unit);
}
function codeHtml(src, lang){
  const lines = [[]];
  for (const [c, t] of LANGS[lang] ? tokenize(src, lang) : [["", src]])
    t.split("\n").forEach((p, i) => { if (i) lines.push([]); if (p) lines[lines.length - 1].push([c, p]); });
  const unit = indentUnit(src);
  return lines.map(line => {
    let html = "", lead = true;
    for (let [c, t] of line){
      if (lead){
        const ws = /^[ \t]*/.exec(t)[0];
        if (ws){ html += guides(ws, unit); t = t.slice(ws.length); }
        if (!t) continue;
        lead = false;
      }
      html += c ? `<span class="tk-${c}">${esc(t)}</span>` : esc(t);
    }
    return html;
  }).join("\n");
}
// A diff, line by line. `plain` (an Edit's own change) has no file headers,
// so a removed "-- comment" is not read as one.
function diffCodeHtml(src, plain){
  return String(src).split("\n").map(l => {
    const c = !plain && /^(?:diff --git|index |\+\+\+ |--- )/.test(l) ? "f" : l.startsWith("@@") ? "h"
            : l[0] === "+" ? "a" : l[0] === "-" ? "r" : "";
    return c ? `<span class="dl-${c}">${esc(l)}</span>` : esc(l);
  }).join("\n");
}
function codeBlock(src, info){
  const named = LANG_OF[info];
  const lang = named || (info ? "" : guessLang(src));
  if (lang === "diff") return `<pre class="code dif">${diffCodeHtml(src)}</pre>`;
  return `<pre class="code"${lang ? ` data-lang="${lang}"` : ""}>${codeHtml(src, lang)}</pre>`;
}

// A line of Claude's that asks something: marked so it is found again in a
// long reply (shown only in Claude's turns; see the CSS).
const ASK = /\?\s*(?:[*_)"'\]]+\s*)?$/;
const askLine = l => ASK.test(l) && l.trim().length > 3 ? `<span class="ask">${inline(l)}</span>` : inline(l);

// Tool calls: the name, then its argument told apart - a command and its
// flags, a pattern, a file path - and under an Edit what it changed.
const PATH_TOOLS = /^(?:Read|Edit|MultiEdit|Write|NotebookEdit|NotebookRead|LS)$/;
const SHELL_TOOLS = /^(?:Bash|PowerShell|Monitor)$/;
const QUIET_TOOLS = new Set(["Read", "Glob", "Grep", "LS", "WebFetch", "WebSearch", "ToolSearch",
  "NotebookRead", "TaskOutput", "BashOutput", "TaskList", "TaskGet", "ListMcpResourcesTool", "ReadMcpResourceTool"]);
let toolNames = new Map();   // tool call id -> tool name, for its result
let toolResults = new Map(); // tool call id -> its result's text
let pendingNow = [];         // the parked requests, as /api/pending last said
function cmdLine(s){
  const {hold, back} = holder();
  const t = s
    .replace(/"(?:\\.|[^"\\])*"|'[^']*'/g, m => hold(`<span class="tq">${esc(m)}</span>`))
    .replace(/(^|\n|[|;&(]\s*)([^\s|;&()\uE000][^\s|;&()]*)/g, (_, pre, w) => pre + hold(`<span class="tc">${esc(w)}</span>`))
    .replace(/(^|\s)(--?[A-Za-z][\w-]*)/g, (_, sp, f) => sp + hold(`<span class="tf">${esc(f)}</span>`))
    .replace(PATH_RE, m => hold(pathSpan(m, "tp")));
  return back(esc(t));
}
// An AskUserQuestion in the chat: the questions and their choices, and the
// answer once given. The window's card (#decide) answers it only while the
// permission hook holds it; after that it is the terminal's to answer.
function answersOf(text){
  const out = new Map(), re = /"((?:[^"\\]|\\.)*)"="((?:[^"\\]|\\.)*)"/g;
  let m;
  while ((m = re.exec(String(text || "")))) out.set(m[1], m[2]);
  return out;
}
// Several questions show one at a time, as the terminal's dialog does: a tab
// per question (☒ once answered) picks the page, with radio inputs and CSS
// only, so a redraw of the log needs nothing kept (2026-11-16).
function askHtml(b){
  const result = toolResults.get(b.id);
  const got = answersOf(result);
  const many = b.questions.length > 1;
  const rid = "aq-" + String(b.id || Math.random()).replace(/[^\w-]/g, "");
  const qs = b.questions.map((q, k) => {
    const ans = got.get(q.question);
    // One choice is the answer whole (a label can hold ", "); several are
    // the labels joined with ", ", so each is looked for in it.
    const picked = new Set(ans == null ? [] : q.multiSelect
      ? q.options.map(o => o.label).filter(l => (", " + ans + ", ").includes(", " + l + ", "))
      : [String(ans)]);
    const known = q.options.some(o => picked.has(o.label));
    return `<section class="askp"><div class="qt">${q.header && !many ? `<span class="chip">${esc(q.header)}</span>` : ""}${esc(q.question)}${
      q.multiSelect ? " <small>(choose any)</small>" : ""}</div><ul>${q.options.map(o =>
      `<li${picked.has(o.label) ? ' class="on"' : ""}>${esc(o.label)}${o.description ? `<small>${esc(o.description)}</small>` : ""}</li>`).join("")}</ul>${
      ans != null && !known ? `<div class="qans">answered: ${esc(ans)}</div>` : ""}</section>`;
  }).join("");
  const tabs = !many ? "" : b.questions.map((q, k) => `<input type="radio" class="ar" name="${rid}" id="${rid}-${k}"${
      k ? "" : " checked"}>`).join("") +
    `<div class="asktabs">${b.questions.map((q, k) => `<label for="${rid}-${k}" title="${esc(q.question)}">${
      got.has(q.question) ? "&#9746;" : "&#9744;"} ${esc(q.header || "Question " + (k + 1))}</label>`).join("")}</div>`;
  const parked = pendingNow.some(p => p.tool === "AskUserQuestion" && p.session_id === b.session &&
                                      JSON.stringify(p.questions || []) === JSON.stringify(b.questions));
  const wait = result != null ? ""
    : parked ? `<div class="qwait">waiting for your answer - answer it in the box at the top</div>`
    : `<div class="qwait">waiting for your answer in the session's terminal (or already answered there)</div>`;
  return `<div class="askq${many ? " many" : ""}"><div class="qh">AskUserQuestion</div>${tabs}${qs}${wait}</div>`;
}
function toolHtml(b){
  if (b.questions && b.questions.length) return askHtml(b);
  const s = String(b.summary || "");
  const arg = !s ? "" : SHELL_TOOLS.test(b.name) ? cmdLine(s)
    : /^(?:Grep|Glob)$/.test(b.name) ? `<span class="tq">${esc(s)}</span>`
    : (PATH_TOOLS.test(b.name) && !s.includes("\n")) || PATH_ONLY.test(s) ? pathSpan(s, "tp")
    : esc(s);
  return `<div class="tool"><b>${esc(b.name)}</b> ${arg}</div>` +
    (b.diff ? `<pre class="code dif tdiff">${diffCodeHtml(b.diff, true)}</pre>` : "");
}
function resultHtml(b){
  const quiet = QUIET_TOOLS.has(toolNames.get(b.id));
  return `<div class="res${b.is_error ? " err" : ""}${quiet ? " quiet" : ""}${b.truncated ? " cut" : ""}">${
    markPlain(b.text)}${b.truncated ? '<span class="cutmark">\n… cut short</span>' : ""}</div>`;
}
// -- end highlighting --

function emph(s){
  return s
    .replace(/\*\*(?!\s)([^\n]+?)(?<!\s)\*\*/g, "<b>$1</b>")
    .replace(/(^|[^\w])__(?!\s)([^\n]+?)(?<!\s)__(?!\w)/g, "$1<b>$2</b>")
    .replace(/(^|[^*\w])\*(?![\s*])([^*\n]+?)(?<!\s)\*(?![*\w])/g, "$1<i>$2</i>")
    .replace(/(^|[^\w])_(?![\s_])([^_\n]+?)(?<!\s)_(?!\w)/g, "$1<i>$2</i>")
    .replace(/~~(?!\s)([^\n]+?)(?<!\s)~~/g, "<s>$1</s>");
}

// Then paths, references, signals and amounts are marked (see highlighting);
// a code span that is a path or a command says so.
const codeSpan = c => PATH_ONLY.test(c)
  ? `<code class="pth" data-copy="${attr(c)}" title="click to open, show in its folder or copy">${esc(c)}</code>`
  : `<code${CMD_RE.test(c) ? ' class="cmd"' : ""}>${esc(c)}</code>`;
function inline(s){
  const {hold, back} = holder();
  s = String(s == null ? "" : s)
    .replace(/`([^`\n]+)`/g, (_, c) => hold(codeSpan(c)))
    .replace(/\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)/g, (_, t, u) => hold(link(u, emph(esc(t)))))
    .replace(/\bhttps?:\/\/[^\s<>()\[\]"'`\uE000\uE001]*[^\s<>()\[\]"'`\uE000\uE001.,;:!?*_~]/g,
             u => hold(link(u, esc(u))));
  return back(emph(esc(markWith(s, hold))));
}

function renderTable(head, sep, rows){
  const align = cells(sep).map(c =>
    c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" :
    c.startsWith(":") ? "left" : "");
  const row = (tag, cs) => "<tr>" + align.map((a, i) =>
    `<${tag}${a ? ` style="text-align:${a}"` : ""}>${inline(cs[i] || "")}</${tag}>`
  ).join("") + "</tr>";
  return `<div class="tbl"><table><thead>${row("th", head)}</thead><tbody>${
    rows.map(r => row("td", r)).join("")}</tbody></table></div>`;
}

// Block Markdown, the subset Claude actually writes: fenced code, headings,
// bullet and numbered lists (nested by indent), block quotes, rules and
// tables. Prose between them stays one pre-wrapped run per paragraph group,
// so line breaks Claude put there survive as they always have.
const FENCE = /^\s*(```|~~~)/;
const HEADING = /^(#{1,6})\s+(.*?)\s*#*\s*$/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;
const QUOTE = /^\s*>\s?(.*)$/;
const indentOf = s => s.replace(/\t/g, "    ").length;

// -- a teammate's message to the session that started it --------------------
// Claude Code hands a teammate's report to its lead as a user turn:
// "Another Claude session sent a message:", one or more
// <teammate-message teammate_id=.. color=.. summary=..> elements (a JSON
// notification or plain text), then a paragraph on how to treat it. The
// terminal shows "Teammate @name finished" and the result; so does this.
const MATE_RE = /<teammate-message\s+([^>]*)>([\s\S]*?)<\/teammate-message>/g;
const MATE_INTRO = /^\s*Another Claude session sent a message:\s*/;
const MATE_TAIL = /(?:^|\n)\s*This came from another Claude session[\s\S]*$/;
const MATE_COLORS = {blue: "--busy", green: "--ok", yellow: "--hl-ref", red: "--fail", purple: "--comp",
                     orange: "--warn", pink: "--hl-variable", cyan: "--hl-string"};
function mateAttr(attrs, name){
  const m = new RegExp("(?:^|\\s)" + name + '="([^"]*)"').exec(attrs);
  return m ? m[1] : "";
}
// Pure: [{name, color, summary, kind, verb, text}] for a text that is only
// teammate messages, else null.
function teammateParts(text){
  if (typeof text !== "string" || text.indexOf("<teammate-message") < 0) return null;
  const parts = [];
  const rest = text.replace(MATE_RE, (all, attrs, body) => {
    const p = {name: mateAttr(attrs, "teammate_id"), color: mateAttr(attrs, "color"),
               summary: mateAttr(attrs, "summary"), kind: "message", verb: "", text: body.trim()};
    let said = null;
    try { said = JSON.parse(p.text); } catch (e) {}
    if (said && typeof said === "object" && !Array.isArray(said)){
      p.kind = String(said.type || "message");
      p.verb = p.kind === "idle_notification" ? "finished"
        : p.kind === "shutdown_approved" || p.kind === "teammate_terminated" ? "shut down"
        : p.kind.replace(/_/g, " ");
      p.text = String(said.result ?? said.message ?? said.content ?? said.reason ?? said.summary ?? "");
    }
    parts.push(p);
    return "";
  });
  if (!parts.length || rest.replace(MATE_TAIL, "").replace(MATE_INTRO, "").trim()) return null;
  return parts;
}
// Pure: the one line a folded teammate message shows.
function teammateLine(parts){
  return parts.map(p => "@" + p.name + " " + (p.verb || "says") + (p.summary || p.text ?
    ": " + String(p.summary || p.text).split("\n").map(l => l.trim()).find(l => l) : "")).join(" · ");
}
function teammateHtml(parts){
  return parts.map(p => {
    const c = MATE_COLORS[p.color];
    return `<div class="mate"${c ? ` style="--mate:var(${c})"` : ""}><div class=mh>Teammate <span class=at>@${
      esc(p.name || "?")}</span>${p.verb ? " " + esc(p.verb) : p.summary ? ": " + esc(p.summary) : ""}</div>${
      p.text ? `<div class=mb>${renderText(p.text)}</div>` : ""}</div>`;
  }).join("");
}

function renderText(text){
  const lines = String(text == null ? "" : text).split("\n");
  const out = [];
  let run = [], lists = [], listHtml = "";
  const flush = () => {
    const t = run.join("\n").replace(/^\n+|\n+$/g, "");
    if (t) out.push(`<div class="txt">${t.split("\n").map(askLine).join("\n")}</div>`);
    run = [];
  };
  const closeList = () => {
    const top = lists.pop();
    listHtml += (top.open ? "</li>" : "") + (top.ordered ? "</ol>" : "</ul>");
  };
  const endLists = () => {
    while (lists.length) closeList();
    if (listHtml) out.push(`<div class="md">${listHtml}</div>`);
    listHtml = "";
  };
  const block = html => { flush(); endLists(); out.push(html); };

  for (let i = 0; i < lines.length; i++){
    const line = lines[i];
    const next = lines[i + 1];

    if (FENCE.test(line)){
      const mark = line.trim().slice(0, 3), body = [];
      const info = (line.trim().slice(3).trim().split(/\s+/)[0] || "").toLowerCase();
      let j = i + 1;
      while (j < lines.length && !lines[j].trim().startsWith(mark)) body.push(lines[j++]);
      block(codeBlock(body.join("\n"), info));
      i = j;
      continue;
    }
    if (line.includes("|") && next !== undefined && TABLE_SEP.test(next) && next.includes("|")){
      const head = cells(line);
      if (cells(next).length === head.length){
        const rows = [];
        let j = i + 2;
        while (j < lines.length && lines[j].includes("|") && lines[j].trim())
          rows.push(cells(lines[j++]));
        block(renderTable(head, next, rows));
        i = j - 1;
        continue;
      }
    }
    const item = ITEM.exec(line);
    if (item && !RULE.test(line)){
      flush();
      const indent = indentOf(item[1]), ordered = /\d/.test(item[2]);
      while (lists.length && indent < lists[lists.length - 1].indent) closeList();
      let top = lists[lists.length - 1];
      if (top && indent === top.indent && top.ordered !== ordered){ closeList(); top = null; }
      else if (top && indent < top.indent) top = null;
      if (!top || indent > top.indent){
        const n = parseInt(item[2], 10);
        listHtml += ordered ? (n > 1 ? `<ol start="${n}">` : "<ol>") : "<ul>";
        lists.push(top = {indent, ordered, open: false});
      }
      listHtml += (top.open ? "</li>" : "") + "<li>" + askLine(item[3]);
      top.open = true;
      continue;
    }
    if (lists.length){
      // A list survives a blank line when it carries on after it, and takes
      // indented lines as the current item's continuation.
      if (!line.trim()){
        const after = lines.slice(i + 1).find(l => l.trim());
        if (after !== undefined && (ITEM.test(after) || /^\s{2,}/.test(after))) continue;
      } else if (/^\s{2,}/.test(line)){
        listHtml += "<br>" + inline(line.trim());
        continue;
      }
      endLists();
    }
    const h = HEADING.exec(line);
    if (h){ block(`<div class="h h${h[1].length}">${inline(h[2])}</div>`); continue; }
    if (RULE.test(line) && line.trim()){ block("<hr>"); continue; }
    if (QUOTE.test(line)){
      const inner = [];
      let j = i;
      while (j < lines.length && QUOTE.test(lines[j])) inner.push(QUOTE.exec(lines[j++])[1]);
      block(`<blockquote>${renderText(inner.join("\n"))}</blockquote>`);
      i = j - 1;
      continue;
    }
    run.push(line);
  }
  flush();
  endLists();
  return out.join("");
}

