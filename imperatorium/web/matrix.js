
const $ = s => document.querySelector(s);

// Tiles are 1 wide by 1.5 high: a chat reads down, so a tile is a column.
const RATIO = 1.5, GAP = 8, EDGE = 10;

// The grid with the biggest tiles that fit: every column count is tried, each
// with as many rows as it needs, and the tile is as large as the tighter of
// width and height allows at that ratio. On a tie the grid with fewer empty
// cells wins. Empty cells only ever fall at the end of the last row, since a
// grid of `cols` columns never needs more than `cols - 1` of them.
function layout(n, W, H, gap = GAP, ratio = RATIO){
  n = Math.max(1, n);
  let best = null;
  for (let cols = 1; cols <= n; cols++){
    const rows = Math.ceil(n / cols);
    const w = Math.min((W - gap * (cols - 1)) / cols, (H - gap * (rows - 1)) / rows / ratio);
    if (!(w > 0)) continue;
    const spare = rows * cols - n;
    if (!best || w > best.w + 0.5 || (w > best.w - 0.5 && spare < best.spare))
      best = {cols, rows, w, h: w * ratio, spare};
  }
  return best || {cols: 1, rows: 1, w: 0, h: 0, spare: 0};
}

const money = v => typeof v === "number" ? "$" + v.toFixed(2) : "";
function count(v){
  if (!(v > 0)) return "0";
  if (v >= 1e9) return (v / 1e9).toFixed(1) + "B";
  if (v >= 1e6) return (v / 1e6).toFixed(1) + "M";
  if (v >= 1e3) return (v / 1e3).toFixed(1) + "k";
  return String(Math.round(v));
}

function project(cwd){
  const parts = String(cwd || "").split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || "";
}

// What a working session with no step in flight is waiting on: the same
// counts as the main window's tile (subagents / background shells / monitors
// / shell processes), in words. "" when it is not working or has none, so a
// session waiting on a long build is not mistaken for one stuck.
const WORK = [["agents", "subagent"], ["shells", "background shell"],
              ["monitors", "monitor"], ["processes", "process"]];
function waitingOn(t){
  const dot = stateOf(t)[0];
  if (t.current || (dot !== "d-busy" && dot !== "d-idle")) return "";
  const w = t.work || {};
  return WORK.filter(([k]) => w[k] > 0)
    .map(([k, word]) => `${w[k]} ${word}${w[k] === 1 ? "" : word.endsWith("s") ? "es" : "s"}`).join(" · ");
}

// In a turn with nothing in flight and nothing it started to wait on: Claude
// is writing, or between one step and the next. Without a line for it the
// tile showed no sign of work at all. Not for a session idle between turns
// with a background task: that one is waiting, and says so.
function agentWorking(t){
  return !t.current && stateOf(t)[0] === "d-busy" && (!!RUNNING[t.status] || t.state === "working");
}

// Claude's replies, a rule between turns, and under them the step in flight.
function tileBody(t){
  let html = "", turn = null;
  for (const r of t.replies || []){
    const rule = turn !== null && r.turn !== turn;
    turn = r.turn;
    html += `<div class="rp${rule ? " newturn" : ""}">${renderText(r.text)}</div>`;
  }
  if (!html && !t.current)
    html = `<div class="none">${esc(t.error || "no replies yet")}</div>`;
  const c = t.current;
  if (c && c.kind === "tool")
    html += `<div class="cur" title="${esc(c.name + " " + (c.summary || ""))}"><span class="spin"></span>` +
      `<b>${esc(c.name)}</b><span class="what">${esc(c.summary || "")}</span>` +
      (c.more ? `<span class="more" title="more calls running">+${c.more}</span>` : "") + `</div>`;
  else if (c && c.kind === "thinking")
    html += `<div class="cur thought"><span class="spin"></span><span class="what">thinking${
      c.text ? " · " + esc(c.text) : ""}</span></div>`;
  const wait = waitingOn(t);
  if (wait)
    html += `<div class="cur wait" title="working, waiting on what it started"><span class="spin"></span>` +
      `<span class="what">waiting on ${esc(wait)}</span></div>`;
  else if (agentWorking(t))
    html += `<div class="cur wait" title="in a turn - writing or about to call a tool"><span class="spin"></span>` +
      `<span class="what">Agent working</span></div>`;
  return html;
}

function tileHead(t){
  const [dot, label] = stateOf(t);
  const meta = [money(t.cost_usd),
    typeof t.context_pct === "number" ? Math.round(t.context_pct) + "%" : ""].filter(Boolean).join(" · ");
  const where = project(t.cwd);
  return `<span class="dot ${dot}" title="${esc(label)}"></span>` +
    `<span class="nm" title="${esc(t.name || "")}${where ? " - " + esc(where) : ""}">${
      esc(t.name || t.sessionId || "")}</span>` +
    (meta ? `<span class="meta" title="the whole session's estimated cost · context used">${meta}</span>` : "");
}

function statusTile(sp){
  sp = sp || {tokens: {}, each: [], limits: {}};
  const tk = sp.tokens || {};
  // The sidebar's meters and model mix, open, at the top.
  const meters = meterHtml({limits: sp.limits, plan: sp.plan,
                            spend: sp.global ? {global: sp.global, current: {models: sp.models}} : null},
                           {lines: false});
  const rows = (sp.each || []).map(e => `<tr><td>${esc(e.name || "")}</td>` +
    `<td class="n">${count(e.tokens)}</td><td class="n">${money(e.cost_usd) || "-"}</td></tr>`).join("");
  const n = sp.sessions || 0;
  // Global: this machine's sends in the account's windows, open or not.
  const wins = ((sp.global || {}).windows || []).map(w => `<tr title="${esc(windowTitle(w) + "\n" + spendDetail(w))}">` +
    `<td>${esc(SPAN_TITLES[w.name] || w.name)}</td><td class="n">${count(w.total_tokens)}</td>` +
    `<td class="n">${money(w.cost_usd || 0)}</td></tr>`).join("");
  return `${meters ? `<div class="meters top">${meters}</div>` : ""}
    <h2>Current sessions</h2>
    <div class="big">${money(sp.cost_usd || 0)}</div>
    <div class="sub">${n} open session${n === 1 ? "" : "s"}, each over its whole life${
      sp.costed < n ? ` · cost known for ${sp.costed}` : ""}${sp.complete === false ? " · counting…" : ""}</div>
    <div class="big tok">${count(sp.total_tokens)} tokens</div>
    <div class="sub">in ${count(tk.input)} · out ${count(tk.output)} · cache read ${
      count(tk.cache_read)} · cache write ${count(tk.cache_write)}</div>
    ${wins ? `<div class="sec" title="${esc(SPEND_NOTE)}"><h2>Global</h2><table>${wins}</table></div>` : ""}
    ${rows ? `<div class="sec"><h2>By session</h2><table>${rows}</table></div>` : ""}`;
}

let data = {tiles: [], spend: null};

// Tiles are kept between polls and only refilled when what they show changes,
// so a tile someone has scrolled up in stays where they put it.
function draw(){
  const grid = $("#grid");
  const W = innerWidth - 2 * EDGE, H = innerHeight - 2 * EDGE;
  const tiles = data.tiles || [];
  // With the status tile always shown, it is laid out as one more tile.
  const always = data.status_always !== false;
  // The ratio picks the grid; then the tiles grow to fill the window, giving
  // up the ratio rather than leaving bands of empty space around it.
  const L = layout(tiles.length + (always ? 1 : 0), W, H);
  const cw = Math.max(0, (W - GAP * (L.cols - 1)) / L.cols);
  const rh = Math.max(0, (H - GAP * (L.rows - 1)) / L.rows);
  Object.assign(grid.style, {
    gridTemplateColumns: `repeat(${L.cols}, ${cw}px)`, gridAutoRows: `${rh}px`, gap: GAP + "px",
    left: EDGE + "px", top: EDGE + "px"});

  const keep = new Map([...grid.querySelectorAll(".tile[data-k]")].map(el => [el.dataset.k, el]));
  const order = [];
  for (const t of tiles){
    const k = t.key || t.sessionId;
    let el = keep.get(k);
    keep.delete(k);
    if (!el){
      el = document.createElement("div");
      el.className = "tile";
      el.dataset.k = k;
      el.dataset.sid = t.sessionId || k;
      el.title = "double-click to show this session in the main window";
      el.innerHTML = `<div class="th"></div><div class="why" hidden></div><div class="tb"></div>`;
    }
    const [dot, label] = stateOf(t);
    el.classList.toggle("attn", dot === "d-attn");
    el.classList.toggle("fail", dot === "d-fail");
    el.classList.toggle("comp", dot === "d-comp");
    el.classList.toggle("busy", dot === "d-busy");
    el.classList.toggle("tasks", dot === "d-idle");
    const head = tileHead(t);
    if (el.dataset.head !== head){ el.querySelector(".th").innerHTML = head; el.dataset.head = head; }
    const why = el.querySelector(".why");
    why.hidden = !t.attention;
    why.textContent = t.attention ? label : "";
    const body = tileBody(t);
    if (el.dataset.body !== body){ el.querySelector(".tb").innerHTML = body; el.dataset.body = body; }
    order.push(el);
  }
  keep.forEach(el => el.remove());

  // The empty cells at the end become one status tile; none when the grid is
  // full, unless it is always shown, when it has its own cell as well. With
  // no sessions at all, the status tile is the whole matrix.
  const span = !tiles.length ? 1 : L.spare + (always ? 1 : 0);
  let st = grid.querySelector(".tile.stat");
  if (span){
    if (!st){ st = document.createElement("div"); st.className = "tile stat"; }
    st.style.gridColumn = `span ${span}`;
    const html = statusTile(data.spend);
    if (st.dataset.sig !== html){ st.innerHTML = html; st.dataset.sig = html; }
    order.push(st);
  } else if (st) st.remove();
  order.forEach((el, i) => { if (grid.children[i] !== el) grid.insertBefore(el, grid.children[i] || null); });
}

// The main window's button lights while this window is up, and closes it
// through here - which still works after the main window has reloaded and
// lost the handle it opened this with.
const channel = "BroadcastChannel" in window ? new BroadcastChannel("imperatorium-matrix") : null;
let answered = null;
if (channel){
  channel.onmessage = e => {
    if (e.data === "close") window.close();
    else if (e.data && e.data.selected) answered = e.data.selected;
  };
  const beat = () => channel.postMessage("open");
  beat();
  setInterval(beat, 1000);
  setInterval(() => checkBoot(() => location.reload()), 5000);
  addEventListener("pagehide", () => channel.postMessage("closed"));
}

async function get(url){
  const r = await fetch(url, {cache: "no-store"});
  if (!r.ok) throw new Error(r.status);
  return r.json();
}

async function poll(){
  try {
    data = await get("/api/matrix");
    $("#gone").hidden = true;
    draw();
  } catch (e) { $("#gone").hidden = false; }
  setTimeout(poll, 1500);
}

// Double click: show that session in the main window. Raising a window
// needs this click, so the window that opened this one is raised from here;
// the main window selects the session when asked on the channel. If no main
// window answers, a new one is opened on that session.
function openInMain(sid){
  answered = null;
  try { if (window.opener && !window.opener.closed) window.opener.focus(); } catch (e) {}
  if (channel) channel.postMessage({select: sid});
  setTimeout(() => {
    if (answered !== sid) window.open("/?session=" + encodeURIComponent(sid), "imperatorium-main");
  }, channel ? 400 : 0);
}

$("#grid").addEventListener("dblclick", e => {
  const el = e.target.closest(".tile[data-k]");
  if (!el) return;
  getSelection().removeAllRanges();
  openInMain(el.dataset.sid);
});

addEventListener("resize", draw);
poll();
