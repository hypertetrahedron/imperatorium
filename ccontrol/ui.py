"""One window: sessions down the side, the selected conversation in the middle.

This replaces a board that told you a session was blocked and a palette that
could not show you what it was blocked on. Seeing the question and answering
it are now the same screen.

Served as a page rather than built in a toolkit: the dispatcher already speaks
HTTP, rendering a conversation is far easier in HTML than in Tk, and the same
page then works on a tablet on the LAN. Open it chromeless with

    msedge --app=http://127.0.0.1:8792/

The shell is static; everything in it is filled from the JSON API, so this
function does no work per request and the page survives the dispatcher
restarting under it.
"""

CSS = """
:root{
  color-scheme: light dark;
  --bg:#f6f6f4; --panel:#fff; --side:#efefea; --ink:#15150f; --muted:#5d5d55;
  --line:#e2e2dc; --warn:#b3541e; --warnbg:#fdf0e6; --ok:#2f6b4f; --busy:#2d5c8a;
  --sel:#e4ecf4; --code:#f2f2ec; --fail:#a3232c;
}
@media (prefers-color-scheme: dark){
  :root{ --bg:#14140f; --panel:#1b1b16; --side:#191914; --ink:#f2f2ec;
         --muted:#a3a39a; --line:#2e2e26; --warn:#e8a06a; --warnbg:#33210f;
         --ok:#7fc0a0; --busy:#84b4e0; --sel:#22303d; --code:#111;
         --fail:#ef7b84}
}
*{box-sizing:border-box}
html,body{height:100%}
body{margin:0;background:var(--bg);color:var(--ink);overflow:hidden;
  font:14px/1.55 ui-sans-serif,system-ui,-apple-system,Segoe UI,sans-serif}
[hidden]{display:none!important}
#app{display:flex;height:100%}

#side{width:270px;flex:none;background:var(--side);border-right:1px solid var(--line);
  display:flex;flex-direction:column}
#side h1{font-size:12px;text-transform:uppercase;letter-spacing:.07em;
  color:var(--muted);margin:0;padding:14px 14px 8px;
  display:flex;justify-content:space-between;align-items:center}
button.tiny{background:transparent;border:1px solid var(--line);color:var(--muted);
  border-radius:4px;padding:1px 6px;font:11px inherit;text-transform:none;
  letter-spacing:0}
#picker{padding:0 10px 8px;border-bottom:1px solid var(--line)}
#find{width:100%;background:var(--panel);color:var(--ink);border:1px solid var(--line);
  border-radius:5px;padding:5px 7px;font:13px inherit}
#find:focus{outline:none;border-color:var(--busy)}
#projects{max-height:210px;overflow-y:auto;margin-top:4px}
.p{padding:5px 7px;border-radius:4px;cursor:pointer;font-size:13px}
.p:hover,.p.on{background:var(--sel)}
.p small{color:var(--muted);display:block;font-size:11px}
#sessions{overflow-y:auto;flex:1}
.s{padding:9px 13px;border-bottom:1px solid var(--line);cursor:pointer}
.s:hover{background:var(--panel)}
.s.on{background:var(--sel)}
.s.attn{border-left:3px solid var(--warn);padding-left:10px}
.s .n{font-weight:600;font-size:13.5px;word-break:break-word}
.s .w{color:var(--muted);font-size:11.5px;margin-top:1px;font-style:italic;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.s .m{color:var(--muted);font-size:11.5px;margin-top:2px;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.dot{display:inline-block;width:7px;height:7px;border-radius:50%;margin-right:5px}
.d-idle{background:var(--ok)}.d-busy{background:var(--busy)}
.d-attn{background:var(--warn)}.d-done{background:var(--muted)}
.d-fail{background:var(--fail)}
.s.fail{border-left:3px solid var(--fail);padding-left:10px}

#main{flex:1;display:flex;flex-direction:column;min-width:0}
#head{padding:11px 16px;border-bottom:1px solid var(--line);background:var(--panel)}
#title{font-weight:600}
#sub{color:var(--muted);font-size:11.5px;word-break:break-all}
#sessionpane{flex:1;display:flex;flex-direction:column;min-height:0}
#log{flex:1;overflow-y:auto;padding:14px 16px}
button.tiny.on{color:var(--ink);border-color:var(--busy)}
.turn{margin:0 0 15px;max-width:900px}
.who{font-size:11px;text-transform:uppercase;letter-spacing:.06em;
  color:var(--muted);margin-bottom:3px}
.who.you{color:var(--ink);font-weight:600}
.who.claude{color:var(--busy);font-weight:600}
.txt{white-space:pre-wrap;word-wrap:break-word}
.tbl{overflow-x:auto;margin:7px 0}
.tbl table{border-collapse:collapse;font-size:13px}
.tbl th,.tbl td{border:1px solid var(--line);padding:4px 9px;vertical-align:top;
  text-align:left}
.tbl th{background:var(--side);font-weight:600}
.tbl tbody tr:nth-child(even){background:var(--panel)}
.tbl code{background:var(--code);border-radius:3px;padding:0 3px;
  font:12px Consolas,ui-monospace,monospace}
.tool{border-left:2px solid var(--busy);padding:3px 0 3px 9px;margin:7px 0;
  font:12.5px/1.5 Consolas,ui-monospace,monospace;color:var(--muted)}
.tool b{color:var(--busy);font-weight:600}
.res{background:var(--code);border-radius:5px;padding:7px 9px;margin:5px 0;
  font:12px/1.45 Consolas,ui-monospace,monospace;white-space:pre-wrap;
  max-height:220px;overflow:auto;word-break:break-all}
.res.err{border-left:2px solid var(--warn)}
.think{color:var(--muted);font-style:italic;white-space:pre-wrap;margin:5px 0}
.note{color:var(--muted);padding:26px 0;text-align:center}

#ask{padding:10px 16px;border-top:1px solid var(--line);background:var(--panel)}
#prompt{width:100%;background:var(--bg);color:var(--ink);border:1px solid var(--line);
  border-radius:6px;padding:8px 9px;font:14px/1.5 inherit;resize:vertical;min-height:62px}
#prompt:focus{outline:none;border-color:var(--busy)}
#bar{display:flex;align-items:center;gap:10px;margin-top:7px;flex-wrap:wrap}
button{background:var(--busy);color:#fff;border:0;border-radius:5px;
  padding:6px 13px;font:13px inherit;cursor:pointer}
button.ghost{background:transparent;color:var(--muted);border:1px solid var(--line)}
button.rec{background:var(--warn);color:#fff;border-color:var(--warn)}
button:disabled{opacity:.45;cursor:default}
#say{color:var(--muted);font-size:12px}

/* -- sidebar tools: filter, grouping, usage meter ---------------------- */
#tools{display:flex;gap:6px;padding:0 10px 8px;align-items:center}
#filter{flex:1;min-width:0;background:var(--panel);color:var(--ink);
  border:1px solid var(--line);border-radius:5px;padding:4px 7px;font:12.5px inherit}
#filter:focus{outline:none;border-color:var(--busy)}
select.tiny{background:var(--panel);color:var(--muted);border:1px solid var(--line);
  border-radius:4px;font:11.5px inherit;padding:2px 3px}
#meter{padding:0 12px 8px;font-size:11.5px;color:var(--muted)}
#meter .row{display:flex;align-items:center;gap:6px;margin-top:3px}
#meter .bar{flex:1;height:5px;border-radius:3px;background:var(--line);overflow:hidden}
#meter .bar i{display:block;height:100%;background:var(--ok)}
#meter .bar i.hi{background:var(--warn)}#meter .bar i.over{background:var(--fail)}
#meter .pace{font-size:11px;margin-left:24px}
#meter .pace.bad{color:var(--fail)}
.g{font-size:10.5px;text-transform:uppercase;letter-spacing:.07em;color:var(--muted);
  padding:8px 13px 3px;display:flex;justify-content:space-between;cursor:default}
.g.fold{cursor:pointer}
.s{position:relative}
.s .d{color:var(--ink);font-size:11.5px;margin-top:2px;opacity:.8;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis;
  font-family:Consolas,ui-monospace,monospace}
.s .acts{position:absolute;top:6px;right:8px;display:none;gap:3px}
.s:hover .acts{display:flex}
.s .acts span{cursor:pointer;color:var(--muted);font-size:12px;padding:0 3px;
  border-radius:3px;background:var(--side)}
.s .acts span:hover{color:var(--ink)}
.s .pin{color:var(--warn);font-size:11px;margin-left:4px}
.badge{display:inline-block;font-size:10.5px;border-radius:3px;padding:0 4px;
  margin-left:4px;border:1px solid var(--line);color:var(--muted);font-style:normal}
.badge.passing{color:var(--ok);border-color:var(--ok)}
.badge.failing{color:var(--fail);border-color:var(--fail)}
.badge.pending{color:var(--busy);border-color:var(--busy)}
.badge.merged{color:#8a63d2;border-color:#8a63d2}
.badge.q{color:var(--busy);border-color:var(--busy)}
.badge.ctx-hi{color:var(--warn);border-color:var(--warn)}

/* -- main: tabs and panels ------------------------------------------- */
#headrow{display:flex;justify-content:space-between;gap:10px;align-items:flex-start}
#acts{display:flex;gap:5px;flex-wrap:wrap;justify-content:flex-end}
#tabs{display:flex;gap:2px;margin-top:8px}
#tabs button{background:transparent;color:var(--muted);border:0;border-bottom:2px solid transparent;
  border-radius:0;padding:3px 10px;font-size:12.5px}
#tabs button.on{color:var(--ink);border-bottom-color:var(--busy)}
.panel{flex:1;overflow-y:auto;padding:14px 16px}
.files{font:12.5px/1.6 Consolas,ui-monospace,monospace;margin-bottom:10px}
.files .add{color:var(--ok)}.files .del{color:var(--fail)}
.files .code{display:inline-block;width:2.3em;color:var(--muted)}
.diff{font:12px/1.45 Consolas,ui-monospace,monospace;white-space:pre;overflow-x:auto;
  background:var(--code);border-radius:5px;padding:8px 10px}
.diff .a{color:var(--ok)}.diff .r{color:var(--fail)}.diff .h{color:var(--busy)}
.diff .f{font-weight:600;color:var(--ink)}
.kv{color:var(--muted);font-size:12px;margin-bottom:8px}
.ev{font:12.5px/1.5 Consolas,ui-monospace,monospace;padding:2px 0;
  border-bottom:1px dashed var(--line);display:flex;gap:8px}
.ev .t{color:var(--muted);flex:none;width:5.2em}
.ev .ok{color:var(--ok)}.ev .bad{color:var(--fail)}.ev .run{color:var(--busy)}
.ev .x{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.ev .err{color:var(--fail);display:block;white-space:normal}
.card{border:1px solid var(--line);border-radius:6px;padding:9px 11px;margin:0 0 9px;
  background:var(--panel)}
.card h3{margin:0 0 4px;font-size:13.5px}
.card .btns{display:flex;gap:6px;margin-top:7px;flex-wrap:wrap}
.card small{color:var(--muted)}
button.sm{padding:3px 9px;font-size:12px}
button.danger{background:var(--fail)}
.hits{font-size:12.5px}
.hit{padding:8px 0;border-bottom:1px solid var(--line)}
.hit .tt{font-weight:600}
.hit .sn{color:var(--muted);font-size:12px;margin-top:2px}
.hit .btns{margin-top:5px;display:flex;gap:6px}
#hq{width:100%;background:var(--panel);color:var(--ink);border:1px solid var(--line);
  border-radius:6px;padding:7px 9px;font:14px inherit;margin-bottom:10px}
table.t{border-collapse:collapse;font-size:12.5px;margin:4px 0 12px}
table.t th,table.t td{border-bottom:1px solid var(--line);padding:3px 10px 3px 0;text-align:left}
table.t td.n{text-align:right;font-variant-numeric:tabular-nums}
#queued{font-size:12px;color:var(--muted);margin-top:6px}
#queued .qi{display:flex;gap:6px;align-items:baseline}
#queued .qi span{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
#queued .qi.err span{color:var(--fail)}
label.sw{display:flex;gap:7px;align-items:center;font-size:13px;margin:4px 0}
#decide{background:var(--warnbg);border-bottom:1px solid var(--warn);padding:8px 16px;
  font-size:13px}
#decide .dq{display:flex;gap:8px;align-items:center;margin:3px 0}
#decide .dq span{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}

@media (max-width:760px){
  #app{flex-direction:column}
  #side{width:auto;max-height:38vh;border-right:0;border-bottom:1px solid var(--line)}
  #headrow{flex-direction:column}
}
"""

SCRIPT = r"""
const $ = s => document.querySelector(s);
let sessions = [], selected = null, lastSig = "", busy = false;

const esc = s => String(s == null ? "" : s)
  .replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;");

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
const SPEAKER = {you: "you", claude: "Claude", tool: "tool result"};

function stateOf(s){
  if (s.attention) return [s.attention_kind === "failure" ? "d-fail" : "d-attn",
    s.attention_reason || s.waitingFor ||
    WAITING[s.last_notification] || s.last_notification || "waiting for you"];
  if (s.state && s.state !== "working") return ["d-done", s.state];
  if (RUNNING[s.status] || s.state === "working")
    return ["d-busy", RUNNING[s.status] || "working"];
  if (s.status === "idle") return ["d-idle","idle"];
  return ["d-done", s.status || "unknown"];
}

function age(t){
  if (!t) return "";
  const d = Math.max(0, Math.floor(Date.now()/1000 - t));
  if (d < 60) return d + "s";
  if (d < 3600) return Math.floor(d/60) + "m";
  return Math.floor(d/3600) + "h";
}

async function get(url){
  const r = await fetch(url, {cache:"no-store"});
  if (!r.ok) throw new Error(r.status);
  return r.json();
}

async function post(url, body){
  const r = await fetch(url, {method:"POST", cache:"no-store",
    headers:{"Content-Type":"application/json"}, body:JSON.stringify(body)});
  return r.json();
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

function groupOf(s){
  if (prefs.group === "folder") return folder(s.cwd) + (s.host ? " @ " + s.host : "");
  if (prefs.group === "state"){
    if (s.attention) return "needs you";
    const dot = stateOf(s)[0];
    if (dot === "d-busy") return "working";
    if (dot === "d-idle") return "idle";
    return "done";
  }
  return "";
}
const STATE_ORDER = ["needs you", "working", "idle", "done"];

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

function tile(s){
  const [dot, label] = stateOf(s);
  const when = age(s.attention_since || s.last_event_at);
  const mark = s.attention ? (s.attention_kind === "failure" ? " fail" : " attn") : "";
  const pinned = (prefs.pinned || []).includes(pkey(s));
  const archived = (prefs.archived || []).includes(pkey(s));
  const ctx = s.usage && s.usage.context_pct != null ? Math.round(s.usage.context_pct) : null;
  return `<div class="s${key(s)===selected?" on":""}${mark}" data-k="${esc(key(s))}">
    <div class="acts"><span data-pin="${esc(pkey(s))}" title="${pinned?"unpin":"pin to top"}">${
      pinned ? "★" : "☆"}</span><span data-arch="${esc(pkey(s))}" title="${
      archived ? "unarchive" : "archive (hide until it needs you)"}">${archived ? "↩" : "✕"}</span></div>
    <div class="n">${esc(s.name || s.id || "(unnamed)")}${pinned ? `<span class="pin">★</span>` : ""}</div>${
      s.window_title ? `<div class="w" title="window title">${esc(s.window_title)}</div>` : ""}${
      s.doing ? `<div class="d" title="${esc(s.doing)}">${esc(s.doing)}${
        s.doing_since ? " · " + age(s.doing_since) : ""}</div>` : ""}
    <div class="m"><span class="dot ${dot}"></span>${esc(label)}${when?" · "+when:""}${
      s.host ? " · " + esc(s.host) : ""}${prBadge(s.pr)}${
      ctx != null ? `<span class="badge${ctx >= 80 ? " ctx-hi" : ""}" title="context used">${ctx}%</span>` : ""}${
      s.queued ? `<span class="badge q" title="prompts queued">${s.queued} queued</span>` : ""}</div>
  </div>`;
}

function drawSessions(){
  // Whatever wants a human first, then this machine, then idle.
  sessions.sort((a,b) =>
    (a.attention?0:1) - (b.attention?0:1) ||
    (a.remote?1:0) - (b.remote?1:0) ||
    (a.status === "idle" ? 1 : 0) - (b.status === "idle" ? 1 : 0) ||
    String(a.name||"").localeCompare(String(b.name||"")));
  if (selected && !sessions.some(s => key(s) === selected)) selected = null;
  if (!selected && sessions.length) selected = key(sessions[0]);

  const q = ($("#filter").value || "").trim().toLowerCase();
  const hit = s => !q || [s.name, s.cwd, s.window_title, s.doing, s.host]
    .some(v => String(v || "").toLowerCase().includes(q));
  const pinned = new Set(prefs.pinned || []), archived = new Set(prefs.archived || []);
  // Archive hides a session until it wants you; a blocked session always shows.
  const shown = sessions.filter(s => hit(s) && (!archived.has(pkey(s)) || s.attention));
  const hidden = sessions.filter(s => hit(s) && archived.has(pkey(s)) && !s.attention);

  const groups = new Map();
  const top = shown.filter(s => pinned.has(pkey(s)));
  if (top.length) groups.set("pinned", top);
  for (const s of shown){
    if (pinned.has(pkey(s))) continue;
    const g = groupOf(s);
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g).push(s);
  }
  let names = [...groups.keys()];
  if (prefs.group === "state")
    names.sort((a,b) => (a==="pinned"?-1:STATE_ORDER.indexOf(a)) - (b==="pinned"?-1:STATE_ORDER.indexOf(b)));
  else if (prefs.group === "folder")
    names.sort((a,b) => a==="pinned" ? -1 : b==="pinned" ? 1 : a.localeCompare(b));

  let html = names.map(g => (g || names.length > 1 ?
      `<div class="g"><span>${esc(g || "sessions")}</span><span>${groups.get(g).length}</span></div>` : "") +
    groups.get(g).map(tile).join("")).join("");
  if (hidden.length){
    html += `<div class="g fold" id="archfold"><span>${showArchived ? "▾" : "▸"} archived</span><span>${hidden.length}</span></div>`;
    if (showArchived) html += hidden.map(tile).join("");
  }
  $("#sessions").innerHTML = html || `<div class="note">${q ? "no match" : "no sessions"}</div>`;

  document.querySelectorAll(".s").forEach(el =>
    el.onclick = () => { selected = el.dataset.k; lastSig = ""; drawSessions(); refreshTab(true); });
  document.querySelectorAll("[data-pin]").forEach(el => el.onclick = e => {
    e.stopPropagation(); savePrefs({pinned: toggleIn("pinned", el.dataset.pin)}); });
  document.querySelectorAll("[data-arch]").forEach(el => el.onclick = e => {
    e.stopPropagation(); savePrefs({archived: toggleIn("archived", el.dataset.arch)}); });
  document.querySelectorAll(".badge.pr").forEach(el => el.onclick = e => {
    e.stopPropagation(); if (el.dataset.url) window.open(el.dataset.url, "_blank"); });
  const fold = $("#archfold");
  if (fold) fold.onclick = () => { showArchived = !showArchived; drawSessions(); };

  drawHead();
}

function drawHead(){
  const s = current();
  if (view !== "session") return;
  $("#title").textContent = s ? (s.name || s.id || "session") : "no session selected";
  $("#sub").textContent = s ? [s.window_title, s.cwd].filter(Boolean).join("  ·  ") : "";
  $("#send").disabled = !s || !!s.remote;
  $("#queue").disabled = !s;
  const acts = [];
  if (s && !s.remote){
    if (s.resumable){
      acts.push(`<button class="sm" data-act="resume-window" title="claude --resume in a new tab">Resume</button>`);
      acts.push(`<button class="sm ghost" data-act="resume-background" title="continue it in the background; the prompt box is its next message">Resume in bg</button>`);
    }
    acts.push(`<button class="sm ghost" data-act="fork-window" title="a copy of this conversation in a new tab; the original is untouched">Fork</button>`);
    acts.push(`<button class="sm ghost" data-act="fork-background" title="a copy, continued in the background with the prompt box as its message">Fork in bg</button>`);
    acts.push(`<button class="sm ghost" data-act="copy" title="${esc(s.resume_command || "")}">Copy resume cmd</button>`);
  }
  const html = acts.join("");
  if ($("#acts").dataset.sig !== html){ $("#acts").innerHTML = html; $("#acts").dataset.sig = html; }
  document.querySelectorAll("[data-act]").forEach(b => b.onclick = () => act(b.dataset.act));
  drawQueued(s);
}

function current(){ return sessions.find(x => key(x) === selected); }

// Markdown tables, the GFM shape Claude writes: a header row, a delimiter row
// of dashes (colons set alignment), then body rows until a line without a
// pipe. Everything else stays pre-wrapped text, and nothing inside a ``` fence
// is touched - a table in a code sample is meant to be read as source.
const TABLE_SEP = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

function cells(line){
  let s = line.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
  return s.split(/(?<!\\)\|/).map(c => c.trim().replace(/\\\|/g, "|"));
}

// Inside a cell only: `code` and **bold**, which Claude uses constantly there
// and which read as noise when left raw in a grid. Runs on escaped text.
function inline(s){
  return esc(s)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>");
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

function renderText(text){
  const lines = String(text == null ? "" : text).split("\n");
  const out = [];
  let run = [], fence = false;
  const flush = () => {
    const t = run.join("\n").replace(/^\n+|\n+$/g, "");
    if (t) out.push(`<div class="txt">${esc(t)}</div>`);
    run = [];
  };
  for (let i = 0; i < lines.length; i++){
    const line = lines[i];
    if (/^\s*(```|~~~)/.test(line)) fence = !fence;
    const next = lines[i + 1];
    if (!fence && line.includes("|") && next !== undefined && TABLE_SEP.test(next)
        && next.includes("|")){
      const head = cells(line);
      if (cells(next).length === head.length){
        const rows = [];
        let j = i + 2;
        while (j < lines.length && lines[j].includes("|") && lines[j].trim())
          rows.push(cells(lines[j++]));
        flush();
        out.push(renderTable(head, next, rows));
        i = j - 1;
        continue;
      }
    }
    run.push(line);
  }
  flush();
  return out.join("");
}

function renderBlocks(blocks){
  return blocks.map(b => {
    if (b.kind === "text")     return renderText(b.text);
    if (b.kind === "thinking") return `<div class="think">${esc(b.text)}</div>`;
    if (b.kind === "tool")     return `<div class="tool"><b>${esc(b.name)}</b> ${esc(b.summary)}</div>`;
    if (b.kind === "result")   return `<div class="res${b.is_error?" err":""}">${
      esc(b.text)}${b.truncated?"\n…":""}</div>`;
    return "";
  }).join("");
}

async function loadLog(force){
  if (!selected) { $("#log").innerHTML = `<div class="note">select a session</div>`; return; }
  let data;
  try { data = await get("/api/conversation?session=" + encodeURIComponent(selected) + "&limit=60"); }
  catch (e) { return; }
  if (data.error && !data.items.length){
    $("#log").innerHTML = `<div class="note">${esc(data.error)}</div>`;
    return;
  }
  // Only repaint when something changed, so the scroll position survives.
  const sig = JSON.stringify(data.items).length + ":" + data.items.length;
  if (!force && sig === lastSig) return;
  lastSig = sig;

  const log = $("#log");
  const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 80;
  log.innerHTML = data.items.map(it =>
    `<div class="turn"><div class="who ${esc(it.speaker || it.role)}">${
      esc(SPEAKER[it.speaker] || it.speaker || it.role)}</div>${renderBlocks(it.blocks)}</div>`
  ).join("") || `<div class="note">nothing yet</div>`;
  if (force || atBottom) log.scrollTop = log.scrollHeight;
}

let ticks = 0;
async function tick(){
  try {
    const data = await get("/api/sessions");
    sessions = data.sessions || [];
    drawSessions();
    if (ticks % 3 === 0) drawMeter();
    if (ticks % 2 === 0) drawPending();
    if (ticks % 20 === 0) loadPrefs();
    await refreshTab(false);
  } catch (e) { /* dispatcher restarting; the page keeps what it has */ }
  ticks++;
  setTimeout(tick, 1500);
}

async function loadPrefs(){
  try { const r = await get("/api/prefs"); Object.assign(prefs, r.prefs || {}); } catch (e) {}
  $("#group").value = prefs.group || "state";
}

// -- parked permission requests ---------------------------------------
// Only present when policy.json parks `ask` decisions for the window. The
// hook is blocked while one shows, and gives up on its own when it expires.
let pendingSig = "";
async function drawPending(){
  let d;
  try { d = await get("/api/pending"); } catch (e) { return; }
  const items = d.pending || [];
  const sig = JSON.stringify(items.map(i => i.id));
  if (sig === pendingSig) return;
  pendingSig = sig;
  $("#decide").hidden = !items.length;
  $("#decide").innerHTML = items.map(i => `<div class="dq"><span title="${esc(i.input)}"><b>${
    esc(folder(i.cwd))}</b> wants ${esc(i.tool)}: ${esc(i.input)}${i.reason ? " - " + esc(i.reason) : ""}</span>
    <button class="sm" data-dv="allow" data-di="${esc(i.id)}">Allow</button>
    <button class="sm danger" data-dv="deny" data-di="${esc(i.id)}">Deny</button></div>`).join("");
  document.querySelectorAll("[data-dv]").forEach(b => b.onclick = async () => {
    const r = await post("/api/decide", {id: b.dataset.di, verdict: b.dataset.dv, who: "window"});
    $("#say").textContent = r.ok ? b.dataset.dv + " sent" : "too late - it already expired";
    pendingSig = ""; drawPending();
  });
}

// -- usage meter ---------------------------------------------------------
const clock = t => new Date(t * 1000).toLocaleTimeString([], {hour:"2-digit", minute:"2-digit"});
const LIMIT_NAMES = {five_hour: "5h", seven_day: "7d", spend_limit: "spend"};

async function drawMeter(){
  let u;
  try { u = await get("/api/usage"); } catch (e) { return; }
  const rows = Object.entries(u.limits || {});
  if (!rows.length){ $("#meter").innerHTML = ""; return; }
  $("#meter").innerHTML = rows.map(([name, w]) => {
    const pct = Math.max(0, Math.min(100, w.used));
    const cls = w.used >= 95 ? "over" : w.used >= 80 ? "hi" : "";
    const p = w.pace;
    let pace = "";
    if (p && p.full_at && p.before_reset)
      pace = `<div class="pace bad">at this pace: full by ${clock(p.full_at)} (${Math.round(p.per_hour)}%/h)</div>`;
    else if (p && p.per_hour > 0)
      pace = `<div class="pace">${Math.round(p.per_hour)}%/h - lasts until reset</div>`;
    return `<div class="row" title="${esc(name)}"><span style="width:18px">${esc(LIMIT_NAMES[name] || name)}</span>
      <span class="bar"><i class="${cls}" style="width:${pct}%"></i></span>
      <span>${Math.round(w.used)}%${w.resets_at ? " · " + clock(w.resets_at) : ""}</span></div>${pace}`;
  }).join("");
}

// -- tabs ------------------------------------------------------------------
// `view` is what the main column shows: a session (with tabs), history
// search, or insights. `tab` is which face of the session.
let view = "session", tab = "log";

function setView(v){
  view = v;
  $("#sessionpane").hidden = v !== "session";
  $("#historypane").hidden = v !== "history";
  $("#insightpane").hidden = v !== "insights";
  $("#tabs").hidden = v !== "session";
  $("#acts").hidden = v !== "session";
  $("#ask").hidden = v !== "session";
  if (v === "history"){ $("#title").textContent = "past sessions"; $("#sub").textContent =
      "search every session on this machine - titles, first prompts, and full text"; $("#hq").focus(); search(); }
  if (v === "insights"){ $("#title").textContent = "insights"; $("#sub").textContent =
      "tool failures across recent sessions, and notifications"; loadInsights(); }
  if (v === "session"){ drawHead(); refreshTab(true); }
  document.querySelectorAll("[data-view]").forEach(b => b.classList.toggle("on", b.dataset.view === v));
}

function setTab(t){
  tab = t;
  document.querySelectorAll("#tabs button").forEach(b => b.classList.toggle("on", b.dataset.tab === t));
  ["log","changes","activity","worktrees"].forEach(n => $("#" + n).hidden = n !== t);
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
  const sig = JSON.stringify([d.files, d.diff.length, d.branch, d.recent]);
  if ($("#changes").dataset.sig === sig) return;
  $("#changes").dataset.sig = sig;
  $("#changes").innerHTML = `<div class="kv">branch <b>${esc(d.branch || "?")}</b>${track ? " · " + esc(track) : ""} · ${
      d.files.length} changed file${d.files.length === 1 ? "" : "s"}</div>
    <div class="files">${files || "<div class='note'>working tree clean</div>"}</div>
    ${d.diff ? `<div class="diff">${diffHtml(d.diff)}${d.truncated ? "\n… (truncated)" : ""}</div>` : ""}
    ${recent ? `<div class="kv" style="margin-top:12px">recent commits</div><div class="files">${recent}</div>` : ""}`;
}

async function loadActivity(){
  const s = current();
  if (!s){ $("#activity").innerHTML = `<div class="note">select a session</div>`; return; }
  let d;
  try { d = await get("/api/activity?session=" + encodeURIComponent(key(s))); } catch (e) { return; }
  if (key(s) !== selected || tab !== "activity") return;
  if (!d.ok){ $("#activity").innerHTML = `<div class="note">${esc(d.error)}</div>`; return; }
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
  $("#activity").innerHTML = `<div class="kv">${t.calls} tool calls · ${t.errors} failed · ${esc(src)}</div>
    <div class="kv">${tools}</div>${rows || `<div class="note">no tool calls yet</div>`}`;
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
        w.ahead || 0} ahead of ${esc(d.base)}${w.behind ? " · " + w.behind + " behind" : ""}${w.dirty ? " · <b>uncommitted changes</b>" : ""}${
        w.subject ? " · last: " + esc(w.subject) : ""}</div>
      <div class="btns"><button class="sm ghost" data-wd="${esc(w.path)}">Diff</button>${w.agent ? `
        <button class="sm" data-wa="merge" data-wp="${esc(w.path)}" ${w.ahead && !w.dirty ? "" : "disabled"}>Merge into ${esc(d.base)}</button>
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

// -- resume, fork --------------------------------------------------------
async function act(what){
  const s = current();
  if (!s) return;
  if (what === "copy"){
    try { await navigator.clipboard.writeText(s.resume_command + (s.cwd ? "" : "")); $("#say").textContent = "copied: " + s.resume_command; }
    catch (e) { $("#say").textContent = s.resume_command; }
    return;
  }
  const [kind, where] = what.split("-");
  const r = await post("/api/resume", {session_id: key(s), where, fork: kind === "fork",
    prompt: $("#prompt").value.trim() || null});
  $("#say").textContent = r.ok ? (r.where === "background" ? kind + "ed in the background · " + (r.id || "")
    : "opened a new tab: " + r.command) : (r.error || "refused");
  if (r.ok && where === "background") $("#prompt").value = "";
}

// -- queue -----------------------------------------------------------------
async function queue(){
  const s = current();
  const text = $("#prompt").value.trim();
  if (!s || !text) return;
  const r = await post("/api/queue", {session_id: key(s), prompt: text});
  if (r.ok){ $("#prompt").value = ""; $("#say").textContent = r.waiting ?
      "queued - delivered when its current turn ends (" + r.waiting + " waiting)" : "delivered now - it was idle"; }
  else $("#say").textContent = r.error || "refused";
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
    selected = b.dataset.open; setView("session"); });
  document.querySelectorAll("[data-rs]").forEach(b => b.onclick = async () => {
    const r = await post("/api/resume", {session_id: b.dataset.rs, where: b.dataset.w, fork: !!b.dataset.fork});
    b.parentNode.insertAdjacentHTML("beforeend", `<small>${esc(r.ok ? (r.id ? "started " + r.id : "opened") : r.error)}</small>`);
  });
  document.querySelectorAll("[data-cp]").forEach(b => b.onclick = async () => {
    try { await navigator.clipboard.writeText(b.dataset.cp); b.textContent = "copied"; } catch (e) { b.textContent = b.dataset.cp; } });
}

// -- insights: failures and notifications ------------------------------
async function loadInsights(){
  $("#errs").innerHTML = `<div class="note">reading recent sessions…</div>`;
  let n;
  try { n = await get("/api/notify"); } catch (e) { n = null; }
  if (n) $("#notif").innerHTML = `
    <label class="sw"><input type="checkbox" data-ch="speak" ${n.speak.enabled ? "checked" : ""}> Say it out loud when a session needs you (after ${n.speak.delay}s)</label>
    <label class="sw"><input type="checkbox" data-ch="ntfy" ${n.ntfy.enabled ? "checked" : ""} ${n.ntfy.configured ? "" : "disabled"}> Push to phone via ntfy (after ${n.ntfy.delay}s)${
      n.ntfy.configured ? "" : " - set notify.ntfy.topic in config.json first"}</label>
    ${n.last_error ? `<div class="note" style="text-align:left;padding:4px 0">last error: ${esc(n.last_error)}</div>` : ""}`;
  document.querySelectorAll("[data-ch]").forEach(c => c.onchange = () =>
    post("/api/notify", {channel: c.dataset.ch, enabled: c.checked}));
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
  if (r.action === "background") return "started in the background" + id;
  if (r.action === "launch") return "opened a new session window";
  return r.action + id;
}

async function send(mode){
  const s = sessions.find(x => key(x) === selected);
  const text = $("#prompt").value.trim();
  if (!s || !text || busy) return;
  busy = true; $("#say").textContent = "sending…";
  const r = await post("/api/dispatch", {
    session_id: key(s), project: s.cwd, prompt: text, confirm: true, mode: mode || "auto"
  });
  busy = false;
  if (r.ok){ $("#prompt").value = ""; $("#say").textContent = outcome(r); }
  else $("#say").textContent = r.error || "refused";
  lastSig = ""; loadLog(true);
}

// -- dictation ----------------------------------------------------------
// The mic lives here, beside the box it fills. The browser records (localhost
// is a secure context) and the dispatcher transcribes on the model it already
// holds - loading costs seconds, using it costs hundredths.
let rec = null, chunks = [];

async function dictate(){
  if (rec && rec.state === "recording"){ rec.stop(); return; }
  let stream;
  try { stream = await navigator.mediaDevices.getUserMedia({audio:true}); }
  catch (e) { $("#say").textContent = "no microphone: " + e.name; return; }
  chunks = [];
  rec = new MediaRecorder(stream);
  rec.ondataavailable = e => { if (e.data.size) chunks.push(e.data); };
  rec.onstop = async () => {
    stream.getTracks().forEach(t => t.stop());
    $("#mic").classList.remove("rec");
    $("#mic").textContent = "\u{1F3A4} Dictate";
    if (!chunks.length) return;
    $("#say").textContent = "transcribing\u2026";
    const r = await fetch("/api/transcribe", {method:"POST", cache:"no-store",
      headers:{"Content-Type":"application/octet-stream"},
      body: new Blob(chunks)});
    const data = await r.json();
    if (!data.ok){ $("#say").textContent = data.error || "transcription failed"; return; }
    if (!data.text){ $("#say").textContent = "nothing heard"; return; }
    const box = $("#prompt");
    box.value = (box.value.trim() ? box.value.replace(/\s*$/, " ") : "") + data.text;
    box.focus();
    $("#say").textContent = "dictated " + data.text.length + " chars";
  };
  rec.start();
  $("#mic").classList.add("rec");
  $("#mic").textContent = "\u25A0 Stop";
  $("#say").textContent = "listening\u2026";
}

// -- starting a project that has no session -----------------------------
// The sidebar lists sessions, so without this a project nobody is currently
// working in would be unreachable - which the old palette could do.
let projects = [], psel = 0;

function drawProjects(){
  const q = $("#find").value.trim().toLowerCase();
  const norm = s => String(s).toLowerCase().replace(/[-_\/]+/g," ");
  const hit = projects.filter(p => !q || norm(p.name).includes(q) ||
                                   norm(p.name).replace(/ /g,"").includes(q.replace(/ /g,"")))
                      .slice(0, 40);
  psel = Math.min(psel, Math.max(0, hit.length - 1));
  $("#projects").innerHTML = hit.map((p,i) =>
    `<div class="p${i===psel?" on":""}" data-n="${esc(p.name)}">${esc(p.name)}
      <small>${esc(p.path)}</small></div>`).join("") ||
    `<div class="note">no match</div>`;
  document.querySelectorAll(".p").forEach((el,i) => {
    el.onclick = () => { psel = i; startProject(el.dataset.n); };
  });
  return hit;
}

async function startProject(name){
  const text = $("#prompt").value.trim();
  $("#say").textContent = "starting " + name + "\u2026";
  const r = await post("/api/dispatch", {
    project: name, prompt: text || "Ready when you are.", confirm: true,
    mode: text ? "auto" : "launch"
  });
  $("#say").textContent = r.ok ? outcome(r) : (r.error || "refused");
  if (r.ok){ $("#prompt").value = ""; togglePicker(false); }
}

function togglePicker(show){
  const open = show === undefined ? $("#picker").hidden : show;
  $("#picker").hidden = !open;
  if (open){ $("#find").value = ""; psel = 0; drawProjects(); $("#find").focus(); }
}

$("#new").onclick = async () => {
  if ($("#picker").hidden){
    try { projects = (await get("/api/projects")).projects || []; } catch(e){}
  }
  togglePicker();
};
$("#find").addEventListener("input", () => { psel = 0; drawProjects(); });
$("#find").addEventListener("keydown", e => {
  const hit = drawProjects();
  if (e.key === "ArrowDown"){ e.preventDefault(); psel = Math.min(psel+1, hit.length-1); drawProjects(); }
  else if (e.key === "ArrowUp"){ e.preventDefault(); psel = Math.max(psel-1, 0); drawProjects(); }
  else if (e.key === "Enter"){ e.preventDefault(); if (hit[psel]) startProject(hit[psel].name); }
  else if (e.key === "Escape"){ e.preventDefault(); togglePicker(false); }
});

$("#mic").onclick = dictate;
document.addEventListener("keydown", e => {
  if (e.key === "F9"){ e.preventDefault(); dictate(); }
});

$("#send").onclick = () => send("auto");
$("#bg").onclick = () => send("background");
$("#queue").onclick = queue;
$("#prompt").addEventListener("keydown", e => {
  if (e.key === "Enter" && e.altKey){ e.preventDefault(); queue(); return; }
  if (e.key === "Enter" && !e.shiftKey){ e.preventDefault(); send(e.ctrlKey ? "background" : "auto"); }
});
$("#filter").addEventListener("input", drawSessions);
$("#group").onchange = () => savePrefs({group: $("#group").value});
document.querySelectorAll("#tabs button").forEach(b => b.onclick = () => setTab(b.dataset.tab));
document.querySelectorAll("[data-view]").forEach(b => b.onclick = () =>
  setView(view === b.dataset.view ? "session" : b.dataset.view));
let hqTimer;
$("#hq").addEventListener("input", () => { clearTimeout(hqTimer); hqTimer = setTimeout(search, 350); });
$("#days").onchange = loadInsights;
loadPrefs().then(tick);
"""


def render_shell():
    return (
        "<!doctype html><html lang=en><head><meta charset=utf-8>"
        '<meta name=viewport content="width=device-width,initial-scale=1,viewport-fit=cover">'
        "<title>Imperatorium</title><style>%s</style></head><body>"
        '<div id=app>'
        '<div id=side><h1>sessions <span><button id=new class=tiny>+ new</button> '
        '<button class=tiny data-view=history title="search and resume past sessions">history</button> '
        '<button class=tiny data-view=insights title="tool failures and notifications">insights</button>'
        '</span></h1>'
        '<div id=meter></div>'
        '<div id=tools><input id=filter placeholder="filter sessions…" autocomplete=off>'
        '<select id=group class=tiny title="group sessions">'
        '<option value=state>by state</option><option value=folder>by folder</option>'
        '<option value=none>no groups</option></select></div>'
        '<div id=picker hidden><input id=find placeholder="project…" autocomplete=off>'
        '<div id=projects></div></div>'
        '<div id=sessions></div></div>'
        '<div id=main><div id=decide hidden></div>'
        '<div id=head><div id=headrow><div><div id=title>loading…</div><div id=sub></div></div>'
        '<div id=acts></div></div>'
        '<div id=tabs><button data-tab=log class=on>Conversation</button>'
        '<button data-tab=changes>Changes</button><button data-tab=activity>Activity</button>'
        '<button data-tab=worktrees>Worktrees</button></div></div>'
        '<div id=sessionpane><div id=log></div>'
        '<div id=changes class=panel hidden></div><div id=activity class=panel hidden></div>'
        '<div id=worktrees class=panel hidden></div></div>'
        '<div id=historypane class=panel hidden>'
        '<input id=hq placeholder="search past sessions - words in any order" autocomplete=off>'
        '<div id=hits class=hits></div></div>'
        '<div id=insightpane class=panel hidden>'
        '<div class=card><h3>Notifications</h3><div id=notif></div></div>'
        '<div class=card><h3>Tool failures <select id=days class=tiny>'
        '<option value=1>last day</option><option value=7 selected>last 7 days</option>'
        '<option value=30>last 30 days</option></select></h3><div id=errs></div></div></div>'
        '<div id=ask><textarea id=prompt placeholder="message this session…"></textarea>'
        '<div id=bar><button id=send title="Enter">Send</button>'
        '<button id=queue class=ghost title="hold until its current turn ends (alt+Enter)">Queue</button>'
        '<button id=bg class=ghost title="a new background session (ctrl+Enter)">Background</button>'
        '<button id=mic class=ghost title="dictate (F9)">&#127908; Dictate</button>'
        '<span id=say></span></div><div id=queued></div></div>'
        "</div></div><script>%s</script></body></html>" % (CSS, SCRIPT)
    )
