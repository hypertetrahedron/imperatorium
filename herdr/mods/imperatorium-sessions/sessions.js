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
