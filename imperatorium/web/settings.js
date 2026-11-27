
let form = null, original = {};
const LIST = {paths: 1, hotkeys: 1, names: 1, phrases: 1};
const WHEN = {live: "applies at once", dispatcher: "after a dispatcher restart",
              hotkey: "after the palette restarts", streamdeck: "after the Stream Deck restarts"};

// What a field holds, as the form shows it, so "changed" is a string compare.
function shown(f, v){
  if (f.type === "bool") return !!v;
  if (LIST[f.type]) return (v || []).join("\n");
  if (f.type === "numbers") return (v || []).join(", ");
  return v == null ? "" : String(v);
}
function control(f){
  const id = "f-" + f.key.replace(/\W/g, "_"), v = shown(f, f.value);
  if (f.type === "bool") return `<input type=checkbox id="${id}"${v ? " checked" : ""}>`;
  if (f.type === "choice")
    return `<select id="${id}"${f.locked ? " disabled" : ""}>` + f.choices.map(c =>
      `<option value="${esc(c)}"${c === v ? " selected" : ""}>${esc(c || "(your own setting)")}</option>`).join("") +
      `</select>` + (f.locked ? `<span class="help">Set in config.json; change it there.</span>` : "");
  if (LIST[f.type]) return `<textarea id="${id}" rows=${Math.max(2, Math.min(8, (f.value || []).length + 1))}` +
    ` spellcheck=false>${esc(v)}</textarea>`;
  if (f.type === "key")
    return `<input type=text id="${id}" value="${esc(v)}" spellcheck=false data-record=1 ` +
      `placeholder="press the keys">`;
  if (f.type === "color")
    return `<span class="clr"><input type=color data-for="${id}" value="${esc(v || themeColor(f.name))}" ` +
      `aria-label="${esc(f.label)}"><input type=text id="${id}" value="${esc(v)}" placeholder="theme default" ` +
      `spellcheck=false size=9><button type=button class=small data-clear="${id}">Theme default</button>` +
      `<span class="ex" data-ex="${esc(f.name)}">Sample</span></span>`;
  if (f.type === "int" || f.type === "optint")
    return `<input type=number id="${id}" min="${f.min}" max="${f.max}" step=1 value="${esc(v)}">`;
  return `<input type=text id="${id}" value="${esc(v)}" spellcheck=false>`;
}
// The theme's own colour for a highlight (the CSS --hl-* custom property).
const themeColor = name => getComputedStyle(document.documentElement).getPropertyValue("--hl-" + name).trim() || "#000000";
const HEX = /^#[0-9a-fA-F]{6}$/;
function wireColors(){
  const ex = (id) => {
    const t = document.getElementById(id), s = t && t.closest(".clr").querySelector(".ex");
    if (s) s.style.color = HEX.test(t.value.trim()) ? t.value.trim() : themeColor(s.dataset.ex);
  };
  for (const pick of document.querySelectorAll("#set input[type=color][data-for]")){
    const text = document.getElementById(pick.dataset.for);
    pick.oninput = () => { text.value = pick.value; ex(text.id); };
    text.addEventListener("input", () => { if (HEX.test(text.value.trim())) pick.value = text.value.trim(); ex(text.id); });
    ex(text.id);
  }
  for (const b of document.querySelectorAll("#set [data-clear]")) b.onclick = () => {
    const text = document.getElementById(b.dataset.clear);
    text.value = "";
    text.closest(".clr").querySelector("input[type=color]").value = themeColor(text.closest(".clr").querySelector(".ex").dataset.ex);
    ex(text.id);
    mark();
  };
}
function read(f){
  const el = document.getElementById("f-" + f.key.replace(/\W/g, "_"));
  return f.type === "bool" ? el.checked : el.value;
}
// The value as the server wants it.
function wire(f, v){
  if (LIST[f.type]) return v.split("\n").map(x => x.trim()).filter(Boolean);
  if (f.type === "int") return v === "" ? null : Number(v);
  if (f.type === "optint") return v === "" ? null : Number(v);
  return v;
}

function draw(d){
  form = d; original = {};
  const by = {};
  for (const f of d.fields) { (by[f.section] = by[f.section] || []).push(f); original[f.key] = shown(f, f.value); }
  const wait = (d.waiting || []).map(p => `<div>${esc(WHEN[p] || p)}: <code>${esc(d.restart[p] || "")}</code></div>`).join("");
  const cats = d.sections.map(sec => ({id: sec.id, title: sec.title}));
  if ((d.shortcuts || []).length) cats.push({id: "keys", title: "Keyboard shortcuts"});
  const findWas = $("#find") ? $("#find").value : "";
  $("#set").innerHTML = `<nav id="cats" aria-label="Settings categories"><h1>Settings</h1>` +
    `<input type=search id=find placeholder="Find a setting" aria-label="Find a setting" spellcheck=false>` +
    `<div id=catlist role=tablist aria-orientation=vertical>${cats.map(c =>
      `<button type=button role=tab data-cat="${esc(c.id)}"><span>${esc(c.title)}</span><i class=cm></i></button>`).join("")}</div>` +
    `<div class="path" title="the config file these settings are saved to">${esc(d.path || "")}</div></nav><div id=pane>` +
    (wait ? `<div id="wait"><b>Saved, waiting for a restart</b>${wait}</div>` : "") +
    d.sections.map(sec => `<section id="sec-${esc(sec.id)}" data-cat="${esc(sec.id)}"><h2>${esc(sec.title)}</h2>` + (by[sec.id] || []).map(f => {
      const id = "f-" + f.key.replace(/\W/g, "_");
      return `<div class="f" data-key="${esc(f.key)}"><label for="${id}">${esc(f.label)}</label><div class="ctl">` +
        control(f) + (f.help ? `<span class="help">${esc(f.help)}</span>` : "") +
        `<span class="when ${f.applies === "live" ? "live" : ""}">${esc(WHEN[f.applies] || "")}</span>` +
        `<span class="err" hidden></span></div></div>`;
    }).join("") + (sec.id === "remote" ? hostsHtml(d.hosts) : "") +
      (sec.id === "notify" ? notifyExtrasHtml(d) : "") +
      (sec.id === "updates" ? updatesHtml() : "") + `</section>`).join("") +
    shortcutsHtml(d) + `<div id=nofind hidden>No setting matches.</div></div>`;
  $("#set").oninput = $("#set").onchange = e => { if (e.target.id !== "find") mark(); };
  wireCats(cats, findWas);
  wireColors();
  wireDesk();
  wireHosts();
  wireUpdates();
  // A key field takes the combination pressed in it rather than typed text.
  for (const el of document.querySelectorAll("#set [data-record]")) el.onkeydown = e => {
    if (e.key === "Tab" || (!e.ctrlKey && !e.altKey && !e.metaKey && e.key !== "Backspace")) return;
    e.preventDefault();
    if (e.key === "Backspace" && !e.ctrlKey && !e.altKey && !e.metaKey) { el.value = ""; mark(); return; }
    const c = comboOf(e);
    if (c) { el.value = c; mark(); }
  };
  mark();
}
// Read-only: every key the window answers to (ui.SHORTCUTS). A key that is
// itself a setting shows that setting's saved value.
function shortcutsHtml(d){
  const list = d.shortcuts || [];
  if (!list.length) return "";
  const val = k => { const f = d.fields.find(x => x.key === k); return f ? f.value : null; };
  const keys = k => {
    const v = val(k);
    if (v == null) return esc(k);
    return (Array.isArray(v) ? v : [v]).map(x => `<kbd>${esc(prettyKey(x))}</kbd>`).join(" or ");
  };
  // "Up / Down, Enter" -> each key boxed, the separators left as text.
  const SEP = /^(, | \/ | - )$/;
  const caps = text => text.split(/(, | \/ | - )/)
    .map(p => SEP.test(p) ? esc(p) : `<kbd>${esc(p)}</kbd>`).join("");
  let where = null, rows = "";
  for (const s of list) {
    if (s.where !== where) { where = s.where; rows += `<tr><th colspan=2>${esc(where)}</th></tr>`; }
    rows += `<tr><td>${/^[a-z]+\.[a-z_]+$/.test(s.keys) ? keys(s.keys) : caps(s.keys)}</td>` +
      `<td>${esc(s.what)}</td></tr>`;
  }
  return `<section id="keys" data-cat="keys"><h2>Keyboard shortcuts</h2><table class="keys">${rows}</table></section>`;
}

// -- categories -------------------------------------------------------------
// One category shows at a time; which one is in the address (#updates, from
// the window's update badge, opens that one) and kept for next time. Every
// section stays in the page, hidden, so Save still reads every field.
let cat = null;
function catFromHash(){ return decodeURIComponent(location.hash.replace(/^#(sec-)?/, "")); }
function showCat(id){
  const ids = [...document.querySelectorAll("#catlist [data-cat]")].map(b => b.dataset.cat);
  if (!ids.includes(id)) id = ids[0];
  cat = id;
  for (const s of document.querySelectorAll("#pane section[data-cat]")) s.hidden = s.dataset.cat !== id;
  for (const b of document.querySelectorAll("#catlist [data-cat]")) {
    const on = b.dataset.cat === id;
    b.classList.toggle("on", on); b.setAttribute("aria-selected", on ? "true" : "false");
    b.tabIndex = on ? 0 : -1;
  }
  $("#pane").scrollTop = 0;
  try { localStorage.setItem("imperatorium.settings.cat", id); } catch (e) {}
  if (catFromHash() !== id) history.replaceState(null, "", "#" + encodeURIComponent(id));
}
// Find: every field whose label, help or key has the text, across all
// categories; the category list dims while the results stand in for it.
function findSettings(q){
  q = q.trim().toLowerCase();
  $("#pane").classList.toggle("finding", !!q);
  $("#catlist").classList.toggle("finding", !!q);
  if (!q) {
    for (const f of document.querySelectorAll("#pane .f[data-key]")) f.hidden = false;
    $("#nofind").hidden = true;
    showCat(cat);
    return;
  }
  let any = false;
  for (const s of document.querySelectorAll("#pane section[data-cat]")) {
    const title = (s.querySelector("h2") || {}).textContent || "";
    const whole = title.toLowerCase().includes(q);
    let hits = 0;
    for (const f of s.querySelectorAll(".f[data-key]")) {
      const hit = whole || (f.textContent + " " + f.dataset.key).toLowerCase().includes(q);
      f.hidden = !hit; if (hit) hits++;
    }
    s.hidden = !hits && !whole;
    if (!s.hidden) any = true;
  }
  $("#nofind").hidden = any;
}
function markCats(){
  for (const b of document.querySelectorAll("#catlist [data-cat]")) {
    const s = document.querySelector(`#pane section[data-cat="${CSS.escape(b.dataset.cat)}"]`);
    b.classList.toggle("dirty", !!(s && s.querySelector(".f.dirty")));
    b.classList.toggle("bad", !!(s && s.querySelector(".f.bad")));
  }
}
function wireCats(cats, findWas){
  let saved = null;
  try { saved = localStorage.getItem("imperatorium.settings.cat"); } catch (e) {}
  const list = $("#catlist");
  list.onclick = e => {
    const b = e.target.closest("[data-cat]");
    if (!b) return;
    if ($("#find").value) { $("#find").value = ""; findSettings(""); }
    showCat(b.dataset.cat);
  };
  // Up and Down move through the categories, as in a list.
  list.onkeydown = e => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const bs = [...list.querySelectorAll("[data-cat]")];
    const i = bs.findIndex(b => b.dataset.cat === cat);
    const next = bs[Math.max(0, Math.min(bs.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)))];
    showCat(next.dataset.cat); next.focus();
  };
  $("#find").oninput = () => findSettings($("#find").value);
  $("#find").onkeydown = e => { if (e.key === "Escape") { $("#find").value = ""; findSettings(""); } };
  showCat(cat || catFromHash() || saved || (cats[0] || {}).id);
  if (findWas) { $("#find").value = findWas; findSettings(findWas); }
}
addEventListener("hashchange", () => { const id = catFromHash(); if (id && id !== cat) showCat(id); });
// Desktop notifications belong to the browser, not config.json: each
// browser asks for its own permission, so the switch takes effect at once
// and is kept beside the main window's other per-browser state.
function notifyExtrasHtml(d){
  const err = d.notify_error ? `<div class="f"><label>Last error</label><div class="ctl">` +
    `<span class="err">${esc(d.notify_error)}</span></div></div>` : "";
  if (!("Notification" in window)) return err;
  let on = false;
  try { on = localStorage.getItem("imperatorium.desknotify") === "1" && Notification.permission === "granted"; } catch (e) {}
  return `<div class="f" id="deskrow"><label for="desk">Desktop notification</label><div class="ctl">` +
    `<input type=checkbox id="desk"${on ? " checked" : ""}>` +
    `<span class="help">From the main window when a session starts waiting for you, only while that window ` +
    `is not in front. This browser only; it asks the browser's permission.` +
    (Notification.permission === "denied" ? " The browser has blocked notifications for this page - allow them in its site settings." : "") +
    `</span><span class="when live">applies at once</span></div></div>` + err;
}
function wireDesk(){
  const box = document.getElementById("desk");
  if (!box) return;
  box.onchange = async e => {
    e.stopPropagation();
    if (box.checked && Notification.permission !== "granted"){
      const p = await Notification.requestPermission();
      if (p !== "granted"){ box.checked = false; say("the browser did not allow notifications (" + p + ")", true); return; }
    }
    try { localStorage.setItem("imperatorium.desknotify", box.checked ? "1" : ""); } catch (x) {}
    say(box.checked ? "Desktop notifications on." : "Desktop notifications off.");
  };
}
// Each host with how its last poll went (`/api/hosts`), a Test button that
// checks it piece by piece (`/api/remote/test`, what `remote check` prints),
// a button that polls them all now - a parked one too - rather than at their
// next turn, and the form that adds a machine (`remote add`, run by the
// dispatcher: `/api/remote/add`, followed with `/api/remote/job`).
function hostsHtml(hosts){
  const list = !hosts || !hosts.length ? `<div class="f"><label>Hosts</label><div class="ctl">` +
    `<span class="help">No remote machines yet. Add one below.</span></div></div>` :
    `<div class="f"><label>Hosts</label><div class="ctl"><ul class="hosts">` + hosts.map(h =>
    `<li>${esc(h.label || h.name)}${h.label && h.label !== h.name ? ` (${esc(h.name)})` : ""}${
      h.tunnel ? " · tunnel" : ""} <span class="hst" data-host="${esc(h.name)}"></span> ` +
    `<button type=button class="small inl" data-test="${esc(h.name)}">Test</button>` +
    `<div class="rres" data-res="${esc(h.name)}" hidden></div></li>`).join("") +
    `</ul><button type=button class="small" id="rrefresh">Refresh now</button>` +
    `<span class="help">Polls every host at once instead of on its next turn (every 20 s, ` +
    `longer for a host that is failing).</span></div></div>`;
  return list + `<div class="f" id="radd"><label for="radd-host">Add a machine</label><div class="ctl">` +
    `<input type=text id="radd-host" spellcheck=false autocomplete=off placeholder="user@host or an ~/.ssh/config alias">` +
    `<input type=text id="radd-label" spellcheck=false autocomplete=off placeholder="label (optional)">` +
    `<label class="chk"><input type=checkbox id="radd-tunnel" checked> Tunnel: reply to its sessions ` +
    `and answer their permission requests from here</label>` +
    `<label class="chk"><input type=checkbox id="radd-claude"> Install or update Claude Code there if it is ` +
    `missing or too old (Anthropic's installer)</label>` +
    `<span><button type=button class="small" id="radd-test">Test connection</button> ` +
    `<button type=button class="small" id="radd-go">Add</button></span>` +
    `<div class="rres" id="radd-res" hidden></div>` +
    `<pre class="rlog" id="radd-log" hidden></pre><div id="radd-done" hidden></div>` +
    `<span class="help">Imperatorium logs in with an SSH key, never a password: if Test connection says ` +
    `the login was refused, it offers to set one up. Add checks a recent Claude Code is on ` +
    `the machine (installing it if you tick the box), copies Imperatorium's binary there, installs its reporting hooks ` +
    `(with a tunnel) and writes the machine into config.json; the dispatcher polls it after a restart.</span>` +
    `</div></div>`;
}
const MARK = {"ok": ["ok", "ok"], "MISSING": ["missing", "miss"], "--": ["not yet", "na"]};
// The answer of /api/remote/test as rows: what is there and what is missing.
function testHtml(d){
  if (!d || (!d.rows && d.error)) return `<div class="rhead miss">${esc((d && d.error) || "not answered")}</div>`;
  const head = d.ok ? (d.reachable ? "Connected. Everything it needs is there." : "")
    : d.reachable ? "Connected, but something is missing:" : "Cannot log in:";
  return `<div class="rhead ${d.ok ? "ok" : "miss"}">${esc(head)}</div><ul class="rrows">` + (d.rows || []).map(r => {
    const [word, cls] = MARK[r.mark] || [r.mark, "na"];
    return `<li class="${cls}"><b>${esc(word)}</b> ${esc(r.what)}: ${esc(r.detail)}</li>`;
  }).join("") + `</ul>` + (d.hint ? `<div class="help">${esc(d.hint)}</div>` : "") +
    (d.auth && d.host ? `<button type=button class="small" data-keyfor="${esc(d.host)}">Set up key login</button>` +
      `<div class="rkey" hidden></div>` : "") +
    (d.reachable && !d.ok && !d.configured ? `<div class="help">Add sets up what it can: the binary and the hooks, ` +
      `and Claude Code if you tick the box.</div>` : "");
}
// /api/remote/key's answer: the key (made just now, or the one there was)
// and the one command, run in a terminal, that copies it to the host. The
// host's password goes to ssh there, never through Imperatorium.
function keyHtml(d, host){
  if (!d || !d.ok) return `<div class="rhead miss">${esc((d && d.error) || "not answered")}</div>`;
  return `<div class="help">${d.made ? `Made a key for this computer: <code>${esc(d.key)}</code>. ` : ""}` +
    `Run this once in a terminal. It asks for ${esc(host)}'s password, then copies the key there; ` +
    `then press Test again.</div><pre class="rcmd">${esc(d.command)}</pre>` +
    `<button type=button class="small" data-copy>Copy</button>`;
}
async function testHost(host, tunnel, out){
  out.hidden = false; out.innerHTML = `<div class="rhead">Testing ${esc(host)}…</div>`;
  let d;
  try { d = await post("/api/remote/test", {host, tunnel}); } catch (e) { d = {error: "dispatcher not answering"}; }
  out.innerHTML = testHtml(d);
  return d;
}
function hostState(st, now){
  if (!st) return {text: "not polled yet", bad: false};
  if (st.error) return {text: `failing, next try in ${Math.max(0, Math.round(st.next_attempt - now))}s: ${st.error}`, bad: true};
  const ago = Math.max(0, Math.round(now - (st.polled_at || now)));
  return {text: `answered ${ago < 60 ? ago + "s" : Math.round(ago / 60) + " min"} ago`, bad: false};
}
async function drawHosts(){
  let d;
  try { d = await get("/api/hosts"); } catch (e) { return; }
  const now = Date.now() / 1000;
  for (const el of document.querySelectorAll("#set .hst")) {
    const s = hostState((d.status || {})[el.dataset.host], now);
    el.textContent = "- " + s.text; el.classList.toggle("bad", s.bad);
  }
}
// The add under way or last finished: its lines, then what comes next.
function drawAddJob(j){
  const log = document.getElementById("radd-log"), done = document.getElementById("radd-done");
  if (!log || !j || !j.job) return;
  log.hidden = false; log.textContent = (j.log || []).join("\n") + (j.running ? "\n…" : "");
  log.scrollTop = log.scrollHeight;
  for (const id of ["radd-go", "radd-test"]) document.getElementById(id).disabled = !!j.running;
  done.hidden = j.running;
  if (j.running) return;
  done.innerHTML = j.added
    ? `<div class="rhead ok">${esc(j.host)} is added. The dispatcher starts polling it when it restarts.</div>` +
      (j.can_restart ? `<button type=button class="small" id="radd-restart">Restart the dispatcher now</button>`
        : `<div class="help">Restart it with <code>imperatorium restart dispatcher</code>.</div>`)
    : `<div class="rhead miss">${esc(j.host)} was not added: the lines above say why.</div>`;
  const rb = document.getElementById("radd-restart");
  if (rb) rb.onclick = async () => {
    rb.disabled = true;
    let d;
    try { d = await post("/api/remote/restart", {}); } catch (e) { d = {error: "dispatcher not answering"}; }
    // A new dispatcher is noticed by checkBoot, which reloads this page.
    say(d.ok ? "Restarting the dispatcher - this page reloads when it is back." : (d.error || "not restarted"), !d.ok);
    if (!d.ok) rb.disabled = false;
  };
}
async function followAdd(){
  let j;
  try { j = await get("/api/remote/job"); } catch (e) { setTimeout(followAdd, 2000); return; }
  drawAddJob(j);
  if (j.running) { setTimeout(followAdd, 1000); return; }
  if (j.added && !document.querySelector(`#set .hst[data-host="${CSS.escape(j.host)}"]`)) {
    // Redraw for the new host and the restart banner, unless that would drop unsaved edits.
    if (form && Object.keys(changes()).length) say(`${j.host} added - save or revert your other changes, then reload to see it listed.`);
    else load();
  }
}
function wireHosts(){
  const btn = document.getElementById("rrefresh");
  if (btn) {
    drawHosts();
    btn.onclick = async () => {
      btn.disabled = true;
      try {
        const d = await post("/api/remote/refresh", {});
        say(d.ok ? `Polling ${d.hosts} host${d.hosts === 1 ? "" : "s"} now.` : (d.error || "not refreshed"), !d.ok);
      } catch (e) { say("dispatcher not answering", true); }
      // An answering host is back within seconds; one that times out, in ~30.
      for (const ms of [3000, 10000, 35000]) setTimeout(drawHosts, ms);
      setTimeout(() => { btn.disabled = false; }, 3000);
    };
  }
  for (const b of document.querySelectorAll("#set [data-test]")) b.onclick = async () => {
    b.disabled = true;
    await testHost(b.dataset.test, true, document.querySelector(`#set [data-res="${CSS.escape(b.dataset.test)}"]`));
    b.disabled = false;
  };
  // Key setup, under any test that says the login was refused.
  const pane = document.getElementById("sec-remote");
  if (pane) pane.onclick = async e => {
    const kb = e.target.closest("[data-keyfor]"), cb = e.target.closest("[data-copy]");
    if (kb) {
      kb.disabled = true;
      let d;
      try { d = await post("/api/remote/key", {host: kb.dataset.keyfor}); } catch (x) { d = {error: "dispatcher not answering"}; }
      const out = kb.nextElementSibling;
      out.hidden = false; out.innerHTML = keyHtml(d, kb.dataset.keyfor);
      kb.disabled = false;
    } else if (cb) {
      const text = cb.previousElementSibling.textContent;
      try { await navigator.clipboard.writeText(text); say("Copied."); }
      catch (x) { say("could not copy: select the command and copy it", true); }
    }
  };
  const host = document.getElementById("radd-host");
  if (!host) return;
  const value = () => host.value.trim(), tunnel = () => document.getElementById("radd-tunnel").checked;
  const need = () => { if (!value()) { say("Name the machine first: user@host or an ~/.ssh/config alias.", true); host.focus(); } return !!value(); };
  const test = document.getElementById("radd-test"), go = document.getElementById("radd-go");
  test.onclick = async () => {
    if (!need()) return;
    test.disabled = true;
    await testHost(value(), tunnel(), document.getElementById("radd-res"));
    test.disabled = false;
  };
  go.onclick = async () => {
    if (!need()) return;
    go.disabled = test.disabled = true;
    let d;
    try { d = await post("/api/remote/add", {host: value(), label: document.getElementById("radd-label").value.trim(),
      tunnel: tunnel(), install_claude: document.getElementById("radd-claude").checked}); }
    catch (e) { d = {error: "dispatcher not answering"}; }
    if (!d.ok) { say(d.error || "not started", true); go.disabled = test.disabled = false; return; }
    document.getElementById("radd-res").hidden = true;
    say(`Adding ${d.host}…`);
    followAdd();
  };
  host.onkeydown = e => { if (e.key === "Enter") { e.preventDefault(); test.click(); } };
  // An add started before this page was drawn (or before a reload).
  get("/api/remote/job").then(j => { drawAddJob(j); if (j && j.running) followAdd(); }).catch(() => {});
}
// The native binary's update check (`/api/update`): this version, the newest
// release the last check saw and what is new in it, and the button that
// installs it. Below it, a preview build, looked for only when asked
// ({preview: true}): the daily check and the badge stay on releases. The
// Python dispatcher has none and updates by git.
function updatesHtml(){
  return `<div class="f"><label>Version</label><div class="ctl"><span id="updst" class="help">…</span>` +
    `<pre id="updnotes" hidden></pre>` +
    `<span><button type=button class="small" id="updcheck">Check now</button> ` +
    `<button type=button class="small" id="updapply" hidden>Update</button></span>` +
    `<span class="help">Update downloads the new binary, checks its SHA-256 and restarts the ` +
    `dispatcher, the hotkey and the Stream Deck; Claude Code sessions keep running, and this ` +
    `window reloads when the dispatcher is back.</span></div></div>` +
    `<div class="f" id="prevf" hidden><label>Preview build</label><div class="ctl"><span id="prevst" class="help">…</span>` +
    `<pre id="prevnotes" hidden></pre>` +
    `<span><button type=button class="small" id="prevcheck">Check for a preview</button> ` +
    `<button type=button class="small" id="prevapply" hidden>Install preview</button></span>` +
    `<span class="help">Previews carry what is coming in the next release, sooner, and break more ` +
    `often. Installing one is checked and restarts things the same way as Update. To go back, ` +
    `<code>imperatorium update --rollback</code> puts back the binary it replaced; the next ` +
    `release newer than the preview is offered as usual.</span></div></div>`;
}
function agoText(s){
  s = Math.max(0, Math.round(s));
  return s < 60 ? s + "s" : s < 7200 ? Math.round(s / 60) + " min" : Math.round(s / 3600) + " h";
}
function updateLine(d, now){
  if (!d || !d.ok) return {text: "Updates come with the native binary; this dispatcher updates by git.", bad: false};
  const head = `Imperatorium ${d.current}`;
  if (d.applying) return {text: head + ` · updating to ${d.applying_to || d.latest}…`, bad: false};
  if (!d.checked_at) return {text: head + (d.check ? " · not checked yet" : " · checking is off"), bad: false};
  const ago = agoText(now - d.checked_at);
  if (d.error) return {text: head + ` · the check ${ago} ago failed: ${d.error}`, bad: true};
  return {text: head + (d.newer ? ` · ${d.latest} is out` : " · up to date") + ` (checked ${ago} ago)`, bad: false};
}
// The preview line: what the last preview check (if any) found.
function previewLine(d, now){
  const p = d && d.preview, onPreview = /-preview/.test((d && d.current) || "");
  if (!p || !p.checked_at) return {text: onPreview ? "This is a preview build. Look for a newer one here."
    : "Not checked. The daily check never offers a preview; look for one here.", bad: false};
  const ago = agoText(now - p.checked_at);
  if (p.error) return {text: `The check ${ago} ago failed: ${p.error}`, bad: true};
  if (p.newer) return {text: `Preview ${p.latest} is available (checked ${ago} ago)` +
    (p.installable === false ? ", but has no build for this machine" : ""), bad: false};
  return {text: (p.latest === d.current ? `You are on the newest preview, ${p.latest}`
    : `The newest preview, ${p.latest}, is not newer than this version`) + ` (checked ${ago} ago)`, bad: false};
}
async function drawUpdates(d){
  if (d === undefined) {
    try { d = await get("/api/update"); } catch (e) { d = null; }
  }
  const st = document.getElementById("updst");
  if (!st) return d;
  const now = Date.now() / 1000, line = updateLine(d, now);
  st.textContent = line.text; st.classList.toggle("bad", line.bad);
  const notes = document.getElementById("updnotes"), apply = document.getElementById("updapply");
  const fresh = !!(d && d.ok && d.newer);
  notes.hidden = !(fresh && d.notes);
  notes.textContent = fresh && d.notes ? `What's new in ${d.latest}\n\n${d.notes}` : "";
  apply.hidden = !(fresh && d.installable !== false && !d.applying);
  apply.textContent = fresh ? `Update to ${d.latest}` : "Update";
  document.getElementById("updcheck").hidden = !(d && d.ok);
  // The preview row: only where the binary can update itself.
  const pf = document.getElementById("prevf");
  pf.hidden = !(d && d.ok);
  if (!pf.hidden) {
    const p = d.preview || {}, pl = previewLine(d, now), ps = document.getElementById("prevst");
    ps.textContent = pl.text; ps.classList.toggle("bad", pl.bad);
    const pn = document.getElementById("prevnotes"), pa = document.getElementById("prevapply");
    const pfresh = !!(p.newer && !p.error);
    pn.hidden = !(pfresh && p.notes);
    pn.textContent = pfresh && p.notes ? `What's new in ${p.latest}\n\n${p.notes}` : "";
    pa.hidden = !(pfresh && p.installable !== false && !d.applying);
    pa.textContent = pfresh ? `Install preview ${p.latest}` : "Install preview";
  }
  return d;
}
async function postUpdate(path, body){
  try { return await post(path, body || {}); }
  catch (e) { return {ok: false, error: "not answered"}; }
}
function wireUpdates(){
  const check = document.getElementById("updcheck"), apply = document.getElementById("updapply");
  if (!check) return;
  const pcheck = document.getElementById("prevcheck"), papply = document.getElementById("prevapply");
  drawUpdates();
  check.onclick = async () => {
    check.disabled = true; say("checking for updates…");
    const d = await postUpdate("/api/update/check");
    drawUpdates(d);
    say(d.error ? "the check failed" : d.newer ? `${d.latest} is out` : "Imperatorium is up to date", !!d.error);
    check.disabled = false;
  };
  const install = (btn, preview) => async () => {
    btn.disabled = true;
    const d = await postUpdate("/api/update/apply", preview ? {preview: true} : {});
    say(d.ok ? `${preview ? "Installing preview" : "Updating to"} ${d.version}: the dispatcher restarts in a moment.`
      : (d.error || "not updated"), !d.ok);
    btn.disabled = false;
    drawUpdates();
  };
  apply.onclick = install(apply, false);
  pcheck.onclick = async () => {
    pcheck.disabled = true; say("looking for a preview…");
    const d = await postUpdate("/api/update/check", {preview: true});
    drawUpdates(d);
    const p = (d && d.preview) || {};
    say(!d.ok || p.error ? "the preview check failed" : p.newer ? `preview ${p.latest} is available` : "no newer preview",
      !d.ok || !!p.error);
    pcheck.disabled = false;
  };
  papply.onclick = install(papply, true);
}
function changes(){
  const out = {};
  for (const f of form.fields) {
    if (f.locked) continue;
    const v = read(f);
    if ((f.type === "bool" ? v : String(v)) !== original[f.key]) out[f.key] = wire(f, v);
  }
  return out;
}
function mark(){
  const ch = changes();
  for (const el of document.querySelectorAll("#set .f[data-key]")) el.classList.toggle("dirty", el.dataset.key in ch);
  const n = Object.keys(ch).length;
  markCats();
  $("#save").disabled = $("#revert").disabled = !n;
  if (n) say(n + " unsaved change" + (n === 1 ? "" : "s"));
}
function say(text, bad){ $("#msg").textContent = text; $("#msg").classList.toggle("bad", !!bad); }

async function load(){
  try {
    const d = await get("/api/settings");
    if (!d.ok) { say(d.error || "settings unavailable", true); return; }
    draw(d); say("");
  } catch (e) { say("dispatcher not answering", true); }
}
async function save(){
  const ch = changes();
  if (!Object.keys(ch).length) return;
  $("#save").disabled = true; say("saving…");
  let d;
  try {
    d = await post("/api/settings", {changes: ch});
  } catch (e) { say("dispatcher not answering", true); $("#save").disabled = false; return; }
  for (const el of document.querySelectorAll("#set .f")) {
    el.classList.remove("bad");
    const e = el.querySelector(".err"); if (e) { e.hidden = true; e.textContent = ""; }
  }
  if (!d.ok) {
    for (const [k, m] of Object.entries(d.errors || {})) {
      const el = document.querySelector(`#set .f[data-key="${CSS.escape(k)}"]`);
      if (el) { el.classList.add("bad"); const e = el.querySelector(".err"); e.hidden = false; e.textContent = m; }
    }
    markCats();
    // A refused value in another category: show that one.
    const first = document.querySelector("#pane .f.bad");
    const in_ = first && first.closest("section[data-cat]");
    if (in_ && in_.dataset.cat !== cat && !$("#find").value) showCat(in_.dataset.cat);
    say(d.error || "not saved", true); $("#save").disabled = false; return;
  }
  draw(d);
  say("Saved." + ((d.restart || []).length ? " Some changes need a restart (see the top)." : " Applied."));
}
// A new dispatcher holds a new window token, so saving from the old page
// would be refused. Reload when nothing is unsaved; otherwise say so.
const onNewDispatcher = () => {
  if (form && Object.keys(changes()).length) say("The dispatcher restarted - copy your changes, then reload this page.", true);
  else location.reload();
};
whileVisible(5000, () => checkBoot(onNewDispatcher));
$("#save").onclick = save;
$("#revert").onclick = () => { draw(form); say("changes discarded"); };
addEventListener("keydown", e => { if ((e.ctrlKey || e.metaKey) && e.key === "s") { e.preventDefault(); save(); } });
addEventListener("beforeunload", e => { if (form && Object.keys(changes()).length) { e.preventDefault(); e.returnValue = ""; } });
load();
