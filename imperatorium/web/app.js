
let sessions = [], selected = null, lastSig = "", busy = false;

function age(t){
  if (!t) return "";
  const d = Math.max(0, Math.floor(Date.now()/1000 - t));
  if (d < 60) return d + "s";
  if (d < 3600) return Math.floor(d/60) + "m";
  return Math.floor(d/3600) + "h";
}

function key(s){ return s.sessionId || s.id; }
// The store's key: host-qualified for a remote session. Pins and archive
// are kept against this so a local and a remote session never share one.
function pkey(s){ return s.key || key(s); }

// -- sidebar preferences -------------------------------------------------
// Kept by the dispatcher, not the browser, so a tablet sees the same pins.
let prefs = {pinned: [], archived: [], group: "state"};
let showArchived = false;

async function savePrefs(change){
  Object.assign(prefs, change);
  try { const r = await post("/api/prefs", change); if (r.prefs) Object.assign(prefs, r.prefs); }
  catch (e) {}
  drawSessions();
}
function toggleIn(list, k){
  const l = (prefs[list] || []).slice();
  const i = l.indexOf(k);
  if (i >= 0) l.splice(i, 1); else l.push(k);
  return l;
}

const folder = cwd => String(cwd || "").replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "(no folder)";

// What a tile calls a session. The server turns a worktree session's
// made-up "<worktree>-<id>" into "<project>-<worktree>"; a chosen name stays.
const nameOf = s => s.display_name || s.name || s.id || "(unnamed)";
// The project a session belongs to - for a worktree, the repository it was
// made from, not the worktree's folder.
const projectOf = s => (s.project || folder(s.cwd)) + (s.host ? " @ " + s.host : "");

function groupOf(s){
  if (prefs.group === "folder") return projectOf(s);
  if (prefs.group === "state"){
    if (s.attention) return s.attention_kind === "complete" ? "complete" : "needs you";
    const dot = stateOf(s)[0];
    if (dot === "d-busy") return "working";
    if (dot === "d-idle") return "waiting on tasks";
    return "quiet";
  }
  return "";
}
const STATE_ORDER = ["needs you", "complete", "working", "waiting on tasks", "quiet"];

function prBadge(pr){
  if (!pr) return "";
  const cls = pr.state === "merged" ? "merged" : pr.state === "closed" ? "" : pr.checks;
  const mark = {passing:"✓", failing:"✗", pending:"…"}[pr.checks] || "";
  const what = pr.state === "open" ? (pr.draft ? "draft" : "") : pr.state;
  return `<span class="badge pr ${esc(cls)}" data-url="${esc(pr.url)}"
    title="${esc(pr.title)}${pr.review ? " · review: " + esc(pr.review) : ""}${
    " · checks: " + esc(pr.checks)}">#${esc(pr.number)}${mark ? " " + mark : ""}${
    what ? " " + esc(what) : ""}</span>`;
}

// Marks a session hosted by the Claude desktop app: it lives in that app's
// window, so someone scanning terminals for it would never find it.
const IDE_ICON = `<svg class="app" viewBox="0 0 13 11" aria-label="VS Code"><title>in VS Code</title><path d="M4 2.5 1 5.5l3 3M9 2.5l3 3-3 3M7.5 1.5l-2 8" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
const APP_ICON = `<svg class="app" viewBox="0 0 13 11" aria-label="Claude desktop app"><title>in the Claude desktop app</title><rect x=".5" y=".5" width="12" height="10" rx="1.5" fill="none" stroke="currentColor"/><path d="M.5 3.5h12" stroke="currentColor"/></svg>`;
// Marks a session working in a worktree rather than the main checkout.
const WT_ICON = `<svg class="app" viewBox="0 0 11 13" aria-label="worktree"><title>in a worktree</title><circle cx="3" cy="2.5" r="1.6" fill="none" stroke="currentColor"/><circle cx="3" cy="10.5" r="1.6" fill="none" stroke="currentColor"/><circle cx="8.5" cy="4.5" r="1.6" fill="none" stroke="currentColor"/><path d="M3 4.1v4.8M8.5 6.1c0 2.2-5.5 1.4-5.5 2.8" fill="none" stroke="currentColor"/></svg>`;

// Pure: the prompt cache's countdown for a tile, "-4" minutes until it
// expires. A closed tile shows it only in the last CACHE_SOON seconds, when
// it is worth acting on; `all` (the open tile) shows it whenever it is warm.
// Nothing once it has expired: the chat's cold-cache button says that.
// A session that scheduled its own next turn (ScheduleWakeup, CronCreate)
// says when, on every tile: it is idle but not waiting for you.
const CACHE_SOON = 600;
function workParts(w, now, all){
  if (!w) return [];
  const out = [];
  const wake = w.alive && w.wakes_at ? w.wakes_at - now : 0;
  if (wake > 0) out.push({text: "wakes " + span(wake), cls: "wake",
    title: `it scheduled its own next turn, in ${span(wake)} (${w.wakeups || 1} pending)`});
  const left = w.cache_expires_at ? w.cache_expires_at - now : 0;
  if (left > 0 && (all || left <= CACHE_SOON)) out.push({text: "-" + Math.ceil(left / 60), cls: "cache",
    title: `prompt cache expires in ${Math.ceil(left / 60)} min; a prompt after that re-sends the whole context`});
  return out;
}
const workHtml = (w, now, cls, all) => workParts(w, now, all).map(p =>
  `<span class="${cls} ${p.cls}" title="${esc(p.title)}">${esc(p.text)}</span>`).join("");

// How long a session has been idle: whole minutes and up ("<1m", "4m",
// "2h 5m"); seconds are more resolution than a wait needs.
const idleSpan = sec => sec < 60 ? "<1m" : span(Math.floor(sec / 60) * 60);

// Pure: what the open tile adds under its first two lines - how long it has
// been idle (or working), its context size, what it has cost so far (from
// the status panel's answer, `spend`), and what it still has running while
// its process lives.
function tileDetails(s, now, spend){
  const out = [], w = s.work || {}, u = s.usage || {};
  const idle = s.status === "idle" && s.status_changed_at;
  if (idle) out.push({text: "idle " + idleSpan(now - idle), title: "idle since its last turn ended"});
  else {
    const at = s.status_changed_at || s.last_event_at;
    if (at) out.push({text: (stateOf(s)[0] === "d-busy" ? "working " : "active ") + span(now - at),
      title: "in this state since"});
  }
  if (w.context_tokens || u.context_pct != null)
    out.push({text: "ctx " + [w.context_tokens ? kfmt(w.context_tokens) : "",
      u.context_pct != null ? "(" + Math.round(u.context_pct) + "%)" : ""].filter(Boolean).join(" "),
      title: "context size", cls: u.context_pct >= CTX_HI ? "ctx-hi" : ""});
  if (spend && spend.cost_usd != null)
    out.push({text: "≈" + usd(spend.cost_usd), title: "the whole session so far, subagents included, at API list prices"});
  if (w.alive){
    const n = (w.agents || 0) + (w.processes || 0);
    if (n) out.push({text: n + " running", cls: "busy",
      title: `${w.agents || 0} subagents, ${w.processes || 0} shell processes ` +
        `(${w.shells || 0} background shells and ${w.monitors || 0} monitors among them)`});
  }
  return out;
}

// Pure: the closed tile's right-hand signals, each only while it is out of
// the ordinary and always in this order, so the eye learns where to look.
// How long it has waited is the open tile's "idle" figure, not said here.
const CTX_HI = 80;
function tileSignals(s, now, open){
  const u = s.usage || {}, out = [];
  if (isUnread(s)) out.push(`<span class="badge new" title="finished a turn since you last looked at it">new</span>`);
  out.push(workHtml(s.work, now, "badge work", open));
  // No cold-cache badge: the chat says that, with the way out (2026-11-14).
  if (!open && u.context_pct >= CTX_HI)
    out.push(`<span class="badge ctx-hi" title="context used">ctx ${Math.round(u.context_pct)}%</span>`);
  // Queued here, and handed to the session's mod but not yet started.
  const waiting = (s.queued || 0) + (s.mod_pending || 0);
  if (waiting) out.push(`<span class="badge q" title="prompts queued">${waiting} queued</span>`);
  if (!open && s.pr && s.pr.checks === "failing" && s.pr.state === "open") out.push(prBadge(s.pr));
  return out.join("");
}

// A closed tile is two lines however little it knows: its dot, name and why
// it is in its state (only when the dot does not already say it), its
// signals on the right; then its window title, or what it is doing, or its
// folder. Open - the selected tile, and whichever one the pointer is over
// (CSS) - the reason moves to a line of its own, the window title and what
// it is doing get a line each, and tileDetails with its pin and close
// follows. Every line is drawn; CSS shows the open ones. The signals are
// the open tile's for the selected one only.
// `short` is for a tile under its project's node, where the project's name
// would only be said twice; the chat header still carries the full name.
// `depth` nests a fork under the session it was forked from (forkOrder).
function tile(s, short, depth){
  const [dot, label] = stateOf(s);
  const fd = Math.min(depth || 0, 4);
  const now = Date.now() / 1000, open = key(s) === selected && view !== "fleet";
  const mark = s.attention ? (s.attention_kind === "failure" ? " fail"
    : s.attention_kind === "complete" ? " comp" : " attn") : "";
  const pinned = (prefs.pinned || []).includes(pkey(s));
  const archived = (prefs.archived || []).includes(pkey(s));
  const plain = dot === "d-ready" || label === "working";
  // The doing line is extra ("x", open only) when a window title is there
  // to be the closed tile's second line.
  const title = s.window_title ? `<div class="w" title="window title">${esc(s.window_title)}</div>` : "";
  const doing = s.doing ? `<div class="d${title ? " x" : ""}" title="${esc(s.doing)}">${esc(s.doing)}${
      s.doing_since ? " · " + age(s.doing_since) : ""}</div>` : "";
  const lines = title + doing || `<div class="w" title="${esc(s.cwd || "")}">${esc(s.cwd || "")}</div>`;
  // ✕ closes a session with a process; with none (a finished background job,
  // a crashed session's leftover file) it takes the tile off the list. Any
  // tile, not only archived ones: a finished job that is not archived used to
  // have no way off the list at all.
  // A job still working but not yet matched to its process gets neither.
  const x = s.remote ? ""
    : s.pid ? `<span data-close="${esc(key(s))}" title="close this session and its window (the conversation stays resumable)">✕</span>`
    : s.state === "working" || RUNNING[s.status] ? ""
    : `<span data-dismiss="${esc(key(s))}" title="nothing is running: take it off the list (the conversation stays resumable)">✕</span>`;
  const acts = `<span data-pin="${esc(pkey(s))}" title="${pinned?"unpin":"pin to top"}">${
      pinned ? "★" : "☆"}</span>${archived ? `<span data-arch="${esc(pkey(s))}" title="unarchive">↩</span>` : ""}${x}`;
  const spend = open && statusFor === key(s) && statusData ? statusData.spend : null;
  const details = `<div class="m"><span class="det">${tileDetails(s, now, spend).map(p =>
    `<span class="${p.cls || ""}" title="${esc(p.title)}">${esc(p.text)}</span>`).join(" · ")}${
    s.host ? " · " + esc(s.host) : ""}${prBadge(s.pr)}</span></div>`;
  return `<div class="s${open ? " on" : ""}${mark}${isUnread(s) ? " unread" : ""}${fd ? " fk" : ""}"${
      fd ? ` style="--fd:${fd}"` : ""} data-k="${esc(key(s))}">
    <div class="t1"><span class="dot ${dot}" title="${esc(label)}"></span><span class="n" title="${esc(nameOf(s))}">${
      forkMark(s, sessions)}${s.desktop_app ? APP_ICON : s.ide === "vscode" ? IDE_ICON : ""}${s.worktree ? WT_ICON : ""}${esc(short && s.short_name || nameOf(s))}${
      pinned ? `<span class="pin">★</span>` : ""}</span>${
      plain ? "" : `<span class="why" title="${esc(label)}">${esc(label)}</span>`}<span class="sig">${
      tileSignals(s, now, open)}</span>${
      // Pin and close sit in the open tile's top right corner (2026-11-16);
      // a closed tile has none.
      open ? `<span class="acts">${acts}</span>` : ""}</div><div class="stl st-${dot}" title="${esc(label)}">${
      esc(label)}</div>${lines}${details}${open ? `<div class="mcpl"></div><div class="tk"></div>` : ""}
  </div>`;
}

// Tiles for one group, with a project's sessions gathered under a node named
// for the project whenever it has more than one. The node carries one dot per
// session, so how many there are and what each is doing still shows when it
// is folded. A click on the node hides or shows all of it; a double click
// hides or shows only the sessions waiting for you, leaving the rest.
// archive does. Grouped by folder, the whole list is that tree: every
// project gets a node, one session or several, in place of a group header.
// Pure: `list` with each fork moved to just under the session it was forked
// from (work.forked_from, read off the transcript), as [session, depth].
// Sessions whose parent is not in `list` keep their place at depth 0; a
// parent's forks keep the order they had. A loop cannot nest anything.
function forkOrder(list){
  const here = new Set(list.map(key)), kids = new Map();
  const parentOf = s => { const p = (s.work || {}).forked_from; return p && p !== key(s) && here.has(p) ? p : null; };
  for (const s of list){
    const p = parentOf(s);
    if (p){ if (!kids.has(p)) kids.set(p, []); kids.get(p).push(s); }
  }
  const out = [], put = new Set();
  const walk = (s, d) => {
    if (put.has(s)) return;
    put.add(s); out.push([s, d]);
    for (const k of kids.get(key(s)) || []) walk(k, d + 1);
  };
  for (const s of list) if (!parentOf(s)) walk(s, 0);
  for (const s of list) walk(s, 0);  // only a loop is left
  return out;
}
// Pure: the "forked from" marker for a tile - the parent's name when it is
// on the list, else its id's head.
function forkMark(s, all){
  const p = (s.work || {}).forked_from;
  if (!p) return "";
  const parent = (all || []).find(x => key(x) === p);
  const name = parent ? nameOf(parent) : p.slice(0, 8);
  return `<span class="fork" title="forked from ${esc(name)}">&#8627;</span>`;
}

function tree(list){
  const fams = new Map();
  for (const s of list){
    const f = projectOf(s);
    if (!fams.has(f)) fams.set(f, []);
    fams.get(f).push(s);
  }
  const tiles = (kids, short) => forkOrder(kids).map(([s, d]) => tile(s, short, d)).join("");
  return [...fams].map(([f, kids]) => {
    // A project with one session needs no header, in any grouping.
    if (kids.length < 2) return tiles(kids);
    const shut = folded.has(f), noWait = waitHidden.has(f);
    const dots = kids.map(s => `<span class="dot ${stateOf(s)[0]}" title="${
      esc(nameOf(s) + ": " + stateOf(s)[1])}"></span>`).join("");
    const shown = shut ? [] : kids.filter(s => !(noWait && s.attention));
    const nNew = kids.filter(isUnread).length;
    return `<div class="fam${shut ? " shut" : ""}" data-fam="${esc(f)}" title="${
      kids.length} sessions - click to ${shut ? "show" : "hide"} them all, double-click to ${
      noWait ? "show" : "hide"} the ones waiting for you"><span class="car">${
      shut ? "▸" : "▾"}</span><span>${esc(f)}</span>${noWait && !shut ? `<span class="nowait">waiting hidden</span>` : ""
      }${nNew ? `<span class="newn" title="sessions that finished a turn since you last looked">${nNew} new</span>` : ""
      }<span class="dots">${dots}</span></div>` +
      (shown.length ? `<div class="kids">${tiles(shown, true)}</div>` : "");
  }).join("");
}

// Folded project nodes, and nodes hiding their waiting sessions, per
// browser: a viewing convenience, not shared state.
const savedSet = name => new Set((() => {
  try { return JSON.parse(localStorage.getItem(name) || "[]"); }
  catch (e) { return []; }
})());
const folded = savedSet("imperatorium.folded"), waitHidden = savedSet("imperatorium.waithidden");
function flip(set, name, f){
  if (set.has(f)) set.delete(f); else set.add(f);
  try { localStorage.setItem(name, JSON.stringify([...set])); } catch (e) {}
  drawSessions();
}
// Unfolding always shows every session, the waiting ones included - the
// opposite of folding, which hides them all.
function toggleFold(f){
  if (folded.has(f) && waitHidden.has(f)){
    waitHidden.delete(f);
    try { localStorage.setItem("imperatorium.waithidden", JSON.stringify([...waitHidden])); } catch (e) {}
  }
  flip(folded, "imperatorium.folded", f);
}
const toggleWaiting = f => flip(waitHidden, "imperatorium.waithidden", f);

// A double click starts with a click, so a click waits briefly to see whether
// a second follows. Kept by node name, not element: the list is redrawn
// every 1.5 s and may replace the node between the two clicks.
const DOUBLE_MS = 300;
let famClick = null;
function famClicked(f){
  const prev = famClick;
  famClick = null;
  if (prev){
    clearTimeout(prev.t);
    if (prev.f === f) return toggleWaiting(f);
    toggleFold(prev.f);
  }
  famClick = {f, t: setTimeout(() => { famClick = null; toggleFold(f); }, DOUBLE_MS)};
}

// A click is a press and a release on the same element. The session list,
// the status panel, the waiting-on tabs and a busy session's log are redrawn
// on a timer or on every pushed change, and a redraw between press and
// release replaces the element under the pointer: the click then lands on
// its container and does nothing - a tile that would not select while
// sessions were busy. So a panel's redraw waits while a press inside it is
// down, and runs once it is released (after the click, which comes in the
// same task as the release).
const pressedIn = new Set(), heldDraws = new Map();
function holdPresses(sel){
  $(sel).addEventListener("pointerdown", e => { if (e.button === 0) pressedIn.add(sel); });
}
// True while `sel` is pressed; `draw` then runs on the release instead.
function held(sel, draw){
  if (!pressedIn.has(sel)) return false;
  heldDraws.set(sel, draw);
  return true;
}
function releasePresses(){
  pressedIn.clear();
  const draws = [...heldDraws.values()];
  heldDraws.clear();
  if (draws.length) setTimeout(() => draws.forEach(f => f()), 0);
}
// textContent replaced only when it differs: the same text set again still
// swaps the node, and drops a selection in it.
function setText(el, text){ if (el.textContent !== text) el.textContent = text; }

// The line under the prompt box is about one session. Switching away hides
// it and switching back shows it again; a reply that lands after a switch is
// kept for the session it is about, not shown on another's.
const said = {};
let saidFor;
function say(text, k){
  if (k === undefined) k = selected;
  said[k] = text;
  if (k === selected){ $("#say").textContent = text; saidFor = k; }
}
function showSaid(){
  if (saidFor === selected) return;
  saidFor = selected;
  $("#say").textContent = said[selected] || "";
}

function drawSessions(){
  if (held("#sessions", drawSessions)) return;
  // Whatever wants a human first, then this machine, then idle.
  const want = s => !s.attention ? 3 : s.attention_kind === "complete" ? 2 : s.attention_kind === "failure" ? 1 : 0;
  const quiet = s => ["d-idle", "d-ready", "d-rev", "d-revc"].includes(stateOf(s)[0]) ? 1 : 0;
  sessions.sort((a,b) =>
    want(a) - want(b) ||
    (a.remote?1:0) - (b.remote?1:0) ||
    quiet(a) - quiet(b) ||
    nameOf(a).localeCompare(nameOf(b)));
  if (selected && !sessions.some(s => key(s) === selected)) selected = null;
  if (!selected && sessions.length) selected = key(sessions[0]);
  showSaid();

  const q = ($("#filter").value || "").trim().toLowerCase();
  const hit = s => !q || [nameOf(s), s.name, s.project, s.cwd, s.window_title, s.doing, s.host]
    .some(v => String(v || "").toLowerCase().includes(q));
  const pinned = new Set(prefs.pinned || []), archived = new Set(prefs.archived || []);
  // Archive hides a session until it wants you; a blocked session always shows.
  const shown = sessions.filter(s => hit(s) && (!archived.has(pkey(s)) || s.attention));
  const hidden = sessions.filter(s => hit(s) && archived.has(pkey(s)) && !s.attention);

  const groups = new Map();
  const top = shown.filter(s => pinned.has(pkey(s)));
  if (top.length) groups.set("pinned", top);
  // A project's sessions stay together, in the group of its most urgent one
  // (the list is already sorted that way); each tile keeps its own dot.
  const famGroup = new Map();
  for (const s of shown){
    if (pinned.has(pkey(s))) continue;
    const f = projectOf(s);
    if (!famGroup.has(f)) famGroup.set(f, groupOf(s));
    const g = famGroup.get(f);
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g).push(s);
  }
  let names = [...groups.keys()];
  if (prefs.group === "state")
    names.sort((a,b) => (a==="pinned"?-1:STATE_ORDER.indexOf(a)) - (b==="pinned"?-1:STATE_ORDER.indexOf(b)));
  else if (prefs.group === "folder")
    names.sort((a,b) => a==="pinned" ? -1 : b==="pinned" ? 1 : a.localeCompare(b));

  const byFolder = g => prefs.group === "folder" && g !== "pinned";
  let html = names.map(g => (!byFolder(g) && (g || names.length > 1) ?
      `<div class="g"><span>${esc(g || "sessions")}</span><span>${groups.get(g).length}</span></div>` : "") +
    tree(groups.get(g))).join("");
  if (hidden.length){
    html += `<div class="g fold" id="archfold"><span>${showArchived ? "▾" : "▸"} archived</span><span>${hidden.length}</span></div>`;
    if (showArchived) html += hidden.map(s => tile(s)).join("");
  }
  html = html || `<div class="note">${q ? "no match" : "no sessions"}</div>`;
  // Unchanged, the tiles stay: a node that is not replaced keeps its hover
  // and can take a click.
  if ($("#sessions").dataset.sig !== html){ $("#sessions").innerHTML = html; $("#sessions").dataset.sig = html; }

  syncDraft();
  drawHead();
  drawTileTasks();
}

// Pure: the Session Subtitle - the branch the session is on and, in a
// worktree, which one, ahead of its folder, so where its work goes is in
// view (2026-11-16). The branch is the transcript's (gitBranch, via the
// status panel's data), so it is there for remote ones. Not the window
// title (2026-11-17): that is the prompt's gist, already on the tile and a
// click away in Show Current Prompt.
function headSub(s, branch){
  const parts = [];
  if (branch) parts.push(`<span class="hbr" title="git branch">&#9095; ${esc(branch)}</span>`);
  if (s.worktree) parts.push(`<span class="hwt" title="a worktree, not the main checkout">${WT_ICON} worktree ${esc(s.worktree)}</span>`);
  else if (branch) parts.push(`<span class="hwt main" title="the project's main checkout">main checkout</span>`);
  const rest = s.cwd ? esc(s.cwd) : "";
  return parts.join(" ") + (parts.length && rest ? " <span class=hsep>·</span> " : "") + rest;
}
// Pure: whether the window's Stop applies to row `s`: a session on this
// machine in the middle of a turn.
const canStop = s => !!s && !s.remote && (!!RUNNING[s.status] || s.state === "working");
// Stop (2026-11-16): the turn ends as with Esc at the terminal - through the
// session's mod when it can (then what came of it is asked for), else Esc
// pressed in its console. Never twice within a few seconds from here: Esc
// Esc in the terminal is its rewind, and the dispatcher refuses it too.
let stopAt = 0;
async function stopTurn(){
  const s = current();
  if (!canStop(s)) return false;
  if (Date.now() - stopAt < 3000) return true;
  stopAt = Date.now();
  const k = key(s);
  say("stopping…", k);
  let r;
  try { r = await post("/api/stop", {session_id: k}); } catch (e) { r = {ok: false, error: "the dispatcher did not answer"}; }
  if (!r.ok){ say(r.error || "could not stop it", k); return true; }
  if (r.via !== "mod"){ say("pressed Esc in its terminal", k); return true; }
  say("asked its mod to stop the turn…", k);
  for (let i = 0; i < 12; i++){
    await new Promise(res => setTimeout(res, 500));
    let got;
    try { got = await get("/api/stop?session=" + encodeURIComponent(k)); } catch (e) { continue; }
    const res = got && got.result;
    if (!res || res.id !== r.stop_id) continue;
    say(res.outcome === "aborted" ? "stopped" : res.outcome === "idle" ? "it had already finished"
      : "could not stop it: " + (res.reason || res.outcome), k);
    return true;
  }
  say("asked it to stop - no answer from its mod yet", k);
  return true;
}

function drawHead(){
  const s = current();
  if (view !== "session") return;
  setText($("#title"), s ? nameOf(s) : "no session selected");
  const sub = s ? headSub(s, statusFor === key(s) && statusData && statusData.stat ? statusData.stat.branch : "") : "";
  if ($("#sub").dataset.sig !== sub){ $("#sub").innerHTML = sub; $("#sub").dataset.sig = sub; }
  $("#sub").title = $("#sub").textContent;
  $("#send").disabled = !s || !!s.remote;
  $("#sendmore").classList.toggle("off", !s || !!s.remote);
  $("#queue").disabled = !s;
  $("#stop").hidden = !canStop(s);
  const acts = [];
  if (s && !s.remote){
    if (s.resumable){
      acts.push(`<button class="sm" data-act="resume-window" title="claude --resume in a new tab">Resume</button>`);
      acts.push(`<button class="sm ghost" data-act="resume-background" title="continue it in the background; the prompt box is its next message">Resume in bg</button>`);
    }
  }
  const html = acts.join("");
  if ($("#acts").dataset.sig !== html){ $("#acts").innerHTML = html; $("#acts").dataset.sig = html; }
  // New, New+close, Fork and Fork in bg live in the New / Fork menu, and
  // Show Window and Copy resume cmd in the View menu; none applies to a
  // remote session, and Show Window needs a running one.
  const local = !!(s && !s.remote);
  $("#showbtn").hidden = !(local && s.pid);
  $("#launch").hidden = !local;
  if (!local) $("#launch").open = false;
  $('#launch [data-act="new-close"]').hidden = !(local && s.pid);
  $("#copybtn").hidden = !local;
  $("#copybtn").title = local ? s.resume_command || "" : "";
  document.querySelectorAll("[data-act]").forEach(b => b.onclick = () => { closeMenus(); act(b.dataset.act); });
  drawQueued(s);
  drawHeavy(s);
}

function current(){ return sessions.find(x => key(x) === selected); }

function renderBlocks(blocks){
  return blocks.map(renderBlock).join("");
}

// A turn of yours that only carries teammates' messages: theirs, not yours.
function mateTurn(it){
  return it.speaker === "you" && (it.blocks || []).length > 0 &&
    it.blocks.every(b => b.kind === "text" && teammateParts(b.text));
}

function renderBlock(b){
  if (b.kind === "text"){
    const mates = teammateParts(b.text);
    return mates ? teammateHtml(mates) : renderText(b.text);
  }
  if (b.kind === "thinking") return `<div class="think">${esc(b.text)}</div>`;
  if (b.kind === "tool")     return toolHtml(b);
  if (b.kind === "result")   return resultHtml(b);
  return "";
}

// -- how each kind of text is shown ----------------------------------------
// Full is the whole block, as it always was; Compact folds a block to its
// first line, opened by a click; Hidden leaves it out. Kept in this browser,
// the same for every session.
const SHAPES = [["prompt", "Prompts"], ["reply", "Replies"], ["thinking", "Thinking"],
  ["tool", "Tool calls"], ["result", "Tool results"], ["system", "System"]];
const MODES = ["full", "compact", "hidden"];
const SUMMARY_NOTE = "earlier conversation, summarised";
let shape = {};
try { shape = JSON.parse(localStorage.getItem("imperatorium.shape") || "{}") || {}; } catch (e) { shape = {}; }
function modeOf(cat){ return MODES.includes(shape[cat]) ? shape[cat] : "full"; }
// View > Display Times: each block of the chat says when it was written, so
// "done in 10 minutes" can be read against a clock. Off unless turned on.
let showTimes = false;
try { showTimes = localStorage.getItem("imperatorium.times") === "1"; } catch (e) {}
// View > Group tool calls: each run of tool calls and their results, one
// after another in a turn, is folded into one group, collapsed until opened.
// Each group opens on its own. Off unless turned on.
let groupTools = false;
try { groupTools = localStorage.getItem("imperatorium.group") === "1"; } catch (e) {}

// Pure: the stamp for a block written at `at` (ISO), seen at `now` (ms):
// "14:05" today, "Nov 14 14:05" before; null when there is no time.
function blockTime(at, now){
  const d = new Date(at || "");
  if (!at || isNaN(d)) return null;
  const p = n => String(n).padStart(2, "0");
  const hm = p(d.getHours()) + ":" + p(d.getMinutes());
  const today = d.toDateString() === new Date(now).toDateString();
  const day = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][d.getMonth()] +
    " " + d.getDate() + " ";
  return {text: (today ? "" : day) + hm, title: d.toLocaleString()};
}
function stampHtml(b){
  const t = showTimes && blockTime(b.at, Date.now());
  return t ? `<span class=bt title="${esc(t.title)}">${esc(t.text)}</span>` : "";
}

function categoryOf(it, b){
  if (it.note) return "system";
  if (b.kind === "text") return it.speaker === "you" ? "prompt" : "reply";
  return b.kind;
}

function oneLine(b){
  const mates = b.kind === "text" && teammateParts(b.text);
  if (mates) return teammateLine(mates);
  const text = b.kind === "tool" ? b.summary : b.text;
  return String(text || "").split("\n").map(l => l.trim()).find(l => l) || "";
}

function compactBlock(b, k, open){
  const tag = b.kind === "tool" ? b.name : b.kind === "result" ? (b.is_error ? "error" : "result")
            : b.kind === "thinking" ? "thinking" : "";
  return `<details class="cmp" data-k="${k}"${open ? " open" : ""}><summary>${
    tag ? `<span class="k${b.is_error ? " err" : ""}">${esc(tag)}</span>` : ""}<span class="ln">${
    esc(oneLine(b))}</span></summary>${renderBlock(b)}</details>`;
}

// What Group tool calls folds together: calls and results. A question card
// is not folded away, since it waits on you.
const groupable = b => (b.kind === "tool" && !b.questions) || b.kind === "result";

// Pure: a group's one line - how many calls, which tools, how many failed.
function groupLine(bs){
  const names = new Map();
  let calls = 0, failed = 0;
  for (const b of bs){
    if (b.kind === "tool"){ calls++; names.set(b.name, (names.get(b.name) || 0) + 1); }
    else if (b.is_error) failed++;
  }
  return {calls, failed, list: [...names].map(([n, c]) => c > 1 ? n + " ×" + c : n).join(", ")};
}

function groupHtml(bs, inner, k, open){
  const g = groupLine(bs);
  return `<details class="tgroup" data-k="${k}"${open ? " open" : ""}><summary><span class="k">${
    g.calls} tool call${g.calls === 1 ? "" : "s"}</span><span class="ln">${esc(g.list)}</span>${
    g.failed ? `<span class="gf">${g.failed} failed</span>` : ""}</summary><div class="gb">${inner}</div></details>`;
}

// One turn's blocks as the controls say. `k` names a block by its place in
// the conversation, so one opened by hand stays open across repaints; a
// group is named "g" + its first block's.
function shapeBlocks(it, i, opened){
  const one = (b, j) => {
    const m = modeOf(categoryOf(it, b));
    if (m === "hidden") return "";
    const k = i + "." + j;
    return stampHtml(b) + (m === "compact" ? compactBlock(b, k, opened.has(k)) : renderBlock(b));
  };
  if (!groupTools) return it.blocks.map(one).join("");
  let out = "", run = [];
  const flush = () => {
    if (!run.length) return;
    const inner = run.map(([b, j]) => one(b, j)).join(""), bs = run.map(([b]) => b), k = "g" + i + "." + run[0][1];
    run = [];
    if (!inner) return;
    out += bs.some(b => b.kind === "tool") ? groupHtml(bs, inner, k, opened.has(k)) : inner;
  };
  it.blocks.forEach((b, j) => {
    if (groupable(b)){ run.push([b, j]); return; }
    // Something hidden between two calls does not end the run.
    if (modeOf(categoryOf(it, b)) === "hidden") return;
    flush();
    out += one(b, j);
  });
  flush();
  return out;
}

function drawShape(){
  // The View menu's button is lit while any kind of text is not shown in full.
  $("#viewmenu>summary").classList.toggle("mod", groupTools || SHAPES.some(([cat]) => modeOf(cat) !== "full"));
  $("#shape").innerHTML = SHAPES.map(([cat, label]) => {
    const m = modeOf(cat);
    return `<label>${label}<select class="tiny${m === "full" ? " full" : ""}" data-shape="${cat}">${
      MODES.map(x => `<option value="${x}"${x === m ? " selected" : ""}>${
        x[0].toUpperCase() + x.slice(1)}</option>`).join("")}</select></label>` + (cat !== "result" ? "" :
      `<label title="fold each run of tool calls and their results into one group, collapsed until you open it">Group tool calls<input type=checkbox data-group${
        groupTools ? " checked" : ""}></label>`);
  }).join("") + `<label title="show when each part of the chat was written">Display Times<input type=checkbox data-times${
    showTimes ? " checked" : ""}></label>`;
}

let logItems = [], opened = new Set(), openedFor = null;
// A long session is thousands of elements, and building them all was most of
// what switching to it cost, so the newest turns are drawn first and earlier
// ones on request. `logShown` is how many, per selected session.
const LOG_WINDOW = 60, LOG_STEP = 200;
let logShown = LOG_WINDOW;
// Pure: the index of the first item drawn.
const logStart = (n, shown) => Math.max(0, n - shown);

// Recently viewed sessions' items, so switching back paints at once and the
// poll that follows only has to confirm nothing changed.
const CONV_KEEP = 8;
const convCache = new Map();  // session key -> {items, sig}, oldest first
function rememberConv(k, items, sig){
  convCache.delete(k);
  convCache.set(k, {items, sig});
  while (convCache.size > CONV_KEEP) convCache.delete(convCache.keys().next().value);
}

function setLog(items, sig, force){
  lastSig = sig;
  logItems = items;
  // New turns can match an open search; earlier indexes stay where they were.
  if (finds.q || finds.filter){
    finds.hits = findHits(items, finds.q, finds.filter, blockShown);
    finds.at = Math.min(Math.max(finds.at, 0), finds.hits.length - 1);
  }
  drawLog(force);
  const asked = items.filter(it => it.speaker === "you" && !mateTurn(it)).pop();
  currentPrompt = asked ? asked.blocks.filter(b => b.kind === "text").map(b => b.text).join("\n\n") : "";
  drawCurrent();
}

// A tile was clicked: show what this page last had for it, if anything.
function showCached(){
  resetFind();
  logFollow = true;
  logFresh = null;
  const hit = convCache.get(selected);
  if (hit){ setLog(hit.items, hit.sig, true); return; }
  lastSig = "";
  logItems = [];
  $("#log").innerHTML = `<div class="note">loading…</div>`;
}

function showEarlier(all){
  const log = $("#log"), below = log.scrollHeight - log.scrollTop;
  logShown = all ? logItems.length : logShown + LOG_STEP;
  drawLog();
  log.scrollTop = log.scrollHeight - below;  // keep what was in view in view
}

// Pure: how the turns drawn (`have`, each {i, sig}) differ from the ones
// wanted: indexes no longer wanted, new ones, and ones whose signature moved.
function logDiff(have, want){
  const was = new Map(have.map(n => [n.i, n.sig]));
  const keep = new Set(want.map(n => n.i));
  return {gone: have.filter(n => !keep.has(n.i)).map(n => n.i),
          fresh: want.filter(n => !was.has(n.i)).map(n => n.i),
          changed: want.filter(n => was.has(n.i) && was.get(n.i) !== n.sig).map(n => n.i)};
}

// What #log holds, as drawLog left it: the "show earlier" bar, one element
// per turn and the end marker. Anything else that writes into the log (a
// note, another session) makes it stale, and the next draw starts over.
const NEWEST = `<div class="newest"><div class="float"><div class="cold" hidden></div>` +
  `<button data-newest="1" title="go to the end and follow it again">Show Most Recent</button></div></div>`;
let logDom = null;   // {for, earlier: {html, el}|null, nodes: [{i, sig, el}], newest}
const htmlEl = html => { const t = document.createElement("template"); t.innerHTML = html; return t.content.firstElementChild; };

function patchLog(log, earlier, want){
  let d = logDom;
  const intact = d && d.for === selected && d.newest.parentNode === log &&
    d.nodes.every(n => n.el.parentNode === log) && (!d.earlier || d.earlier.el.parentNode === log) &&
    log.children.length === d.nodes.length + 1 + (d.earlier ? 1 : 0);
  if (!intact){
    // From nothing: one parse for the lot, then each piece found again.
    log.innerHTML = earlier + want.map(w => w.html).join("") + NEWEST;
    const kids = [...log.children], n = want.length + 1 + (earlier ? 1 : 0);
    if (kids.length !== n){ logDom = null; return; }   // markup that does not nest: drawn, not tracked
    let at = 0;
    logDom = {for: selected, earlier: earlier ? {html: earlier, el: kids[at++]} : null,
      nodes: want.map(w => ({i: w.i, sig: w.sig, el: kids[at++]})), newest: kids[at]};
    return;
  }
  const diff = logDiff(d.nodes, want), gone = new Set(diff.gone), changed = new Set(diff.changed);
  d.nodes = d.nodes.filter(n => { if (gone.has(n.i)) n.el.remove(); return !gone.has(n.i); });
  let prev = null;
  if (earlier){
    if (!d.earlier || d.earlier.html !== earlier){
      const el = htmlEl(earlier);
      if (d.earlier) d.earlier.el.replaceWith(el); else log.insertBefore(el, log.firstChild);
      d.earlier = {html: earlier, el};
    }
    prev = d.earlier.el;
  } else if (d.earlier){ d.earlier.el.remove(); d.earlier = null; }
  const have = new Map(d.nodes.map(n => [n.i, n]));
  d.nodes = want.map(w => {
    let n = have.get(w.i);
    if (!n) n = {i: w.i, sig: w.sig, el: htmlEl(w.html)};
    else if (changed.has(w.i)){ const el = htmlEl(w.html); n.el.replaceWith(el); n = {i: w.i, sig: w.sig, el}; }
    const at = prev ? prev.nextSibling : log.firstChild;
    if (n.el !== at) log.insertBefore(n.el, at);
    prev = n.el;
    return n;
  });
  if (log.lastElementChild !== d.newest) log.append(d.newest);
}

function drawLog(force){
  if (!selected) return;
  // A busy session repaints the log on every change; not under a press.
  if (held("#log", () => drawLog(force))) return;
  const log = $("#log");
  if (openedFor !== selected){ opened = new Set(); openedFor = selected; logShown = LOG_WINDOW; logDom = null; }
  if (force) logFollow = true;
  const start = logStart(logItems.length, logShown);
  const earlier = start ? `<div class="earlier"><button data-earlier="1">show ${Math.min(start, LOG_STEP)} earlier</button>${
    start > LOG_STEP ? ` <button data-earlier="all">show all ${start}</button>` : ""}</div>` : "";
  const findSet = new Set(finds.hits);
  toolNames = new Map(); toolResults = new Map();
  for (const it of logItems) for (const b of it.blocks){
    if (b.kind === "tool" && b.id){ toolNames.set(b.id, b.name); if (b.questions) b.session = selected; }
    if (b.kind === "result" && b.id) toolResults.set(b.id, b.text);
  }
  // Each turn is its own element, kept while what it would be drawn from is
  // unchanged: a busy session adds a turn or finishes the last one, and the
  // rest of a long conversation stays as it is (scroll, selection, hover,
  // marks and open blocks included). Its signature is the html itself, so
  // anything a turn is drawn from - the shape controls, the blocks opened by
  // hand, the tool a result answers - changes it - plus the search, whose
  // marks are put into the element.
  const want = [];
  logItems.slice(start).forEach((it, j) => {
    const i = start + j;
    // Where the session compacted: the summary that replaced everything above
    // it, folded away, since the real conversation is now shown in full.
    let html;
    if (it.note === SUMMARY_NOTE)
      html = modeOf("system") === "hidden" ? "" : `<details class="compact"><summary>compacted here - show the summary it kept</summary>${
        renderBlocks(it.blocks)}</details>`;
    else {
      const body = shapeBlocks(it, i, opened);
      const mate = mateTurn(it);
      html = !body ? "" : `<div class="turn${mate ? " mate-turn" : it.speaker === "you" ? " you-turn" : ""}${it.note ? " sys" : ""}${findSet.has(i) ? " found" : ""}" data-i="${i}"><div class="who ${mate ? "mate" : esc(it.speaker || it.role)}">${
        esc(mate ? "teammate" : it.note || SPEAKER[it.speaker] || it.speaker || it.role)}${it.via ? ` <span class="via" title="arrived through the session inbox: the window, the palette, impctl or another session">via inbox</span>` : ""}</div>${body}</div>`;
    }
    if (html) want.push({i, html, sig: findSet.has(i) ? html + "\0" + finds.q : html});
  });
  // Prompts sent from here that the conversation does not have yet, after it.
  pendingSent[selected] = settlePending(pendingSent[selected] || [], logItems);
  pendingSent[selected].forEach((p, j) => {
    const html = pendingHtml(p);
    want.push({i: logItems.length + j, html, sig: html});
  });
  if (!want.length){
    log.innerHTML = earlier + `<div class="note">${logItems.length ? "everything here is hidden - see the display settings below" : "nothing yet"}</div>` + NEWEST;
    logDom = null;
  } else patchLog(log, earlier, want);
  drawCold(current());
  markFinds();
  if (logFollow) log.scrollTop = log.scrollHeight;
  markAway();
}

// -- a prompt just sent ------------------------------------------------------
// Shown at the end of the chat the moment it is sent, marked as on its way,
// until the conversation has it. Each one waits for one more turn of yours
// than the conversation had when it was sent (`base`, counting the ones
// already waiting). One that was not sent stays, marked, until dismissed or
// until you go on: another prompt sent after it was refused, or one more turn
// of yours in the conversation (typed at the terminal) - 2026-11-18: a bare
// /model's refusal stayed up through the prompts after it.
// Counted in prompts, not turns: prompts with no reply between them come back
// as one turn of yours - a stopped one leaves no reply (2026-11-17: two
// sends, each stopped with Esc, then a third, came back as two turns, and
// the third's note stayed for good) - but each keeps its own text.
const pendingSent = {};   // session key -> [{id, text, base, counted, state, note, after}]
let pendingSeq = 0;
const yourTurns = items => items.reduce((n, it) => it.speaker !== "you" || it.note ? n :
  n + (it.blocks || []).filter(b => b.kind === "text" && !teammateParts(b.text) &&
    !/^\[Request interrupted/.test(b.text)).length, 0);

// Pure: the prompts still waiting, given the conversation's items now.
function settlePending(list, items){
  const n = yourTurns(items);
  return list.filter(p => p.state !== "failed" ? n <= p.base
    : !list.some(x => x.id > p.after && x.state !== "failed") && !(p.counted && n > p.base));
}

function pendingHtml(p){
  const note = p.state === "sending" ? "sending…" : p.state === "failed" ? "not sent - " + p.note : p.note;
  return `<div class="turn you-turn pend ${p.state}" data-pend="${p.id}"><div class="who you">${esc(SPEAKER.you)}<span class="pnote">${
    esc(note)}</span>${p.state === "sending" ? "" : `<button class="ghost tiny" data-unpend="${p.id}" title="${
    p.state === "failed" ? "dismiss - the text is still in the prompt box" : "stop showing this here - it was sent"}" aria-label="dismiss">&#10005;</button>`}</div>${
    renderText(p.text)}</div>`;
}

function addPending(k, text){
  const list = pendingSent[k] || (pendingSent[k] = []);
  const p = {id: ++pendingSeq, text, state: "sending", note: "", counted: k === selected,
    base: (k === selected ? yourTurns(logItems) : 0) + list.filter(x => x.state !== "failed").length};
  list.push(p);
  if (k === selected){ logFollow = true; drawLog(); }
  return p;
}

// The dispatcher's answer: sent (waiting for Claude) or not. One that was
// not sent no longer counts towards the ones after it.
function settleSend(k, p, r, how){
  if (r.ok){ p.state = "sent"; p.note = how + " - waiting for Claude to respond"; }
  else {
    p.state = "failed"; p.note = r.error || "refused"; p.after = pendingSeq;
    for (const x of pendingSent[k] || []) if (x.id > p.id && x.state !== "failed") x.base--;
  }
  if (k === selected) drawLog();
}

function dropPending(id){
  for (const k in pendingSent) pendingSent[k] = pendingSent[k].filter(p => p.id !== id);
  drawLog();
}

// -- following the end of the conversation ---------------------------------
// The log follows new text while its end is in view. Only the reader's own
// scrolling changes that: scrolling up stops it, scrolling back to the end
// (or Show Most Recent) starts it again. Whether to follow is not re-measured
// on each redraw, because the log also shrinks under the reader - the process
// drawer opening, the status panel unfolding, the prompt box growing - and
// that pushed the end out of view and quietly stopped it following.
let logFollow = true;
const LOG_END = 24;  // px from the end that still counts as at the end
const logAtEnd = log => log.scrollHeight - log.scrollTop - log.clientHeight <= LOG_END;
function markAway(){
  const log = $("#log");
  log.classList.toggle("away", !logAtEnd(log));
}

// -- reviewed -----------------------------------------------------------------
// A session waiting on you is reviewed once you have done something in its
// chat - a click, a press, a turn of the wheel - with its current
// conversation shown to the end: the dispatcher stops flagging it (diamond,
// bar, Matrix outline, Stream Deck, notifications) until it next needs you.
// Sent once per wait.
// Only something you did counts. The log following new text to the end,
// the window coming back into focus or a redraw do not, or a session left
// open on a desk you walked away from would read as reviewed.
// And only finished work: a session working, waiting on its own tasks
// (shells, monitors, subagents, wakeups) or blocked on a permission or a
// question cannot be reviewed yet - the dispatcher refuses it too.
let logFresh = null;
const reviewedSent = {};
const TOUCH_MS = 1000;  // a scroll this soon after your input is still yours
let touchedAt = 0, touchedKey = null;
function inProgress(s){
  return ["busy", "shell", "waiting"].includes(s.status) || s.state === "blocked" ||
    !!s.waitingFor || (s.waiting_tasks || 0) > 0 ||
    pendingNow.some(p => p.session_id === key(s) || p.session_id === s.sessionId);
}
function touched(){
  const s = current();
  touchedAt = Date.now(); touchedKey = s ? key(s) : null;
  // After the event has had its effect (a click on Show Most Recent, a wheel
  // turn that reaches the end).
  setTimeout(maybeReviewed, 150);
}
function recentlyTouched(s){ return touchedKey === key(s) && Date.now() - touchedAt < TOUCH_MS; }
// Guarded: it can run while the script is still loading, before `view` and
// `tab` exist.
function maybeReviewed(){ try { reviewedCheck(); } catch (e) {} }
function reviewedCheck(){
  const s = current();
  if (!s || !s.attention || logFresh !== key(s) || !recentlyTouched(s) || inProgress(s)) return;
  if (view !== "session" || tab !== "log" || document.hidden) return;
  if (!logAtEnd($("#log"))) return;
  const k = key(s), since = s.attention_since || s.status_changed_at || 0;
  if (reviewedSent[k] === since) return;
  reviewedSent[k] = since;
  post("/api/reviewed", {session_id: k}).then(r => {
    if (!r || !r.ok || !r.reviewed) return;
    const row = sessions.find(x => key(x) === k);
    if (row && row.attention){ row.attention = false; row.reviewed = true; drawSessions(); }
  }).catch(() => { delete reviewedSent[k]; });
}
function showNewest(){
  const log = $("#log");
  logFollow = true;
  log.scrollTop = log.scrollHeight;
  markAway();
}

async function loadLog(force){
  if (!selected) { $("#log").innerHTML = `<div class="note">select a session</div>`; return; }
  let data;
  // The whole conversation, as the terminal shows it; the dispatcher answers
  // "unchanged" when what this page already has is current - including
  // what it had cached from an earlier visit.
  const want = selected;
  const known = lastSig ? "&known=" + encodeURIComponent(lastSig) : "";
  try { data = await get("/api/conversation?session=" + encodeURIComponent(want) + known); }
  catch (e) { return; }
  // A reply for a session no longer selected (a poll still in flight when a
  // tile was clicked) must not paint over the new one.
  if (want !== selected) {
    if (data.items && data.sig) rememberConv(want, data.items, data.sig);
    return;
  }
  if (data.unchanged){ logFresh = want; markAway(); return; }
  if (data.error && !data.items.length){
    $("#log").innerHTML = `<div class="note">${esc(data.error)}</div>`;
    return;
  }
  // Only repaint when something changed, so the scroll position survives.
  const sig = data.sig || (JSON.stringify(data.items).length + ":" + data.items.length);
  rememberConv(want, data.items, sig);
  logFresh = want;
  if (!force && sig === lastSig){ markAway(); return; }
  setLog(data.items, sig, force);
}

// -- the prompt being worked on --------------------------------------------
// The last thing you asked, pinned above the chat on demand, so a long turn
// can be read against what it is meant to be doing.
let currentPrompt = "", showCurrent = false, currentFolded = false;
function drawCurrent(){
  const box = $("#current");
  box.hidden = !showCurrent || tab !== "log" || view !== "session";
  $("#curbtn").textContent = showCurrent ? "Hide Current Prompt" : "Show Current Prompt";
  box.classList.toggle("fold", currentFolded);
  $("#curfold").innerHTML = currentFolded ? "&#9656;" : "&#9662;";
  const html = currentPrompt ? renderText(currentPrompt)
    : `<div class="note" style="padding:6px 0">no prompt yet in this session</div>`;
  if ($("#curbody").dataset.sig !== html){ $("#curbody").innerHTML = html; $("#curbody").dataset.sig = html; }
}

// -- the selected session, in numbers --------------------------------------
// Under the send buttons: what the tile says, plus what only the transcript
// and process tree know - background work, the prompt cache, tokens, cost.
let statusData = null, statusFor = null, statusAt = 0, statusSkew = 0, statusFolded = false;
try { statusFolded = localStorage.getItem("imperatorium.statusFolded") === "1"; } catch (e) {}

function kfmt(n){
  if (n == null) return "";
  if (n >= 1e9) return (n/1e9).toFixed(2) + "B";
  if (n >= 1e6) return (n/1e6).toFixed(n >= 1e7 ? 0 : 1) + "M";
  if (n >= 1e3) return (n/1e3).toFixed(n >= 1e4 ? 0 : 1) + "k";
  return String(Math.round(n));
}

function span(sec){
  sec = Math.max(0, Math.round(sec));
  if (sec < 60) return sec + "s";
  if (sec < 3600) return Math.round(sec/60) + "m";
  const h = Math.floor(sec/3600), m = Math.round((sec % 3600)/60);
  if (h < 24) return h + "h" + (m ? " " + m + "m" : "");
  return Math.floor(h/24) + "d " + (h % 24) + "h";
}

const plural = (n, word) => n + " " + word + (n === 1 ? "" : "s");

async function loadStatus(force){
  if (!selected){ $("#status").hidden = true; return; }
  if (force || statusFor !== selected || Date.now() - statusAt >= 3000){
    statusAt = Date.now();
    const want = selected;
    try {
      const d = await get("/api/session_status?session=" + encodeURIComponent(want));
      if (want !== selected) return;
      // The open tile says what the session has cost: redrawn when that moves.
      const cost = s => s && s.spend ? s.spend.cost_usd : null;
      const moved = cost(d) !== cost(statusFor === want ? statusData : null);
      statusData = d; statusFor = want;
      if (d.now) statusSkew = d.now - Date.now()/1000;
      if (moved) drawSessions();
    } catch (e) {}
  }
  drawStatus();
  loadTimeline();
}

// Pure: the panel's HTML for row `s` and its /api/session_status answer `d`.
function statusHtml(s, d, now, folded){
  const st = d.stat || {}, u = s.usage || {}, have = !!d.stat;
  const [dot, label] = stateOf(s);
  const cells = [], sum = [`<span class="dot ${dot}"></span>${esc(label)}`];
  const cell = (k, v, title, cls) => {
    if (v === "" || v == null) return;
    cells.push(`<div class="c" title="${esc(title || "")}"><b>${esc(k)}</b><span class="${
      cls || ""}">${esc(v)}</span></div>`);
  };
  const since = s.attention_since || s.status_changed_at;
  cell("state", label + (since ? " · " + age(since) : ""), s.attention_reason || s.doing || "");
  const ag = st.agents || [], sh = st.shells || [], mo = st.monitors || [], wk = st.wakeups || [];
  if (have){
    cell("subagents", String(ag.length), ag.map(a => a.label).join("\n"), ag.length ? "hot" : "");
    cell("shells", String(sh.length), "commands it put in the background and has not seen finish\n" +
      sh.map(x => x.label).join("\n"), sh.length ? "hot" : "");
    cell("monitors", String(mo.length), mo.map(m => m.label).join("\n"), mo.length ? "hot" : "");
    if (wk.length)
      cell("wakeups", String(wk.length), "turns it scheduled for itself (ScheduleWakeup, CronCreate)\n" +
        wk.map(x => x.label).join("\n"), "hot");
    if (d.shell_processes != null)
      cell("processes", String(d.shell_processes), "shell processes running under its claude " +
        "process - commands and monitors alike, read from the process tree",
        d.shell_processes ? "hot" : "");
  }
  if (ag.length) sum.push(plural(ag.length, "subagent"));
  if (sh.length) sum.push(plural(sh.length, "shell"));
  if (mo.length) sum.push(plural(mo.length, "monitor"));
  if (wk.length) sum.push(plural(wk.length, "wakeup"));
  const c = st.cache;
  if (c && c.expires_at){
    const left = c.expires_at - now;
    cell("cache", (left > 0 ? `warm · ${span(left)} left` : `cold · ${span(-left)} ago`) +
      (c.hit != null ? ` · ${Math.round(c.hit * 100)}% hit` : ""),
      `prompt cache: ${c.ttl >= 3600 ? "1 hour" : "5 minute"} lifetime from the last reply; ` +
      "a prompt after it expires re-sends the whole context at full price. " +
      "Hit is the share of the last request read from cache.", left > 0 ? "ok" : "warn");
    // Negative, like the tile's: it is counting down to expiry.
    sum.push(left > 0 ? `cache -${span(left)}` : "cache cold");
  }
  if (u.context_pct != null || st.context_tokens)
    cell("context", [u.context_pct != null ? Math.round(u.context_pct) + "%" : "",
      st.context_tokens ? kfmt(st.context_tokens) + (u.context_size ? " / " + kfmt(u.context_size) : "") : ""
    ].filter(Boolean).join(" · "), "", u.context_pct >= 80 ? "warn" : "");
  const sp = d.spend;
  if (sp && sp.cost_usd != null){
    cell("cost", "≈$" + sp.cost_usd.toFixed(2), "the whole session, subagents included, estimated at API list prices" +
      (u.cost_usd != null ? `; Claude Code reports $${u.cost_usd.toFixed(2)} for its current run` : ""));
    sum.push("≈$" + sp.cost_usd.toFixed(2));
  } else if (u.cost_usd != null){
    cell("cost", "$" + u.cost_usd.toFixed(2), "as Claude Code reports it to the statusline, for its current run");
    sum.push("$" + u.cost_usd.toFixed(2));
  }
  const t = (sp && sp.tokens) || st.tokens;
  if (t){
    cell("tokens", `${kfmt(t.input + t.cache_read + t.cache_write)} in · ${kfmt(t.output)} out`,
      `input ${t.input}\ncache read ${t.cache_read}\ncache write ${t.cache_write}\noutput ${t.output}` +
      (sp && sp.tokens ? "\n(the whole session, subagents included)" : "\n(this session's transcript; subagents keep their own)"));
    cell("cached", `${kfmt(t.cache_read)} read · ${kfmt(t.cache_write)} written`);
  }
  cell("model", u.model || st.model_id || "");
  cell("mode", st.permission_mode || "", "permission mode");
  if (st.prompts != null) cell("prompts", st.prompts + (st.turns ? ` · ${st.turns} turns` : ""));
  if (st.last_turn_ms) cell("last turn", span(st.last_turn_ms / 1000));
  if (st.started) cell("age", span(now - st.started));
  if (u.api_duration_ms)
    cell("API time", span(u.api_duration_ms / 1000) + (u.duration_ms ? " of " + span(u.duration_ms / 1000) : ""));
  if (u.lines_added != null || u.lines_removed != null)
    cell("lines", `+${u.lines_added || 0} −${u.lines_removed || 0}`);
  if (st.compactions) cell("compactions", String(st.compactions));
  if (s.queued) cell("queued", String(s.queued));
  // How a prompt from here reaches it: the Imperatorium mod (as your own
  // words) or the session's messaging socket (as another session's message).
  if (!s.remote) cell("link", s.mod ? "mod" + (s.mod.mod_version ? " " + s.mod.mod_version : "")
                                  : s.dispatchable ? "socket" : "",
                      s.mod ? "prompts sent from this window arrive as your own words"
                            : "prompts arrive as a message from another session");
  cell("branch", st.branch || "");
  if (st.version) cell("version", st.version, "Claude Code");
  if (d.note) cell("note", d.note);
  if (d.error) cell("error", d.error, "", "warn");
  const run = [...ag.map(x => ["agent", x]), ...sh.map(x => ["shell", x]), ...mo.map(x => ["monitor", x])]
    .map(([k, x]) => `<div class="ri"><span class="k">${k}${x.foreground ? " · now" : ""}</span>${
      esc(x.label || x.id)}${x.started ? " · " + age(x.started) : ""}${
      x.expires ? " · ends in " + span(x.expires - now) : ""}</div>`).join("");
  return `<div class="hd"><span class="sum">${sum.join(" · ")}</span><span>${folded ? "&#9656;" : "&#9662;"}</span></div>` +
    `<div class="bd"><div class="grid">${cells.join("")}</div>${run ? `<div class="run">${run}</div>` : ""}</div>`;
}

function drawStatus(){
  const box = $("#status"), s = current();
  box.hidden = !s;
  if (!s) return;
  if (held("#status", drawStatus)) return;
  const d = statusFor === selected && statusData ? statusData : {};
  box.classList.toggle("fold", statusFolded);
  const html = statusHtml(s, d, Date.now() / 1000 + statusSkew, statusFolded);
  if (box.dataset.sig !== html){
    if (!box.querySelector(":scope > .bd > .bdx")){
      box.innerHTML = '<div class="hd"></div><div class="bd"><div class="bdx"></div></div>';
      box.querySelector(".bd").prepend(tlBox);
    }
    const tmp = document.createElement("div");
    tmp.innerHTML = html;
    box.querySelector(":scope > .hd").innerHTML = tmp.querySelector(".hd").innerHTML;
    box.querySelector(".bdx").innerHTML = tmp.querySelector(".bd").innerHTML;
    box.dataset.sig = html;
  }
  drawTimeline();
}

// -- the session timeline, atop the open status panel ------------------------
// claude-lens's drawing, line for line (its context stack, turn ruler, cost per
// turn, tool calls and subagent runs over one time axis), fed by
// /api/timeline, which reads the same things out of the session's transcript.
const TLX = (() => {
const fmtC = (n) => {                       // compact: 12.9K / 4.2M
  if (n == null) return "–";
  const a = Math.abs(n);
  if (a >= 1e9) return (n / 1e9).toFixed(1) + "B";
  if (a >= 1e6) return (n / 1e6).toFixed(1) + "M";
  if (a >= 1e4) return (n / 1e3).toFixed(1) + "K";
  return n.toLocaleString("en-US");
};
const fmtFull = (n) => n == null ? "–" : n.toLocaleString("en-US");
const fmtCost = (c, est) => (est ? "~$" : "$") + (c >= 100 ? c.toFixed(0) : c >= 1 ? c.toFixed(2) : c.toFixed(3));
/* The window and each billing block say whether their cost is estimated and
   how many requests were on a model with no price. A payload that predates
   the fields says neither, which is read as estimated: absence of a claim
   to precision is not one. Unpriced requests are counted as $0 on purpose, so
   the number of them is stated wherever the cost is. The blocks table is
   column-encoded, so its flag arrives as 1/0 rather than true/false: only a
   missing flag defaults to estimated, and 0 is a real "measured". */

const fmtDur = (s) => {
  if (s == null) return "–";
  if (s < 60) return s + "s";
  if (s < 3600) return Math.round(s / 60) + "m";
  return (s / 3600).toFixed(1) + "h";
};
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;   // textContent only — data is untrusted
  return e;
};

// Imperatorium shows local time, always.
let tzLocal = true;

const pad2 = (n) => String(n).padStart(2, "0");
const toMs = (x) => typeof x === "number" ? x : Date.parse(x || "");
// [year, month0, day, hour, minute] of an instant in the chosen zone
const zoned = (ms) => {
  const d = new Date(ms);
  return tzLocal
    ? [d.getFullYear(), d.getMonth(), d.getDate(), d.getHours(), d.getMinutes()]
    : [d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate(), d.getUTCHours(), d.getUTCMinutes()];
};
// ISO string or epoch ms -> "YYYY-MM-DD", or "" when it does not parse
const dayKey = (x) => {
  const ms = toMs(x); if (isNaN(ms)) return "";
  const [y, m, d] = zoned(ms);
  return y + "-" + pad2(m + 1) + "-" + pad2(d);
};
const fmtHM = (x) => {
  const ms = toMs(x); if (isNaN(ms)) return "";
  const z = zoned(ms);
  return pad2(z[3]) + ":" + pad2(z[4]);
};
// "YYYY-MM-DD HH:MM", the spelling every table and tooltip uses

const fmtBytes = (n) => {
  n = n || 0;
  if (n >= 1e6) return (n / 1e6).toFixed(1) + " MB";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + " KB";
  return n + " B";
};

const SVGNS = "http://www.w3.org/2000/svg";
const svgEl = (t, at) => {
  const e = document.createElementNS(SVGNS, t);
  for (const k in at) e.setAttribute(k, at[k]);
  return e;
};
/* A <title> child is the only tooltip that works on a focused element as well
   as a hovered one, and it costs no JavaScript. The floating #tooltip is
   still worth its code on the daily bars, where several numbers are wanted at
   once; everywhere else this is enough. */
const svgTip = (node, text) => {
  const t = document.createElementNS(SVGNS, "title");
  t.textContent = text;
  node.append(t);
  return node;
};
/* Clean axis ceiling: 1, 2 or 5 times a power of ten. */
const niceTop = (maxV) => {
  const m = Math.max(maxV, 1e-9);
  const pow = Math.pow(10, Math.floor(Math.log10(m)));
  return [1, 2, 5, 10].map(k => k * pow).find(v => v >= m);
};
const hhmm = fmtHM;

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const CAUSES = [
  ["model_switch", "model switch", 1],
  ["effort_switch", "effort switch", 2],
  ["speed_switch", "speed switch", 3],
  ["idle_gap", "idle gap", 4],
  ["compact", "compaction", 5],
  ["unknown", "unknown", 0],
];
const CAUSE_BY = new Map(CAUSES.map(c => [c[0], c]));
const causeMeta = (c) => CAUSE_BY.get(c) || CAUSE_BY.get("unknown");
const EVENT_LABEL = { compact: "compaction", model_switch: "model switch",
                      effort_switch: "effort switch",
                      speed_switch: "speed switch" };

/* The turns of one session (claude-lens turnsFor), against the prompt rows
   the dispatcher read out of the same transcript: `ts` is in epoch seconds. */
let TL_ROWS = [];
function turnsFor(sid, points) {
  const rows = TL_ROWS;
  const out = [];
  let ri = 0;
  for (let i = 0; i < points.length; i++) {
    if (!points[i].turn) continue;
    const t = points[i].t;
    while (ri + 1 < rows.length && rows[ri + 1].ts <= t) ri++;
    const row = rows.length && rows[ri].ts <= t ? rows[ri] : null;
    if (out.length) out[out.length - 1].end = i;
    out.push({ n: out.length + 1, i: i, end: points.length, t: t, row: row });
  }
  return out;
}

/* ---------- the session's own clock ----------
   A long session is mostly not happening. One in this database ran for 137
   hours and worked for perhaps nine of them, so a linear axis drew every
   burst of work as a sliver and the nights in between as most of the chart.
   "activity" keeps time linear *inside* a working stretch and collapses any
   idle run past IDLE_GAP_S into a fixed gutter, drawn as a hatched break and
   labelled with what it swallowed - the compression is on the page rather
   than hidden in the scale. "elapsed" is the plain linear axis, one button
   away, because there are questions ("was this an evening or a fortnight?")
   that only it answers. */
const IDLE_GAP_S = 20 * 60;
const IDLE_PX = 24;
const LIVE_MIN_PX = 3;
const RUN_MIN_W = 2;

/* The series split at the same idle threshold the axis uses, so the areas and
   the axis agree about where the session stopped happening. */
function runsOf(points) {
  const runs = [];
  let cur = [];
  for (const p of points) {
    if (cur.length && p.t - cur[cur.length - 1].t > IDLE_GAP_S) {
      runs.push(cur);
      cur = [];
    }
    cur.push(p);
  }
  if (cur.length) runs.push(cur);
  return runs;
}

/* Sorted times split into working stretches and the idle runs between them. */
function segmentsOf(times) {
  const segs = [];
  let start = times[0], prev = times[0];
  for (let i = 1; i < times.length; i++) {
    if (times[i] - prev > IDLE_GAP_S) {
      segs.push({ t0: start, t1: prev, idle: false });
      segs.push({ t0: prev, t1: times[i], idle: true });
      start = times[i];
    }
    prev = times[i];
  }
  segs.push({ t0: start, t1: prev, idle: false });
  return segs;
}

/* Every time the timeline places a mark at, sorted: the requests and both
   ends of every subagent run. */
function timelineTimes(points, spans) {
  const times = [];
  for (const p of points) times.push(p.t);
  for (const sp of spans) { times.push(sp.t0); times.push(sp.t1); }
  return times.sort((a, b) => a - b);
}

/* ---------- how wide the timeline is drawn ----------
   Up to TL_VIEW_S of drawn time, the whole session fits the visible width, as
   it always has. Past that the drawing keeps the scale at which TL_VIEW_S
   fills the view and grows wider than it, and the plot scrolls sideways: a
   five-day session squeezed into one screen drew every turn as a hairline,
   and the reader could not point at any of them.

   "Drawn time" is what the axis spends its width on, so it depends on the
   mode. On the elapsed axis it is wall-clock time, first request to last. On
   the activity axis the idle runs are fixed gutters that take no share of the
   scale, so it is the working time alone, and each gutter is added on at its
   own width - a session that worked nine hours across six days fits the view
   in activity mode and scrolls in elapsed, which is the difference between
   the two axes stated in pixels.

   TL_MAX_W bounds the SVG for a session that ran for weeks on the elapsed
   axis; past it the scale gives way rather than the browser. */
const TL_VIEW_S = 12 * 3600;
const TL_MAX_W = 120000;
const TL_PAD_L = 52, TL_PAD_R = 12;
function timelineWidth(points, spans, visW, mode) {
  visW = Math.max(visW || 860, 320);
  const times = timelineTimes(points, spans);
  if (times.length < 2) return visW;
  const segs = segmentsOf(times);
  let drawn = 0, gutters = 0;
  for (const s of segs) {
    if (!s.idle) drawn += s.t1 - s.t0;
    else if (mode === "activity") gutters++;
    else drawn += s.t1 - s.t0;
  }
  if (drawn <= TL_VIEW_S) return visW;
  const visPlot = visW - TL_PAD_L - TL_PAD_R;
  const plotW = visPlot * drawn / TL_VIEW_S + gutters * IDLE_PX;
  return Math.min(Math.ceil(plotW + TL_PAD_L + TL_PAD_R), TL_MAX_W);
}

function timeScale(times, x0, plotW, mode) {
  const t0 = times[0], t1 = times[times.length - 1];
  if (times.length < 2 || t1 <= t0) {
    const mid = x0 + plotW / 2;
    return { t0: t0, t1: t1, breaks: [], starts: [t0], px: () => mid };
  }
  /* Both modes split the session at its idle runs; what differs is what a
     break is given. In "activity" it is a fixed gutter, in "elapsed" its true
     share of the width. Either way the split exists, because the drawing has
     to know where not to join one working stretch to the next. */
  const segs = segmentsOf(times);
  const live = segs.filter(s => !s.idle);
  const idleN = segs.length - live.length;
  /* A fortnight of nights would otherwise spend the whole chart on gutters,
     so in activity mode the breaks never take more than a third of it
     between them. A null idlePx is the linear axis: an idle run is scaled
     like any other stretch of time. */
  const idlePx = mode === "activity"
    ? (idleN ? Math.min(IDLE_PX, plotW * 0.34 / idleN) : 0) : null;
  const liveSecs = live.reduce((a, s) => a + (s.t1 - s.t0), 0);
  const idleSecs = segs.reduce((a, s) => a + (s.idle ? s.t1 - s.t0 : 0), 0);
  const scaled = liveSecs + (idlePx === null ? idleSecs : 0);
  let avail = plotW - (idlePx === null ? 0 : idleN * idlePx);
  const floor = Math.min(LIVE_MIN_PX, avail / live.length);
  avail -= floor * live.length;
  const width = (s) => scaled > 0 ? avail * (s.t1 - s.t0) / scaled
                                  : avail / segs.length;
  let x = x0;
  const breaks = [], starts = [];
  for (const s of segs) {
    s.x0 = x;
    if (s.idle) {
      x += idlePx === null ? width(s) : idlePx;
      breaks.push({ x0: s.x0, x1: x, secs: s.t1 - s.t0 });
    } else {
      starts.push(s.t0);
      x += floor + width(s);
    }
    s.x1 = x;
  }
  const px = (t) => {
    if (!(t > t0)) return x0;
    if (t >= t1) return x0 + plotW;
    for (const s of segs) {
      if (t > s.t1) continue;
      const d = s.t1 - s.t0;
      return d > 0 ? s.x0 + (s.x1 - s.x0) * (t - s.t0) / d : s.x0;
    }
    return x0 + plotW;
  };
  return { t0: t0, t1: t1, breaks: breaks, starts: starts, px: px };
}

/* Subagents run concurrently far more often than not, so their bars are
   packed into as few rows as will hold them without overlapping - first fit,
   in start order, with a few pixels of clearance so two touching runs still
   read as two. */
/* Twice the 95th percentile of the requests that carried any tool results at
   all, never above the real maximum and never below two - a session where
   every request carried exactly one would otherwise divide by a ceiling of
   two and draw every bar at half height, which says less than a full one. */
function toolCeiling(points, toolMax) {
  const nz = points.map(p => p.tools || 0).filter(n => n > 0).sort((a, b) => a - b);
  if (!nz.length) return 1;
  const p95 = nz[Math.min(nz.length - 1, Math.floor(0.95 * nz.length))];
  return Math.max(1, Math.min(toolMax, Math.max(2, 2 * p95)));
}

const AGENT_LANE_H = 12, AGENT_LANE_GAP = 3, AGENT_LANE_MAX = 8;
const AGENT_MIN_W = 6;
const COST_LANE_H = 24, TOOL_LANE_H = 22;
/* The clear space between one lane and the next. Small on purpose: with a
   hairline in the middle of it, seven pixels is the difference between four
   lanes and one block with stripes through it. */
const LANE_GAP = 9;
function packAgents(spans, px) {
  const ends = [], bars = [];
  for (const sp of spans) {
    // A subagent that answered in ninety seconds of a nine-hour session is
    // half a pixel wide honestly drawn. AGENT_MIN_W is the width below which
    // a bar stops being a mark at all; it overstates a short run's duration
    // and the tooltip carries the real one.
    const x0 = px(sp.t0), x1 = Math.max(px(sp.t1), x0 + AGENT_MIN_W);
    let row = 0;
    while (row < ends.length && ends[row] > x0 - 4) row++;
    if (row >= AGENT_LANE_MAX) row = AGENT_LANE_MAX - 1;
    ends[row] = Math.max(ends[row] || 0, x1);
    bars.push({ sp: sp, x0: x0, x1: x1, row: row });
  }
  return { bars: bars, rows: Math.min(ends.length, AGENT_LANE_MAX) };
}

const agentLabel = (sp) => sp.desc || sp.type || "subagent";
const fmtGap = (s) => s >= 86400 ? Math.round(s / 86400) + "d"
                    : s >= 3600 ? Math.round(s / 3600) + "h"
                    : Math.round(s / 60) + "m";
// Epoch seconds, not the ISO day string dayOf() takes.
const calDay = (t) => dayKey(t * 1000);
const dayTime = (t) => {
  const z = zoned(t * 1000);
  return MONTHS[z[1]] + " " + z[2] + ", " + pad2(z[3]) + ":" + pad2(z[4]);
};

/* The three parts of a request's context - what was read from cache, what was
   uncached input, and what was written to cache - add up to the context total
   exactly, so the stack's own outline is the context curve and no fourth
   series is needed. Order is cost order, cheapest at the bottom, the same
   order the cost-composition bars use. */
const CTX_LAYERS = [
  ["cache read", 1, p => p.cr || 0],
  ["uncached input", 2, p => Math.max((p.ctx || 0) - (p.cr || 0) - (p.cw || 0), 0)],
  ["cache write", 3, p => p.cw || 0],
];

/* One drawing, four lanes over a single time axis: the turn ruler (which
   prompt is this?), the context stack, the subagent runs, and the axis. They
   share an x, which is the whole reason they are one chart rather than four -
   a spike in context can be read off against the turn that caused it and
   against whatever was running underneath at the time.

   `mode` is "activity" or "elapsed"; see timeScale. */
function sessionTimelineSvg(points, spans, sid, W, mode, readout) {
  W = Math.max(W || 860, 320);
  const padL = TL_PAD_L, padR = TL_PAD_R, padT = 6, axisH = 20;
  const rulerH = 16, ctxH = 176;
  const turns = turnsFor(sid, points);
  const times = timelineTimes(points, spans);
  const plotW = W - padL - padR;
  const sc = timeScale(times, padL, plotW, mode);
  const pack = packAgents(spans, sc.px);
  // Exactly the packed rows: a trailing AGENT_LANE_GAP would put the lane's
  // bottom - and so the bottom of every full-height mark - below the last bar.
  const agentH = pack.rows
    ? pack.rows * (AGENT_LANE_H + AGENT_LANE_GAP) - AGENT_LANE_GAP : 0;
  /* A lane that would be empty is not drawn at all, rather than drawn blank:
     a session whose prompts are all outside the payload's row cap has no
     costs to show, and one that called no tools has no density to show. */
  const costMax = Math.max(...turns.map(t => (t.row && t.row.cost) || 0), 0);
  const costH = costMax > 0 ? COST_LANE_H : 0;
  const toolMax = Math.max(...points.map(p => p.tools || 0), 0);
  const toolH = toolMax > 0 ? TOOL_LANE_H : 0;
  /* The tool lane is scaled to twice its 95th percentile, not to its maximum,
     for the reason ironScale() gives about the calendar: one request that
     unpacked forty tool results pushes every ordinary one - and ordinary is
     one or two - onto a bar less than a pixel tall. Anything above the
     ceiling is drawn at full height and capped, so it reads as off the scale
     rather than as merely tall, and the crosshair still gives the number. */
  const toolCap = toolCeiling(points, toolMax);
  // One request's bar in the tool lane: two or three pixels on a long
  // session, so they butt together, and capped so a short one does not draw
  // a row of slabs. The pointer tracking hit-tests against the same width.
  const toolBw = Math.max(1.5, Math.min(6, plotW / Math.max(points.length, 1)));
  /* Lanes are separated by a gap and a rule rather than butted together. The
     cost bars used to stand on the context plot's own ceiling line, which
     saved a line and cost the reader the boundary: the two lanes read as one
     block with a stripe through it. Small is enough - LANE_GAP plus a hairline
     is all it takes for four lanes to read as four. */
  const rulerY = padT;
  const costY = rulerY + rulerH;
  const ctxY = costY + costH + (costH ? LANE_GAP : 0);
  const toolY = ctxY + ctxH + LANE_GAP;
  const agentY = toolY + toolH + LANE_GAP;
  /* The bands the whole drawing is ruled into, top to bottom, and *only*
     where something is actually drawn. Anything that belongs to every lane -
     a turn's wash, a compressed-idle hatch, a turn tick, the hover highlight
     - is drawn once per entry rather than as one rect down the whole chart.
     One rect was the obvious thing and it defeated the point of separating
     the lanes at all: it painted the gaps back in with the same wash, so the
     clear space between the lanes was never clear and four lanes went on
     reading as one block with stripes through it.

     The ruler and the cost bars are a single entry because they are a single
     zone - the half of the chart that is about a whole prompt - and a gap
     between them would say otherwise. */
  const lanes = [[rulerY, rulerY + rulerH + costH], [ctxY, ctxY + ctxH]];
  if (toolH) lanes.push([toolY, toolY + toolH]);
  if (agentH) lanes.push([agentY, agentY + agentH]);
  const bandTop = rulerY, bandBot = lanes[lanes.length - 1][1];
  const H = bandBot + axisH;
  // Draw `make(y, height)` once for every lane.
  const perLane = (make) => { for (const l of lanes) make(l[0], l[1] - l[0]); };
  // Where a turn column can be clicked or focused: the ruler and the cost
  // bar under it are one target, so a turn too narrow to number is still a
  // comfortable thing to hit.
  const turnHitH = rulerH + costH;
  /* The two halves of the drawing, and the y that divides them. Above it
     everything is a property of a whole prompt (which turn, what it cost);
     below it everything happened *inside* one (this request's context, these
     tool results, that subagent). The readout under the chart follows the
     pointer across this line and changes what it is reporting, which is why
     the line has to be one number both it and the layout agree on. */
  const zoneSplit = costH ? costY + costH + LANE_GAP / 2 : rulerY + rulerH;

  // Not role="img": that flattens the whole subtree into one opaque picture
  // for a screen reader's browse mode, which would bury the tabindex="0"
  // turn columns, subagent bars, event ticks and cache-miss marks inside it -
  // native Tab order still reaches them, but browse mode might never surface
  // them. role="group" keeps the children as their own focusable stops, each
  // with its own <title> for a name; the group's own aria-label says what the
  // whole drawing is.
  const svg = svgEl("svg", { viewBox: "0 0 " + W + " " + H, width: W, height: H,
                             preserveAspectRatio: "xMinYMin meet",
                             class: "ctx-svg", role: "group",
                             "aria-label": "Session timeline: context, " +
                               "turns, tool calls and subagent runs over time" });
  const cap = svgEl("title", {});
  cap.textContent = points.length + " requests, " + turns.length +
    " turn(s), " + spans.length + " subagent run(s), " +
    (mode === "activity" ? "idle time compressed" : "linear time");
  svg.append(cap);
  if (!points.length && !spans.length) return svg;

  // The hatch the compressed-idle gutters are filled with. The id carries the
  // session so two open detail panels cannot collide over it.
  const hid = "hatch-" + sid;
  const defs = svgEl("defs", {});
  const pat = svgEl("pattern", { id: hid, width: 6, height: 6,
                                 patternUnits: "userSpaceOnUse",
                                 patternTransform: "rotate(45)" });
  pat.append(svgEl("rect", { width: 6, height: 6, class: "gap-bg" }));
  pat.append(svgEl("line", { x1: 0, y1: 0, x2: 0, y2: 6, class: "gap-hatch" }));
  defs.append(pat);
  svg.append(defs);

  /* Turn bands run the full height of the drawing rather than just the
     context lane, so a subagent bar can be read against the turn that
     launched it without tracing a line up the chart. */
  for (const t of turns) {
    const x0 = sc.px(t.t);
    const x1 = t.end < points.length ? sc.px(points[t.end].t) : padL + plotW;
    const w = Math.max(x1 - x0, 1);
    // The wash marks every other turn: through the plot it is a boundary
    // marker, and two adjacent washes would not be one.
    if (!(t.n % 2))
      perLane((y, h) => svg.append(svgEl("rect", { x: x0, y: y, width: w,
                                                   height: h,
                                                   class: "turn-band" })));
    /* The wash alone disappears under the filled area, which covers most of
       the plot on any session that got anywhere. The ruler strip is the same
       banding at a strength that survives, in the lanes nothing is drawn
       over - and *every* turn gets one, in one of two tones. Filling only the
       even turns left the odd ones as bare background, which read as gaps
       between the bands rather than as bands, so nothing suggested they could
       be hovered either. */
    svg.append(svgEl("rect", { x: x0, y: rulerY, width: w, height: rulerH,
                               class: t.n % 2 ? "turn-strip-a"
                                              : "turn-strip-b" }));
  }

  const top = niceTop(Math.max(...points.map(p => p.ctx || 0), 1));
  const py = (v) => ctxY + ctxH - ctxH * Math.min(v, top) / top;
  for (let i = 0; i <= 4; i++) {
    const v = top * i / 4, y = py(v);
    svg.append(svgEl("line", { x1: padL, x2: W - padR, y1: y, y2: y,
                               class: i ? "grid-line" : "base-line" }));
    const t = svgEl("text", { x: padL - 6, y: y + 4, "text-anchor": "end",
                              class: "tick-label y-gutter" });
    t.textContent = v ? fmtC(v) : "0";
    svg.append(t);
  }

  /* Stacked areas, bottom-up, one shape per working stretch rather than one
     across the session. An area that spans an idle run draws a straight ramp
     from the last request before a night to the first after it - a climb of
     context that never happened, and on a linear axis it is most of the
     chart. Splitting leaves the gap empty, which is what is true. */
  for (const run of runsOf(points)) {
    // A stretch of one request has no width to fill; RUN_MIN_W gives it just
    // enough to be a mark rather than an invisible zero-area polygon.
    const xs = run.map(p => sc.px(p.t));
    if (run.length === 1) xs.push(xs[0] + RUN_MIN_W);
    const rp = run.length === 1 ? [run[0], run[0]] : run;
    let base = rp.map(() => 0);
    for (const layer of CTX_LAYERS) {
      const get = layer[2], slot = layer[1];
      const upper = rp.map((p, i) => base[i] + get(p));
      const fwd = rp.map((p, i) => xs[i] + "," + py(upper[i]));
      const back = [];
      for (let i = rp.length - 1; i >= 0; i--)
        back.push(xs[i] + "," + py(base[i]));
      svg.append(svgEl("polygon", { points: fwd.concat(back).join(" "),
                                    class: "fill-i" + slot, opacity: 0.85 }));
      base = upper;
    }
    svg.append(svgEl("polyline", { class: "ctx-line",
      points: rp.map((p, i) => xs[i] + "," + py(p.ctx || 0)).join(" ") }));
  }

  /* Compressed idle runs, drawn over the areas so the curve cannot appear to
     carry on through a night it did not. */
  for (const b of sc.breaks) {
    const w = b.x1 - b.x0;
    if (w < 1) continue;
    const tip = fmtGap(b.secs) + " idle" +
                (mode === "activity" ? ", not drawn to scale" : "");
    perLane((y, h) => {
      const r = svgEl("rect", { x: b.x0, y: y, width: w, height: h,
                                fill: "url(#" + hid + ")", class: "gap-band" });
      svgTip(r, tip);
      svg.append(r);
    });
    /* The duration is written down the gutter rather than under it: a 24px
       column has no room for "23h idle" beside a clock time, and the axis row
       is worth more to the working stretches either side of it than to the
       thing that did not happen. A break wide enough to read across gets it
       the right way up. */
    const cx = (b.x0 + b.x1) / 2, cy = ctxY + ctxH / 2;
    if (w >= 12 && ctxH >= 40) {
      const at = { x: cx, y: cy, class: "gap-label", "text-anchor": "middle" };
      if (w < 60) at.transform = "rotate(-90 " + cx + " " + cy + ")";
      const lab = svgEl("text", at);
      lab.textContent = fmtGap(b.secs) + " idle";
      svg.append(lab);
    }
  }

  for (const p of points) {
    if (!p.event) continue;
    const x = sc.px(p.t);
    svg.append(svgEl("line", { x1: x, x2: x, y1: ctxY, y2: ctxY + ctxH,
                               class: "ev-line" }));
    const hit = svgEl("line", { x1: x, x2: x, y1: ctxY, y2: ctxY + ctxH,
                                class: "ev-hit", tabindex: 0 });
    svgTip(hit, hhmm(p.t * 1000) + " " + (EVENT_LABEL[p.event] || p.event) +
                (p.model ? " · " + shortModel(p.model) : ""));
    svg.append(hit);
  }
  for (const p of points) {
    if (!p.miss) continue;
    const m = causeMeta(p.cause);
    const c = svgEl("circle", { cx: sc.px(p.t), cy: py(p.ctx || 0), r: 4,
                                class: "ctx-miss c-" + m[2], tabindex: 0 });
    svgTip(c, hhmm(p.t * 1000) + " cache miss · " + m[1] +
              " · " + fmtC(p.cw || 0) + " tokens re-written");
    svg.append(c);
  }

  /* Tool-call density: one bar per request, as tall as the results that
     request was carrying. It sits directly under the curve because the two
     are the same measurement seen twice - a turn that reads twenty files is
     a turn whose context climbs, and this is where you see which of the
     climbs were reading and which were writing. Whatever failed is drawn in
     the error colour over its share of the bar, so a run of red under a flat
     stretch of curve explains the flatness. */
  if (toolH) {
    const cap = svgEl("text", { x: padL - 6, y: toolY + toolH - 1,
                                "text-anchor": "end", class: "lane-label" });
    cap.textContent = "tools";
    svg.append(cap);
    // The floor of this lane is drawn once, with the other lane rules.
    // One request is two or three pixels on a long session, so the bars are
    // butted together rather than gapped, and capped so a short session does
    // not draw a row of slabs.
    const bw = toolBw;
    for (const p of points) {
      const n = p.tools || 0;
      if (!n) continue;
      const h = Math.max(1.5, (toolH - 2) * Math.min(n, toolCap) / toolCap);
      const x = sc.px(p.t) - bw / 2;
      svg.append(svgEl("rect", { x: x, y: toolY + toolH - h, width: bw,
                                 height: h, class: "tool-bar" }));
      if (n > toolCap)
        svg.append(svgEl("rect", { x: x, y: toolY + toolH - h, width: bw,
                                   height: 2, class: "tool-bar-over" }));
      if (p.terr) {
        const eh = Math.max(1.5, h * Math.min(p.terr, n) / n);
        svg.append(svgEl("rect", { x: x, y: toolY + toolH - eh, width: bw,
                                   height: eh, class: "tool-bar-err" }));
      }
    }
  }

  /* Cost per turn, in the strip under the ruler. Deliberately one flat
     colour: height already carries the magnitude, and every colour in this
     drawing is spoken for by a category (miss cause, stack layer, subagent
     run), so a fourth palette would read as membership in one of them. */
  if (costH) {
    // At the top of its own lane, not the bottom: the bottom of the cost lane
    // is the top of the context plot, where the y axis prints its ceiling, and
    // the two captions landed on each other.
    const cap = svgEl("text", { x: padL - 6, y: costY + 10,
                                "text-anchor": "end", class: "lane-label" });
    cap.textContent = "cost";
    svg.append(cap);
    for (const t of turns) {
      const c = (t.row && t.row.cost) || 0;
      if (!(c > 0)) continue;
      const x0 = sc.px(t.t);
      const x1 = t.end < points.length ? sc.px(points[t.end].t) : padL + plotW;
      // Standing on the context plot's own ceiling line, which is at exactly
      // this y and saves drawing a second baseline over it.
      const h = Math.max(1.5, (costH - 3) * c / costMax);
      svg.append(svgEl("rect", { x: x0 + 1,
                                 width: Math.max(x1 - x0 - 2, 1),
                                 y: costY + costH - h, height: h,
                                 class: "turn-cost" }));
    }
  }

  /* The turn ruler. Every boundary gets a tick; a number only where the last
     one left room, because a two-hundred-prompt session would otherwise draw
     a grey smear. The whole column - ruler plus the cost bar under it - is
     the hit area, so a turn too narrow to number is still comfortable to
     hover. */
  let lastLabel = -1e9;
  for (const t of turns) {
    const x = sc.px(t.t);
    const x1 = t.end < points.length ? sc.px(points[t.end].t) : padL + plotW;
    perLane((y, h) => svg.append(svgEl("line", { x1: x, x2: x, y1: y,
                                                 y2: y + h,
                                                 class: "turn-tick" })));
    if (x - lastLabel >= 20) {
      const lab = svgEl("text", { x: x + 3, y: rulerY + 11, class: "turn-num" });
      lab.textContent = t.n;
      svg.append(lab);
      lastLabel = x;
    }
    const hit = svgEl("rect", { x: x, y: rulerY, width: Math.max(x1 - x, 2),
                                height: turnHitH, class: "turn-hit",
                                tabindex: 0 });
    /* The turn itself rides on the node, so focus can report it without
       re-deriving anything. Its <title> is deliberately one short line: the
       readout under the chart carries the detail now, and a four-line native
       tooltip over the ruler would sit on top of exactly what moving the
       readout out of the way was meant to stop covering. */
    hit.__turn = t;
    svgTip(hit, "Turn " + t.n + " · " + dayTime(t.t));
    svg.append(hit);
  }

  /* Subagent runs: launch to last request, one bar each. Nothing anywhere
     records that an agent *ended*, so the right edge is the last request it
     made - the end of its work, if not quite the end of its life. */
  if (pack.rows) {
    const cap = svgEl("text", { x: padL - 6, y: agentY + AGENT_LANE_H - 2,
                                "text-anchor": "end", class: "lane-label" });
    cap.textContent = "agents";
    svg.append(cap);
  }
  for (const bar of pack.bars) {
    const y = agentY + bar.row * (AGENT_LANE_H + AGENT_LANE_GAP);
    const w = Math.max(bar.x1 - bar.x0, AGENT_MIN_W);
    const sp = bar.sp;
    const g = svgEl("g", { class: "agent-run", tabindex: 0 });
    g.append(svgEl("rect", { x: bar.x0, y: y, width: w, height: AGENT_LANE_H,
                             rx: 3, class: "agent-bar" }));
    if (w >= 46) {
      const lab = svgEl("text", { x: bar.x0 + 4, y: y + AGENT_LANE_H - 3,
                                  class: "agent-label" });
      // Trimmed to what the bar can hold, at the ~5.4px a 9px character
      // averages, rather than clipped - a clip path per bar would be a lot of
      // defs for a label that is only ever a hint at the tooltip.
      const room = Math.floor((w - 8) / 5.4);
      const text = agentLabel(sp);
      lab.textContent = text.length > room
        ? text.slice(0, Math.max(room - 1, 1)) + "…" : text;
      g.append(lab);
    }
    g.__span = sp;
    const lab = agentLabel(sp);
    svgTip(g, lab + (sp.type && sp.type !== lab ? " · " + sp.type : "") +
              " · " + fmtDur(sp.t1 - sp.t0));
    svg.append(g);
  }

  /* Time axis. In activity mode every working stretch is labelled where it
     starts, because after the gutters those are the only x positions that
     mean anything definite; in elapsed mode five even ticks do. Either way
     the date is spelled out whenever it changes, which on a session that ran
     over four days is most of them. */
  /* Lane rules, drawn after everything they separate: the compressed-idle
     hatch spans the full height and would otherwise cover them.

     Each one is the floor of the lane above it *and* the boundary with the
     lane below, which is why there are only two: the context plot's own zero
     line already does the job between the curve and the tool lane, and a
     second rule 9px under it would only be a second rule. */
  for (const y of [costH ? costY + costH : null,
                   toolH ? toolY + toolH : null]) {
    if (y == null) continue;
    svg.append(svgEl("line", { x1: padL, x2: W - padR, y1: y, y2: y,
                               class: "lane-rule" }));
  }

  addTracking(svg, points, spans, sc, readout,
              { W: W, py: py, top: bandTop, bot: bandBot, split: zoneSplit,
                turns: turns, padL: padL, plotW: plotW, mode: mode,
                lanes: lanes, toolY: toolY, toolH: toolH, toolBw: toolBw });

  const axisY = H - 6;
  /* Labels are placed by the box each one would occupy rather than by how
     far apart their anchors are: a centred "Aug 24, 08:12 PM" is 90px wide
     and two of them 64px apart still print on top of each other, which is
     what the old spacing test let through. CHAR_W is the average advance of
     the 11px tick font, near enough for a reservation. */
  const CHAR_W = 5.6, placed = [];
  const claim = (x0, x1) => {
    for (const p of placed) if (x0 < p[1] + 7 && x1 > p[0] - 7) return false;
    placed.push([x0, x1]);
    return true;
  };
  const putLabel = (x, text, anchor) => {
    const w = text.length * CHAR_W;
    const x0 = anchor === "start" ? x : anchor === "end" ? x - w : x - w / 2;
    if (!claim(x0, x0 + w)) return false;
    const lab = svgEl("text", { x: x, y: axisY, class: "tick-label",
                                "text-anchor": anchor });
    lab.textContent = text;
    svg.append(lab);
    return true;
  };
  const cand = [];
  if (mode === "activity") {
    for (const t of sc.starts) cand.push(t);
  } else {
    // Four ticks across one view's width, and as many more as the drawing
    // is wider than that, so a timeline that scrolls is labelled all the
    // way along rather than at four points spread over its whole length.
    const nT = Math.max(4, Math.round(plotW / 200));
    for (let i = 0; i < nT; i++) cand.push(sc.t0 + (sc.t1 - sc.t0) * i / nT);
  }
  // Both ends first, so a session always says when it began and ended however
  // crowded the middle gets; then the working stretches, left to right.
  const shown = [];
  const put = (t, anchor) => {
    const day = calDay(t);
    const long = dayTime(t);
    // A time on the same day as a label already shown needs no date on it -
    // but only once that neighbour is on the page, so the date is decided
    // after placement rather than in candidate order.
    const near = shown.some(s => s.day === day);
    if (putLabel(sc.px(t), near ? hhmm(t * 1000) : long, anchor))
      shown.push({ day: day, t: t });
  };
  put(sc.t0, "start");
  if (sc.t1 > sc.t0) put(sc.t1, "end");
  for (const t of cand) if (t > sc.t0) put(t, "middle");
  return svg;
}

/* ---------- the readout ----------
   What the pointer is over, written into a strip under the chart rather than
   into a panel that follows the pointer. The panel was the obvious thing and
   the wrong thing: on a drawing this dense it covered the marks it was
   describing, the crosshair included, so reading a value meant losing sight
   of where the value was. A fixed strip cannot occlude anything, and it has
   room for a line of prompt text that a tooltip did not.

   Its height is reserved rather than fitted, so moving between the two halves
   of the chart does not shuffle everything below it up and down. */
function roFields(pairs) {
  const row = el("div", "ro-fields");
  for (const [k, v] of pairs) {
    if (v == null) continue;
    const s = el("span", "ro-f");
    s.append(el("span", "k", k), el("span", "v", v));
    row.append(s);
  }
  return row;
}

function setReadout(box, head, pairs, line, cls) {
  if (!box) return;
  const h = el("div", "ro-head");
  h.textContent = head;
  const l = el("div", "ro-line");
  l.textContent = line || "";
  box.className = "tl-readout" + (cls ? " " + cls : "");
  box.replaceChildren(h, roFields(pairs), l);
}

/* What the strip says with the pointer somewhere else: the session in one
   line, and a note about the two halves - which is the only place the split
   is explained, and it is worth explaining once. */
function restReadout(box, points, spans, turns, mode) {
  if (!box) return;
  setReadout(box, "Session", [
    ["requests", fmtFull(points.length)],
    ["turns", turns.length ? fmtFull(turns.length) : null],
    ["subagents", spans.length ? fmtFull(spans.length) : null],
    ["axis", mode === "activity" ? "activity (idle compressed)" : "elapsed"],
  ], "Point at the top band for a whole prompt, at the context lane for one " +
     "request, at a tool bar for its calls, or at a subagent bar for its run.");
}

/* Which subagents were running at an instant, and which were launched inside
   a turn: the same list read two ways, one per half of the chart. */
const spansAt = (spans, t) => spans.filter(s => s.t0 <= t && t <= s.t1);
const spansIn = (spans, t0, t1) => spans.filter(s => s.t0 >= t0 && s.t0 < t1);

/* ---------- pointer tracking ----------
   One listener on the SVG root. A hit rectangle would be one element instead
   of thousands, but it would also sit on top of every mark underneath it and
   take their hovers away; a listener per point would be several thousand of
   them on a panel the reader may have opened only to glance at.

   Above geo.split the pointer is reading a *prompt* and the whole turn column
   lights up; below it the pointer is reading a *request* and a crosshair
   snaps to the nearest one. The readout changes with it. Keyboard reaches the
   same two states through focusin, because every turn column and subagent run
   is focusable and carries the object it stands for. */
function addTracking(svg, points, spans, sc, readout, geo) {
  if (!points.length && !spans.length) return;
  const xs = points.map(p => sc.px(p.t));
  const xh = svgEl("g", { class: "xh", visibility: "hidden" });
  const line = svgEl("line", { y1: geo.top, y2: geo.bot, class: "xh-line" });
  const dot = svgEl("circle", { r: 3.5, class: "xh-dot" });
  xh.append(line, dot);
  /* One highlight rect per lane, for the reason the turn bands are drawn
     that way: a single rect over the whole column would fill the gaps that
     separate the lanes, and the highlight would be the one thing on the chart
     that put them back together. */
  const hot = svgEl("g", { class: "turn-hot", visibility: "hidden" });
  for (const l of geo.lanes)
    hot.append(svgEl("rect", { y: l[0], height: l[1] - l[0] }));
  svg.append(hot, xh);

  /* Nearest by drawn x, not by index: the activity axis is piecewise, so
     equal pixel steps are not equal steps in time and only a search over the
     positions actually drawn agrees with what the reader is pointing at. */
  const nearest = (x) => {
    let lo = 0, hi = xs.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (xs[mid] < x) lo = mid + 1; else hi = mid;
    }
    if (lo > 0 && Math.abs(xs[lo - 1] - x) <= Math.abs(xs[lo] - x)) lo--;
    return lo;
  };
  const turnAt = (i) => {
    for (let k = geo.turns.length - 1; k >= 0; k--)
      if (geo.turns[k].i <= i) return geo.turns[k];
    return null;
  };
  const turnEndT = (t) =>
    t.end < points.length ? points[t.end].t : Number.POSITIVE_INFINITY;

  const clear = () => {
    xh.setAttribute("visibility", "hidden");
    hot.setAttribute("visibility", "hidden");
    restReadout(readout, points, spans, geo.turns, geo.mode);
  };

  // ---- the overall half: one whole prompt
  const showTurn = (t) => {
    xh.setAttribute("visibility", "hidden");
    const x0 = sc.px(t.t);
    const x1 = t.end < points.length ? sc.px(points[t.end].t)
                                     : geo.padL + geo.plotW;
    for (const r of hot.children) {
      r.setAttribute("x", x0);
      r.setAttribute("width", Math.max(x1 - x0, 2));
    }
    hot.setAttribute("visibility", "visible");
    let tools = 0, terr = 0, miss = 0;
    for (let i = t.i; i < t.end; i++) {
      tools += points[i].tools || 0;
      terr += points[i].terr || 0;
      miss += points[i].miss || 0;
    }
    const r = t.row;
    const launched = spansIn(spans, t.t, turnEndT(t));
    setReadout(readout, "Turn " + t.n + " · " + dayTime(t.t), [
      ["cost", r ? fmtCost(r.cost, r.est) : null],
      ["output", r ? fmtFull(r.out) : null],
      ["wall", r ? fmtDur(r.wall_s || 0) : null],
      ["API calls", fmtFull(t.end - t.i)],
      ["tools", tools ? fmtFull(tools) + (terr ? " (" + terr + " failed)" : "")
                      : null],
      ["cache misses", miss ? fmtFull(miss) : null],
      ["subagents", launched.length ? fmtFull(launched.length) : null],
      ["context at end", points[t.end - 1]
        ? fmtC(points[t.end - 1].ctx || 0) : null],
    ], r && r.text ? r.text.replace(/\s+/g, " ")
                   : "(this prompt's row is not in the payload)", "ro-overall");
  };

  // ---- the intra half: one request
  const showPoint = (i) => {
    hot.setAttribute("visibility", "hidden");
    const p = points[i];
    xh.setAttribute("visibility", "visible");
    line.setAttribute("x1", xs[i]);
    line.setAttribute("x2", xs[i]);
    dot.setAttribute("cx", xs[i]);
    dot.setAttribute("cy", geo.py(p.ctx || 0));
    const t = turnAt(i);
    const running = spansAt(spans, p.t);
    const notes = [];
    if (p.miss) notes.push("cache miss: " + causeMeta(p.cause)[1]);
    if (p.event) notes.push(EVENT_LABEL[p.event] || p.event);
    if (running.length)
      notes.push("running: " + running.map(agentLabel).join(", "));
    setReadout(readout, dayTime(p.t) + (t ? " · in turn " + t.n : ""), [
      ["context", fmtFull(p.ctx || 0)],
      ["cache read", fmtFull(p.cr || 0)],
      ["cache write", fmtFull(p.cw || 0)],
      ["uncached in",
       fmtFull(Math.max((p.ctx || 0) - (p.cr || 0) - (p.cw || 0), 0))],
      ["tool results", p.tools
        ? fmtFull(p.tools) + (p.terr ? " (" + p.terr + " failed)" : "") : null],
      ["model", p.model ? shortModel(p.model) : null],
    ], notes.join(" · "), "ro-intra");
  };

  /* ---- the tool calls under the pointer, in the tool lane
     The requests whose bars the pointer is actually over. On a long session
     bars are a pixel or two wide and several share the pointer's column, so
     all of them are reported together - what the reader sees as one bar is
     reported as one bar. The first and last pick the crosshair and say the
     window: a request's calls are the ones made since the request before it,
     so the window opens at that earlier request, not at the first bar. */
  const toolsNear = (x) => {
    const reach = Math.max(geo.toolBw / 2, 3);
    const out = [];
    let i = nearest(x);
    while (i > 0 && xs[i - 1] >= x - reach) i--;
    for (; i < xs.length && xs[i] <= x + reach; i++)
      if (points[i].tools && Math.abs(xs[i] - x) <= reach) out.push(i);
    return out;
  };
  const fmtMs = (ms) => ms == null ? null
    : ms < 1000 ? Math.round(ms) + "ms"
    : ms < 60000 ? (ms / 1000).toFixed(1) + "s"
    : fmtDur(Math.round(ms / 1000));
  const TOOL_FIELDS_MAX = 8;
  const showTools = (idx) => {
    hot.setAttribute("visibility", "hidden");
    // The crosshair on the busiest of them, which is the bar most likely to
    // be the one the reader is looking at.
    let at = idx[0];
    for (const i of idx) if (points[i].tools > points[at].tools) at = i;
    xh.setAttribute("visibility", "visible");
    line.setAttribute("x1", xs[at]);
    line.setAttribute("x2", xs[at]);
    dot.setAttribute("cx", xs[at]);
    dot.setAttribute("cy", geo.py(points[at].ctx || 0));
    const byName = new Map();
    let n = 0, err = 0, ms = null, bytes = null;
    for (const i of idx) {
      for (const a of points[i].tl) {
        let g = byName.get(a.name);
        if (!g) byName.set(a.name, g = { n: 0, err: 0 });
        g.n += a.n;
        g.err += a.err;
        if (a.ms != null) ms = (ms || 0) + a.ms;
        if (a.bytes != null) bytes = (bytes || 0) + a.bytes;
      }
      n += points[i].tools;
      err += points[i].terr || 0;
    }
    const first = idx[0], last = idx[idx.length - 1];
    const from = first > 0 && points[first - 1].session === points[first].session
      ? points[first - 1].t : null;
    const t = turnAt(last);
    const names = [...byName.entries()]
      .sort((a, b) => b[1].n - a[1].n || (a[0] < b[0] ? -1 : 1));
    const pairs = [
      ["calls", fmtFull(n) + (err ? " (" + fmtFull(err) + " failed)" : "")],
      ["run time", fmtMs(ms)],
      ["results", bytes != null ? fmtBytes(bytes) : null],
    ];
    for (const [name, g] of names.slice(0, TOOL_FIELDS_MAX))
      pairs.push([name, fmtFull(g.n) + (g.err ? " (" + g.err + " failed)" : "")]);
    if (names.length > TOOL_FIELDS_MAX)
      pairs.push(["+" + (names.length - TOOL_FIELDS_MAX) + " more",
                  fmtFull(names.slice(TOOL_FIELDS_MAX)
                    .reduce((s, e) => s + e[1].n, 0))]);
    const line2 =
      (from != null ? "Called between " + hhmm(from * 1000) + " and "
                    : "Called before ") + hhmm(points[last].t * 1000) +
      (idx.length > 1 ? ", " + idx.length + " requests' worth" : "") +
      "; the results went back with the request under the crosshair.";
    setReadout(readout, "Tool calls · " + dayTime(points[last].t) +
                        (t ? " · in turn " + t.n : ""),
               pairs, line2, "ro-intra");
  };

  // ---- one subagent, when the pointer is actually on its bar
  const showSpan = (sp) => {
    xh.setAttribute("visibility", "hidden");
    hot.setAttribute("visibility", "hidden");
    // An agent whose `agents` row never arrived has neither a description
    // nor a type, so agentLabel() falls back to the word "subagent" - and
    // printing it again as the type would be the same word twice.
    const label = agentLabel(sp);
    setReadout(readout, label, [
      ["type", sp.type && sp.type !== label ? sp.type : null],
      ["model", sp.model ? shortModel(sp.model) : null],
      ["ran", fmtDur(sp.t1 - sp.t0)],
      ["API calls", sp.calls ? fmtFull(sp.calls) : "none recorded"],
      ["output", sp.calls ? fmtFull(sp.out) : null],
      ["cost", sp.calls ? fmtCost(sp.cost, sp.est) : null],
    ], dayTime(sp.t0) + " → " + hhmm(sp.t1 * 1000), "ro-intra");
  };

  svg.addEventListener("mousemove", (e) => {
    /* getScreenCTM rather than arithmetic on the bounding box. .ctx-svg is
       width:100% over a viewBox that is usually narrower - the drawing is
       sized to the visible width and the cell it sits in is as wide as the
       sessions table - so preserveAspectRatio letterboxes it, and a plain
       (client - left) * viewBox/box ratio lands the crosshair a few hundred
       pixels from the pointer. The CTM knows the whole transform. */
    const m = svg.getScreenCTM();
    if (!m) return;
    const pt = new DOMPoint(e.clientX, e.clientY).matrixTransform(m.inverse());
    if (pt.y < geo.top || pt.y > geo.bot) { clear(); return; }
    const run = e.target.closest && e.target.closest(".agent-run");
    if (run && run.__span) { showSpan(run.__span); return; }
    if (pt.y < geo.split) {
      const t = turnAt(nearest(pt.x));
      if (t) showTurn(t); else clear();
      return;
    }
    if (!points.length) return;
    // The tool lane, and half the gap either side of it: a bar the pointer
    // is over reports its calls; a stretch of the lane with no bar under the
    // pointer falls through to the request, as it did before.
    if (geo.toolH && pt.y >= geo.toolY - LANE_GAP / 2 &&
        pt.y <= geo.toolY + geo.toolH + LANE_GAP / 2) {
      const idx = toolsNear(pt.x);
      if (idx.length) { showTools(idx); return; }
    }
    showPoint(nearest(pt.x));
  });
  svg.addEventListener("mouseleave", clear);
  // Keyboard reaches the same two states: a focused turn column reports its
  // prompt, a focused subagent run reports itself.
  svg.addEventListener("focusin", (e) => {
    if (e.target.__turn) showTurn(e.target.__turn);
    else if (e.target.__span) showSpan(e.target.__span);
  });
  // focusin has no matching cleanup of its own: Tab past the last turn-hit or
  // agent-run leaves the hot rect and readout pinned to whatever was focused
  // last, the same staleness mouseleave exists to prevent. relatedTarget is
  // where focus is going *to*, so this only fires the same clear() mouseleave
  // uses once focus has actually left the svg subtree - not on every move
  // between two focusable children inside it. It can be null (focus left the
  // document entirely, e.g. Tab to the browser chrome), which is also outside
  // the svg.
  svg.addEventListener("focusout", (e) => {
    if (!e.relatedTarget || !svg.contains(e.relatedTarget)) clear();
  });
  clear();
}
/* The chart's own legend: the three stacked layers, then only the marks this
   session actually put on the page - a cause swatch per miss that happened,
   the event line, the subagent bar, the compressed-gap hatch. */
function ctxLegend(points, spans, mode) {
  const box = el("div", "mini-legend");
  for (const layer of CTX_LAYERS) {
    const s = el("span");
    s.append(el("span", "sw sw-i" + layer[1]), document.createTextNode(layer[0]));
    box.append(s);
  }
  const seen = [];
  for (const p of points)
    if (p.miss) {
      const m = causeMeta(p.cause);
      if (!seen.includes(m)) seen.push(m);
    }
  for (const m of seen) {
    const s = el("span");
    s.append(el("span", "sw sw-c" + m[2]), document.createTextNode("miss: " + m[1]));
    box.append(s);
  }
  if (points.some(p => p.event))
    box.append(el("span", null, "dashed line: model switch / compaction"));
  if (points.some(p => p.turn))
    box.append(el("span", null, "numbered bands: prompts, in order"));
  if (points.some(p => p.tools)) {
    const s = el("span");
    s.append(el("span", "sw sw-tool"), document.createTextNode("tool results"));
    box.append(s);
  }
  if (points.some(p => p.terr)) {
    const s = el("span");
    s.append(el("span", "sw sw-toolerr"),
             document.createTextNode("failed tool calls"));
    box.append(s);
  }
  if (spans.length) {
    const s = el("span");
    s.append(el("span", "sw sw-agent"), document.createTextNode("subagent run"));
    box.append(s);
  }
  if (mode === "activity" && points.length)
    box.append(el("span", null, "hatched: idle time, compressed"));
  return box;
}

const SHORT_MODEL = new Map();
function shortModel(m) {
  let s = SHORT_MODEL.get(m);
  if (s === undefined) {
    s = m.replace(/^claude-/, "").replace(/-\d{8}$/, "");
    SHORT_MODEL.set(m, s);
  }
  return s;
}

  /* claude-lens's drawTimeline, for one panel: sized to the panel, scrolled
     sideways past twelve hours of drawn time, and - the first time a session
     is drawn - scrolled to its latest stretch rather than its first. */
  function render(plot, d, sid, mode, toEnd){
    TL_ROWS = d.rows || [];
    const points = d.points || [], spans = d.spans || [];
    for (const p of points){ p.session = sid; if (!p.tl) p.tl = []; }
    const hid = String(sid).replace(/[^A-Za-z0-9_-]/g, "_");
    plot.style.maxWidth = "";
    const w = plot.clientWidth;
    if (w > 0) plot.style.maxWidth = w + "px";
    const old = plot.querySelector(".tl-scroll");
    const frac = old && old.scrollWidth > old.clientWidth
      ? old.scrollLeft / (old.scrollWidth - old.clientWidth) : 0;
    const fullW = timelineWidth(points, spans, w, mode);
    const readout = el("div", "tl-readout");
    const svg = sessionTimelineSvg(points, spans, hid, fullW, mode, readout);
    if (fullW <= Math.max(w || 860, 320)){
      plot.replaceChildren(svg, readout, ctxLegend(points, spans, mode));
      return;
    }
    svg.style.width = fullW + "px";
    const sc = el("div", "tl-scroll");
    sc.append(svg);
    const gut = svgEl("svg", { width: TL_PAD_L, height: svg.getAttribute("height"),
                               "aria-hidden": "true" });
    for (const n of svg.querySelectorAll(".y-gutter, .lane-label"))
      gut.append(n.cloneNode(true));
    const pin = el("div", "tl-gutter");
    pin.append(gut);
    const frame = el("div", "tl-frame");
    frame.append(sc, pin);
    plot.replaceChildren(frame, readout, ctxLegend(points, spans, mode));
    if (toEnd) sc.scrollLeft = sc.scrollWidth;
    else if (frac > 0) sc.scrollLeft = frac * (sc.scrollWidth - sc.clientWidth);
  }
  return {render};
})();

let tlData = null, tlFor = null, tlAt = 0, tlDrawn = "", tlAxis = "activity";
try { if (localStorage.getItem("imperatorium.tlAxis") === "elapsed") tlAxis = "elapsed"; } catch (e) {}
const tlBox = document.createElement("div");
tlBox.className = "tlx";
tlBox.innerHTML = `<div class="sec-head"><h3>Session timeline</h3><div class="seg mini-seg" role=group aria-label="Time axis">` +
  `<button type=button class=btn data-axis=activity title="Idle stretches collapsed to a hatched gutter, so the work fills the chart">activity</button>` +
  `<button type=button class=btn data-axis=elapsed title="Real elapsed time, linear - idle stretches at their true width">elapsed</button>` +
  `</div></div><div class="tl-plot"></div><div class="tl-note"></div>`;
tlBox.addEventListener("click", e => {
  const b = e.target.closest("[data-axis]");
  if (!b || b.dataset.axis === tlAxis) return;
  tlAxis = b.dataset.axis;
  try { localStorage.setItem("imperatorium.tlAxis", tlAxis); } catch (e) {}
  tlDrawn = "";
  drawTimeline();
});

// Every 5 s while the panel is open; the dispatcher answers "same" when the
// transcript has not moved, so an idle session costs nothing to watch.
async function loadTimeline(){
  const s = current();
  if (!s || statusFolded || view !== "session" || tab !== "log") return;
  if (tlFor === selected && Date.now() - tlAt < 5000){ drawTimeline(); return; }
  tlAt = Date.now();
  const want = selected, mine = tlFor === want && tlData && tlData.sig;
  let d;
  try {
    d = await get("/api/timeline?session=" + encodeURIComponent(want) + (mine ? "&sig=" + encodeURIComponent(tlData.sig) : ""));
  } catch (e) {
    d = {ok: false, error: "this dispatcher does not draw timelines"};
  }
  if (want !== selected) return;
  if (!(d.same && tlFor === want)){ tlData = d; tlFor = want; }
  drawTimeline();
}

function drawTimeline(){
  const plot = tlBox.querySelector(".tl-plot"), note = tlBox.querySelector(".tl-note");
  for (const b of tlBox.querySelectorAll("[data-axis]"))
    b.setAttribute("aria-pressed", String(b.dataset.axis === tlAxis));
  if (!tlBox.isConnected || statusFolded) return;
  const d = tlFor === selected ? tlData : null;
  const ok = !!(d && d.ok && ((d.points || []).length || (d.spans || []).length));
  tlBox.querySelector(".seg").hidden = !ok;
  note.textContent = !d ? "reading the transcript\u2026" : !d.ok ? (d.error || "")
    : ok ? "" : "No requests in this session yet.";
  if (!ok){ plot.replaceChildren(); plot.style.maxWidth = ""; tlDrawn = ""; return; }
  plot.style.maxWidth = "";
  const w = plot.clientWidth;
  if (!w) return;
  const key = selected + "|" + d.sig + "|" + tlAxis + "|" + w;
  if (key === tlDrawn){ if (w) plot.style.maxWidth = w + "px"; return; }
  const fresh = !tlDrawn.startsWith(selected + "|");
  // Not under the pointer: a redraw would take the readout out from under it.
  if (!fresh && plot.matches(":hover") && tlDrawn.endsWith("|" + w)){ plot.style.maxWidth = w + "px"; return; }
  tlDrawn = key;
  TLX.render(plot, d, selected, tlAxis, fresh);
}
window.addEventListener("resize", () => drawTimeline());

// -- what the session is waiting on ------------------------------------------
// The shells, monitors and subagents the selected session has going, for its
// tile's task list (an item there opens in place of the chat). The strip of
// their output that sat under the chat went 2026-11-15: the tile shows it.
let procTasks = [], procFor = null;

function procWanted(){
  const s = current(), w = (s && s.work) || {};
  return view === "session" && tab === "log" && s && (s.remote ? !!s.pid || !!s.status : w.alive);
}

let remoteAskedAt = 0;
async function loadProcs(){
  const want = selected;
  if (!procWanted()){ procTasks = []; procFor = want; drawProcs(); return; }
  const s = current();
  if (s && s.remote && procFor === want && Date.now() - remoteAskedAt < 3000) return;
  if (s && s.remote) remoteAskedAt = Date.now();
  loadTaskExtra();
  let d;
  try { d = await get("/api/awaited?session=" + encodeURIComponent(want)); } catch (e) { return; }
  if (want !== selected || !d.ok) return;
  procTasks = d.tasks || []; procFor = want;
  drawProcs();
}

function drawProcs(){
  drawTileTasks();
  drawTaskView();
}

// -- the session's tasks, in its tile ------------------------------------------
// The open tile ends in a folded list of what the session has going: the
// tool call in flight, background shells, monitors, subagents (each with the
// call it is in the middle of beneath it), scheduled wakeups, the processes
// its shells started, its MCP servers and any hook failures. What ended in
// the last half hour waits under its own folded "Recently completed" row.
// An item opens in place of the chat (the tool call jumps to it in the chat
// instead); a click anywhere else on the tile, Chat, or Escape returns.
// Teammates waiting for a message fold under their own "Idle Agents" row,
// between what runs and what ended; one that takes up work again is back
// among the running rows on the next poll.
let tasksOpen = false, doneOpen = false, idleOpen = false, taskView = null, taskLast = null, tvShown = "";
try {
  tasksOpen = localStorage.getItem("imperatorium.tasksOpen") === "1";
  doneOpen = localStorage.getItem("imperatorium.doneOpen") === "1";
  idleOpen = localStorage.getItem("imperatorium.idleOpen") === "1";
} catch (e) {}
const TASK_MARK = {tool: "tool", shell: "$", monitor: "mon", agent: "agent", wakeup: "wake",
                   process: "proc", mcp: "mcp", hooks: "hook"};
const TASK_KINDS_SAID = [["tool", "tool call"], ["shell", "shell"], ["monitor", "monitor"],
                         ["agent", "subagent"], ["wakeup", "wakeup"], ["process", "process"]];
const LIVE_KINDS = new Set(["shell", "monitor", "agent", "wakeup"]);

// Pure: {running, idle, done, extra, mcp} for row `s`, given what
// /api/awaited said. `idle` is teammates waiting for a message; `extra` is
// what describes the session rather than runs in it; `mcp` (its MCP servers)
// is drawn apart from the Task List, as a line of the tile's own summary.
function taskList(s, tasks){
  const out = {running: [], idle: [], done: [], extra: [], mcp: null};
  if (!s) return out;
  if (s.doing && (RUNNING[s.status] || s.state === "working"))
    out.running.push({id: "tool", kind: "tool", label: s.doing, started: s.doing_since});
  for (const t of tasks || []){
    if (t.done) out.done.push(t);
    else if (t.idle) out.idle.push(t);
    else if (t.kind === "mcp") out.mcp = t;
    else if (LIVE_KINDS.has(t.kind)) out.running.push(t);
    else out.extra.push(t);
  }
  const n = (s.work || {}).processes || 0;
  if (n) out.running.push({id: "processes", kind: "process", count: n,
                           label: n + (n === 1 ? " process" : " processes") + " started by its shells"});
  return out;
}
// Pure: "2 shells · 1 subagent · 3 processes", or "none running". Every row
// in `list` is counted (a processes row by how many it stands for), so the
// heading never says none above a running row. Idle teammates are counted
// on their own row, not here.
function taskSummary(list){
  const n = {};
  for (const t of list){
    const k = TASK_KINDS_SAID.some(([w]) => w === t.kind) ? t.kind : "task";
    n[k] = (n[k] || 0) + (t.count || 1);
  }
  const said = TASK_KINDS_SAID.concat([["task", "task"]]).filter(([k]) => n[k])
    .map(([k, w]) => n[k] + " " + w + (n[k] === 1 ? "" : w.endsWith("s") ? "es" : "s"));
  return said.join(" · ") || "none running";
}
const taskWhen = (t, now) => t.done ? (t.ended ? age(t.ended) + " ago" : "")
  : t.kind === "wakeup" ? (t.fires_at ? (t.fires_at > now ? "in " + age(2 * now - t.fires_at) : "due") : "")
  : age(t.started);
const STATUS_WORD = {completed: "done", failed: "failed", killed: "killed", stopped: "stopped",
                     cancelled: "cancelled", fired: "fired", "timed out": "timed out"};
function foldTasks(open){
  tasksOpen = open;
  try { localStorage.setItem("imperatorium.tasksOpen", open ? "1" : "0"); } catch (e) {}
  drawTileTasks();
}
function foldDone(open){
  doneOpen = open;
  try { localStorage.setItem("imperatorium.doneOpen", open ? "1" : "0"); } catch (e) {}
  drawTileTasks();
}
function foldIdle(open){
  idleOpen = open;
  try { localStorage.setItem("imperatorium.idleOpen", open ? "1" : "0"); } catch (e) {}
  drawTileTasks();
}
function taskRow(t, now, sub, running){
  const on = (taskView && taskView.id === t.id ? " on" : "") +
    (running && t.kind !== "wakeup" ? " run" : "") + (t.idle ? " idle" : "");
  const live = !t.done && t.mtime && now - t.mtime < 5;
  const why = t.kind === "tool" ? "show it in the chat: " : "show it: ";
  const status = t.idle ? `<span class=st title="waiting for a message">idle</span>`
    : t.done && t.status && t.status !== "completed"
    ? `<span class="st ${t.status === "failed" ? "bad" : ""}">${esc(STATUS_WORD[t.status] || t.status)}</span>` : "";
  // A subagent's own calls are not listed beneath it: opening it shows them.
  return `<div class="tki${on}${t.done ? " done" : ""}${sub ? " sub" : ""}" data-task="${esc(t.id)}" title="${
    esc(why + (t.label || t.id))}"><span class=k>${TASK_MARK[t.kind] || "?"}</span><span class=l>${
    esc(t.label || t.id)}</span>${status}${live ? "<span class=live></span>" : ""}<span class=a>${
    esc(taskWhen(t, now))}</span></div>`;
}
// Pure: the rows under the tile's Tasks header for taskList's `l`. Nothing
// running says so in the header ("Tasks · none running"), not again in a row
// beneath it, so with nothing else to list this is "".
function taskItems(l, now){
  let items = l.running.map(t => taskRow(t, now, false, true)).join("");
  if (l.idle.length)
    items += `<div class="tki grp" data-idle-toggle title="teammates waiting for a message">` +
      `<span class=ar>${idleOpen ? "&#9662;" : "&#9656;"}</span><span class=l>Idle Agents (${l.idle.length})</span></div>` +
      (idleOpen ? l.idle.map(t => taskRow(t, now, true)).join("") : "");
  if (l.done.length)
    items += `<div class="tki grp" data-done-toggle title="tasks that ended in the last half hour">` +
      `<span class=ar>${doneOpen ? "&#9662;" : "&#9656;"}</span><span class=l>Recently completed (${l.done.length})</span></div>` +
      (doneOpen ? l.done.map(t => taskRow(t, now, true)).join("") : "");
  return items + l.extra.map(t => taskRow(t, now)).join("");
}
// -- hook errors --------------------------------------------------------------
// The tile's "hook errors" chip opens a pop-up saying what went wrong and what
// to do (2026-11-18: the chip said nothing more, and nothing opened from it).
// The errors are what Claude Code recorded for the session's Stop hooks, its
// last ten. The chip is for the ones since the hooks last ran cleanly: once a
// later turn's Stop hooks run without an error, they are earlier ones.

// Pure: the hooks row's errors since the Stop hooks last ran cleanly.
const hookErrorsNow = h => !h ? [] : (h.errors || []).filter(e => !h.ok_at || (e.at || 0) > h.ok_at);

// What a hook's error text says went wrong, the first that matches.
const HOOK_KINDS = [
  {re: /not recognized as|command not found|not found|no such file|cannot find|ENOENT/i,
   what: "The hook's program could not be found",
   why: "Claude Code could not start the command - it is not installed, the path is wrong, or it is not on the PATH Claude Code runs it with.",
   fix: "Check the path in the hook's command, or install what it runs."},
  {re: /timed? ?out|timeout|deadline/i,
   what: "The hook took too long",
   why: "Claude Code stopped waiting for it.",
   fix: "Make the hook faster, or give it a longer \"timeout\" (seconds) in its settings entry."},
  {re: /permission denied|access is denied|EACCES|EPERM/i,
   what: "The hook was not allowed to run or to reach a file",
   why: "The system refused it - a script without permission to run, or a file it cannot open.",
   fix: "Check the script can be run (chmod +x on a script) and the files it uses are readable."},
  {re: /ECONNREFUSED|connection refused|actively refused|could not connect|failed to connect|unreachable/i,
   what: "The hook could not reach a service",
   why: "It calls something - a local server, a port, a URL - that did not answer.",
   fix: "Start that service, or check the address the hook uses."},
  {re: /json|parse|unexpected token|invalid output/i,
   what: "Claude Code could not read the hook's output",
   why: "A hook that prints to stdout must print the JSON Claude Code expects, or nothing.",
   fix: "Send the hook's own messages to stderr, or print valid JSON."},
  {re: /exit(ed)? (with )?(code|status)|non-?zero|status \d|code \d/i,
   what: "The hook ran and failed",
   why: "Its command ended with an error code.",
   fix: "Run it yourself to see why (below)."},
];
const HOOK_OTHER = {what: "The hook reported an error", why: "Claude Code recorded the text below.",
                    fix: "Run it yourself to see all of its output (below)."};
const hookKind = text => HOOK_KINDS.find(k => k.re.test(text || "")) || HOOK_OTHER;

// Pure: the pop-up's body for the hooks row `h` of session `s`.
function hookHelpHtml(h, s){
  const errors = (h && h.errors) || [], now = hookErrorsNow(h);
  if (!errors.length) return `<p class=none>No hook errors recorded for this session.</p>`;
  const cwd = (s && s.cwd) || "the session's folder";
  const hooks = [...new Set(errors.flatMap(e => e.hooks || []))];
  const ours = hooks.some(c => /imperatorium/i.test(c));
  const kinds = [...new Set(now.map(e => hookKind(e.error)))];
  const errHtml = e => `<li class="${now.includes(e) ? "" : "old"}"><span class=when>${
    esc(e.at ? clock(e.at) : "")}</span> <b>${esc(e.event || "Stop")}</b> - ${esc(hookKind(e.error).what)}<pre>${esc(e.error || "")}</pre></li>`;
  return [
    now.length
      ? `<p>A hook is a command Claude Code runs at a point in a session - these ran when a turn ended (Stop). One that fails does not stop the session, but whatever it was for (a check, a notification, a log) did not happen.</p>`
      : `<p class=ok>The Stop hooks ran without an error at ${esc(clock(h.ok_at))}: these are earlier ones, kept for reference.</p>`,
    kinds.map(k => `<div class=hkkind><b>${esc(k.what)}.</b> ${esc(k.why)} ${esc(k.fix)}</div>`).join(""),
    hooks.length ? `<h4>Stop hooks that ran</h4><ul class=hkcmds>${hooks.map(c => `<li><code>${esc(c)}</code></li>`).join("")}</ul>` : "",
    `<h4>What to do</h4><ol class=hksteps>`,
    `<li>Find the hook: in the session's terminal (View &gt; Show Window) run <code>/hooks</code> - it lists each hook and the settings file it comes from.</li>`,
    `<li>See why it fails: run its command yourself in <code>${esc(cwd)}</code>; or start the session with <code>claude --debug</code>, which logs each hook's full output.</li>`,
    `<li>Fix or remove it where it is set: <code>~/.claude/settings.json</code> (every project), <code>.claude/settings.json</code> or <code>.claude/settings.local.json</code> in the project, or the plugin that adds it (<code>/plugin</code>).</li>`,
    ours ? `<li>One of them is Imperatorium's own hook. Check the dispatcher answers with <code>imperatorium status</code>; <code>imperatorium install</code> puts its hooks back as they should be.</li>` : "",
    `<li>The chip goes once a later turn's Stop hooks run without an error.</li></ol>`,
    `<h4>Errors (newest last)</h4><ul class=hkerrs>${errors.map(errHtml).join("")}</ul>`,
  ].join("");
}
function hooksRow(){
  return procFor === selected ? (procTasks || []).find(t => t.kind === "hooks") || null : null;
}
function drawHookHelp(){
  const s = current();
  $("#hksub").textContent = s ? nameOf(s) : "";
  $("#hookpane").innerHTML = hookHelpHtml(hooksRow(), s);
}
function openHookHelp(){
  openPopup("hooks", true);
  $("#hookbox [data-popclose]").focus();
}

function drawTileTasks(){
  const box = document.querySelector("#sessions .s.on .tk");
  if (!box) return;
  const s = current(), now = Date.now() / 1000;
  const l = taskList(s, procFor === selected ? procTasks : []);
  const items = taskItems(l, now);
  // The chip is a button of its own beside the header, which folds the list.
  const hooks = l.extra.find(t => t.kind === "hooks");
  const html = `<div class=tkhr><button type=button class=tkh data-tasks-toggle aria-expanded="${tasksOpen}">` +
    `<span class=ar>&#9656;</span>Tasks · ${esc(taskSummary(l.running))}</button>${
      hookErrorsNow(hooks).length ? `<button type=button class="st bad hkchip" data-hookerr title="what went wrong, and what to do">hook errors</button>` : ""
    }</div><div class=tkl>${items}</div>`;
  box.classList.toggle("open", tasksOpen);
  if (box.dataset.sig !== html){ box.innerHTML = html; box.dataset.sig = html; }
  const line = document.querySelector("#sessions .s.on .mcpl");
  if (line){
    const m = mcpLine(l.mcp);
    if (line.dataset.sig !== m){ line.innerHTML = m; line.dataset.sig = m; }
  }
}
// Pure: the open tile's MCP Line - its MCP servers, under the Meta Line and
// aligned with it, in a colour of its own so it does not read as one of the
// tasks (2026-11-16). A click opens the list, as the task row did.
function mcpLine(t){
  if (!t) return "";
  const names = (t.servers && t.servers.length ? t.servers : String(t.label || "").split(", ")).filter(Boolean);
  if (!names.length) return "";
  return `<span class=mk data-task="${esc(t.id)}" title="MCP servers connected to this session: ${
    esc(names.join(", "))}">MCP</span><span class=ml data-task="${esc(t.id)}">${esc(names.join(" · "))}</span>`;
}
const taskShown = () => !!taskView && taskView.k === selected && view === "session" && tab === "log";
function openTask(id){
  if (id === "tool"){ closeTask(); jumpToTool(); return; }
  if (taskView && taskView.id === id){ closeTask(); return; }
  if (tab !== "log") setTab("log");
  taskView = {k: selected, id}; taskLast = null; tvShown = ""; tvExtra = null; tvArm = "";
  drawProcs();
  loadTaskExtra(true);
}
function closeTask(){
  if (!taskView) return;
  taskView = null; taskLast = null; tvShown = ""; tvExtra = null; tvArm = "";
  drawProcs();
}
// The tool call in flight is the newest one in the chat.
function jumpToTool(){
  const t = [...$("#log").querySelectorAll(".tool")].pop();
  if (!t) return;
  t.scrollIntoView({block: "center"});
  logFollow = logAtEnd($("#log"));
  markAway();
}

// What a view needs beyond the task list: a subagent's whole chat, or the
// processes' numbers. Fetched when opened and every 2 s while it shows.
let tvExtra = null, tvExtraAt = 0, tvArm = "";
async function loadTaskExtra(force){
  if (!taskShown()) return;
  const v = taskView, t = currentTask();
  const kind = v.id === "processes" ? "process" : t && t.kind;
  if (kind !== "agent" && kind !== "process") return;
  if (!force && Date.now() - tvExtraAt < 2000) return;
  tvExtraAt = Date.now();
  let d;
  try {
    d = kind === "agent"
      ? await get("/api/subagent?session=" + encodeURIComponent(v.k) + "&agent=" + encodeURIComponent(v.id))
      : await get("/api/processes?session=" + encodeURIComponent(v.k));
  } catch (e) { return; }
  if (taskView !== v) return;
  tvExtra = d; tvShown = "";
  drawTaskView();
}
function currentTask(){
  if (!taskView) return null;
  return (procFor === selected ? procTasks : []).find(x => x.id === taskView.id) || null;
}

// The head's buttons, by kind: copying needs nothing, the others are asked
// twice (the first click arms, the second acts).
function taskActions(t){
  const out = [];
  if (!t) return "";
  if (t.text && (t.kind === "shell" || t.kind === "monitor")) out.push(`<button class="ghost sm" data-tvact="copy">Copy output</button>`);
  if (t.path) out.push(`<button class="ghost sm" data-tvact="path" title="${esc(t.path)}">Copy path</button>`);
  const s = current();
  if (!t.done && (t.kind === "shell" || t.kind === "monitor" || t.kind === "agent") && s && s.dispatchable)
    out.push(`<button class="ghost sm${tvArm === "stop" ? " danger" : ""}" data-tvact="stop" title="sends the session a prompt asking it to stop this task">${
      tvArm === "stop" ? "Click again to ask" : "Ask Claude to stop it"}</button>`);
  return out.join("");
}
function drawTaskView(){
  if (taskView && taskView.k !== selected){ taskView = null; taskLast = null; tvShown = ""; tvExtra = null; }
  const on = taskShown();
  $("#taskview").hidden = !on;
  if (tab === "log") $("#log").hidden = on;
  if (!on) return;
  const now = Date.now() / 1000;
  if (taskView.id === "processes") return drawProcView(now);
  const t = currentTask();
  if (t) taskLast = t;
  const show = t || taskLast;
  if (!show){ setHead(`<button class="ghost sm" data-taskback>&#8592; Chat</button><b>loading…</b>`); return; }
  const gone = !t || show.done;
  const when = gone ? `<span class="when ended">${show.done ? esc((STATUS_WORD[show.status] || show.status || "ended") +
      (show.ended ? " " + age(show.ended) + " ago" : "")) : "ended"} - its last output</span>`
    : `<span class=when>${show.kind === "wakeup" ? esc(taskWhen(show, now))
      : show.started ? "started " + esc(age(show.started)) + " ago" : ""}</span>`;
  let numbers = "";
  if (show.kind === "agent" && tvExtra && tvExtra.ok){
    const tk = tvExtra.tokens || {}, all = (tk.input || 0) + (tk.output || 0) + (tk.cache_read || 0) + (tk.cache_write || 0);
    numbers = `<span class=when title="this subagent's own model, tokens and list-price cost">${esc([
      tvExtra.model, tvExtra.context_tokens ? "context " + fmtTokens(tvExtra.context_tokens) : "",
      all ? fmtTokens(all) + " tokens" : "", tvExtra.cost_usd != null ? "≈ $" + tvExtra.cost_usd.toFixed(2) : ""]
      .filter(Boolean).join(" · "))}</span>`;
  }
  setHead(`<button class="ghost sm" data-taskback title="back to the chat (Esc)">&#8592; Chat</button>` +
    `<span class=k>${TASK_MARK[show.kind] || "?"}</span><b title="${esc(show.label || show.id)}">${esc(show.label || show.id)}</b>` +
    numbers + when + `<span class=acts>${taskActions(show)}</span>`);
  // A subagent reads as a chat once its transcript has come.
  if (show.kind === "agent" && tvExtra && tvExtra.ok && (tvExtra.items || []).length){
    const sig = show.id + "\nchat\n" + tvExtra.sig;
    if (sig === tvShown) return;
    const box = $("#tvtext"), atEnd = box.scrollHeight - box.scrollTop - box.clientHeight < 24 || !tvShown.startsWith(show.id + "\n");
    box.classList.add("chat");
    box.innerHTML = tvExtra.items.map(it => `<div class="turn${it.speaker === "you" ? " you-turn" : ""}"><div class="who ${
      esc(it.speaker || it.role)}">${esc(it.speaker === "you" ? "its task" : SPEAKER[it.speaker] || it.speaker || it.role)}</div>${
      renderBlocks(it.blocks)}</div>`).join("");
    tvShown = sig;
    if (atEnd) box.scrollTop = box.scrollHeight;
    return;
  }
  const text = show.kind === "wakeup"
    ? (show.fires_at ? "Fires at " + clock(show.fires_at) + (show.recurring ? ", and again after that" : "") + ".\n\n" : "") + (show.label || "")
    : show.kind === "hooks" ? (show.errors || []).map(e => (e.at ? clock(e.at) + "  " : "") + e.event + ": " + e.error).join("\n")
    : show.kind === "mcp" ? "MCP servers connected to this session:\n\n" + (show.servers || []).map(n => "  " + n).join("\n")
    : show.text || "";
  setText2(show.id, text);
}
function setHead(html){
  if ($("#tvhead").dataset.sig !== html){ $("#tvhead").innerHTML = html; $("#tvhead").dataset.sig = html; }
}
function setText2(id, text){
  const pre = $("#tvtext"), sig = id + "\n" + text;
  if (sig === tvShown) return;
  pre.classList.remove("chat");
  const atEnd = pre.scrollHeight - pre.scrollTop - pre.clientHeight < 4 || !tvShown.startsWith(id + "\n");
  pre.innerHTML = text.trim() ? esc(text.replace(/\n+$/, "")) : `<span class=none>no output yet</span>`;
  tvShown = sig;
  if (atEnd) pre.scrollTop = pre.scrollHeight;
}
const fmtTokens = n => n >= 1e6 ? (n / 1e6).toFixed(1) + "M" : n >= 1e3 ? Math.round(n / 1e3) + "k" : String(n);
// The processes under the session, with what each costs; End is asked twice.
function drawProcView(now){
  setHead(`<button class="ghost sm" data-taskback title="back to the chat (Esc)">&#8592; Chat</button>` +
    `<span class=k>proc</span><b>Processes started by its shells</b><span class=when>refreshed every 2 s</span>`);
  const d = tvExtra;
  if (!d){ setText2("processes", ""); return; }
  const rows = (d.processes || []).map(p => `<tr><td style="padding-left:${(p.depth - 1) * 14}px">${esc(p.name)}</td><td>${p.pid}</td>` +
    `<td>${p.started ? esc(age(p.started)) : ""}</td><td>${p.cpu_s != null ? p.cpu_s.toFixed(1) + " s" : ""}</td>` +
    `<td>${p.mem_mb != null ? Math.round(p.mem_mb) + " MB" : ""}</td><td class=exe title="${esc(p.exe || "")}">${esc(p.exe || "")}</td>` +
    `<td><button class="ghost sm${tvArm === "kill:" + p.pid ? " danger" : ""}" data-kill="${p.pid}">${
      tvArm === "kill:" + p.pid ? "Click again to end" : "End"}</button></td></tr>`).join("");
  const html = rows ? `<table class=procs><tr><th>process</th><th>pid</th><th>age</th><th>CPU</th><th>memory</th><th>executable</th><th></th></tr>${rows}</table>`
    : `<span class=none>${esc(d.error || "no processes beneath the session")}</span>`;
  const sig = "processes\n" + html;
  if (sig === tvShown) return;
  const box = $("#tvtext");
  box.classList.remove("chat");
  box.innerHTML = html;
  tvShown = sig;
}
$("#taskview").addEventListener("click", async e => {
  if (e.target.closest("[data-taskback]")) return closeTask();
  const t = currentTask() || taskLast;
  const act = e.target.closest("[data-tvact]");
  if (act && t){
    const what = act.dataset.tvact;
    if (what === "copy" || what === "path"){
      try { await navigator.clipboard.writeText(what === "copy" ? t.text : t.path); act.textContent = "copied"; }
      catch (x) { act.textContent = "copy failed"; }
      return;
    }
    if (what === "stop"){
      if (tvArm !== "stop"){ tvArm = "stop"; $("#tvhead").dataset.sig = ""; return drawTaskView(); }
      tvArm = "";
      const r = await post("/api/dispatch", {session_id: taskView.k, prompt:
        `Please stop the background task ${t.id} (${t.label || t.kind}) now, with TaskStop. ` +
        `I asked for this from the Imperatorium window.`});
      $("#say").textContent = r.ok ? "asked the session to stop it" : (r.error || "not sent");
      $("#tvhead").dataset.sig = "";
      return drawTaskView();
    }
  }
  const kill = e.target.closest("[data-kill]");
  if (kill){
    const pid = +kill.dataset.kill;
    if (tvArm !== "kill:" + pid){ tvArm = "kill:" + pid; tvShown = ""; return drawTaskView(); }
    tvArm = "";
    const r = await post("/api/kill_process", {session_id: taskView.k, pid});
    $("#say").textContent = r.ok ? "ended process " + pid : (r.error || "not ended");
    loadTaskExtra(true);
  }
});
document.addEventListener("keydown", e => {
  if (e.key === "Escape" && taskShown() && !typing(e)){ e.preventDefault(); closeTask(); }
});

async function procLoop(){
  try { await loadProcs(); } catch (e) {}
  setTimeout(procLoop, 1000);
}

// Where `el` sits in the log's own scroll coordinates. Not `offsetTop`: the
// chat text size zooms every turn (`#log>*{zoom:var(--chatz)}`), and Chrome
// gives a zoomed element's offsetTop in its zoomed units while the log
// scrolls in plain pixels - at 15px text (zoom 1.07) a jump overshot by 7% of
// the distance, thousands of pixels in a long chat. The bounding rect is in
// viewport pixels whatever the zoom.
function logY(el){
  const log = $("#log");
  return el.getBoundingClientRect().top - log.getBoundingClientRect().top - log.clientTop + log.scrollTop;
}

// -- jump between your prompts ---------------------------------------------
function jump(dir){
  const log = $("#log");
  const top = log.scrollTop, turns = [...log.querySelectorAll(".turn.you-turn")];
  // Compare where each prompt would be scrolled to, not where it sits, or the
  // prompt just jumped to counts as the next one down.
  const at = x => logY(x) - 8;
  const t = dir < 0 ? turns.filter(x => at(x) < top - 2).pop()
                    : turns.find(x => at(x) > top + 2);
  // Above the first drawn prompt: draw the earlier turns and go on from there.
  if (!t && dir < 0 && logStart(logItems.length, logShown) > 0){ showEarlier(); return jump(dir); }
  if (!t) return;
  log.scrollTop = at(t);
  t.classList.add("flash");
  setTimeout(() => t.classList.remove("flash"), 900);
}

// -- one unsent draft per session ------------------------------------------
// The box belongs to whichever session it was typed for; switching sessions
// parks it and brings back that session's own. Kept in this browser only.
let drafts = {}, draftFor = null;
try { drafts = JSON.parse(localStorage.getItem("imperatorium.drafts") || "{}") || {}; } catch (e) { drafts = {}; }
function saveDrafts(){
  try { localStorage.setItem("imperatorium.drafts", JSON.stringify(drafts)); } catch (e) {}
}
function noteDraft(){
  if (draftFor === null) return;
  const v = $("#prompt").value;
  if (v) drafts[draftFor] = v; else delete drafts[draftFor];
  saveDrafts();
}
function syncDraft(){
  const k = view === "fleet" ? "fleet" : selected;
  if (draftFor === k) return;
  noteDraft();
  draftFor = k;
  $("#prompt").value = (k && drafts[k]) || "";
}
// After a send: clear the draft it came from, and the box only if it still
// shows that session - the person may have moved on while it was in flight.
function clearPrompt(k){
  k = k === undefined ? draftFor : k;
  delete drafts[k]; saveDrafts();
  if (draftFor === k) $("#prompt").value = "";
}

// -- what changed since you last looked --------------------------------------
// A session that finished a turn while you were looking elsewhere is marked
// "new" until you select it. When it went idle is its status change time.
let seenAt = {};
try { seenAt = JSON.parse(localStorage.getItem("imperatorium.seen") || "{}") || {}; } catch (e) { seenAt = {}; }
const doneAt = s => s.status === "idle" && s.status_changed_at ? s.status_changed_at : 0;
function isUnread(s){ return key(s) !== selected && doneAt(s) > (seenAt[key(s)] || 0) + 1; }

// Pure: the seen map after a new session list. The session being looked at
// is seen now; one met for the first time counts as seen as it stands, so
// opening the window does not mark everything new; gone ones drop out.
function updateSeen(seen, list, looking, now){
  if (!list.length) return seen;
  const out = {};
  for (const s of list){
    const k = key(s);
    out[k] = k === looking ? Math.max(now, doneAt(s)) : (k in seen ? seen[k] : doneAt(s));
  }
  return out;
}

// -- attention: title, favicon, desktop notifications -------------------------
let attnBefore = null, deskNotify = false;
try { deskNotify = localStorage.getItem("imperatorium.desknotify") === "1"; } catch (e) {}
// The switch lives in the Settings window, another page on this origin.
try { addEventListener("storage", e => { if (e.key === "imperatorium.desknotify") deskNotify = e.newValue === "1"; }); } catch (e) {}

// Pure: sessions that started waiting for you since the last list (none on
// the first, so opening the window does not announce what already waits).
function newlyWaiting(before, list){
  if (!before) return [];
  return list.filter(s => s.attention && !before.has(key(s)));
}

// Pure: the session to switch to because it just started - one that was not
// on the last list and started within two minutes, the newest if several.
// None on the first list, and none without a start time, so a remote host
// reconnecting or the dispatcher restarting does not yank the view.
function newlyStarted(before, list, now){
  if (!before) return null;
  const at = s => { const t = Number(s.startedAt) || 0; return t > 1e12 ? t / 1000 : t; };
  const fresh = list.filter(s => !before.has(key(s)) && at(s) && now - at(s) < 120)
                    .sort((a, b) => at(b) - at(a));
  return fresh.length ? key(fresh[0]) : null;
}

// Pure: a /clear followed through (2026-11-17). A cleared session goes on in
// the same claude process under a new session id - and with the process's
// old start time, so newlyStarted misses it. `before` maps each key on the
// last list to its pid; the answer is {from, to} for a new local row whose
// pid an old row had and no longer has (gone, or left resumable), the
// selected one first; null when none, and on the first list.
function clearedInto(before, list, selected){
  if (!before) return null;
  const now = new Map(list.map(s => [key(s), s]));
  const found = [];
  for (const s of list){
    if (s.remote || !s.pid || before.has(key(s))) continue;
    for (const [k, pid] of before){
      const after = now.get(k);
      if (k !== key(s) && pid === s.pid && (!after || after.pid !== pid)) found.push({from: k, to: key(s)});
    }
  }
  return found.find(f => f.from === selected) || found[0] || null;
}

// Pure: the title counts the sessions waiting for you. hotkey.py accepts the
// "(N) " prefix when it looks for this window by title.
const titleFor = n => (n ? `(${n}) ` : "") + "Imperatorium";

function notifyWaiting(list){
  const fresh = newlyWaiting(attnBefore, list);
  attnBefore = new Set(list.filter(s => s.attention).map(key));
  if (!fresh.length || !deskNotify || !("Notification" in window) || Notification.permission !== "granted") return;
  if (document.visibilityState === "visible" && document.hasFocus()) return;
  for (const s of fresh.slice(0, 3)){
    const n = new Notification(nameOf(s) + " needs you", {body: stateOf(s)[1], tag: "imperatorium-" + key(s)});
    n.onclick = () => { window.focus(); select(key(s)); n.close(); };
  }
}

// The tab's icon is the most urgent state's dot, in its colour and shape.
let favSig = "";
function drawFavicon(list){
  const has = f => list.some(f);
  const state = has(s => s.attention && !["failure", "complete"].includes(s.attention_kind)) ? "attn"
    : has(s => s.attention && s.attention_kind === "failure") ? "fail"
    : has(s => s.attention) ? "comp" : has(s => stateOf(s)[0] === "d-busy") ? "busy"
    : list.length ? "idle" : "none";
  const sig = state + matchMedia("(prefers-color-scheme: dark)").matches;
  if (sig === favSig) return;
  favSig = sig;
  const c = getComputedStyle(document.documentElement).getPropertyValue(
    {attn: "--warn", fail: "--fail", comp: "--comp", busy: "--busy", idle: "--ok", none: "--done"}[state]).trim() || "#888";
  const shape = state === "attn" ? `<rect x="7" y="7" width="18" height="18" rx="3" transform="rotate(45 16 16)" fill="${c}"/>`
    : state === "fail" ? `<path d="M8 8L24 24M24 8L8 24" stroke="${c}" stroke-width="6" stroke-linecap="round"/>`
    : state === "comp" ? `<path d="M6 17L13 24L26 9" fill="none" stroke="${c}" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/>`
    : state === "idle" ? `<circle cx="16" cy="16" r="10" fill="none" stroke="${c}" stroke-width="5"/>`
    : `<circle cx="16" cy="16" r="12" fill="${c}"/>`;
  $("#fav").href = "data:image/svg+xml," + encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">${shape}</svg>`);
}

// -- a new session list, pushed or polled -------------------------------------
const rowSig = s => [s.status, s.status_changed_at, s.last_event_at, s.doing, s.attention,
                     s.attention_kind, s.waiting_tasks, s.reviewed].join("|");
let keysBefore = null, pidsBefore = null;
function gotSessions(list){
  const was = current(), wasSig = was ? rowSig(was) : "";
  sessions = list;
  catchFollowed();
  const cleared = clearedInto(pidsBefore, list, selected);
  if (cleared) catchCleared(cleared);
  else catchStarted(newlyStarted(keysBefore, list, Date.now() / 1000));
  keysBefore = new Set(list.map(key));
  pidsBefore = new Map(list.filter(s => !s.remote && s.pid).map(s => [key(s), s.pid]));
  const looking = view === "session" && document.visibilityState === "visible" ? selected : null;
  seenAt = updateSeen(seenAt, list, looking, Date.now() / 1000);
  try { localStorage.setItem("imperatorium.seen", JSON.stringify(seenAt)); } catch (e) {}
  notifyWaiting(list);
  document.title = titleFor(list.filter(s => s.attention).length);
  drawFavicon(list);
  drawSessions();
  // News about the open session: read its conversation now, not next tick.
  const now = current();
  if (now && was && key(now) === key(was) && rowSig(now) !== wasSig) refreshTab(false);
}

// Server-sent events: the dispatcher pushes the list when it changes, and at
// once when a hook reports. Polling takes over while the stream is down;
// EventSource reconnects by itself.
// The dispatcher sends `ping` every 15 s of quiet (2026-11-21); a stream
// not heard from for STREAM_QUIET is taken for dead even if the browser
// thinks it open: polling takes over and the stream is opened again.
let streamUp = false, streamAt = 0, streamES = null;
const STREAM_QUIET = 45000;
function openStream(){
  if (!("EventSource" in window)) return;
  const es = streamES = new EventSource("/api/stream");
  const heard = () => { if (es === streamES) streamAt = Date.now(); };
  es.addEventListener("sessions", e => {
    let d;
    try { d = JSON.parse(e.data); } catch (x) { return; }
    if (es !== streamES) return;
    heard();
    streamUp = true;
    gotSessions(d.sessions || []);
  });
  es.addEventListener("ping", heard);
  es.onopen = () => { heard(); checkBoot(reloadForNewDispatcher); };
  es.onerror = () => { if (es === streamES) streamUp = false; };
}
// Pure: whether a stream last heard from at `at` is to be given up on.
function streamStale(up, at, now){ return up && now - at > STREAM_QUIET; }
function watchStream(){
  if (!streamStale(streamUp, streamAt, Date.now())) return;
  streamUp = false;
  if (streamES) streamES.close();
  openStream();
}
// Said once when the dispatcher stops answering, and taken back when it does.
const DOWN_SAID = "the dispatcher is not answering - retrying";
let downSaid = false;
function noteDown(down){
  if (down === downSaid) return;
  downSaid = down;
  if (down) $("#say").textContent = DOWN_SAID;
  else if ($("#say").textContent === DOWN_SAID) $("#say").textContent = "";
}

// -- keyboard ----------------------------------------------------------------
// j/k move through the list as shown, n goes to the next session waiting for
// you, / filters; alt+1-9, alt+N and ctrl+K work from inside the prompt box too.
const typing = e => { const t = e.target;
  return !!t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)); };
const visibleKeys = () => [...document.querySelectorAll("#sessions .s")].map(el => el.dataset.k);

// Pure: the next session waiting for you after `current`, the longest
// waiting first, wrapping round.
function nextWaiting(list, current){
  const w = list.filter(s => s.attention).sort((a, b) =>
    (a.attention_since || 0) - (b.attention_since || 0) || nameOf(a).localeCompare(nameOf(b)));
  if (!w.length) return null;
  const i = w.findIndex(s => key(s) === current);
  return key(w[(i + 1) % w.length]);
}

function pick(k){
  if (!k) return;
  select(k);
  const el = [...document.querySelectorAll("#sessions .s")].find(x => x.dataset.k === k);
  if (el) el.scrollIntoView({block: "nearest"});
}
function stepSession(dir){
  const ks = visibleKeys();
  if (!ks.length) return;
  const i = ks.indexOf(selected);
  pick(ks[i < 0 ? 0 : (i + dir + ks.length) % ks.length]);
}
function goWaiting(){
  const k = nextWaiting(sessions, selected);
  if (k) pick(k); else $("#say").textContent = "nothing is waiting for you";
}

// ctrl+K: jump to any session by name.
let jsel = 0;
function jumpHits(){
  const q = $("#jq").value.trim().toLowerCase();
  return sessions.filter(s => !q || [nameOf(s), s.project, s.cwd, s.window_title, s.doing, s.host]
    .some(v => String(v || "").toLowerCase().includes(q))).slice(0, 50);
}
function drawJump(){
  const hit = jumpHits();
  jsel = Math.max(0, Math.min(jsel, hit.length - 1));
  $("#jlist").innerHTML = hit.map((s, i) => {
    const [dot, label] = stateOf(s);
    return `<div class="ji${i === jsel ? " on" : ""}" data-jk="${esc(key(s))}"><span class="dot ${dot}"></span><span class="jn">${
      esc(nameOf(s))}</span><span class="jw">${esc(label)}${s.window_title ? " · " + esc(s.window_title) : ""}</span></div>`;
  }).join("") || `<div class="note">no match</div>`;
  const on = $("#jlist .ji.on");
  if (on) on.scrollIntoView({block: "nearest"});
  return hit;
}
function openJump(show){
  $("#jumpbox").hidden = !show;
  if (show){ $("#jq").value = ""; jsel = 0; drawJump(); $("#jq").focus(); }
}

// -- heavy sessions ------------------------------------------------------------
// A full context, or one behind an expired prompt cache, makes the next
// prompt expensive: a full context is said above the prompt box, a cold
// cache in the chat (coldHtml), neither on the tile. How large a
// context the cold cache is said for is a setting (chat.cold_cache_tokens),
// carried on the row as work.cold_tokens; 0 says it for any.
// The cache clock runs from the last main-chain reply, so a working turn
// keeps it warm with each step; it can still lapse under one long tool call
// or a foreground subagent, but then the turn's own next step pays the
// rewrite and there is no prompt to warn about - so it is said only once the
// session is not working (2026-11-21: a >1h turn showed "cache expired").
const CTX_WARN = 85;
function heavyWarnings(s, now){
  const out = [], w = s.work || {}, ctx = (s.usage || {}).context_pct;
  if (ctx != null && ctx >= CTX_WARN)
    out.push({kind: "context", short: "ctx " + Math.round(ctx) + "%",
      text: `context ${Math.round(ctx)}% full - consider /compact or a fresh session before more work`});
  const tok = w.context_tokens, working = !!RUNNING[s.status] || s.state === "working";
  if (!working && tok != null && tok >= (w.cold_tokens || 0) && w.cache_expires_at && w.cache_expires_at <= now)
    out.push({kind: "cold", short: "cold " + kfmt(tok),
      text: `prompt cache expired: the next prompt re-sends ${kfmt(tok)} tokens of context at the full cache-write price`});
  return out;
}
// A cold cache is not said above the prompt box but in the chat, as a button
// over its end that offers the way out: a fresh session, or one with this
// session closed.
function drawHeavy(s){
  const html = (s ? heavyWarnings(s, Date.now() / 1000) : []).filter(h => h.kind !== "cold")
    .map(h => `<div>${esc(h.text)}</div>`).join("");
  if ($("#heavy").dataset.sig !== html){ $("#heavy").innerHTML = html; $("#heavy").dataset.sig = html; }
  drawCold(s);
}

// Closed with its X, the button stays away for that expiry: it is back only
// once the cache has been warmed and has gone cold again.
let coldGone = {};
const coldKey = s => key(s) + "@" + (s.work || {}).cache_expires_at;

// Pure: the cold-cache button for row `s`, or "" while its cache is warm, it
// is small, it is not on this machine, or it was closed (in `gone`).
function coldHtml(s, now, gone){
  if (!s || s.remote || !heavyWarnings(s, now).some(h => h.kind === "cold")) return "";
  if ((gone || coldGone)[coldKey(s)]) return "";
  const w = s.work, cost = w.resume_cost_usd;
  return `<div class="coldbtn" role="group" aria-label="prompt cache expired">` +
    `<button class="coldx" data-coldact="dismiss" aria-label="close" title="close - it comes back only when the cache next expires">&times;</button>` +
    `<div class="ct"><b>Prompt cache expired.</b> ` +
    `Continuing this session re-sends ${esc(kfmt(w.context_tokens))} tokens of context` +
    (cost != null ? ` - about ${esc(usd(cost))} at the cache-write price` : " at the full cache-write price") +
    `. Click to start a new session, or to close this one and start a new session.` +
    `<div class="cn">Or just carry on: a prompt entered as usual continues this session.</div></div><div class="segs">` +
    `<button data-coldact="new" title="a fresh session in this project, selected here; this one stays open">New session</button>` +
    (s.pid ? `<button data-coldact="new-close" title="a fresh session in this project, selected here; this one is then closed (its conversation stays resumable)">Close this &amp; start new</button>` : "") +
    `</div></div>`;
}
function drawCold(s){
  const box = $("#log .newest .cold");
  if (!box) return;
  const html = coldHtml(s, Date.now() / 1000);
  if (box.dataset.sig !== html){ box.innerHTML = html; box.dataset.sig = html; box.hidden = !html; }
  const log = $("#log"), was = log.classList.contains("cold");
  log.classList.toggle("cold", !!html);
  if (!!html !== was && logFollow) log.scrollTop = log.scrollHeight;
}

// -- find in the conversation ----------------------------------------------------
let finds = {q: "", filter: "", hits: [], at: -1};
const blockText = b => b.kind === "tool" ? (b.name || "") + " " + (b.summary || "") : String(b.text || "");

// Pure: the indexes of the items that match. "tool" looks only at tool calls
// and "fail" only at failed results (all of them when `q` is empty); with no
// filter every block `shown` lets through is searched, and "" finds nothing.
function findHits(items, q, filter, shown){
  q = String(q || "").toLowerCase();
  if (!q && !filter) return [];
  const out = [];
  items.forEach((it, i) => {
    if ((it.blocks || []).some(b => {
      if (shown && !shown(it, b)) return false;
      if (filter === "tool" && b.kind !== "tool") return false;
      if (filter === "fail" && !(b.kind === "result" && b.is_error)) return false;
      return !q || blockText(b).toLowerCase().includes(q);
    })) out.push(i);
  });
  return out;
}
const blockShown = (it, b) => modeOf(it.note === SUMMARY_NOTE ? "system" : categoryOf(it, b)) !== "hidden";

function runFind(){
  finds.q = $("#cfind").value.trim();
  finds.filter = $("#cfilter").value;
  finds.hits = findHits(logItems, finds.q, finds.filter, blockShown);
  finds.at = finds.hits.length;   // the first step lands on the newest
  drawLog();
  goFind(-1);
}
function resetFind(){
  finds = {q: "", filter: "", hits: [], at: -1};
  $("#cfind").value = ""; $("#cfilter").value = ""; $("#cfcount").textContent = "";
}
function goFind(dir){
  const n = finds.hits.length;
  if (!n){ $("#cfcount").textContent = finds.q || finds.filter ? "0" : ""; return; }
  finds.at = ((finds.at + dir) % n + n) % n;
  const i = finds.hits[finds.at];
  if (i < logStart(logItems.length, logShown)){ logShown = logItems.length - i + 2; drawLog(); }
  $("#cfcount").textContent = `${finds.at + 1}/${n}`;
  markFinds();
  const el = [...document.querySelectorAll("#log .turn[data-i]")].find(t => +t.dataset.i === i);
  if (!el) return;
  const q = finds.q.toLowerCase();
  el.querySelectorAll("details.cmp:not([open]),details.tgroup:not([open])").forEach(d => {
    if (!q || d.textContent.toLowerCase().includes(q)){ d.open = true; opened.add(d.dataset.k); }
  });
  const m = el.querySelector("mark.cur") || el;
  $("#log").scrollTop = Math.max(0, logY(m) - $("#log").clientHeight / 3);
}
// Highlights every match in the drawn turns, and the current one strongest.
function markFinds(){
  const q = finds.q.toLowerCase(), cur = finds.hits[finds.at];
  document.querySelectorAll("#log .turn.found").forEach(t => {
    if (q && !t.dataset.marked){ wrapMatches(t, q); t.dataset.marked = "1"; }
    t.classList.toggle("cur-find", +t.dataset.i === cur);
  });
  document.querySelectorAll("#log mark.cur").forEach(m => m.classList.remove("cur"));
  const ct = $("#log .turn.cur-find mark");
  if (ct) ct.classList.add("cur");
}
function wrapMatches(root, q){
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT), nodes = [];
  while (walk.nextNode()) nodes.push(walk.currentNode);
  for (const n of nodes){
    const text = n.nodeValue, low = text.toLowerCase();
    if (!low.includes(q)) continue;
    const frag = document.createDocumentFragment();
    let at = 0, i;
    while ((i = low.indexOf(q, at)) >= 0){
      frag.append(text.slice(at, i));
      const m = document.createElement("mark");
      m.textContent = text.slice(i, i + q.length);
      frag.append(m);
      at = i + q.length;
    }
    frag.append(text.slice(at));
    n.replaceWith(frag);
  }
}

// -- diff rows, for line comments -------------------------------------------------
// Pure: each line of a unified diff with its class and, for lines inside a
// hunk, the file and line it is on - the new side for added and context
// lines, the old side for removed ones.
function diffRows(text){
  let file = null, oldN = 0, newN = 0, inHunk = false;
  return String(text || "").split("\n").map(l => {
    if (l.startsWith("diff --git")){
      inHunk = false;
      const m = / b\/(.*)$/.exec(l);
      file = m ? m[1] : null;
      return {cls: "f", text: l};
    }
    if (!inHunk && l.startsWith("+++ ")){ const p = l.slice(4); if (p !== "/dev/null") file = p.replace(/^b\//, ""); return {cls: "", text: l}; }
    const h = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(l);
    if (h){ oldN = +h[1]; newN = +h[2]; inHunk = true; return {cls: "hh", text: l}; }
    if (!inHunk || !file || l.startsWith("\\")) return {cls: "", text: l};
    if (l.startsWith("+")) return {cls: "a", text: l, file, line: newN++, side: "new"};
    if (l.startsWith("-")) return {cls: "r", text: l, file, line: oldN++, side: "old"};
    if (l.startsWith(" ")){ oldN++; return {cls: "", text: l, file, line: newN++, side: "new"}; }
    inHunk = false;
    return {cls: "", text: l};
  });
}

// Pure: the comments as one prompt for the session.
function reviewPrompt(list){
  return "Review comments on your uncommitted changes, left on the diff in Imperatorium's " +
    "Changes tab. Please address each one:\n\n" + list.map(c => {
      const where = c.side === "old" ? `${c.file} (removed line ${c.line})` : `${c.file}:${c.line}`;
      const code = String(c.code || "").trim();
      return `- \`${where}\`${code ? " - `" + code.slice(0, 160).replace(/`/g, "'") + "`" : ""}\n  ${
        c.text.replace(/\n/g, "\n  ")}`;
    }).join("\n");
}

// -- cost history ------------------------------------------------------------------
const niceMax = v => { const p = Math.pow(10, Math.floor(Math.log10(v)));
  return [1, 2, 2.5, 5, 10].map(m => m * p).find(m => m >= v); };
const dayLabel = s => { const [y, m, d] = s.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString([], {month: "short", day: "numeric"}); };
const dollars = v => "$" + (v >= 100 ? v.toFixed(0) : v.toFixed(2));

// Pure: daily cost as SVG bars - one series, so no legend; each bar's hover
// target is its whole column and says the day, cost and tokens.
function costChart(days){
  if (!days || !days.length) return "";
  const W = 720, H = 150, L = 44, B = 20, T = 8, n = days.length;
  const top = niceMax(Math.max(0.01, ...days.map(d => d.cost)));
  const slot = (W - L) / n, bw = Math.max(1, slot - 2);
  const y = v => T + (H - T - B) * (1 - v / top);
  const grid = [0, top / 2, top].map(v => `<line class="ax" x1="${L}" x2="${W}" y1="${y(v).toFixed(1)}" y2="${
    y(v).toFixed(1)}"${v ? ' stroke-dasharray="2 3"' : ""}/><text x="${L - 6}" y="${(y(v) + 4).toFixed(1)}" text-anchor="end">$${
    !v || v >= 10 ? Math.round(v) : v.toFixed(1)}</text>`).join("");
  const bars = days.map((d, i) => {
    const x = L + i * slot + 1, top_ = y(d.cost), h = H - B - top_, r = Math.min(4, bw / 2, h);
    const path = h > 0.5 ? `<path class="bar" d="M${x.toFixed(1)},${H - B}V${(top_ + r).toFixed(1)}Q${x.toFixed(1)},${
      top_.toFixed(1)} ${(x + r).toFixed(1)},${top_.toFixed(1)}H${(x + bw - r).toFixed(1)}Q${(x + bw).toFixed(1)},${
      top_.toFixed(1)} ${(x + bw).toFixed(1)},${(top_ + r).toFixed(1)}V${H - B}Z"/>` : "";
    return `<g><title>${esc(dayLabel(d.date) + " · " + dollars(d.cost) + " · " + kfmt(d.tokens) + " tokens")}</title><rect class="hit" x="${
      (L + i * slot).toFixed(1)}" y="${T}" width="${slot.toFixed(1)}" height="${H - T - B}"/>${path}</g>`;
  }).join("");
  // The end labels align to the chart's edges so neither is cut off.
  const xl = [...new Set([0, Math.floor((n - 1) / 2), n - 1])].map(i => {
    const [x, anchor] = i === 0 ? [L, "start"] : i === n - 1 ? [W, "end"] : [L + i * slot + slot / 2, "middle"];
    return `<text x="${x.toFixed(1)}" y="${H - 5}" text-anchor="${anchor}">${esc(dayLabel(days[i].date))}</text>`;
  }).join("");
  return `<svg class="chart" viewBox="0 0 ${W} ${H}" role="img" aria-label="estimated cost per day">${grid}${bars}${xl}</svg>`;
}

async function loadCosts(){
  $("#costs").innerHTML = `<div class="note">reading transcripts…</div>`;
  let d;
  try { d = await get("/api/costs?days=" + ($("#cdays").value || 30)); } catch (e) { return; }
  if (!d.ok){ $("#costs").innerHTML = `<div class="note">${esc(d.error || "unavailable")}</div>`; return; }
  const row = (name, x, extra) => `<tr><td>${esc(name)}</td>${extra || ""}<td class="n">${kfmt(x.tokens)}</td><td class="n">${dollars(x.cost)}</td></tr>`;
  $("#costs").innerHTML = `<div class="kv">${dollars(d.total)} over ${d.window_days} days · ${kfmt(d.total_tokens)} tokens${
      d.complete ? "" : " · still reading older transcripts - reopen for the rest"}</div>
    ${costChart(d.days)}
    <table class="t"><tr><th>project</th><th>sessions</th><th>tokens</th><th>est. cost</th></tr>${
      d.projects.map(p => row(p.project, p, `<td class="n">${p.sessions}</td>`)).join("")}</table>
    <table class="t"><tr><th>model</th><th>tokens</th><th>est. cost</th></tr>${d.models.map(m => row(m.model, m)).join("")}</table>
    <small>What these tokens would cost at API list prices, from the transcripts (subagents included) - an estimate, and
      notional on a subscription. A live session's cost in its status panel is Claude Code's own figure.${
      d.unpriced_tokens ? " " + kfmt(d.unpriced_tokens) + " tokens from models not in the price table are left out." : ""}</small>`;
}

let ticks = 0;
async function tick(){
  watchStream();
  try {
    if (!streamUp) gotSessions((await get("/api/sessions")).sessions || []);
    else drawSessions();  // ages on the tiles keep counting between pushes
    if (ticks % 3 === 0) drawMeter();
    if (ticks % 2 === 0) drawPending();
    if (ticks % 2 === 1) loadFleet();
    if (ticks % 20 === 0) loadPrefs();
    if (ticks % 7 === 0) checkBoot(reloadForNewDispatcher);
    loadStatus(false);
    await refreshTab(false);
    noteDown(false);
  } catch (e) { /* dispatcher restarting; the page keeps what it has */ if (e && e.down) noteDown(true); }
  ticks++;
  setTimeout(tick, 1500);
}

async function loadPrefs(){
  try { const r = await get("/api/prefs"); Object.assign(prefs, r.prefs || {}); } catch (e) {}
  $("#group").value = prefs.group || "state";
}

// -- parked permission requests ---------------------------------------
// Only present when policy.json parks `ask` decisions for the window. The
// hook is blocked while one shows; it stays until it is answered, sent to
// the console, or the session stops waiting for it. One from a remote host
// says which host, first.
let pendingSig = "";
const hostTag = i => i.host ? `<span class="dqhost" title="a session on ${esc(i.host)}">${esc(i.host)}</span>` : "";
async function drawPending(){
  let d;
  try { d = await get("/api/pending"); } catch (e) { return; }
  const items = d.pending || [];
  const sig = JSON.stringify(items.map(i => i.id));
  if (sig === pendingSig) return;
  pendingSig = sig;
  const asked = pendingNow.some(i => i.questions) || items.some(i => i.questions);
  pendingNow = items;
  // A question's line in the chat says where to answer it.
  if (asked && typeof drawLog === "function") drawLog();
  $("#decide").hidden = !items.length;
  keepQuestions();
  $("#decide").innerHTML = items.map(i => i.questions ? questionHtml(i) : `<div class="dq"><span title="${esc(i.input)}">${
    hostTag(i)}<b>${esc(folder(i.cwd))}</b> wants ${esc(i.tool)}: ${esc(i.input)}${i.reason ? " - " + esc(i.reason) : ""}</span>
    <button class="sm" data-dv="allow" data-di="${esc(i.id)}">Allow</button>
    <button class="sm danger" data-dv="deny" data-di="${esc(i.id)}">Deny</button>
    <button class="sm ghost" data-dv="pass" data-di="${esc(i.id)}" title="answer it in the session's console instead${
      i.host ? " (on " + esc(i.host) + ")" : ""}">Console</button></div>`).join("");
  document.querySelectorAll("[data-dv]").forEach(b => b.onclick = async () => {
    const card = b.closest(".dqq");
    const body = {id: b.dataset.di, verdict: b.dataset.dv, who: "window"};
    if (card && b.dataset.dv === "allow") body.answers = questionAnswers(card);
    const r = await post("/api/decide", body);
    const done = card ? {allow: "answer sent", pass: "left to the terminal", deny: "declined"}[b.dataset.dv]
      : b.dataset.dv === "pass" ? "left to the console" : b.dataset.dv + " sent";
    $("#say").textContent = r.ok ? done : "too late - it already expired";
    pendingSig = ""; drawPending();
  });
  document.querySelectorAll("#decide .dqq").forEach(wireQuestion);
}

// An AskUserQuestion parked by the permission hook: the same choices the
// terminal's dialog offers, answered here instead. "Other" is free text, as
// in the terminal; a multi-select answer is its labels joined with ", ".
// Like Claude Code's dialog it shows one question at a time (2026-11-16):
// with several, a row of tabs - one per question, ☐ until answered, then a
// Submit tab that reviews the answers - and ←/→ to move between them;
// picking a single-select choice moves on to the next. Number keys pick a
// choice while the card has focus.
function questionHtml(i){
  const n = i.questions.length, many = n > 1;
  const tabs = !many ? "" : `<div class="qtabs" role="tablist">` +
    `<button type="button" class="qnav" data-qstep="-1" title="previous question (←)" aria-label="previous question">&#8592;</button>` +
    i.questions.map((q, k) => `<button type="button" class="qtab${k ? "" : " on"}" role="tab" data-qtab="${k}" title="${
      esc(q.question)}"><span class="qbox">&#9744;</span>${esc(q.header || "Question " + (k + 1))}</button>`).join("") +
    `<button type="button" class="qtab qsubmit" role="tab" data-qtab="${n}" title="review and submit">&#10004; Submit</button>` +
    `<button type="button" class="qnav" data-qstep="1" title="next question (→)" aria-label="next question">&#8594;</button></div>`;
  const qs = i.questions.map((q, k) => `<div class="qq" data-k="${k}" data-multi="${q.multiSelect ? 1 : ""}"
      data-q="${esc(q.question)}"${k ? " hidden" : ""}>
    <div class="qt">${!many && q.header ? `<span class="chip">${esc(q.header)}</span>` : ""}${esc(q.question)}${
      q.multiSelect ? ` <small>(choose any)</small>` : ""}</div>
    <div class="qo">${q.options.map((o, m) => `<button type="button" data-opt="${esc(o.label)}"><span class="qn">${
      q.multiSelect ? "&#9744;" : (m + 1) + "."}</span>${esc(o.label)}${o.description ? `<small>${esc(o.description)}</small>` : ""}</button>`).join("")}
      <input type="text" class="qother" placeholder="Other - type your own answer" aria-label="Other answer"></div></div>`).join("");
  const review = !many ? "" : `<div class="qrev" data-k="${n}" hidden><div class="qt">Review your answers</div><div class="qrl"></div></div>`;
  return `<div class="dq dqq" data-di="${esc(i.id)}" tabindex="-1"><div>${hostTag(i)}<b>${esc(folder(i.cwd))}</b> is asking you${
    many ? " " + n + " questions" : ""}:</div>${tabs}${qs}${review}
    <div class="qa"><button class="sm" data-dv="allow" data-di="${esc(i.id)}" disabled>Submit answer${many ? "s" : ""}</button>
    <button class="sm ghost" data-dv="pass" data-di="${esc(i.id)}" title="Show the question at the session's terminal now">Answer in terminal</button>
    <button class="sm danger" data-dv="deny" data-di="${esc(i.id)}" title="Tell Claude you declined to answer">Decline</button>
    <span class="qkeys">${many ? "&#8592;/&#8594; question · " : ""}1-9 choose · Enter ${many ? "next" : "submit"}</span></div></div>`;
}

// Pure: what each question of a card has been answered with, by question.
function questionAnswers(card){
  const out = {};
  card.querySelectorAll(".qq").forEach(qq => {
    const picked = [...qq.querySelectorAll("button.on")].map(b => b.dataset.opt);
    const other = qq.querySelector(".qother").value.trim();
    if (other) picked.push(other);
    if (picked.length) out[qq.dataset.q] = picked.join(", ");
  });
  return out;
}

// A card's page and choices, kept across a redraw of the Decision Bar (one
// more request arriving redraws it) by the request's id.
const questionKept = new Map();
function keepQuestions(){
  questionKept.clear();
  document.querySelectorAll("#decide .dqq").forEach(card => questionKept.set(card.dataset.di, {
    at: +(card.dataset.at || 0),
    qs: [...card.querySelectorAll(".qq")].map(qq => ({
      on: [...qq.querySelectorAll("button.on")].map(b => b.dataset.opt),
      other: qq.querySelector(".qother").value})),
  }));
}

function wireQuestion(card){
  const submit = card.querySelector('[data-dv="allow"]');
  const pages = [...card.querySelectorAll(".qq, .qrev")];
  const qqs = [...card.querySelectorAll(".qq")];
  const answered = qq => !!qq.querySelector("button.on") || !!qq.querySelector(".qother").value.trim();
  const check = () => {
    submit.disabled = Object.keys(questionAnswers(card)).length < qqs.length;
    qqs.forEach((qq, k) => {
      const box = card.querySelector(`[data-qtab="${k}"] .qbox`);
      if (box) box.innerHTML = answered(qq) ? "&#9746;" : "&#9744;";
      if (qq.dataset.multi) qq.querySelectorAll("button[data-opt] .qn").forEach(m =>
        m.innerHTML = m.parentNode.classList.contains("on") ? "&#9746;" : "&#9744;");
    });
  };
  const show = k => {
    const at = Math.max(0, Math.min(pages.length - 1, k));
    card.dataset.at = at;
    pages.forEach((p, i) => p.hidden = i !== at);
    card.querySelectorAll("[data-qtab]").forEach(t => {
      t.classList.toggle("on", +t.dataset.qtab === at);
      t.setAttribute("aria-selected", +t.dataset.qtab === at ? "true" : "false");
    });
    const rev = card.querySelector(".qrev");
    if (rev && !rev.hidden){
      const got = questionAnswers(card);
      rev.querySelector(".qrl").innerHTML = qqs.map((qq, i) => `<div class="qr" data-qtab="${i}" title="change this answer"><span class="qrq">${
        esc(qq.dataset.q)}</span><span class="${got[qq.dataset.q] ? "qra" : "qra none"}">${
        esc(got[qq.dataset.q] || "not answered")}</span></div>`).join("");
    }
  };
  const step = d => show(+(card.dataset.at || 0) + d);
  // The next page, or with the last one done, the answer goes.
  const onward = () => {
    if (+(card.dataset.at || 0) < pages.length - 1) step(1);
    else if (!submit.disabled) submit.click();
  };
  card.addEventListener("click", e => {
    const t = e.target.closest("[data-qtab]");
    if (t){ show(+t.dataset.qtab); return; }
    const n = e.target.closest("[data-qstep]");
    if (n) step(+n.dataset.qstep);
  });
  card.addEventListener("keydown", e => {
    if (e.ctrlKey || e.altKey || e.metaKey) return;
    const inOther = e.target.classList && e.target.classList.contains("qother");
    if (inOther){
      if (e.key === "Enter"){ e.preventDefault(); onward(); }
      return;
    }
    if (e.key === "ArrowLeft"){ e.preventDefault(); step(-1); }
    else if (e.key === "ArrowRight"){ e.preventDefault(); step(1); }
    else if (/^[1-9]$/.test(e.key)){
      const page = pages[+(card.dataset.at || 0)];
      const opts = page && page.classList.contains("qq") ? page.querySelectorAll("button[data-opt]") : [];
      if (+e.key <= opts.length){ e.preventDefault(); opts[+e.key - 1].click(); }
      else if (+e.key === opts.length + 1){ e.preventDefault(); page.querySelector(".qother").focus(); }
    }
    else if (e.key === "ArrowDown" || e.key === "ArrowUp"){
      const page = pages[+(card.dataset.at || 0)];
      const opts = [...page.querySelectorAll("button[data-opt], .qother, .qr")];
      if (!opts.length) return;
      e.preventDefault();
      const i = opts.indexOf(document.activeElement);
      opts[Math.max(0, Math.min(opts.length - 1, i < 0 ? 0 : i + (e.key === "ArrowDown" ? 1 : -1)))].focus();
    }
  });
  qqs.forEach(qq => {
    const multi = !!qq.dataset.multi, other = qq.querySelector(".qother");
    qq.querySelectorAll("button[data-opt]").forEach(b => b.onclick = () => {
      if (!multi) { qq.querySelectorAll("button.on").forEach(x => x !== b && x.classList.remove("on")); other.value = ""; }
      b.classList.toggle("on");
      check();
      // A single choice is the whole answer: on to the next question, as the
      // terminal's dialog does. (One question alone waits for Submit.)
      if (!multi && b.classList.contains("on") && pages.length > 1) onward();
    });
    other.oninput = () => {
      if (!multi && other.value.trim()) qq.querySelectorAll("button.on").forEach(x => x.classList.remove("on"));
      check();
    };
  });
  // Back where it was before a redraw, choices and all.
  const kept = questionKept.get(card.dataset.di);
  if (kept){
    qqs.forEach((qq, k) => {
      const q = kept.qs[k];
      if (!q) return;
      qq.querySelectorAll("button[data-opt]").forEach(b => b.classList.toggle("on", q.on.includes(b.dataset.opt)));
      qq.querySelector(".qother").value = q.other || "";
    });
  }
  check();
  show(kept ? kept.at : 0);
}

// -- usage meter (the drawing is shared with the matrix) ---------------
async function drawMeter(){
  let u;
  try { u = await get("/api/usage"); } catch (e) { return; }
  const html = meterHtml(u);
  if ($("#meter").dataset.sig !== html){ $("#meter").innerHTML = html; $("#meter").dataset.sig = html; }
}

// A click opens the Meter Details and another folds them; remembered here.
let meterOpen = false;
try { meterOpen = localStorage.getItem("imperatorium.meterOpen") === "1"; } catch (e) {}
function foldMeter(open){
  meterOpen = open;
  $("#meter").classList.toggle("open", open);
  $("#meter").setAttribute("aria-expanded", String(open));
  try { localStorage.setItem("imperatorium.meterOpen", open ? "1" : "0"); } catch (e) {}
}
$("#meter").addEventListener("click", () => foldMeter(!meterOpen));
$("#meter").addEventListener("keydown", e => {
  if (e.key === "Enter" || e.key === " ") { e.preventDefault(); foldMeter(!meterOpen); }
});
foldMeter(meterOpen);

// -- tabs ------------------------------------------------------------------
// `view` is what the main column shows: a session (with tabs), history
// search, or insights. `tab` is which face of the session.
let view = "session", tab = "log";
const PROMPT_PH = $("#prompt").placeholder;

// History and Insights are pop-ups over the window (2026-11-16), as the
// New Session dialog is: the chat stays where it was behind them, and Esc,
// their ✕ or a click outside them goes back to it. They used to take the
// main area's place, with no plain way back. `view` stays "session".
const POPUPS = {history: "#histbox", insights: "#insightbox", project: "#projbox", hooks: "#hookbox"};
function openPopup(which, open){
  const box = $(POPUPS[which]);
  if (!box || box.hidden === !open) return;
  box.hidden = !open;
  document.querySelectorAll("[data-view]").forEach(b => b.classList.toggle("on", b.dataset.view === which && open));
  // Project Changes opens on the selected session's project unless asked
  // for another (openProject).
  if (!open){ if (which === "project") projFor = null; return; }
  closeMenus();
  for (const [w, sel] of Object.entries(POPUPS)) if (w !== which){ $(sel).hidden = true; if (w === "project" && which !== "project") projFor = null; }
  if (which === "history"){ $("#hq").focus(); search(); }
  else if (which === "project"){ $("#pjrefresh").focus(); loadProject(); }
  else if (which === "hooks") drawHookHelp();
  else loadInsights();
}
// Kept for its callers: "session" closes the pop-ups and leaves Fleet;
// "fleet" shows Fleet where the chat goes.
function setView(v){
  for (const w of Object.keys(POPUPS)) openPopup(w, w === v);
  if (v === "fleet") openFleetView(); else leaveFleet();
}

function setTab(t){
  if (t !== "log") closeTask();
  tab = t;
  $("#tabpick").value = t;
  ["log","changes","activity","worktrees"].forEach(n => $("#" + n).hidden = n !== t);
  // Find and prompt jump only mean anything in the conversation.
  $("#findbar").hidden = $("#jump").hidden = t !== "log";
  if (t !== "log" && !$("#findpop").hidden) openFind(false);
  drawCurrent();
  loadProcs();
  refreshTab(true);
}

let tabAt = 0;
async function refreshTab(force){
  if (view !== "session") return;
  if (tab === "log") return loadLog(force);
  // The other faces cost a git or transcript read; every few seconds is plenty.
  const now = Date.now();
  if (!force && now - tabAt < (tab === "activity" ? 3000 : 6000)) return;
  tabAt = now;
  if (tab === "changes") return loadChanges();
  if (tab === "activity") return loadActivity();
  if (tab === "worktrees") return loadWorktrees();
}

function diffHtml(text){
  return text.split("\n").map(l => {
    const c = l.startsWith("diff --git") ? "f" : l.startsWith("@@") ? "h" :
      l.startsWith("+") && !l.startsWith("+++") ? "a" : l.startsWith("-") && !l.startsWith("---") ? "r" : "";
    return c ? `<span class="${c}">${esc(l)}</span>` : esc(l);
  }).join("\n");
}

async function loadChanges(){
  const s = current();
  if (!s){ $("#changes").innerHTML = `<div class="note">select a session</div>`; return; }
  let d;
  try { d = await get("/api/changes?session=" + encodeURIComponent(key(s))); } catch (e) { return; }
  if (key(s) !== selected || tab !== "changes") return;
  if (!d.ok){ $("#changes").innerHTML = `<div class="note">${esc(d.error)}</div>`; return; }
  if (!d.repo){ $("#changes").innerHTML = `<div class="note">not a git repository</div>`; return; }
  const track = [d.upstream, d.ahead ? d.ahead + " ahead" : "", d.behind ? d.behind + " behind" : ""]
    .filter(Boolean).join(" · ");
  const files = d.files.map(f => `<div><span class="code">${esc(f.code)}</span>${esc(f.path)}${
    f.added != null ? ` <span class="add">+${f.added}</span> <span class="del">-${f.removed}</span>` : ""}</div>`).join("");
  const recent = (d.recent || []).map(c => `<div><span class="code">${esc(c.hash)}</span>${esc(c.subject)} <small>${age(c.when)}</small></div>`).join("");
  const sig = JSON.stringify([d.files, d.diff.length, d.branch, d.recent, reviewSig()]);
  if ($("#changes").dataset.sig === sig) return;
  $("#changes").dataset.sig = sig;
  changesView = {key: key(s), d, track, files, recent};
  drawChanges();
}

// Line comments on the diff: click a line to comment, then send them all to
// the session as one prompt. Kept per session in this browser until sent.
let changesView = null, editing = null;
let reviews = {};
try { reviews = JSON.parse(localStorage.getItem("imperatorium.review") || "{}") || {}; } catch (e) { reviews = {}; }
const saveReviews = () => { try { localStorage.setItem("imperatorium.review", JSON.stringify(reviews)); } catch (e) {} };
// Not the text being typed: a poll must not redraw under the comment box.
const reviewSig = () => JSON.stringify([reviews[selected] || [], editing && [editing.file, editing.line, editing.side]]);
const sameSpot = (a, b) => a.file === b.file && a.line === b.line && a.side === b.side;

function drawChanges(){
  const v = changesView;
  if (!v || v.key !== selected) return;
  const d = v.d, list = reviews[v.key] || [];
  const rows = d.diff ? diffRows(d.diff) : [];
  const placed = new Set();
  const body = rows.map((r, i) => {
    const spot = r.file && r.line ? {file: r.file, line: r.line, side: r.side} : null;
    let html = `<div class="dl${r.cls ? " " + r.cls : ""}"${spot ? ` data-r="${i}" title="click to comment on this line"` : ""}>${esc(r.text) || " "}</div>`;
    if (!spot) return html;
    list.forEach((c, j) => {
      if (!sameSpot(c, spot)) return;
      placed.add(j);
      html += `<div class="rv"><span class="rx" data-rx="${j}" title="remove">✕</span>${esc(c.text)}</div>`;
    });
    if (editing && sameSpot(editing, spot))
      html += `<div class="rv"><textarea id="rvtext" placeholder="comment for the session on this line">${esc(editing.text)}</textarea>
        <div class="btns"><button class="sm" id="rvadd">Add comment</button><button class="sm ghost" id="rvcancel">Cancel</button></div></div>`;
    return html;
  }).join("");
  const orphans = list.map((c, j) => [c, j]).filter(([, j]) => !placed.has(j));
  const review = list.length ? `<div id="review"><b>${plural(list.length, "comment")}</b> on this diff${
      orphans.length ? orphans.map(([c, j]) => `<div class="orph">${esc(c.file)}:${c.line} (no longer in the diff): ${
        esc(c.text)} <span class="rx" data-rx="${j}" style="cursor:pointer">✕</span></div>`).join("") : ""}
    <div class="btns"><button class="sm" id="rvsend" ${current() && !current().remote ? "" : "disabled"}>Send to session</button>
      <button class="sm ghost" id="rvbox">Put in prompt box</button><button class="sm ghost" id="rvclear">Clear</button></div></div>` : "";
  $("#changes").innerHTML = `${review}<div class="kv">branch <b>${esc(d.branch || "?")}</b>${v.track ? " · " + esc(v.track) : ""} · ${
      d.files.length} changed file${d.files.length === 1 ? "" : "s"}${rows.length ? " · click a line in the diff to comment on it" : ""}</div>
    <div class="files">${v.files || "<div class='note'>working tree clean</div>"}</div>
    ${d.diff ? `<div class="diff">${body}${d.truncated ? "\n… (truncated)" : ""}</div>` : ""}
    ${v.recent ? `<div class="kv" style="margin-top:12px">recent commits</div><div class="files">${v.recent}</div>` : ""}`;
  const box = $("#rvtext");
  if (box){ box.focus(); box.setSelectionRange(box.value.length, box.value.length);
    box.oninput = () => { editing.text = box.value; };
    box.onkeydown = e => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)){ e.preventDefault(); addReview(); }
                           if (e.key === "Escape"){ e.preventDefault(); editing = null; redrawChanges(); } }; }
}

function redrawChanges(){ $("#changes").dataset.sig = ""; drawChanges(); }

function addReview(){
  if (!editing) return;
  const text = editing.text.trim(), k = changesView && changesView.key;
  if (text && k){
    (reviews[k] = reviews[k] || []).push({file: editing.file, line: editing.line, side: editing.side,
      code: editing.code, text});
    saveReviews();
  }
  editing = null;
  redrawChanges();
}

function clearReviews(k){ delete reviews[k]; saveReviews(); redrawChanges(); }

async function sendReview(toBox){
  const s = current(), list = reviews[selected] || [];
  if (!s || !list.length) return;
  const text = reviewPrompt(list);
  if (toBox){
    const box = $("#prompt");
    box.value = (box.value.trim() ? box.value.replace(/\s*$/, "\n\n") : "") + text;
    noteDraft(); clearReviews(key(s)); box.focus();
    return;
  }
  say("sending review…", key(s));
  const r = await post("/api/dispatch", {session_id: key(s), project: s.cwd, prompt: text, confirm: true, mode: "auto"});
  say(r.ok ? outcome(r) : (r.error || "refused"), key(s));
  if (r.ok) clearReviews(key(s));
}

$("#changes").addEventListener("click", e => {
  const t = e.target;
  if (t.closest("#rvadd")) return addReview();
  if (t.closest("#rvcancel")){ editing = null; return redrawChanges(); }
  if (t.closest("#rvsend")) return sendReview(false);
  if (t.closest("#rvbox")) return sendReview(true);
  if (t.closest("#rvclear")) return clearReviews(selected);
  const x = t.closest("[data-rx]");
  if (x){ (reviews[selected] || []).splice(+x.dataset.rx, 1); saveReviews(); return redrawChanges(); }
  const row = t.closest(".dl[data-r]");
  if (!row || !changesView || getSelection().toString()) return;
  const r = diffRows(changesView.d.diff)[+row.dataset.r];
  if (!r) return;
  editing = {file: r.file, line: r.line, side: r.side, code: r.text.slice(1), text: ""};
  redrawChanges();
});

async function loadActivity(){
  const s = current();
  if (!s){ $("#activity").innerHTML = `<div class="note">select a session</div>`; $("#activity").dataset.sig = ""; return; }
  let d;
  try { d = await get("/api/activity?session=" + encodeURIComponent(key(s))); } catch (e) { return; }
  if (key(s) !== selected || tab !== "activity") return;
  if (!d.ok){ $("#activity").innerHTML = `<div class="note">${esc(d.error)}</div>`; $("#activity").dataset.sig = ""; return; }
  const t = d.tally;
  const tools = Object.entries(t.by_tool).sort((a,b) => b[1][0] - a[1][0])
    .map(([k,[n,e]]) => `${esc(k)} ${n}${e ? ` <span style="color:var(--fail)">(${e} failed)</span>` : ""}`).join(" · ");
  const src = d.source === "hooks" ? "live, from tool hooks" :
    d.source === "conversation" ? "from the conversation (no times - install the tool hooks for live timing)" : (d.note || "");
  const rows = d.timeline.slice().reverse().map(e => {
    const mark = e.ok === true ? `<span class="ok">✓</span>` : e.ok === false ? `<span class="bad">✗</span>` :
      `<span class="run">…</span>`;
    const when = e.start ? clock(e.start) : "";
    const dur = e.start && e.end ? " " + Math.max(0, e.end - e.start).toFixed(1) + "s" : "";
    return `<div class="ev"><span class="t">${when}</span>${mark}<span class="x"><b>${esc(e.tool)}</b> ${esc(e.summary)}${
      esc(dur)}${e.error ? `<span class="err">${esc(e.error)}</span>` : ""}</span></div>`;
  }).join("");
  const html = `<div class="kv">${t.calls} tool calls · ${t.errors} failed · ${esc(src)}</div>
    <div class="kv">${tools}</div>${rows || `<div class="note">no tool calls yet</div>`}`;
  if ($("#activity").dataset.sig !== html){ $("#activity").innerHTML = html; $("#activity").dataset.sig = html; }
}

async function loadWorktrees(){
  const s = current();
  if (!s || s.remote){ $("#worktrees").innerHTML = `<div class="note">${s ? "only for sessions on this machine" : "select a session"}</div>`; return; }
  let d;
  try { d = await get("/api/worktrees?project=" + encodeURIComponent(s.cwd || "")); } catch (e) { return; }
  if (key(s) !== selected || tab !== "worktrees") return;
  if (!d.ok){ $("#worktrees").innerHTML = `<div class="note">${esc(d.error)}</div>`; return; }
  if (!d.repo){ $("#worktrees").innerHTML = `<div class="note">not a git repository</div>`; return; }
  const cards = d.worktrees.map(w => `<div class="card">
      <h3>${esc(w.branch || "(detached)")}</h3>
      <small>${esc(w.path)}</small>
      <div class="kv" style="margin:4px 0 0">${w.agent ? "background session" : "made by hand - not managed here"} · ${
        w.ahead || 0} ahead of ${esc(w.base || d.base)}${w.base_recorded ? " (made from it)" : ""}${w.behind ? " · " + w.behind + " behind" : ""}${w.dirty ? " · <b>uncommitted changes</b>" : ""}${
        w.subject ? " · last: " + esc(w.subject) : ""}</div>
      <div class="btns"><button class="sm ghost" data-wd="${esc(w.path)}">Diff</button>${w.agent ? `
        <button class="sm" data-wa="merge" data-wp="${esc(w.path)}" ${w.ahead && !w.dirty ? "" : "disabled"}>Merge into ${esc(w.base || d.base)}</button>
        <button class="sm ghost" data-wa="pr" data-wp="${esc(w.path)}" ${w.dirty ? "disabled" : ""}>Push + open PR</button>
        <button class="sm danger" data-wa="discard" data-wp="${esc(w.path)}">Discard</button>` : ""}</div>
      <div class="wdiff" data-for="${esc(w.path)}"></div></div>`).join("");
  const sig = JSON.stringify(d.worktrees);
  if ($("#worktrees").dataset.sig === sig) return;
  $("#worktrees").dataset.sig = sig;
  $("#worktrees").innerHTML = `<div class="kv">worktrees of ${esc(d.main)} (base ${esc(d.base)})</div>${
    cards || `<div class="note">no linked worktrees - background sessions leave theirs under .claude/worktrees</div>`}`;
  document.querySelectorAll("[data-wd]").forEach(b => b.onclick = async () => {
    const r = await get("/api/worktree_diff?project=" + encodeURIComponent(s.cwd) + "&path=" + encodeURIComponent(b.dataset.wd));
    const box = [...document.querySelectorAll(".wdiff")].find(x => x.dataset.for === b.dataset.wd);
    box.innerHTML = r.ok ? `<pre class="files">${esc(r.log)}</pre><div class="diff">${diffHtml(r.diff)}${r.truncated ? "\n… (truncated)" : ""}</div>`
      : `<div class="note">${esc(r.error)}</div>`;
  });
  document.querySelectorAll("[data-wa]").forEach(b => b.onclick = () => finishWorktree(s, b.dataset.wa, b.dataset.wp));
}

async function finishWorktree(s, action, path, force){
  const say = {merge: "Merge this branch into the main checkout?",
               pr: "Push this branch to origin and open a pull request?",
               discard: "Remove this worktree and delete its branch?"}[action];
  if (!force && !confirm(say)) return;
  const r = await post("/api/worktree", {project: s.cwd, path, action, force: !!force});
  if (!r.ok && r.needs_force){
    if (confirm(r.error + ". Discard anyway?")) return finishWorktree(s, action, path, true);
    return;
  }
  $("#say").textContent = r.ok ? (action === "pr" && r.url ? "opened " + r.url : action + " done: " + (r.branch || ""))
    : (r.error || "refused");
  $("#worktrees").dataset.sig = ""; loadWorktrees();
}

// -- Project Changes (2026-11-17) ---------------------------------------------
// One pop-up per project: the main checkout, each worktree and each branch
// ahead of its base without one, the sessions working in each, and first the
// files more than one of them touches - where a merge meets another's work.
// Read when opened and on Refresh, never polled.
let projFor = null, projData = null;

// Pure: a folder as the window compares them.
const normDir = p => String(p || "").replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();

// Pure: the projects there are sessions on this machine in, by name, each
// with a folder to ask about - the main checkout's when a session is in it.
function projectChoices(list){
  const by = new Map();
  for (const s of list){
    if (s.remote || !s.cwd) continue;
    const name = s.project || s.cwd.split(/[\\/]/).filter(Boolean).pop();
    const had = by.get(name);
    if (!had || (had.worktree && !s.worktree)) by.set(name, {name, cwd: s.cwd, worktree: !!s.worktree});
  }
  return [...by.values()].map(({name, cwd}) => ({name, cwd})).sort((a, b) => a.name.localeCompare(b.name));
}

// Pure: for each line of work, the sessions working in it - each session in
// the line whose folder is, or holds, its own, the deepest such.
function lineSessions(lines, list){
  const out = lines.map(() => []);
  for (const s of list){
    if (s.remote || !s.cwd) continue;
    const c = normDir(s.cwd);
    let best = -1, depth = -1;
    lines.forEach((l, i) => {
      const p = normDir(l.path);
      if (p && (c === p || c.startsWith(p + "/")) && p.length > depth){ best = i; depth = p.length; }
    });
    if (best >= 0) out[best].push(s);
  }
  return out;
}

// Pure: what a line is called.
function lineName(l){
  const leaf = String(l.path || "").split(/[\\/]/).filter(Boolean).pop() || "";
  return l.kind === "main" ? "main checkout" + (l.branch ? " · " + l.branch : "")
    : l.kind === "worktree" ? (l.branch || "(detached)") + " · worktree " + leaf
    : l.branch + " · branch, no worktree";
}

function openProject(cwd){
  if (cwd) projFor = cwd;
  openPopup("project", true);
}

async function loadProject(){
  const choices = projectChoices(sessions);
  const s = current();
  if (!projFor) projFor = s && !s.remote && s.cwd ? s.cwd : (choices[0] || {}).cwd || null;
  const pick = $("#pjpick");
  const here = choices.find(c => normDir(c.cwd) === normDir(projFor));
  pick.innerHTML = (here ? "" : `<option value="${esc(projFor || "")}">${esc(projFor || "no project")}</option>`) +
    choices.map(c => `<option value="${esc(c.cwd)}">${esc(c.name)}</option>`).join("");
  pick.value = projFor || "";
  const pane = $("#projpane");
  if (!projFor){ pane.innerHTML = `<div class="note">no session on this machine to take a project from</div>`; return; }
  pane.innerHTML = `<div class="note">reading every worktree and branch…</div>`;
  const asked = projFor;
  let d;
  try { d = await get("/api/project_changes?project=" + encodeURIComponent(asked)); } catch (e) { d = {ok: false, error: "the dispatcher did not answer"}; }
  if (asked !== projFor || $("#projbox").hidden) return;
  projData = d;
  drawProject();
}

function drawProject(){
  const d = projData, pane = $("#projpane");
  if (!d.ok){ pane.innerHTML = `<div class="note">${esc(d.error || "could not read it")}</div>`; return; }
  if (!d.repo){ pane.innerHTML = `<div class="note">not a git repository</div>`; return; }
  const lines = d.lines || [], who = lineSessions(lines, sessions);
  const shared = new Set((d.overlaps || []).map(o => o.path));
  const HOW = {committed: "committed", uncommitted: "not committed", both: "committed and not"};
  const ov = (d.overlaps || []).map(o => `<details class="pjov" data-file="${esc(o.path)}"><summary><b>${esc(o.path)}</b> <span class="kv">${
      o.in.map(x => esc(lineName(lines[x.line])) + " (" + HOW[x.how] + ")").join(" · ")}</span></summary><div class="pjdiffs"></div></details>`).join("");
  const cards = lines.map((l, i) => {
    const mine = who[i], pr = mine.map(s => s.pr).find(Boolean);
    const files = [...new Set([...(l.committed || []), ...(l.uncommitted || [])])].sort();
    const un = new Set(l.uncommitted || []);
    const stats = [
      un.size ? `<b>${un.size} not committed</b>` : "nothing uncommitted",
      l.kind === "main" && !l.base ? (l.upstream ? `${l.ahead || 0} ahead, ${l.behind || 0} behind ${esc(l.upstream)}` : "")
        : `${l.ahead || 0} ahead${l.behind ? ", " + l.behind + " behind" : ""} of ${esc(l.base || d.base || "?")}`,
      l.subject ? "last: " + esc(l.subject) : "",
    ].filter(Boolean).join(" · ");
    // Its commits are all in another line: branched from it, or merged in.
    const held = (l.held_by || []).map(k => lines[k]).filter(Boolean);
    const heldNote = held.length && (l.committed || []).length
      ? `<div class="kv">its commits are all in ${held.map(h => esc(lineName(h))).join(", ")}</div>` : "";
    const chips = mine.map(s => `<button class="sm ghost pjs" data-pjs="${esc(key(s))}" title="open this session">${esc(nameOf(s))}</button>`).join("");
    const acts = l.kind === "worktree" && l.agent ? `<div class="btns">
        <button class="sm" data-pja="merge" data-pjp="${esc(l.path)}" ${l.ahead && !un.size ? "" : "disabled"}>Merge into ${esc(l.base || d.base)}</button>
        <button class="sm ghost" data-pja="pr" data-pjp="${esc(l.path)}" ${un.size ? "disabled" : ""}>Push + open PR</button>
        <button class="sm danger" data-pja="discard" data-pjp="${esc(l.path)}">Discard</button></div>` : "";
    const list = files.map(f => `<div class="pjf${shared.has(f) ? " shared" : ""}" data-pjf="${esc(f)}" data-pjl="${i}" title="show its diff">${
        un.has(f) ? "<span class=code>~</span>" : "<span class=code>+</span>"}${esc(f)}${shared.has(f) ? " <span class=kv>· also elsewhere</span>" : ""}</div><div class="pjfd" hidden></div>`).join("");
    return `<div class="card pjline${shared.size && files.some(f => shared.has(f)) ? " meets" : ""}">
      <h3>${esc(lineName(l))}${prBadge(pr)}</h3>
      ${l.path ? `<small>${esc(l.path)}</small>` : ""}
      <div class="kv" style="margin:4px 0 0">${stats}${l.error ? " · <b>" + esc(l.error) + "</b>" : ""}</div>
      ${heldNote}
      ${chips ? `<div class="pjwho">${chips}</div>` : `<div class="kv">no session working here</div>`}
      ${files.length ? `<details class="pjfiles"><summary>${files.length} file${files.length > 1 ? "s" : ""}${l.truncated ? " (list cut short)" : ""}</summary><div class="files">${list}</div></details>` : ""}
      ${acts}</div>`;
  }).join("");
  const more = d.unlisted_branches ? `<div class="note">${d.unlisted_branches} older branches not looked at</div>` : "";
  pane.innerHTML = `<div class="kv">${esc(d.main)} · base ${esc(d.base || "none")} · ${lines.length} line${lines.length === 1 ? "" : "s"} of work</div>
    <div class="card"><h3>Files in more than one line of work</h3>${ov ||
      `<div class="kv">none - no two lines of work touch the same file</div>`}</div>${cards}${more}`;
  pane.querySelectorAll("[data-pjs]").forEach(b => b.onclick = () => { openPopup("project", false); pick(b.dataset.pjs); });
  pane.querySelectorAll(".pjov").forEach(el => el.addEventListener("toggle", () => {
    if (!el.open || el.dataset.read) return;
    el.dataset.read = "1";
    const o = d.overlaps.find(x => x.path === el.dataset.file);
    const box = el.querySelector(".pjdiffs");
    box.innerHTML = o.in.map(x => `<div class="pjdh">${esc(lineName(lines[x.line]))}</div><div class="pjd" data-i="${x.line}">reading…</div>`).join("");
    o.in.forEach(x => fileDiff(lines[x.line], o.path).then(h => {
      const slot = box.querySelector(`.pjd[data-i="${x.line}"]`);
      if (slot) slot.innerHTML = h;
    }));
  }));
  pane.querySelectorAll("[data-pjf]").forEach(el => el.onclick = async () => {
    const box = el.nextElementSibling;
    box.hidden = !box.hidden;
    if (!box.hidden && !box.innerHTML) box.innerHTML = await fileDiff(lines[+el.dataset.pjl], el.dataset.pjf);
  });
  pane.querySelectorAll("[data-pja]").forEach(b => b.onclick = async () => {
    await finishWorktree({cwd: d.main}, b.dataset.pja, b.dataset.pjp);
    loadProject();
  });
}

// One file's diff in one line of work, as HTML.
async function fileDiff(l, file){
  const at = l.kind === "branch" ? l.branch : l.path;
  let r;
  try {
    r = await get("/api/project_file_diff?project=" + encodeURIComponent(projFor) + "&kind=" + encodeURIComponent(l.kind) +
      "&at=" + encodeURIComponent(at) + "&file=" + encodeURIComponent(file));
  } catch (e) { r = {ok: false, error: "the dispatcher did not answer"}; }
  if (!r.ok) return `<div class="note">${esc(r.error || "could not read it")}</div>`;
  if (r.untracked) return `<div class="note">a new file, not added to git yet</div>`;
  return r.diff ? `<div class="diff">${diffHtml(r.diff)}${r.truncated ? "\n… (truncated)" : ""}</div>` : `<div class="note">no difference</div>`;
}

// The tile's ✕: ends the session, and with it its terminal tab.
async function closeSession(k){
  const s = sessions.find(x => key(x) === k);
  if (!s || !confirm("Close " + nameOf(s) + " and its window? The conversation stays resumable." +
      (s.status === "busy" ? "\n\nIt is working right now - its turn will be interrupted." : ""))) return;
  $("#say").textContent = "closing " + nameOf(s) + "…";
  const r = await post("/api/close", {session_id: k});
  $("#say").textContent = !r.ok ? "could not close " + nameOf(s) + ": " + (r.error || "refused")
    : r.how === "killed" ? nameOf(s) + " had to be ended forcibly, so its tab is still open"
    : r.how === "exit" && r.tab_closed === false ? nameOf(s) + " has exited; its tab is still open"
    : "closed " + nameOf(s);
}

// A tile's ✕ when nothing is running: no process to end, so the dispatcher
// only takes it off the list until it starts again.
async function dismissSession(k){
  const s = sessions.find(x => key(x) === k);
  if (!s || !confirm("Remove " + nameOf(s) + " from the list? Nothing is running, so there is " +
      "nothing to close. The conversation stays resumable.")) return;
  const r = await post("/api/dismiss", {session_id: k});
  $("#say").textContent = r.ok ? "removed " + nameOf(s)
    : "could not remove " + nameOf(s) + ": " + (r.error || "refused");
  if (!r.ok) return;
  // The dispatcher also dropped it from the archive, so the prefs follow.
  sessions = sessions.filter(x => key(x) !== k);
  await loadPrefs();
  drawSessions();
}

// -- resume, fork --------------------------------------------------------
async function act(what){
  const s = current();
  if (!s) return;
  if (what === "copy"){
    try { await navigator.clipboard.writeText(s.resume_command + (s.cwd ? "" : "")); $("#say").textContent = "copied: " + s.resume_command; }
    catch (e) { $("#say").textContent = s.resume_command; }
    return;
  }
  if (what === "show"){
    const r = await post("/api/focus", {session_id: key(s)});
    say(r.ok ? (r.note || "raised its window") : (r.error || "refused"), key(s));
    return;
  }
  if (what === "new" || what === "new-close"){
    const close = what === "new-close";
    if (close && !confirm("Open a new session in " + (s.cwd || "this project") + " and close " + nameOf(s) + "?" +
        (s.status === "busy" ? "\n\nIt is working right now - its turn will be interrupted." : ""))) return;
    say(close ? "opening a new session and closing this one…" : "opening a new session…", key(s));
    const r = await post("/api/new", {session_id: key(s), cwd: s.cwd || null, close});
    if (!r.ok){ say(r.error || "refused", key(s)); return; }
    follow(r.session_id);
    const c = r.closed;
    say("opened a new session in " + (s.cwd || "its project") + " - selecting it once it starts" +
      (!c ? "" : !c.ok ? "; could not close the old one: " + (c.error || "refused")
        : c.how === "killed" ? "; the old one had to be ended forcibly, so its tab is still open"
        : c.tab_closed === false && c.how !== "gone" ? "; the old one has exited, its tab is still open" : "; the old one is closed"), key(s));
    return;
  }
  const [kind, where] = what.split("-");
  const r = await post("/api/resume", {session_id: key(s), where, fork: kind === "fork",
    prompt: $("#prompt").value.trim() || null});
  say(r.ok ? (r.where === "background" ? kind + "ed in the background · " + (r.id || "")
    : "opened a new tab: " + r.command) : (r.error || "refused"), key(s));
  if (r.ok && where === "background") clearPrompt(key(s));
}

// A session launched from here is selected once it shows up on the board -
// it takes a few seconds to start. Given up after a minute, so a launch that
// never came up does not yank the view away later.
let following = null;
function follow(id){ following = id ? {id, until: Date.now() + 60000} : null; }
function catchFollowed(){
  if (!following) return;
  if (Date.now() > following.until){ following = null; return; }
  const s = sessions.find(x => key(x) === following.id);
  if (!s) return;
  following = null;
  select(key(s));
}

// Any other new session - started from a terminal, the project picker, a
// fork - is switched to as well, unless you are mid-sentence in the prompt
// box, where a switch would leave your next keystrokes aimed at the wrong
// session; then it is only announced.
function catchStarted(k){
  if (!k || k === selected) return;
  const box = $("#prompt");
  if (document.activeElement === box && box.value.trim()){
    const s = sessions.find(x => key(x) === k);
    $("#say").textContent = "new session " + (s ? nameOf(s) : k) + " started - not switched while you type";
    return;
  }
  pick(k);
}

// A session cleared with /clear: its new conversation is opened and the
// prompt box takes the keys, ready for the first prompt. One you were
// looking at always follows; any other only when you are not mid-sentence,
// as catchStarted.
function catchCleared({from, to}){
  if (to === selected) return;
  const box = $("#prompt");
  if (from !== selected && document.activeElement === box && box.value.trim()){
    const s = sessions.find(x => key(x) === to);
    $("#say").textContent = (s ? nameOf(s) : to) + " was cleared - not switched while you type";
    return;
  }
  pick(to);
  if (view === "session" && !box.disabled) box.focus();
}

function select(id){
  selected = id;
  if (view !== "session") setView("session");
  showCached(); drawSessions(); refreshTab(true); loadStatus(true); loadProcs();
}

// -- queue -----------------------------------------------------------------
async function queue(){
  if (view === "fleet") return;
  const s = current();
  const text = $("#prompt").value.trim();
  if (!s || !text) return;
  const r = await post("/api/queue", {session_id: key(s), prompt: text});
  if (r.ok){ clearPrompt(key(s)); say(r.waiting ?
      "queued - delivered when its current turn ends (" + r.waiting + " waiting)" : "delivered now - it was idle", key(s)); }
  else say(r.error || "refused", key(s));
  queuedSig = "";
}

let queuedSig = "";
async function drawQueued(s){
  if (!s || !s.queued){ $("#queued").innerHTML = ""; queuedSig = ""; return; }
  let d;
  try { d = await get("/api/queue"); } catch (e) { return; }
  const items = (d.queue || {})[pkey(s)] || [];
  const sig = JSON.stringify(items);
  if (sig === queuedSig) return;
  queuedSig = sig;
  $("#queued").innerHTML = items.map(i => `<div class="qi${i.error ? " err" : ""}"><span title="${esc(i.prompt)}">queued: ${
    esc(i.prompt)}${i.error ? " - last try: " + esc(i.error) : ""}</span><button class="tiny" data-uq="${esc(i.id)}">cancel</button></div>`).join("");
  document.querySelectorAll("[data-uq]").forEach(b => b.onclick = async () => {
    await post("/api/unqueue", {session: pkey(s), id: b.dataset.uq}); queuedSig = ""; });
}

// -- history search --------------------------------------------------------
let searchSeq = 0;
async function search(){
  const q = $("#hq").value.trim();
  const seq = ++searchSeq;
  $("#hits").innerHTML = `<div class="note">searching…</div>`;
  let d;
  try { d = await get("/api/history?q=" + encodeURIComponent(q)); } catch (e) { return; }
  if (seq !== searchSeq) return;
  if (!d.ok){ $("#hits").innerHTML = `<div class="note">${esc(d.error)}</div>`; return; }
  $("#hits").innerHTML = (d.results.map(r => `<div class="hit">
      <div class="tt">${esc(r.title || r.first_prompt || r.session_id)}</div>
      <div class="sn">${esc(folder(r.cwd))} · ${esc(new Date(r.modified*1000).toLocaleString())}${
        r.branch ? " · " + esc(r.branch) : ""}${r.live ? " · <b>live</b>" : ""}${r.where ? " · matched in " + esc(r.where) : ""}</div>
      ${r.snippet ? `<div class="sn">${esc(r.snippet)}</div>` : ""}
      <div class="btns">${r.live ? `<button class="sm" data-open="${esc(r.session_id)}">Open</button>` :
        `<button class="sm" data-rs="${esc(r.session_id)}" data-w="window">Resume</button>
         <button class="sm ghost" data-rs="${esc(r.session_id)}" data-w="background">Resume in bg</button>`}
        <button class="sm ghost" data-rs="${esc(r.session_id)}" data-w="window" data-fork="1">Fork</button>
        <button class="sm ghost" data-cp="claude --resume ${esc(r.session_id)}">Copy cmd</button></div>
    </div>`).join("") || `<div class="note">nothing found</div>`) +
    (d.complete ? "" : `<div class="note">still indexing older sessions - search again for complete results</div>`);
  document.querySelectorAll("[data-open]").forEach(b => b.onclick = () => {
    openPopup("history", false); pick(b.dataset.open); });
  document.querySelectorAll("[data-rs]").forEach(b => b.onclick = async () => {
    const r = await post("/api/resume", {session_id: b.dataset.rs, where: b.dataset.w, fork: !!b.dataset.fork});
    b.parentNode.insertAdjacentHTML("beforeend", `<small>${esc(r.ok ? (r.id ? "started " + r.id : "opened") : r.error)}</small>`);
  });
  document.querySelectorAll("[data-cp]").forEach(b => b.onclick = async () => {
    try { await navigator.clipboard.writeText(b.dataset.cp); b.textContent = "copied"; } catch (e) { b.textContent = b.dataset.cp; } });
}

// -- insights: cost and failures --------------------------------------
async function loadInsights(){
  $("#errs").innerHTML = `<div class="note">reading recent sessions…</div>`;
  loadCosts();
  let d;
  try { d = await get("/api/errors?days=" + ($("#days").value || 7)); } catch (e) { return; }
  if (!d.ok){ $("#errs").innerHTML = `<div class="note">${esc(d.error)}</div>`; return; }
  const rate = (e, n) => n ? (100 * e / n).toFixed(1) + "%" : "-";
  const cats = Object.entries(d.by_category).sort((a,b) => b[1] - a[1]);
  const tools = Object.entries(d.by_tool).filter(([,v]) => v[1]).sort((a,b) => b[1][1] - a[1][1]);
  const projs = Object.entries(d.by_project).filter(([,v]) => v.calls).sort((a,b) => b[1].errors - a[1].errors);
  $("#errs").innerHTML = `<div class="kv">${d.sessions} sessions · ${d.calls} tool calls · ${d.errors} failed (${rate(d.errors, d.calls)})</div>
    <table class="t"><tr><th>why it failed</th><th>count</th><th>example</th></tr>${cats.map(([c,n]) =>
      `<tr><td>${esc(c)}</td><td class="n">${n}</td><td><small>${esc((d.examples[c] || "").slice(0,140))}</small></td></tr>`).join("")}</table>
    <table class="t"><tr><th>tool</th><th>failed</th><th>calls</th><th>rate</th></tr>${tools.map(([t,[n,e]]) =>
      `<tr><td>${esc(t)}</td><td class="n">${e}</td><td class="n">${n}</td><td class="n">${rate(e,n)}</td></tr>`).join("")}</table>
    <table class="t"><tr><th>project</th><th>sessions</th><th>failed</th><th>calls</th><th>rate</th></tr>${projs.map(([p,v]) =>
      `<tr><td>${esc(p)}</td><td class="n">${v.sessions}</td><td class="n">${v.errors}</td><td class="n">${v.calls}</td><td class="n">${rate(v.errors,v.calls)}</td></tr>`).join("")}</table>`;
}

// What a successful dispatch actually did. "inject" alone read as "done", but
// it only means the session's inbox took the message: Claude Code's own
// inbound policy still decides, and on the Pi it once held one out of sight.
function outcome(r){
  const where = r.host ? " on " + r.host : "";
  const id = r.bg_id ? " · " + r.bg_id : "";
  if (r.action === "inject")
    return "delivered to the inbox of " + (r.session_name || "the session") +
           where + " - it runs once the session accepts it";
  if (r.action === "remote") return "started in the background" + where + id;
  if (r.action === "background") return "started in the background" + id +
    (r.branch ? " · worktree " + r.branch + " from " + r.base : "");
  if (r.action === "launch") return "opened a new session window";
  if (r.action === "typed")
    return "typed into " + (r.session_name || "the session") + "'s prompt" +
           (r.cleaned ? " (made one line)" : "");
  if (r.action === "command")
    return "typed " + r.command + " into " + (r.session_name || "the session") + "'s terminal";
  return r.action + id;
}

async function send(mode){
  if (view === "fleet"){ if (mode !== "background") askFleet(); return; }
  const s = sessions.find(x => key(x) === selected);
  const text = $("#prompt").value.trim();
  if (!s || !text || busy) return;
  busy = true; say("sending…", key(s));
  // A background send starts another session; only this one's goes in its chat.
  const p = mode === "background" ? null : addPending(key(s), text);
  let r;
  try {
    r = await post("/api/dispatch", {
      session_id: key(s), project: s.cwd, prompt: text, confirm: true, mode: mode || "auto"
    });
  } catch (e) { r = {ok: false, error: "the dispatcher did not answer"}; }
  busy = false;
  if (p) settleSend(key(s), p, r, r.action === "typed" ? "typed at its prompt" : "sent");
  if (r.ok){ clearPrompt(key(s)); say(outcome(r), key(s)); }
  else say(r.error || "refused", key(s));
  lastSig = ""; loadLog(true);
}

// Typed at the session's prompt, so it counts as you there - what a sent
// message cannot (see Control.send_typed).
async function sendTyped(){
  closeMenus();
  if (view === "fleet") return;
  const s = sessions.find(x => key(x) === selected);
  const text = $("#prompt").value.trim();
  if (!s || !text || busy) return;
  busy = true; say("typing…", key(s));
  const p = addPending(key(s), text);
  let r;
  try { r = await post("/api/typed", {session_id: key(s), prompt: text}); }
  catch (e) { r = {ok: false, error: "the dispatcher did not answer"}; }
  busy = false;
  settleSend(key(s), p, r, "typed at its prompt");
  if (r.ok){ clearPrompt(key(s)); say(outcome(r), key(s)); }
  else say(r.error || "refused", key(s));
  lastSig = ""; loadLog(true);
}

// -- dictation ----------------------------------------------------------
// The mic lives here, beside the box it fills. The browser records (localhost
// is a secure context) and the dispatcher transcribes on the model it already
// holds - loading costs seconds, using it costs hundredths.
let rec = null, chunks = [];

// The recording as a 16 kHz mono WAV: the browser decodes its own
// MediaRecorder output (WebM/Opus) and resamples it, so the dictation
// process only ever reads WAV - whisper.cpp in imperatorium-voice has no
// Opus decoder, and faster-whisper reads WAV as well. Anything the browser
// cannot decode goes as it was recorded.
async function wav16k(blob){
  try {
    const ctx = new AudioContext();
    const decoded = await ctx.decodeAudioData(await blob.arrayBuffer());
    ctx.close();
    const n = Math.max(1, Math.ceil(decoded.duration * 16000));
    const off = new OfflineAudioContext(1, n, 16000);
    const src = off.createBufferSource();
    src.buffer = decoded; src.connect(off.destination); src.start();
    const pcm = (await off.startRendering()).getChannelData(0);
    const out = new DataView(new ArrayBuffer(44 + pcm.length * 2));
    const tag = (o, s) => { for (let i = 0; i < 4; i++) out.setUint8(o + i, s.charCodeAt(i)); };
    tag(0, "RIFF"); out.setUint32(4, 36 + pcm.length * 2, true); tag(8, "WAVE");
    tag(12, "fmt "); out.setUint32(16, 16, true); out.setUint16(20, 1, true); out.setUint16(22, 1, true);
    out.setUint32(24, 16000, true); out.setUint32(28, 32000, true); out.setUint16(32, 2, true);
    out.setUint16(34, 16, true); tag(36, "data"); out.setUint32(40, pcm.length * 2, true);
    for (let i = 0; i < pcm.length; i++){
      const s = Math.max(-1, Math.min(1, pcm[i]));
      out.setInt16(44 + i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
    }
    return new Blob([out.buffer], {type: "audio/wav"});
  } catch (e) { return blob; }
}

// " on CPU (small)" when the dictation process has fallen back to the CPU,
// which is why a dictation is slow there; "" on a GPU or before it is known.
const onCpu = r => r && r.device === "cpu" ? " on CPU" + (r.model ? " (" + r.model + ")" : "") : "";

// F9 held (voice.shortcut = hold, the default) records until it is let go;
// toggle records from one press to the next. A release that comes while
// the microphone is still opening stops the recording as soon as it starts.
let dictMode = "hold", opening = false, stopWanted = false;
async function loadDictMode(){
  try {
    const s = await get("/api/settings");
    const f = (s.fields || []).find(x => x.key === "voice.shortcut");
    if (f && f.value) dictMode = f.value;
  } catch (e) {}
}
function stopDictation(){
  if (rec && rec.state === "recording") rec.stop();
  else if (opening) stopWanted = true;
}

async function dictate(){
  if (rec && rec.state === "recording"){ rec.stop(); return; }
  if (opening) return;
  let stream;
  opening = true; stopWanted = false;
  try { stream = await navigator.mediaDevices.getUserMedia({audio:true}); }
  catch (e) { opening = false; $("#say").textContent = "no microphone: " + e.name; return; }
  opening = false;
  chunks = [];
  let cpuAt = null;  // the warm-up's answer: where the model runs
  // A recording stops itself at voice.max_seconds (120 s until the warm-up
  // says otherwise): the upload stays a few MB of WAV, well under the
  // dispatcher's 32 MiB cap, and what was said is transcribed, not lost.
  const started = Date.now();
  let capTimer = null, capped = 0;
  const cap = secs => {
    clearTimeout(capTimer);
    capTimer = setTimeout(() => {
      if (rec && rec.state === "recording"){ capped = secs; rec.stop(); }
    }, Math.max(0, secs * 1000 - (Date.now() - started)));
  };
  cap(120);
  rec = new MediaRecorder(stream);
  rec.ondataavailable = e => { if (e.data.size) chunks.push(e.data); };
  rec.onstop = async () => {
    clearTimeout(capTimer);
    stream.getTracks().forEach(t => t.stop());
    $("#mic").classList.remove("rec");
    $("#mic").textContent = "\u{1F3A4} Dictate";
    if (!chunks.length) return;
    $("#say").textContent = "transcribing" + onCpu(cpuAt) + "\u2026";
    const r = await fetch("/api/transcribe", {method:"POST", cache:"no-store",
      headers:{"Content-Type":"application/octet-stream"},
      body: await wav16k(new Blob(chunks))});
    const data = await r.json();
    if (!data.ok){ $("#say").textContent = data.error || "transcription failed"; return; }
    if (!data.text){ $("#say").textContent = "nothing heard"; return; }
    const box = $("#prompt");
    box.value = (box.value.trim() ? box.value.replace(/\s*$/, " ") : "") + data.text;
    box.focus();
    noteDraft();
    $("#say").textContent = "dictated " + data.text.length + " chars" + onCpu(data) +
      (capped ? " - stopped at " + capped + " s (voice.max_seconds)" : "");
  };
  rec.start();
  // The model lives in its own process, ended when idle; start it now so it
  // is built while you speak rather than after.
  post("/api/voice/warm", {}).then(r => {
    cpuAt = r;
    if (r && r.max_seconds > 0) cap(r.max_seconds);
    if (rec && rec.state === "recording" && onCpu(r)) $("#say").textContent = "listening" + onCpu(r) + "\u2026";
  }).catch(() => {});
  $("#mic").classList.add("rec");
  $("#mic").textContent = "\u25A0 Stop";
  $("#say").textContent = dictMode === "hold" && f9Held ? "listening - let go of F9 to stop\u2026" : "listening\u2026";
  if (stopWanted){ stopWanted = false; rec.stop(); }
}

// -- the New Session dialog --------------------------------------------------
// From the sidebar's ☰ menu (2026-11-16, in place of the picker that opened
// in the sidebar): any project in the catalogue - including one nobody is
// working in now, which the sidebar would not otherwise reach - and the
// permission mode the session starts in. A project on this machine opens in
// a new terminal tab (/api/new) and is selected here once it starts; one on
// a remote host has no terminal here, so it starts as a background session
// there (/api/dispatch), in the mode Settings gives background sessions.
let projects = [], psel = 0;
const MODE_KEY = "imperatorium.newMode";

// Pure: the catalogue entries matching `q`, at most 40.
function projectHits(list, q){
  q = String(q || "").trim().toLowerCase();
  const norm = s => String(s).toLowerCase().replace(/[-_\/]+/g," ");
  const nq = norm(q);
  return list.filter(p => !q || norm(p.name).includes(nq) || norm(p.path || "").includes(nq) ||
                          norm(p.name).replace(/ /g,"").includes(q.replace(/ /g,""))).slice(0, 40);
}
function drawProjects(){
  const hit = projectHits(projects, $("#find").value);
  psel = Math.min(psel, Math.max(0, hit.length - 1));
  $("#projects").innerHTML = hit.map((p,i) =>
    `<div class="p${i===psel?" on":""}" role=option aria-selected="${i===psel}" data-pi="${i}">${esc(p.name)}${
      p.host ? ` <span class="badge">${esc(p.host)}</span>` : ""}
      <small>${esc(p.path)}</small></div>`).join("") ||
    `<div class="note">no match</div>`;
  const on = $("#projects").querySelector(".p.on");
  if (on) on.scrollIntoView({block: "nearest"});
  const p = hit[psel];
  $("#nstart").disabled = !p;
  $("#nbnote").textContent = !p ? "" : p.host
    ? "on " + p.host + ": starts as a background session there, in the mode Settings gives background sessions"
    : "";
  $("#nmode").disabled = !!(p && p.host);
  return hit;
}
async function startProject(p){
  if (!p) return;
  const mode = $("#nmode").value;
  try { localStorage.setItem(MODE_KEY, mode); } catch (e) {}
  if (p.host){
    // No terminal here for a remote host: a background session over there.
    const text = $("#prompt").value.trim();
    $("#say").textContent = "starting " + p.name + " on " + p.host + "…";
    const r = await post("/api/dispatch", {project: p.name, path: p.path, host: p.host,
      prompt: text || "Ready when you are.", confirm: true, mode: "background"});
    $("#say").textContent = r.ok ? outcome(r) : (r.error || "refused");
    if (r.ok){ if (text) clearPrompt(); openNew(false); }
    return;
  }
  $("#say").textContent = "starting " + p.name + (mode !== "default" ? " in " + mode + " mode" : "") + "…";
  const r = await post("/api/new", {cwd: p.path, permission_mode: mode});
  if (!r.ok){ $("#say").textContent = r.error || "refused"; return; }
  follow(r.session_id);
  openNew(false);
  $("#say").textContent = "opened a new session in " + p.name + " - selecting it once it starts";
}
async function openNew(show){
  const open = show === undefined ? $("#newbox").hidden : show;
  $("#newbox").hidden = !open;
  if (!open) return;
  closeMenus();
  try { $("#nmode").value = localStorage.getItem(MODE_KEY) || "default"; } catch (e) {}
  if (!$("#nmode").value) $("#nmode").value = "default";
  $("#find").value = ""; psel = 0; drawProjects(); $("#find").focus();
  try { projects = (await get("/api/projects")).projects || []; } catch (e) {}
  if (!$("#newbox").hidden) drawProjects();
}

// -- fleet ----------------------------------------------------------------
// Fleet (2026-11-20; docs/research-super-agent.md): ask about every session
// at once, the watches the dispatcher checks itself (no model in the loop),
// and what an agent or Ask has proposed. Nothing proposed is watched or run
// until it is confirmed here - the dispatcher takes a confirmation only from
// this page - and a confirmed prompt still reaches its session framed as
// someone else's, never as yours.
// Since 2026-11-21 it is the first row of the session list rather than a
// pop-up in the menu: picking it shows Fleet where the chat goes (`view` is
// "fleet"), with the watches folded at the top, and the prompt box asks.
let fleetBook = null, fleetFired = null, fleetAsking = false, fleetChat = [];
const WATCH_NUM = {waiting: ["minutes", "10"], spend: ["dollars", "5"]};
const FLEET_PH = "ask about every session… who needs me? what finished while I was away? which one is costing the most?";
let fleetWatchesOpen = false;
try { fleetWatchesOpen = localStorage.getItem("imperatorium.fleetWatches") === "1"; } catch (e) {}

// A session by its key, or - as Ask sometimes cites one - by its name or
// an id prefix of 6 or more (only when that names exactly one).
function sessionByKey(k){
  const exact = sessions.find(s => pkey(s) === k || key(s) === k);
  if (exact || !k) return exact;
  const t = k.toLowerCase();
  const hits = sessions.filter(s => [nameOf(s), s.name].some(n => n && n.toLowerCase() === t)
    || (t.length >= 6 && [pkey(s), key(s)].some(id => id && id.toLowerCase().startsWith(t))));
  return hits.length === 1 ? hits[0] : undefined;
}
// Ask names sessions as [[session:KEY]]; each becomes a link that opens it.
// One that names no session here stays as plain text, never a dead link.
function citeHtml(html){
  return html.replace(/\[\[session:([^\]\s<]+)\]\]/g, (m, k) => {
    const s = sessionByKey(k);
    return s ? `<a href="#" class=cite data-cite="${attr(pkey(s))}">${esc(nameOf(s))}</a>` : esc(k);
  });
}
function fleetWhen(t){ const a = age(t); return a ? a + " ago" : ""; }
function proposalHtml(x, kind){
  const why = x.reason || x.note;
  return `<div class=flitem><div class=flwhat><b>${esc(kind === "watch" ? "Watch: " + x.text : x.text)}</b>
    <small>proposed by ${esc(x.source || "?")} ${esc(fleetWhen(x.created_at))}</small>${
    why ? `<div class=flwhy>${esc(why)}</div>` : ""}${
    x.prompt ? `<div class=flprompt title="delivered when its turn ends, framed as another session's message">${esc(x.prompt)}</div>` : ""}</div>
    <div class=btns><button class=sm data-fl="${kind}:confirm:${attr(x.id)}">${kind === "watch" ? "Watch" : "Run it"}</button>
    <button class="ghost sm" data-fl="${kind}:${kind === "watch" ? "remove" : "decline"}:${attr(x.id)}">Decline</button></div></div>`;
}
function watchHtml(w){
  const st = w.status === "active" ? "" : ` <small class=flst>${esc(w.status)}</small>`;
  const fired = w.fired_count ? ` <small>fired ${w.fired_count}x, last ${esc(fleetWhen(w.last_fired_at))}</small>` : "";
  const flip = w.status === "active" ? ["pause", "Pause"] : ["resume", w.status === "fired" ? "Again" : "Resume"];
  return `<div class=flitem><div class=flwhat>${esc(w.text)}${st}${fired}</div><div class=btns>
    <button class="ghost sm" data-fl="watch:${flip[0]}:${attr(w.id)}">${flip[1]}</button>
    <button class="ghost sm" data-fl="watch:remove:${attr(w.id)}">Remove</button></div></div>`;
}
function fleetTargets(){
  const projects = [...new Set(sessions.map(s => s.project).filter(Boolean))].sort();
  return `<option value="">every session</option>` +
    projects.map(p => `<option value="p:${attr(p)}">project ${esc(p)}</option>`).join("") +
    sessions.map(s => `<option value="s:${attr(pkey(s))}">${esc(nameOf(s))}</option>`).join("");
}
// Pure: the Fleet row's two lines, from the book - what waits for you
// on the right, then how many watches are on (or what it is for).
function fleetRowText(b){
  const n = b ? b.proposed || 0 : 0;
  const on = b ? (b.watches || []).filter(w => w.status === "active").length : 0;
  return {sum: n ? n + " to confirm" : "", sub: on ? on + (on === 1 ? " watch on" : " watches on") : "ask about every session", attn: n > 0};
}
function drawFleetRow(){
  const r = fleetRowText(fleetBook), row = $("#fleetrow");
  setText($("#fleetsum"), r.sum);
  setText($("#fleetsub"), r.sub);
  row.classList.toggle("attn", r.attn);
  row.classList.toggle("on", view === "fleet");
}
// Pure: one Ask exchange as two chat turns - the question, then the answer
// (or that it is being asked).
function askTurnHtml(t){
  return `<div class="turn you-turn"><div class="who you">${esc(SPEAKER.you || "you")}</div><div class=txt>${esc(t.q)}</div></div>` +
    `<div class="turn"><div class="who claude">Ask</div>${t.html ||
      `<small>asking - a model reads a snapshot of every session; this takes a few seconds and uses your plan's usage</small>`}</div>`;
}
function drawFleet(){
  const b = fleetBook || {watches: [], actions: [], fired: []};
  const props = b.watches.filter(w => w.status === "proposed").map(w => proposalHtml(w, "watch"))
    .concat(b.actions.filter(a => a.status === "proposed").map(a => proposalHtml(a, "action")));
  const ph = props.join("") || `<small>Nothing waiting. An agent with Imperatorium's tools, or Ask, can propose a watch or an action; it shows here until you answer it.</small>`;
  if ($("#flprop").dataset.sig !== ph){ $("#flprop").innerHTML = ph; $("#flprop").dataset.sig = ph; }
  const live = b.watches.filter(w => w.status !== "proposed");
  const wh = live.map(watchHtml).join("") || `<small>No watches yet.</small>`;
  if ($("#flwatch").dataset.sig !== wh){ $("#flwatch").innerHTML = wh; $("#flwatch").dataset.sig = wh; }
  const on = live.filter(w => w.status === "active").length;
  setText($("#flwcount"), live.length ? `(${on} on${live.length > on ? ", " + (live.length - on) + " off" : ""})` : "");
  const recent = b.fired.slice(0, 12).map(f => ({at: f.at, text: f.text, cls: "ok"}))
    .concat(b.actions.filter(a => a.status !== "proposed" && a.answered_at).map(a => ({at: a.answered_at,
      text: a.text + " - " + a.status + (a.status === "failed" && a.result && a.result.error ? ": " + a.result.error : ""),
      cls: a.status === "failed" ? "bad" : "run"})))
    .sort((x, y) => y.at - x.at).slice(0, 15);
  setHtml("#flfired", recent.map(r => `<div class=ev><span class=t>${esc(fleetWhen(r.at))}</span><span class="${r.cls} x" title="${attr(r.text)}">${esc(r.text)}</span></div>`).join("")
    || `<small>Nothing yet.</small>`);
  setHtml("#flchat", fleetChat.map(askTurnHtml).join(""));
  const pick = $("#wtarget"), had = pick.value, opts = fleetTargets();
  if (pick.dataset.sig !== opts){ pick.innerHTML = opts; pick.dataset.sig = opts; pick.value = had; }
  const num = WATCH_NUM[$("#wkind").value];
  $("#wnum").hidden = !num;
  if (num){ $("#wnum").placeholder = num[0]; $("#wnum").title = num[0]; }
}
function setHtml(sel, html){ const el = $(sel); if (el.dataset.sig !== html){ el.innerHTML = html; el.dataset.sig = html; } }
function fleetBadge(){
  const n = fleetBook ? fleetBook.proposed || 0 : 0, b = $("#fleetbtn");
  b.hidden = !n;
  b.textContent = n + " to confirm";
  drawFleetRow();
}
// A watch that fired since the last look: said in the window, and on the
// desktop when the window is not in front (as a session waiting is).
function fleetNews(book){
  const latest = (book.fired[0] || {}).at || 0;
  if (fleetFired !== null) for (const f of book.fired.filter(f => f.at > fleetFired).slice(0, 3)){
    $("#say").textContent = "watch: " + f.text;
    if (deskNotify && "Notification" in window && Notification.permission === "granted"
        && !(document.visibilityState === "visible" && document.hasFocus())){
      const n = new Notification("Imperatorium watch", {body: f.text, tag: "imperatorium-watch-" + f.watch + "-" + f.at});
      n.onclick = () => { window.focus(); const s = sessionByKey(f.session); if (s) select(key(s)); n.close(); };
    }
  }
  fleetFired = Math.max(fleetFired || 0, latest);
}
async function loadFleet(){
  let d;
  try { d = await get("/api/fleet/book"); } catch (e) { return; }
  if (!d || !d.ok) return;
  fleetNews(d);
  fleetBook = d;
  fleetBadge();
  if (view === "fleet") drawFleet();
}
async function fleetDo(kind, op, id){
  const r = await post(kind === "watch" ? "/api/fleet/watch" : "/api/fleet/action", {op, id});
  if (!r.ok) $("#flnote").textContent = r.error || (r.result && r.result.error) || "refused";
  else $("#flnote").textContent = kind === "action" && op === "confirm" ? "done" : "";
  await loadFleet();
}
async function addWatch(){
  const kind = $("#wkind").value, t = $("#wtarget").value, body = {op: "add", kind};
  if (t.startsWith("s:")) body.session = t.slice(2); else if (t.startsWith("p:")) body.project = t.slice(2);
  const num = WATCH_NUM[kind];
  if (num && $("#wnum").value.trim()) body[num[0]] = +$("#wnum").value;
  const r = await post("/api/fleet/watch", body);
  $("#flnote").textContent = r.ok ? "" : r.error || "refused";
  if (r.ok) $("#wnum").value = "";
  await loadFleet();
}
// Ask, from the prompt box while Fleet is showing: the question and its
// answer join the Fleet chat (this page only; at most the last 20).
async function askFleet(){
  const q = $("#prompt").value.trim();
  if (!q || fleetAsking) return;
  fleetAsking = true;
  $("#send").disabled = true;
  clearPrompt("fleet");
  const turn = {q, html: ""};
  fleetChat.push(turn);
  if (fleetChat.length > 20) fleetChat.shift();
  drawFleet();
  fleetScroll();
  let r;
  try { r = await post("/api/ask", {question: q}, undefined, 200000); } catch (e) { r = {ok: false, error: "the dispatcher did not answer"}; }
  fleetAsking = false;
  if (view === "fleet") $("#send").disabled = false;
  if (!r.ok) turn.html = `<div class=flerr>${esc(r.error || "no answer")}</div>`;
  else {
    const meta = [r.model, r.seconds != null ? r.seconds + " s" : "", typeof r.cost_usd === "number" ? "~" + usd(r.cost_usd) : ""].filter(Boolean).join(" - ");
    const extra = (r.proposals || []).length ? `<div class=flwhy>${r.proposals.length} proposal${r.proposals.length === 1 ? "" : "s"} above, waiting for you to confirm</div>` : "";
    const refused = (r.refused || []).length ? `<div class=flwhy>not proposed: ${esc(r.refused.join("; "))}</div>` : "";
    turn.html = `<div class="flans md">${citeHtml(renderText(r.answer))}</div>${extra}${refused}<small>${esc(meta)}</small>`;
  }
  await loadFleet();
  drawFleet();
  fleetScroll();
}
function fleetScroll(){ const l = $("#fllog"); l.scrollTop = l.scrollHeight; }
// Into Fleet and out of it. The session stays selected behind it, so
// leaving (a tile, j/k, a link in an answer) goes straight back to one.
function openFleetView(){
  if (view !== "fleet"){
    closeTask();
    if (!$("#findpop").hidden) openFind(false);
    view = "fleet";
    $("#app").classList.add("fleet");
    $("#sessionpane").hidden = true;
    $("#fleetview").hidden = false;
    setText($("#title"), "Fleet");
    $("#sub").innerHTML = "every session at once - ask in the box below; nothing proposed happens until you confirm it here";
    $("#sub").dataset.sig = "";
    $("#sub").title = "";
    $("#prompt").placeholder = FLEET_PH;
    $("#send").textContent = "Ask";
    $("#send").title = "a model with no tools reads a snapshot of every session and answers (Enter)";
    $("#send").disabled = fleetAsking;
    $("#flwfold").open = fleetWatchesOpen;
    drawSessions();
  }
  drawFleet();
  fleetScroll();
  loadFleet();
  $("#prompt").focus();
}
function leaveFleet(){
  if (view !== "fleet") return;
  view = "session";
  $("#app").classList.remove("fleet");
  $("#fleetview").hidden = true;
  $("#sessionpane").hidden = false;
  $("#prompt").placeholder = PROMPT_PH;
  $("#send").textContent = "Send";
  $("#send").title = "Enter";
  drawFleetRow();
}
$("#fleetview").addEventListener("click", e => {
  const b = e.target.closest("[data-fl]");
  if (b){ const [kind, op, ...id] = b.dataset.fl.split(":"); b.disabled = true; fleetDo(kind, op, id.join(":")); return; }
  const c = e.target.closest("[data-cite]");
  if (c){
    e.preventDefault();
    const s = sessionByKey(c.dataset.cite);
    if (s) select(key(s));
  }
});
$("#flwfold").addEventListener("toggle", () => {
  fleetWatchesOpen = $("#flwfold").open;
  try { localStorage.setItem("imperatorium.fleetWatches", fleetWatchesOpen ? "1" : "0"); } catch (e) {}
});
$("#wkind").onchange = drawFleet;
$("#wadd").onclick = addWatch;
$("#fleetbtn").onclick = openFleetView;
$("#fleetrow").onclick = openFleetView;
$("#fleetrow").addEventListener("keydown", e => {
  if (e.key === "Enter" || e.key === " "){ e.preventDefault(); openFleetView(); }
});

// -- chat matrix ---------------------------------------------------------
// Its own window, so it can sit on another screen. The matrix page says it is
// up once a second; the button closes it by asking it to close itself, which
// still works after this page reloads and loses the handle it opened it with.
const MATRIX = "imperatorium-matrix";
const matrixChannel = "BroadcastChannel" in window ? new BroadcastChannel(MATRIX) : null;
let matrixWin = null, matrixSeen = 0;

function matrixUp(){
  return (matrixWin && !matrixWin.closed) || Date.now() - matrixSeen < 2500;
}

function showMatrixButton(){
  const b = $("#matrixbtn"), up = matrixUp();
  b.classList.toggle("on", up);
  b.setAttribute("aria-pressed", up ? "true" : "false");
}

// A double click on a tile asks for its session here. This window answers
// so the matrix knows not to open another one.
if (matrixChannel) matrixChannel.onmessage = e => {
  const m = e.data;
  if (m && m.select){
    matrixChannel.postMessage({selected: m.select});
    if (sessions.some(x => key(x) === m.select)) select(m.select); else follow(m.select);
    window.focus();
    return;
  }
  if (m === "open" || m === "closed"){
    matrixSeen = m === "open" ? Date.now() : 0;
    showMatrixButton();
  }
};

function toggleMatrix(){
  if (matrixUp()){
    if (matrixWin && !matrixWin.closed) matrixWin.close();
    if (matrixChannel) matrixChannel.postMessage("close");
    matrixWin = null; matrixSeen = 0;
  } else {
    const w = Math.round(screen.availWidth * .85), h = Math.round(screen.availHeight * .85);
    matrixWin = window.open("/matrix", MATRIX, `popup,width=${w},height=${h}`);
    if (!matrixWin) $("#say").textContent = "the browser blocked the Chat Matrix window - allow pop-ups for this page";
  }
  showMatrixButton();
}
$("#matrixbtn").onclick = toggleMatrix;

// -- settings ----------------------------------------------------------------
// Its own window, like the Chat Matrix; a second click brings it forward.
$("#setbtn").onclick = () => {
  const w = window.open("/settings", "imperatorium-settings", "popup,width=1080,height=820");
  if (!w) $("#say").textContent = "the browser blocked the Settings window - allow pop-ups for this page";
  else w.focus();
};
whileVisible(1000, showMatrixButton);
// The update badge (gap J3): while the native binary's last check saw a
// newer release. A click opens Settings at its Updates section, which shows
// what is new and installs it. The Python dispatcher answers 404: no badge.
async function showUpdate(){
  let d = null;
  try { const r = await fetch("/api/update", {cache: "no-store"}); if (r.ok) d = await r.json(); } catch (e) { return; }
  const b = $("#updbtn"), on = !!(d && d.ok && d.check && d.newer && !d.applying);
  b.hidden = !on;
  if (on) b.title = `Imperatorium ${d.latest} is out (this is ${d.current}): see what is new and update`;
}
$("#updbtn").onclick = () => {
  const w = window.open("/settings#updates", "imperatorium-settings", "popup,width=1080,height=820");
  if (!w) $("#say").textContent = "the browser blocked the Settings window - allow pop-ups for this page";
  else w.focus();
};
showUpdate();
setInterval(showUpdate, 10 * 60 * 1000);

$("#new").onclick = () => openNew(true);
$("#find").addEventListener("input", () => { psel = 0; drawProjects(); });
$("#newbox").addEventListener("keydown", e => {
  if (e.key === "Escape"){ e.preventDefault(); e.stopPropagation(); openNew(false); return; }
  if (e.target.id === "nmode" || e.target.tagName === "BUTTON") return;
  const hit = projectHits(projects, $("#find").value);
  if (e.key === "ArrowDown"){ e.preventDefault(); psel = Math.min(psel+1, hit.length-1); drawProjects(); }
  else if (e.key === "ArrowUp"){ e.preventDefault(); psel = Math.max(psel-1, 0); drawProjects(); }
  else if (e.key === "Enter"){ e.preventDefault(); startProject(hit[psel]); }
});
$("#projects").addEventListener("click", e => {
  const el = e.target.closest("[data-pi]");
  if (!el) return;
  psel = +el.dataset.pi; drawProjects();
});
$("#projects").addEventListener("dblclick", e => {
  const el = e.target.closest("[data-pi]");
  if (el) startProject(projectHits(projects, $("#find").value)[+el.dataset.pi]);
});
$("#nstart").onclick = () => startProject(projectHits(projects, $("#find").value)[psel]);
$("#ncancel").onclick = () => openNew(false);
$("#newbox").addEventListener("click", e => { if (e.target.id === "newbox") openNew(false); });

$("#mic").onclick = dictate;
$("#stop").onclick = () => { stopTurn(); };
let f9Held = false;
document.addEventListener("keydown", e => {
  if (e.key !== "F9") return;
  e.preventDefault();
  if (e.repeat) return;
  if (dictMode !== "hold"){ dictate(); return; }
  if (f9Held) return;
  f9Held = true;
  if (!(rec && rec.state === "recording")) dictate();
});
document.addEventListener("keyup", e => {
  if (e.key !== "F9" || !f9Held) return;
  e.preventDefault();
  f9Held = false;
  stopDictation();
});
// Focus lost mid-hold: the release goes elsewhere, so stop here.
addEventListener("blur", () => { if (f9Held){ f9Held = false; stopDictation(); } });
addEventListener("focus", loadDictMode);
loadDictMode();

$("#send").onclick = () => send("auto");
$("#sendtyped").onclick = sendTyped;
$("#bg").onclick = () => send("background");
$("#queue").onclick = queue;
$("#prompt").addEventListener("keydown", e => {
  if (quick.key && comboOf(e) === quick.key){ e.preventDefault(); openQuick(true); return; }
  if (e.key === "Enter" && e.altKey){ e.preventDefault(); queue(); return; }
  if (e.key === "Enter" && !e.shiftKey){ e.preventDefault(); send(e.ctrlKey ? "background" : "auto"); }
});
$("#filter").addEventListener("input", drawSessions);
["#sessions", "#status", "#log"].forEach(holdPresses);
// On the window, so a press released outside the panel still lets it redraw.
window.addEventListener("pointerup", releasePresses, true);
window.addEventListener("pointercancel", releasePresses, true);
window.addEventListener("blur", releasePresses);
$("#group").onchange = () => savePrefs({group: $("#group").value});
$("#tabpick").onchange = () => setTab($("#tabpick").value);
$("#curbtn").onclick = () => { closeMenus(); showCurrent = !showCurrent; currentFolded = false; drawCurrent(); };
// The header's menus: one open at a time, shut by a click elsewhere or Esc.
// A change inside one (a display shape) leaves it open.
function closeMenus(except){
  document.querySelectorAll("details.menu[open]").forEach(d => { if (d !== except) d.open = false; });
}
document.querySelectorAll("details.menu").forEach(d =>
  d.addEventListener("toggle", () => { if (d.open) closeMenus(d); }));
document.addEventListener("click", e => {
  if (!(e.target.closest && e.target.closest("details.menu"))) closeMenus();
});
document.addEventListener("keydown", e => {
  if (e.key === "Escape" && document.querySelector("details.menu[open]")){ closeMenus(); e.preventDefault(); }
});
$("#curhd").onclick = () => { currentFolded = !currentFolded; drawCurrent(); };
$("#jprev").onclick = () => jump(-1);
$("#status").addEventListener("click", e => {
  if (!e.target.closest(".hd")) return;
  statusFolded = !statusFolded;
  try { localStorage.setItem("imperatorium.statusFolded", statusFolded ? "1" : ""); } catch (e) {}
  drawStatus();
  loadTimeline();
});
$("#jnext").onclick = () => jump(1);
drawShape();
$("#shape").addEventListener("change", e => {
  if (e.target.matches("[data-times]")){
    showTimes = e.target.checked;
    try { localStorage.setItem("imperatorium.times", showTimes ? "1" : "0"); } catch (x) {}
    drawLog();
    return;
  }
  if (e.target.matches("[data-group]")){
    groupTools = e.target.checked;
    try { localStorage.setItem("imperatorium.group", groupTools ? "1" : "0"); } catch (x) {}
    drawShape();
    drawLog();
    return;
  }
  const cat = e.target.dataset.shape;
  if (!cat) return;
  if (e.target.value === "full") delete shape[cat]; else shape[cat] = e.target.value;
  try { localStorage.setItem("imperatorium.shape", JSON.stringify(shape)); } catch (e) {}
  drawShape();
  drawLog();
});
// -- paths in the chat -------------------------------------------------------
// A click on a marked path (Settings > Highlighting) opens the Path Menu, as
// a click on one does in Claude Code's terminal (2026-11-16): Open (with the
// program the file's type is associated with), Open folder (the folder that
// holds it, with it selected) or Copy path. The dispatcher resolves a
// relative path against the session's folder; a remote session's files are
// not on this machine, so it can only copy. A click that ended a selection
// is the selection's.
function pathClicked(e){
  const pth = e.target.closest && e.target.closest(".pth");
  if (!pth || String(getSelection() || "")) return false;
  if (!document.documentElement.classList.contains(pth.classList.contains("tp") ? "hl-tools" : "hl-prose")) return false;
  e.preventDefault();
  openPathMenu(pth.dataset.copy || pth.textContent, e.clientX, e.clientY);
  return true;
}
let pathFor = null;
function openPathMenu(p, x, y){
  const s = current(), remote = !s || !!s.remote;
  pathFor = {path: p, session: s ? key(s) : ""};
  const m = $("#pathmenu");
  m.innerHTML = `<div class="cap" title="${esc(p)}">${esc(p)}</div>` +
    `<button data-pact="open"${remote ? " disabled" : ""} title="${remote ? "the file is on " + esc(s && s.host || "another machine")
      : "open it with the program its type is associated with"}">Open</button>` +
    `<button data-pact="reveal"${remote ? " disabled" : ""} title="${remote ? "the file is on " + esc(s && s.host || "another machine")
      : "open the folder it is in, with it selected"}">Open folder</button>` +
    `<button data-pact="copy">Copy path</button>`;
  m.hidden = false;
  const w = m.offsetWidth, h = m.offsetHeight;
  m.style.left = Math.max(4, Math.min(x, innerWidth - w - 4)) + "px";
  m.style.top = Math.max(4, y + h + 8 > innerHeight ? y - h - 4 : y + 4) + "px";
  const first = m.querySelector("button:not([disabled])");
  if (first) first.focus();
}
function closePathMenu(){ $("#pathmenu").hidden = true; pathFor = null; }
$("#pathmenu").addEventListener("click", async e => {
  const b = e.target.closest("[data-pact]");
  if (!b || !pathFor || b.disabled) return;
  const {path, session} = pathFor, how = b.dataset.pact;
  closePathMenu();
  if (how === "copy"){
    navigator.clipboard.writeText(path).then(() => { $("#say").textContent = "copied: " + path; }, () => {});
    return;
  }
  let r;
  try { r = await post("/api/open_path", {path, session_id: session, reveal: how === "reveal"}); }
  catch (x) { r = {ok: false, error: "the dispatcher did not answer"}; }
  $("#say").textContent = r.ok ? (how === "reveal" ? "shown in its folder: " : r.how === "edit" ? "opened in an editor: " : "opened: ") + (r.path || path)
    : (r.error || "could not open it");
});
$("#pathmenu").addEventListener("keydown", e => {
  const bs = [...$("#pathmenu").querySelectorAll("button:not([disabled])")];
  const i = bs.indexOf(document.activeElement);
  if (e.key === "Escape"){ e.preventDefault(); e.stopPropagation(); closePathMenu(); }
  else if (e.key === "ArrowDown" || e.key === "ArrowUp"){
    e.preventDefault();
    if (bs.length) bs[(i + (e.key === "ArrowDown" ? 1 : bs.length - 1)) % bs.length].focus();
  }
});
document.addEventListener("pointerdown", e => {
  if (!$("#pathmenu").hidden && !e.target.closest("#pathmenu")) closePathMenu();
}, true);
window.addEventListener("blur", closePathMenu);
$("#tvtext").addEventListener("click", pathClicked);

$("#log").addEventListener("click", e => {
  if (pathClicked(e)) return;
  const un = e.target.closest && e.target.closest("[data-unpend]");
  if (un){ dropPending(+un.dataset.unpend); return; }
  const b = e.target.closest && e.target.closest("[data-earlier]");
  if (b) showEarlier(b.dataset.earlier === "all");
  if (e.target.closest && e.target.closest("[data-newest]")) showNewest();
  const c = e.target.closest && e.target.closest("[data-coldact]");
  if (c && c.dataset.coldact === "dismiss"){
    const s = current();
    if (s){ coldGone[coldKey(s)] = true; drawCold(s); }
  } else if (c) act(c.dataset.coldact);
});
// A scroll is the reader's, or ours to the end (which lands at the end, so
// it keeps following). Content growing does not fire one.
$("#log").addEventListener("scroll", () => {
  logFollow = logAtEnd($("#log"));
  markAway();
  maybeReviewed();
}, {passive: true});
// What you do in the chat, as opposed to what it does on its own.
["pointerdown", "wheel", "touchstart", "keydown"].forEach(t =>
  $("#log").addEventListener(t, touched, {passive: true}));
// The log resized - the drawer, status panel or prompt box changed, the
// window did, or the tab came back into view: stay at the end if following.
new ResizeObserver(() => { if (logFollow) showNewest(); else markAway(); }).observe($("#log"));
if (document.fonts) document.fonts.ready.then(() => { if (logFollow) showNewest(); });
// `toggle` does not bubble; listen on the way down.
$("#log").addEventListener("toggle", e => {
  const d = e.target;
  if (!d.matches || !d.matches("details.cmp,details.tgroup")) return;
  if (d.open) opened.add(d.dataset.k); else opened.delete(d.dataset.k);
  // Opening a block is not new text: the view stays put, and if that pushed
  // the end out of view the log stops following, as if scrolled.
  logFollow = logAtEnd($("#log"));
  markAway();
}, true);
document.addEventListener("keydown", e => {
  if (!e.altKey || view !== "session" || tab !== "log") return;
  if (e.key === "ArrowUp"){ e.preventDefault(); jump(-1); }
  if (e.key === "ArrowDown"){ e.preventDefault(); jump(1); }
});
$("#prompt").addEventListener("input", noteDraft);
document.querySelectorAll("[data-view]").forEach(b => b.onclick = () => openPopup(b.dataset.view, true));
for (const [which, sel] of Object.entries(POPUPS)){
  $(sel).addEventListener("click", e => {
    if (e.target.id === sel.slice(1) || e.target.closest("[data-popclose]")) openPopup(which, false);
  });
  $(sel).addEventListener("keydown", e => {
    if (e.key === "Escape"){ e.preventDefault(); e.stopPropagation(); openPopup(which, false); }
  });
}
let hqTimer;
$("#hq").addEventListener("input", () => { clearTimeout(hqTimer); hqTimer = setTimeout(search, 350); });
$("#days").onchange = loadInsights;
$("#pjpick").onchange = () => { projFor = $("#pjpick").value; loadProject(); };
$("#pjrefresh").onclick = () => loadProject();
// A new dispatcher came up: load its page, on the same session. The prompt
// box's draft is already in localStorage.
function reloadForNewDispatcher(){
  noteDraft();
  location.replace(location.pathname + (selected ? "?session=" + encodeURIComponent(selected) : ""));
}

// Named, so the matrix can find this window again; opened by the matrix to
// show one session, it selects that session once it is listed.
window.name = "imperatorium-main";
{
  const asked = new URLSearchParams(location.search).get("session");
  if (asked){ follow(asked); history.replaceState(null, "", location.pathname); }
}

// -- quick phrases -----------------------------------------------------------
// The Settings window's phrase list, opened from the prompt box by its own
// shortcut. Read when opened, so a change in Settings needs no reload.
let quick = {key: "ctrl+space", phrases: []}, qsel = 0;
async function loadQuick(){
  try { const d = await get("/api/quick"); if (d.ok) quick = d; } catch (e) {}
}
function quickHits(){
  const q = $("#qq").value.trim().toLowerCase();
  return (quick.phrases || []).map((p, i) => ({p, i}))
    .filter(h => !q || h.p.toLowerCase().includes(q));
}
function drawQuick(){
  const hit = quickHits();
  qsel = Math.max(0, Math.min(qsel, hit.length - 1));
  $("#qlist").innerHTML = hit.map((h, n) =>
    `<div class="ji${n === qsel ? " on" : ""}" data-qi="${h.i}"><span class=qn>${h.i < 9 ? h.i + 1 : ""}</span>` +
    `<span>${esc(h.p)}</span></div>`).join("") ||
    `<div class=note>${(quick.phrases || []).length ? "no match"
      : "No phrases yet - add them in Settings, under Quick phrases."}</div>`;
  const on = $("#qlist .ji.on");
  if (on) on.scrollIntoView({block: "nearest"});
}
async function openQuick(show){
  $("#quickbox").hidden = !show;
  if (!show){ $("#prompt").focus(); return; }
  $("#qq").value = ""; qsel = 0; drawQuick(); $("#qq").focus();
  await loadQuick(); drawQuick();
}
// Into the prompt box at the cursor, replacing a selection, with a space
// either side where it would otherwise run into the text around it.
function takeQuick(i){
  const phrase = (quick.phrases || [])[i];
  openQuick(false);
  if (phrase == null) return;
  const box = $("#prompt"), v = box.value;
  const a = box.selectionStart ?? v.length, b = box.selectionEnd ?? v.length;
  const before = a && !/\s$/.test(v.slice(0, a)) ? " " : "";
  const after = b < v.length && !/^\s/.test(v.slice(b)) ? " " : "";
  box.setRangeText(before + phrase + after, a, b, "end");
  box.focus();
  noteDraft();
}
$("#qq").addEventListener("input", () => { qsel = 0; drawQuick(); });
$("#qq").addEventListener("keydown", e => {
  const hit = quickHits();
  if (e.key === "ArrowDown"){ e.preventDefault(); qsel = Math.min(qsel + 1, hit.length - 1); drawQuick(); }
  else if (e.key === "ArrowUp"){ e.preventDefault(); qsel = Math.max(qsel - 1, 0); drawQuick(); }
  else if (e.key === "Enter"){ e.preventDefault(); if (hit[qsel]) takeQuick(hit[qsel].i); }
  else if (e.key === "Escape" || comboOf(e) === quick.key){ e.preventDefault(); openQuick(false); }
  // A digit picks by number until something has been typed to filter with.
  else if (/^[1-9]$/.test(e.key) && !$("#qq").value && !e.ctrlKey && !e.altKey && !e.metaKey){
    e.preventDefault(); if (+e.key <= (quick.phrases || []).length) takeQuick(+e.key - 1);
  }
});
$("#qlist").addEventListener("click", e => {
  const el = e.target.closest("[data-qi]");
  if (el) takeQuick(+el.dataset.qi);
});
$("#quickbox").addEventListener("click", e => { if (e.target.id === "quickbox") openQuick(false); });
loadQuick();

$("#jq").addEventListener("input", () => { jsel = 0; drawJump(); });
$("#jq").addEventListener("keydown", e => {
  const hit = jumpHits();
  if (e.key === "ArrowDown"){ e.preventDefault(); jsel = Math.min(jsel + 1, hit.length - 1); drawJump(); }
  else if (e.key === "ArrowUp"){ e.preventDefault(); jsel = Math.max(jsel - 1, 0); drawJump(); }
  else if (e.key === "Enter"){ e.preventDefault(); const s = hit[jsel]; openJump(false); if (s) pick(key(s)); }
  else if (e.key === "Escape"){ e.preventDefault(); openJump(false); }
});
// One listener for the whole list, so a redraw binds nothing: the clicks
// land on whatever is there now. The small controls inside a tile come first
// and end the click, as their own handlers did by stopping it.
$("#sessions").addEventListener("click", e => {
  const t = e.target;
  let el;
  if ((el = t.closest("[data-pin]"))) return savePrefs({pinned: toggleIn("pinned", el.dataset.pin)});
  if ((el = t.closest("[data-arch]"))) return savePrefs({archived: toggleIn("archived", el.dataset.arch)});
  if ((el = t.closest("[data-close]"))) return closeSession(el.dataset.close);
  if ((el = t.closest("[data-dismiss]"))) return dismissSession(el.dataset.dismiss);
  if ((el = t.closest(".badge.pr"))){ if (el.dataset.url) window.open(el.dataset.url, "_blank"); return; }
  if (t.closest("#archfold")){ showArchived = !showArchived; drawSessions(); return; }
  if ((el = t.closest("[data-fam]"))) return famClicked(el.dataset.fam);
  const s = t.closest(".s");
  if (!s) return;
  if (t.closest("[data-hookerr]")) return openHookHelp();
  if (t.closest("[data-tasks-toggle]")) return foldTasks(!tasksOpen);
  if (t.closest("[data-done-toggle]")) return foldDone(!doneOpen);
  if (t.closest("[data-idle-toggle]")) return foldIdle(!idleOpen);
  const item = t.closest("[data-task]");
  if (item && s.dataset.k === selected) return item.dataset.task === "hooks" ? openHookHelp() : openTask(item.dataset.task);
  closeTask();
  select(s.dataset.k);
});

$("#jlist").addEventListener("click", e => {
  const el = e.target.closest("[data-jk]");
  if (el){ openJump(false); pick(el.dataset.jk); }
});
$("#jumpbox").addEventListener("click", e => { if (e.target.id === "jumpbox") openJump(false); });
document.addEventListener("keydown", e => {
  const k = (e.key || "").toLowerCase(), ctl = (e.ctrlKey || e.metaKey) && !e.altKey;
  if (ctl && k === "k"){ e.preventDefault(); openJump($("#jumpbox").hidden); return; }
  if (ctl && k === "f" && view === "session" && tab === "log"){ e.preventDefault(); openFind(true); return; }
  if (!$("#jumpbox").hidden || !$("#newbox").hidden || !$("#histbox").hidden || !$("#insightbox").hidden || !$("#projbox").hidden || !$("#hookbox").hidden) return;
  if (e.altKey && !e.ctrlKey && /^[1-9]$/.test(e.key)){ e.preventDefault(); pick(visibleKeys()[+e.key - 1]); return; }
  if (e.altKey && !e.ctrlKey && k === "n"){ e.preventDefault(); goWaiting(); return; }
  // Esc in the prompt box stops a working session's turn, as at its
  // terminal; with nothing to stop it leaves the box, as before.
  if (typing(e)){
    if (e.key === "Escape" && e.target.id === "prompt"){
      if (view === "session" && canStop(current())){ e.preventDefault(); stopTurn(); }
      else e.target.blur();
    }
    return;
  }
  if (e.ctrlKey || e.altKey || e.metaKey) return;
  if (k === "j"){ e.preventDefault(); stepSession(1); }
  else if (k === "k"){ e.preventDefault(); stepSession(-1); }
  else if (k === "n"){ e.preventDefault(); goWaiting(); }
  else if (e.key === "/"){ e.preventDefault(); $("#filter").focus(); }
});

let findTimer;
$("#cfind").addEventListener("input", () => { clearTimeout(findTimer); findTimer = setTimeout(runFind, 200); });
$("#cfilter").onchange = runFind;
$("#cfind").addEventListener("keydown", e => {
  if (e.key === "Enter"){ e.preventDefault(); goFind(e.shiftKey ? 1 : -1); }
});
// The find window: the Find button (or ctrl+F) opens it with the box ready;
// the button again, its close, or Esc in it closes it and clears the find,
// so no highlights are left behind with nothing on screen to explain them.
// It is not modal: the chat stays usable while it is open.
function openFind(on){
  $("#findpop").hidden = !on;
  $("#findbtn").classList.toggle("on", on);
  $("#findbtn").setAttribute("aria-expanded", String(on));
  if (on){ $("#cfind").focus(); $("#cfind").select(); return; }
  if (finds.q || finds.filter){ resetFind(); drawLog(); }
  else resetFind();
}
$("#findbtn").onclick = () => openFind($("#findpop").hidden);
$("#cfclose").onclick = () => { openFind(false); $("#findbtn").focus(); };
$("#findpop").addEventListener("keydown", e => {
  if (e.key === "Escape"){ e.preventDefault(); e.stopPropagation(); openFind(false); $("#findbtn").focus(); }
});
$("#cfprev").onclick = () => goFind(-1);
$("#cfnext").onclick = () => goFind(1);
$("#cdays").onchange = loadCosts;

loadPrefs().then(tick);
openStream();
procLoop();
