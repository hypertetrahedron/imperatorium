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
.s .app{display:inline-block;width:13px;height:11px;margin-right:5px;vertical-align:-1px;
  color:var(--muted)}
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
#sessionpane{flex:1;display:flex;flex-direction:column;min-height:0;position:relative}
#log{flex:1;overflow-y:auto;padding:14px 16px;position:relative}
#strip{display:flex;align-items:center;gap:10px;padding:4px 16px;
  border-top:1px solid var(--line);background:var(--panel);font-size:11.5px;color:var(--muted)}
#shape{display:flex;flex-wrap:wrap;gap:4px 12px;flex:1;min-width:0}
#shape label{display:flex;align-items:center;gap:4px;white-space:nowrap}
#shape select.full{color:var(--muted)}
#shape select:not(.full){color:var(--ink);border-color:var(--busy)}
#jump{display:flex;gap:4px;flex:none}
#jump button{padding:2px 8px;font-size:12px;opacity:.75}
#jump button:hover{opacity:1}
details.cmp{margin:3px 0}
details.cmp>summary{cursor:pointer;list-style:none;white-space:nowrap;overflow:hidden;
  text-overflow:ellipsis;color:var(--muted);font-size:12.5px}
details.cmp>summary::before{content:"▸ ";font-size:10px}
details.cmp[open]>summary::before{content:"▾ "}
details.cmp[open]>summary .ln{display:none}
details.cmp>summary .k{font:11px Consolas,ui-monospace,monospace;color:var(--busy);margin-right:5px}
details.cmp>summary .k.err{color:var(--warn)}
.turn.flash{outline:2px solid var(--busy);outline-offset:4px;border-radius:2px}
#current{border-bottom:1px solid var(--line);background:var(--panel);padding:8px 16px;
  max-height:30vh;overflow-y:auto}
#current .hd{display:flex;justify-content:space-between;align-items:center;cursor:pointer;
  font-size:11px;text-transform:uppercase;letter-spacing:.06em;color:var(--muted)}
#current .bd{margin-top:5px}
#current.fold .bd{display:none}
#tabs button.cur{margin-left:auto;font-size:11.5px}
#tabs button.cur.open{color:var(--ink);border-bottom-color:transparent}
details.compact{margin:0 0 15px;color:var(--muted)}
#log .earlier{text-align:center;margin:0 0 14px}
#log .earlier button{font-size:11.5px}
details.compact>summary{cursor:pointer;font-size:11px;text-transform:uppercase;
  letter-spacing:.06em;list-style:none;text-align:center;border-top:1px dashed var(--line);
  padding-top:6px}
details.compact[open]>summary{margin-bottom:6px}
button.tiny.on{color:var(--ink);border-color:var(--busy)}
.turn{margin:0 0 15px;max-width:900px}
.who{font-size:11px;text-transform:uppercase;letter-spacing:.06em;
  color:var(--muted);margin-bottom:3px}
.who.you{color:var(--ink);font-weight:600}
.who .via{color:var(--muted);font-weight:400;font-size:11px}
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
.txt code,.md code,.h code,blockquote code{background:var(--code);border-radius:3px;
  padding:0 3px;font:12.5px Consolas,ui-monospace,monospace}
.txt a,.md a,.tbl a,.h a,blockquote a{color:var(--busy)}
.h{font-weight:600;margin:12px 0 4px}
.h1{font-size:18px}.h2{font-size:16px}.h3{font-size:14.5px}
.h4,.h5,.h6{font-size:13.5px;color:var(--muted)}
.md ul,.md ol{margin:4px 0;padding-left:22px}
.md li{margin:2px 0}
pre.code{background:var(--code);border-radius:5px;padding:8px 10px;margin:6px 0;
  font:12.5px/1.45 Consolas,ui-monospace,monospace;overflow-x:auto;white-space:pre}
blockquote{border-left:3px solid var(--line);margin:6px 0;padding:0 0 0 10px;
  color:var(--muted)}
#log hr{border:0;border-top:1px solid var(--line);margin:10px 0}
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
#status{margin-top:8px;border-top:1px dashed var(--line);padding-top:5px;font-size:12px;
  color:var(--muted)}
#status .hd{display:flex;justify-content:space-between;gap:10px;cursor:pointer;user-select:none}
#status .hd .sum{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
#status.fold .bd{display:none}
#status .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(190px,1fr));
  gap:2px 16px;margin-top:5px}
#status .c{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
#status .c b{font-weight:400;margin-right:6px}
#status .c span{color:var(--ink)}
#status .c span.hot{color:var(--busy)}#status .c span.ok{color:var(--ok)}
#status .c span.warn{color:var(--warn)}
#status .ri{font:11.5px/1.5 Consolas,ui-monospace,monospace;white-space:nowrap;overflow:hidden;
  text-overflow:ellipsis}
#status .ri .k{color:var(--busy);margin-right:6px}
#status .run{margin-top:4px}

/* -- sidebar tools: filter, grouping, usage meter ---------------------- */
#tools{display:flex;gap:6px;padding:0 10px 8px;align-items:center}
#filter{flex:1;min-width:0;background:var(--panel);color:var(--ink);
  border:1px solid var(--line);border-radius:5px;padding:4px 7px;font:12.5px inherit}
#filter:focus{outline:none;border-color:var(--busy)}
select.tiny{background:var(--panel);color:var(--muted);border:1px solid var(--line);
  border-radius:4px;font:11.5px inherit;padding:2px 3px}
#meter{padding:0 12px 8px;font-size:11.5px;color:var(--muted)}
#meter .row{display:flex;align-items:center;gap:6px;margin-top:3px}
#meter .bar{flex:1;height:5px;border-radius:3px;background:var(--line);position:relative}
#meter .bar i{display:block;height:100%;border-radius:3px;background:var(--ok)}
#meter .bar b{position:absolute;top:-2px;bottom:-2px;width:2px;margin-left:-1px;background:var(--ink)}
#meter .bar i.hi{background:var(--warn)}#meter .bar i.over{background:var(--fail)}
#meter .pace{font-size:11px;margin-left:24px}
#meter .pace.bad{color:var(--fail)}
.g{font-size:10.5px;text-transform:uppercase;letter-spacing:.07em;color:var(--muted);
  padding:8px 13px 3px;display:flex;justify-content:space-between;cursor:default}
.g.fold{cursor:pointer}
.fam{padding:7px 13px 4px;font-weight:600;font-size:12.5px;display:flex;align-items:center;
  gap:5px;border-bottom:1px solid var(--line);cursor:pointer;user-select:none}
.fam:hover{background:var(--panel)}
.fam .car{color:var(--muted);font-size:10px;width:9px}
.fam .dots{margin-left:auto;display:flex;gap:3px}
.fam .dot{margin-right:0}
.kids{margin-left:13px;border-left:2px solid var(--line)}
.kids .s{padding-left:11px}
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
.work{font-variant-numeric:tabular-nums}
.work.cache{color:var(--ok)}
.work.busy{color:var(--busy)}
#status .hd .work+.work{margin-left:6px}
.badge.work.cache{border-color:var(--ok)}
.badge.work.busy{border-color:var(--busy)}

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
// `system` is Claude Code itself (a task finishing, a command's output); the
// item's `note` says which.
const SPEAKER = {you: "you", claude: "Claude", tool: "tool result", system: "Claude Code"};

function stateOf(s){
  if (s.attention) return [s.attention_kind === "failure" ? "d-fail" : "d-attn",
    s.attention_reason || s.waitingFor ||
    WAITING[s.last_notification] || s.last_notification || "waiting for you"];
  if (s.state && s.state !== "working") return ["d-done", s.state];
  if (RUNNING[s.status] || s.state === "working")
    return ["d-busy", RUNNING[s.status] || "working"];
  // Idle between turns but a command it started is still running: it will wake
  // when that exits, so it is not free for more work yet.
  if (s.status === "idle" && s.background_tasks)
    return ["d-busy", s.background_tasks === 1 ? "waiting on a background task"
                      : `waiting on ${s.background_tasks} background tasks`];
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

// What a tile calls a session. The server turns a worktree session's
// made-up "<worktree>-<id>" into "<project>-<worktree>"; a chosen name stays.
const nameOf = s => s.display_name || s.name || s.id || "(unnamed)";
// The project a session belongs to - for a worktree, the repository it was
// made from, not the worktree's folder.
const projectOf = s => (s.project || folder(s.cwd)) + (s.host ? " @ " + s.host : "");

function groupOf(s){
  if (prefs.group === "folder") return projectOf(s);
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

// Marks a session hosted by the Claude desktop app: it lives in that app's
// window, so someone scanning terminals for it would never find it.
const APP_ICON = `<svg class="app" viewBox="0 0 13 11" aria-label="Claude desktop app"><title>in the Claude desktop app</title><rect x=".5" y=".5" width="12" height="10" rx="1.5" fill="none" stroke="currentColor"/><path d="M.5 3.5h12" stroke="currentColor"/></svg>`;
// Marks a session working in a worktree rather than the main checkout.
const WT_ICON = `<svg class="app" viewBox="0 0 11 13" aria-label="worktree"><title>in a worktree</title><circle cx="3" cy="2.5" r="1.6" fill="none" stroke="currentColor"/><circle cx="3" cy="10.5" r="1.6" fill="none" stroke="currentColor"/><circle cx="8.5" cy="4.5" r="1.6" fill="none" stroke="currentColor"/><path d="M3 4.1v4.8M8.5 6.1c0 2.2-5.5 1.4-5.5 2.8" fill="none" stroke="currentColor"/></svg>`;

// `short` is for a tile under its project's node, where the project's name
// would only be said twice; the chat header still carries the full name.
// Pure: a session's other work at a glance, shared by its tile and the status
// panel's header line so the two always say the same. "-58" is minutes until
// the prompt cache expires (nothing once it has); "0/2/1/0" counts subagents,
// background shells, monitors and shell processes, while its process lives.
function workParts(w, now){
  if (!w) return [];
  const out = [];
  const left = w.cache_expires_at ? w.cache_expires_at - now : 0;
  if (left > 0) out.push({text: "-" + Math.ceil(left / 60), cls: "cache",
    title: `prompt cache expires in ${Math.ceil(left / 60)} min; a prompt after that re-sends the whole context`});
  if (w.alive){
    const n = [w.agents, w.shells, w.monitors, w.processes].map(x => x || 0);
    out.push({text: n.join("/"), cls: n.some(Boolean) ? "busy" : "",
      title: `${n[0]} subagents / ${n[1]} background shells / ${n[2]} monitors / ${n[3]} shell processes`});
  }
  return out;
}
const workHtml = (w, now, cls, skip) => workParts(w, now).filter(p => p.cls !== skip).map(p =>
  `<span class="${cls} ${p.cls}" title="${esc(p.title)}">${esc(p.text)}</span>`).join("");

function tile(s, short){
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
    <div class="n" title="${esc(nameOf(s))}">${s.desktop_app ? APP_ICON : ""}${
      s.worktree ? WT_ICON : ""}${esc(short && s.short_name || nameOf(s))}${pinned ? `<span class="pin">★</span>` : ""}</div>${
      s.window_title ? `<div class="w" title="window title">${esc(s.window_title)}</div>` : ""}${
      s.doing ? `<div class="d" title="${esc(s.doing)}">${esc(s.doing)}${
        s.doing_since ? " · " + age(s.doing_since) : ""}</div>` : ""}
    <div class="m"><span class="dot ${dot}"></span>${esc(label)}${when?" · "+when:""}${
      s.host ? " · " + esc(s.host) : ""}${prBadge(s.pr)}${
      ctx != null ? `<span class="badge${ctx >= 80 ? " ctx-hi" : ""}" title="context used">${ctx}%</span>` : ""}${
      s.queued ? `<span class="badge q" title="prompts queued">${s.queued} queued</span>` : ""}${
      workHtml(s.work, Date.now() / 1000, "badge work")}</div>
  </div>`;
}

// Tiles for one group, with a project's sessions gathered under a node named
// for the project whenever it has more than one. The node carries one dot per
// session, so how many there are and what each is doing still shows when it
// is folded. A folded node keeps showing a session that needs you, as the
// archive does. Grouped by folder, the whole list is that tree: every
// project gets a node, one session or several, in place of a group header.
function tree(list, group){
  const fams = new Map();
  for (const s of list){
    const f = projectOf(s);
    if (!fams.has(f)) fams.set(f, []);
    fams.get(f).push(s);
  }
  return [...fams].map(([f, kids]) => {
    if (kids.length < 2 && !(prefs.group === "folder" && group !== "pinned"))
      return kids.map(s => tile(s)).join("");
    const shut = folded.has(f);
    const dots = kids.map(s => `<span class="dot ${stateOf(s)[0]}" title="${
      esc(nameOf(s) + ": " + stateOf(s)[1])}"></span>`).join("");
    const shown = shut ? kids.filter(s => s.attention) : kids;
    return `<div class="fam${shut ? " shut" : ""}" data-fam="${esc(f)}" title="${
      kids.length} sessions - click to ${shut ? "unfold" : "fold"}"><span class="car">${
      shut ? "▸" : "▾"}</span><span>${esc(f)}</span><span class="dots">${dots}</span></div>` +
      (shown.length ? `<div class="kids">${shown.map(s => tile(s, true)).join("")}</div>` : "");
  }).join("");
}

// Folded project nodes, per browser: a viewing convenience, not shared state.
const folded = new Set((() => {
  try { return JSON.parse(localStorage.getItem("ccontrol.folded") || "[]"); }
  catch (e) { return []; }
})());
function toggleFold(f){
  if (folded.has(f)) folded.delete(f); else folded.add(f);
  try { localStorage.setItem("ccontrol.folded", JSON.stringify([...folded])); } catch (e) {}
  drawSessions();
}

function drawSessions(){
  // Whatever wants a human first, then this machine, then idle.
  sessions.sort((a,b) =>
    (a.attention?0:1) - (b.attention?0:1) ||
    (a.remote?1:0) - (b.remote?1:0) ||
    (stateOf(a)[0] === "d-idle" ? 1 : 0) - (stateOf(b)[0] === "d-idle" ? 1 : 0) ||
    nameOf(a).localeCompare(nameOf(b)));
  if (selected && !sessions.some(s => key(s) === selected)) selected = null;
  if (!selected && sessions.length) selected = key(sessions[0]);

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
    tree(groups.get(g), g)).join("");
  if (hidden.length){
    html += `<div class="g fold" id="archfold"><span>${showArchived ? "▾" : "▸"} archived</span><span>${hidden.length}</span></div>`;
    if (showArchived) html += hidden.map(s => tile(s)).join("");
  }
  $("#sessions").innerHTML = html || `<div class="note">${q ? "no match" : "no sessions"}</div>`;

  document.querySelectorAll(".s").forEach(el =>
    el.onclick = () => { selected = el.dataset.k; showCached(); drawSessions(); refreshTab(true); loadStatus(true); });
  document.querySelectorAll("[data-fam]").forEach(el => el.onclick = () => toggleFold(el.dataset.fam));
  document.querySelectorAll("[data-pin]").forEach(el => el.onclick = e => {
    e.stopPropagation(); savePrefs({pinned: toggleIn("pinned", el.dataset.pin)}); });
  document.querySelectorAll("[data-arch]").forEach(el => el.onclick = e => {
    e.stopPropagation(); savePrefs({archived: toggleIn("archived", el.dataset.arch)}); });
  document.querySelectorAll(".badge.pr").forEach(el => el.onclick = e => {
    e.stopPropagation(); if (el.dataset.url) window.open(el.dataset.url, "_blank"); });
  const fold = $("#archfold");
  if (fold) fold.onclick = () => { showArchived = !showArchived; drawSessions(); };

  syncDraft();
  drawHead();
}

function drawHead(){
  const s = current();
  if (view !== "session") return;
  $("#title").textContent = s ? nameOf(s) : "no session selected";
  $("#sub").textContent = s ? [s.window_title, s.cwd].filter(Boolean).join("  ·  ") : "";
  $("#send").disabled = !s || !!s.remote;
  $("#queue").disabled = !s;
  const acts = [];
  if (s && !s.remote){
    if (s.resumable){
      acts.push(`<button class="sm" data-act="resume-window" title="claude --resume in a new tab">Resume</button>`);
      acts.push(`<button class="sm ghost" data-act="resume-background" title="continue it in the background; the prompt box is its next message">Resume in bg</button>`);
    }
    acts.push(`<button class="sm ghost" data-act="new" title="a fresh claude session in this project, in a new tab">New</button>`);
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

function emph(s){
  return s
    .replace(/\*\*(?!\s)([^\n]+?)(?<!\s)\*\*/g, "<b>$1</b>")
    .replace(/(^|[^\w])__(?!\s)([^\n]+?)(?<!\s)__(?!\w)/g, "$1<b>$2</b>")
    .replace(/(^|[^*\w])\*(?![\s*])([^*\n]+?)(?<!\s)\*(?![*\w])/g, "$1<i>$2</i>")
    .replace(/(^|[^\w])_(?![\s_])([^_\n]+?)(?<!\s)_(?!\w)/g, "$1<i>$2</i>")
    .replace(/~~(?!\s)([^\n]+?)(?<!\s)~~/g, "<s>$1</s>");
}

function inline(s){
  const kept = [];
  const hold = h => "\u0000" + (kept.push(h) - 1) + "\u0000";
  s = String(s == null ? "" : s)
    .replace(/`([^`\n]+)`/g, (_, c) => hold("<code>" + esc(c) + "</code>"))
    .replace(/\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)/g, (_, t, u) => hold(link(u, emph(esc(t)))))
    .replace(/\bhttps?:\/\/[^\s<>()\[\]"'`\u0000]*[^\s<>()\[\]"'`\u0000.,;:!?*_~]/g,
             u => hold(link(u, esc(u))));
  return emph(esc(s)).replace(/\u0000(\d+)\u0000/g, (_, i) => kept[+i]);
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

function renderText(text){
  const lines = String(text == null ? "" : text).split("\n");
  const out = [];
  let run = [], lists = [], listHtml = "";
  const flush = () => {
    const t = run.join("\n").replace(/^\n+|\n+$/g, "");
    if (t) out.push(`<div class="txt">${inline(t)}</div>`);
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
      let j = i + 1;
      while (j < lines.length && !lines[j].trim().startsWith(mark)) body.push(lines[j++]);
      block(`<pre class="code">${esc(body.join("\n"))}</pre>`);
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
      listHtml += (top.open ? "</li>" : "") + "<li>" + inline(item[3]);
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

function renderBlocks(blocks){
  return blocks.map(renderBlock).join("");
}

function renderBlock(b){
  if (b.kind === "text")     return renderText(b.text);
  if (b.kind === "thinking") return `<div class="think">${esc(b.text)}</div>`;
  if (b.kind === "tool")     return `<div class="tool"><b>${esc(b.name)}</b> ${esc(b.summary)}</div>`;
  if (b.kind === "result")   return `<div class="res${b.is_error?" err":""}">${
    esc(b.text)}${b.truncated?"\n…":""}</div>`;
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
try { shape = JSON.parse(localStorage.getItem("ccontrol.shape") || "{}") || {}; } catch (e) { shape = {}; }
function modeOf(cat){ return MODES.includes(shape[cat]) ? shape[cat] : "full"; }

function categoryOf(it, b){
  if (it.note) return "system";
  if (b.kind === "text") return it.speaker === "you" ? "prompt" : "reply";
  return b.kind;
}

function oneLine(b){
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

// One turn's blocks as the controls say. `k` names a block by its place in
// the conversation, so one opened by hand stays open across repaints.
function shapeBlocks(it, i, opened){
  return it.blocks.map((b, j) => {
    const m = modeOf(categoryOf(it, b));
    if (m === "hidden") return "";
    const k = i + "." + j;
    return m === "compact" ? compactBlock(b, k, opened.has(k)) : renderBlock(b);
  }).join("");
}

function drawShape(){
  $("#shape").innerHTML = SHAPES.map(([cat, label]) => {
    const m = modeOf(cat);
    return `<label>${label}<select class="tiny${m === "full" ? " full" : ""}" data-shape="${cat}">${
      MODES.map(x => `<option value="${x}"${x === m ? " selected" : ""}>${
        x[0].toUpperCase() + x.slice(1)}</option>`).join("")}</select></label>`;
  }).join("");
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
  drawLog(force);
  const asked = items.filter(it => it.speaker === "you").pop();
  currentPrompt = asked ? asked.blocks.filter(b => b.kind === "text").map(b => b.text).join("\n\n") : "";
  drawCurrent();
}

// A tile was clicked: show what this page last had for it, if anything.
function showCached(){
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

function drawLog(force){
  if (!selected) return;
  const log = $("#log");
  if (openedFor !== selected){ opened = new Set(); openedFor = selected; logShown = LOG_WINDOW; }
  const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 80;
  const start = logStart(logItems.length, logShown);
  const earlier = start ? `<div class="earlier"><button data-earlier="1">show ${Math.min(start, LOG_STEP)} earlier</button>${
    start > LOG_STEP ? ` <button data-earlier="all">show all ${start}</button>` : ""}</div>` : "";
  log.innerHTML = earlier + logItems.slice(start).map((it, j) => {
    const i = start + j;
    // Where the session compacted: the summary that replaced everything above
    // it, folded away, since the real conversation is now shown in full.
    if (it.note === SUMMARY_NOTE)
      return modeOf("system") === "hidden" ? "" : `<details class="compact"><summary>compacted here - show the summary it kept</summary>${
        renderBlocks(it.blocks)}</details>`;
    const body = shapeBlocks(it, i, opened);
    if (!body) return "";
    return `<div class="turn${it.speaker === "you" ? " you-turn" : ""}"><div class="who ${esc(it.speaker || it.role)}">${
      esc(it.note || SPEAKER[it.speaker] || it.speaker || it.role)}${it.via ? ` <span class="via" title="arrived through the session inbox: the window, the palette, impctl or another session">via inbox</span>` : ""}</div>${body}</div>`;
  }).join("") || `<div class="note">${logItems.length ? "everything here is hidden - see the display settings below" : "nothing yet"}</div>`;
  if (force || atBottom) log.scrollTop = log.scrollHeight;
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
  if (data.unchanged) return;
  if (data.error && !data.items.length){
    $("#log").innerHTML = `<div class="note">${esc(data.error)}</div>`;
    return;
  }
  // Only repaint when something changed, so the scroll position survives.
  const sig = data.sig || (JSON.stringify(data.items).length + ":" + data.items.length);
  rememberConv(want, data.items, sig);
  if (!force && sig === lastSig) return;
  setLog(data.items, sig, force);
}

// -- the prompt being worked on --------------------------------------------
// The last thing you asked, pinned above the chat on demand, so a long turn
// can be read against what it is meant to be doing.
let currentPrompt = "", showCurrent = false, currentFolded = false;
function drawCurrent(){
  const box = $("#current");
  box.hidden = !showCurrent || tab !== "log" || view !== "session";
  $("#curbtn").classList.toggle("open", showCurrent);
  $("#curbtn").textContent = showCurrent ? "Hide Current Prompt" : "Show Current Prompt";
  box.classList.toggle("fold", currentFolded);
  $("#curfold").innerHTML = currentFolded ? "&#9656;" : "&#9662;";
  $("#curbody").innerHTML = currentPrompt ? renderText(currentPrompt)
    : `<div class="note" style="padding:6px 0">no prompt yet in this session</div>`;
}

// -- the selected session, in numbers --------------------------------------
// Under the send buttons: what the tile says, plus what only the transcript
// and process tree know - background work, the prompt cache, tokens, cost.
let statusData = null, statusFor = null, statusAt = 0, statusSkew = 0, statusFolded = false;
try { statusFolded = localStorage.getItem("ccontrol.statusFolded") === "1"; } catch (e) {}

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
      statusData = d; statusFor = want;
      if (d.now) statusSkew = d.now - Date.now()/1000;
    } catch (e) {}
  }
  drawStatus();
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
  const ag = st.agents || [], sh = st.shells || [], mo = st.monitors || [];
  if (have){
    cell("subagents", String(ag.length), ag.map(a => a.label).join("\n"), ag.length ? "hot" : "");
    cell("shells", String(sh.length), "commands it put in the background and has not seen finish\n" +
      sh.map(x => x.label).join("\n"), sh.length ? "hot" : "");
    cell("monitors", String(mo.length), mo.map(m => m.label).join("\n"), mo.length ? "hot" : "");
    if (d.shell_processes != null)
      cell("processes", String(d.shell_processes), "shell processes running under its claude " +
        "process - commands and monitors alike, read from the process tree",
        d.shell_processes ? "hot" : "");
  }
  if (ag.length) sum.push(plural(ag.length, "subagent"));
  if (sh.length) sum.push(plural(sh.length, "shell"));
  if (mo.length) sum.push(plural(mo.length, "monitor"));
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
  if (u.cost_usd != null){
    cell("cost", "$" + u.cost_usd.toFixed(2), "as Claude Code reports it to the statusline");
    sum.push("$" + u.cost_usd.toFixed(2));
  }
  const t = st.tokens;
  if (t){
    cell("tokens", `${kfmt(t.input + t.cache_read + t.cache_write)} in · ${kfmt(t.output)} out`,
      `input ${t.input}\ncache read ${t.cache_read}\ncache write ${t.cache_write}\noutput ${t.output}` +
      "\n(this session's transcript; subagents keep their own)");
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
  cell("branch", st.branch || "");
  if (st.version) cell("version", st.version, "Claude Code");
  if (d.note) cell("note", d.note);
  if (d.error) cell("error", d.error, "", "warn");
  const run = [...ag.map(x => ["agent", x]), ...sh.map(x => ["shell", x]), ...mo.map(x => ["monitor", x])]
    .map(([k, x]) => `<div class="ri"><span class="k">${k}${x.foreground ? " · now" : ""}</span>${
      esc(x.label || x.id)}${x.started ? " · " + age(x.started) : ""}${
      x.expires ? " · ends in " + span(x.expires - now) : ""}</div>`).join("");
  // Ends with the tile's work totals, so the folded line matches it. The
  // tile's cache countdown is left out: "cache -Nm" above already says it.
  const work = workHtml(s.work, Date.now() / 1000, "work", "cache");
  return `<div class="hd"><span class="sum">${sum.join(" · ")}${work ? " · " + work : ""}</span><span>${folded ? "&#9656;" : "&#9662;"}</span></div>` +
    `<div class="bd"><div class="grid">${cells.join("")}</div>${run ? `<div class="run">${run}</div>` : ""}</div>`;
}

function drawStatus(){
  const box = $("#status"), s = current();
  box.hidden = !s;
  if (!s) return;
  const d = statusFor === selected && statusData ? statusData : {};
  box.classList.toggle("fold", statusFolded);
  box.innerHTML = statusHtml(s, d, Date.now() / 1000 + statusSkew, statusFolded);
}

// -- jump between your prompts ---------------------------------------------
function jump(dir){
  const log = $("#log");
  const top = log.scrollTop, turns = [...log.querySelectorAll(".turn.you-turn")];
  // Compare where each prompt would be scrolled to, not where it sits, or the
  // prompt just jumped to counts as the next one down.
  const at = x => x.offsetTop - 8;
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
try { drafts = JSON.parse(localStorage.getItem("ccontrol.drafts") || "{}") || {}; } catch (e) { drafts = {}; }
function saveDrafts(){
  try { localStorage.setItem("ccontrol.drafts", JSON.stringify(drafts)); } catch (e) {}
}
function noteDraft(){
  if (draftFor === null) return;
  const v = $("#prompt").value;
  if (v) drafts[draftFor] = v; else delete drafts[draftFor];
  saveDrafts();
}
function syncDraft(){
  if (draftFor === selected) return;
  noteDraft();
  draftFor = selected;
  $("#prompt").value = (selected && drafts[selected]) || "";
}
// After a send: clear the draft it came from, and the box only if it still
// shows that session - the person may have moved on while it was in flight.
function clearPrompt(k){
  k = k === undefined ? draftFor : k;
  delete drafts[k]; saveDrafts();
  if (draftFor === k) $("#prompt").value = "";
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
    loadStatus(false);
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
  return `<b style="left:${(done * 100).toFixed(1)}%" title="${Math.round(done * 100)}% of the window elapsed"></b>`;
};

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
      <span class="bar"><i class="${cls}" style="width:${pct}%"></i>${timeTick(name, w.resets_at)}</span>
      <span>${Math.round(w.used)}%${w.resets_at ? " · " + resetLabel(w.resets_at) : ""}</span></div>${pace}`;
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
  $("#strip").hidden = v !== "session" || tab !== "log";
  if (v === "history"){ $("#title").textContent = "past sessions"; $("#sub").textContent =
      "search every session on this machine - titles, first prompts, and full text"; $("#hq").focus(); search(); }
  if (v === "insights"){ $("#title").textContent = "insights"; $("#sub").textContent =
      "tool failures across recent sessions, and notifications"; loadInsights(); }
  if (v === "session"){ drawHead(); refreshTab(true); }
  document.querySelectorAll("[data-view]").forEach(b => b.classList.toggle("on", b.dataset.view === v));
}

function setTab(t){
  tab = t;
  document.querySelectorAll("#tabs button[data-tab]").forEach(b => b.classList.toggle("on", b.dataset.tab === t));
  ["log","changes","activity","worktrees"].forEach(n => $("#" + n).hidden = n !== t);
  $("#strip").hidden = t !== "log";
  drawCurrent();
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
  if (what === "new"){
    const r = await post("/api/new", {session_id: key(s), cwd: s.cwd || null});
    $("#say").textContent = r.ok ? "opened a new session in " + (s.cwd || "its project") : (r.error || "refused");
    return;
  }
  const [kind, where] = what.split("-");
  const r = await post("/api/resume", {session_id: key(s), where, fork: kind === "fork",
    prompt: $("#prompt").value.trim() || null});
  $("#say").textContent = r.ok ? (r.where === "background" ? kind + "ed in the background · " + (r.id || "")
    : "opened a new tab: " + r.command) : (r.error || "refused");
  if (r.ok && where === "background") clearPrompt(key(s));
}

// -- queue -----------------------------------------------------------------
async function queue(){
  const s = current();
  const text = $("#prompt").value.trim();
  if (!s || !text) return;
  const r = await post("/api/queue", {session_id: key(s), prompt: text});
  if (r.ok){ clearPrompt(key(s)); $("#say").textContent = r.waiting ?
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
  if (r.ok){ clearPrompt(key(s)); $("#say").textContent = outcome(r); }
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
    noteDraft();
    $("#say").textContent = "dictated " + data.text.length + " chars";
  };
  rec.start();
  // The model lives in its own process, ended when idle; start it now so it
  // is built while you speak rather than after.
  post("/api/voice/warm", {}).catch(() => {});
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
  if (r.ok){ clearPrompt(); togglePicker(false); }
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
document.querySelectorAll("#tabs button[data-tab]").forEach(b => b.onclick = () => setTab(b.dataset.tab));
$("#curbtn").onclick = () => { showCurrent = !showCurrent; currentFolded = false; drawCurrent(); };
$("#curhd").onclick = () => { currentFolded = !currentFolded; drawCurrent(); };
$("#jprev").onclick = () => jump(-1);
$("#status").addEventListener("click", e => {
  if (!e.target.closest(".hd")) return;
  statusFolded = !statusFolded;
  try { localStorage.setItem("ccontrol.statusFolded", statusFolded ? "1" : ""); } catch (e) {}
  drawStatus();
});
$("#jnext").onclick = () => jump(1);
drawShape();
$("#shape").addEventListener("change", e => {
  const cat = e.target.dataset.shape;
  if (!cat) return;
  if (e.target.value === "full") delete shape[cat]; else shape[cat] = e.target.value;
  try { localStorage.setItem("ccontrol.shape", JSON.stringify(shape)); } catch (e) {}
  drawShape();
  drawLog();
});
$("#log").addEventListener("click", e => {
  const b = e.target.closest && e.target.closest("[data-earlier]");
  if (b) showEarlier(b.dataset.earlier === "all");
});
// `toggle` does not bubble; listen on the way down.
$("#log").addEventListener("toggle", e => {
  const d = e.target;
  if (!d.matches || !d.matches("details.cmp")) return;
  if (d.open) opened.add(d.dataset.k); else opened.delete(d.dataset.k);
}, true);
document.addEventListener("keydown", e => {
  if (!e.altKey || view !== "session" || tab !== "log") return;
  if (e.key === "ArrowUp"){ e.preventDefault(); jump(-1); }
  if (e.key === "ArrowDown"){ e.preventDefault(); jump(1); }
});
$("#prompt").addEventListener("input", noteDraft);
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
        '<button data-tab=worktrees>Worktrees</button>'
        '<button id=curbtn class=cur title="the prompt this session is working on">'
        'Show Current Prompt</button></div></div>'
        '<div id=sessionpane><div id=current hidden><div class=hd id=curhd>'
        '<span id=curcap>current prompt</span><span id=curfold>&#9662;</span></div>'
        '<div class=bd id=curbody></div></div><div id=log></div>'
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
        '<div id=strip><div id=shape title="how each kind of text shows in the chat"></div>'
        '<div id=jump><button class=ghost id=jprev title="previous prompt (alt+Up)">&#9650;</button>'
        '<button class=ghost id=jnext title="next prompt (alt+Down)">&#9660;</button></div></div>'
        '<div id=ask><textarea id=prompt placeholder="message this session…"></textarea>'
        '<div id=bar><button id=send title="Enter">Send</button>'
        '<button id=queue class=ghost title="hold until its current turn ends (alt+Enter)">Queue</button>'
        '<button id=bg class=ghost title="a new background session (ctrl+Enter)">Background</button>'
        '<button id=mic class=ghost title="dictate (F9)">&#127908; Dictate</button>'
        '<span id=say></span></div><div id=status hidden></div><div id=queued></div></div>'
        "</div></div><script>%s</script></body></html>" % (CSS, SCRIPT)
    )
