// Pure helpers for the sessions sidebar; node tests them (herdr/tests).

// Waiting on you first (longest waiting first), then working, then idle,
// then the rest; by name within each.
export function order(rows) {
  const rank = (s) => s.attention ? 0 : s.status === "busy" ? 1 : s.pid ? 2 : 3;
  return [...(rows ?? [])].sort((a, b) =>
    rank(a) - rank(b)
    || (rank(a) === 0 ? (a.attention_since ?? 0) - (b.attention_since ?? 0) : 0)
    || String(a.short_name || a.name || "").localeCompare(String(b.short_name || b.name || "")));
}

const WINDOWS = [["five_hour", "5h"], ["seven_day", "7d"]];

// The usage meters to draw: [{label, pct, text}], from /api/usage.
export function meterRows(usage, now) {
  const out = [];
  const limits = usage?.limits ?? {};
  for (const [key, label] of WINDOWS) {
    const w = limits[key];
    if (!w || typeof w.used !== "number") continue;
    const resets = w.resets_at ?? w.reset_at;
    out.push({
      label,
      pct: Math.max(0, Math.min(100, w.used)),
      text: Math.round(w.used) + "%" + (resets && resets > now ? " · resets in " + span(resets - now) : ""),
    });
  }
  return out;
}

export function span(seconds) {
  const s = Math.max(0, Math.round(seconds));
  if (s < 3600) return Math.max(1, Math.round(s / 60)) + "m";
  if (s < 86400) return Math.floor(s / 3600) + "h" + String(Math.round((s % 3600) / 60)).padStart(2, "0");
  return Math.floor(s / 86400) + "d" + Math.round((s % 86400) / 3600) + "h";
}

// "3m" since a status change (epoch seconds), "" when unknown.
export function ago(at, now) {
  if (!at) return "";
  const s = now - at;
  if (s < 60) return "now";
  return span(s);
}

// [tab id, name] for each unnamed tab Imperatorium started (id "imp-" plus
// the session id's first 12 characters) whose session is known.
export function tabNames(spaces, sessions) {
  const out = [];
  for (const sp of spaces ?? []) {
    for (const tab of sp.tabs ?? []) {
      const id = String(tab.id ?? "");
      const local = id.includes("/") ? id.slice(id.lastIndexOf("/") + 1) : id;
      if (tab.named || !local.startsWith("imp-")) continue;
      const s = (sessions ?? []).find((r) => String(r.sessionId ?? "").slice(0, 12) === local.slice(4));
      const name = s && (s.display_name || s.name);
      if (name) out.push([id, String(name).slice(0, 32)]);
    }
  }
  return out;
}

const CHAT = "imperatorium-chat";

// A chat tab's name, for a session that has no herdr terminal here.
export function chatTabName(s) {
  const name = s.display_name || s.name || s.short_name || s.project || String(s.sessionId ?? "").slice(0, 8);
  return "Chat · " + String(name).slice(0, 28);
}

function sameFolder(a, b) {
  const norm = (p) => String(p ?? "").replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  return Boolean(a) && Boolean(b) && norm(a) === norm(b);
}

// The herdr commands that show a session's chat as a native pane, in order.
// In an hc terminal here: its tab gets a chat pane beside the terminal (once;
// later clicks just go to the terminal). Anywhere else (Windows Terminal, a
// remote host): a tab whose only pane is the chat, in the space for its
// folder when there is one; a second click goes back to that tab.
export function chatCommands(s, panes, spaces) {
  const props = { session: s.sessionId };
  const term = s.hc?.address && !s.remote ? (panes ?? []).find((p) => p.terminal === s.hc.address) : null;
  if (term) {
    const cmds = [["pane.focus", { terminal: s.hc.address }]];
    const has = (panes ?? []).some((p) => p.tab === term.tab && p.mod === CHAT);
    if (!has) cmds.push(["pane.split", { direction: "right", pane: term.pane, mod: CHAT, props }]);
    return cmds;
  }
  const name = chatTabName(s);
  for (const sp of spaces ?? []) {
    const tab = (sp.tabs ?? []).find((t) => t.name === name);
    if (tab) return [["space.select", { id: sp.id }], ["tab.select", { id: tab.id }]];
  }
  const sp = (spaces ?? []).find((x) => sameFolder(x.folder, s.cwd));
  const args = { name, mod: CHAT, props };
  if (sp) args.space = sp.id;
  return (spaces ?? []).length ? [["tab.new", args]] : [];
}
