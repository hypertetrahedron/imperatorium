// Pure helpers for the History dialog; node tests them (herdr/tests).

// "today 14:05", "yesterday", "3d ago", "2026-10-01" for a modified time
// (epoch seconds or milliseconds, or an ISO string).
export function when(modified, now) {
  let at = modified;
  if (typeof at === "string") at = Date.parse(at) / 1000;
  if (typeof at !== "number" || !isFinite(at)) return "";
  if (at > 1e12) at /= 1000;
  const age = now - at;
  if (age < 0) return "";
  const d = new Date(at * 1000);
  const days = Math.floor(age / 86400);
  if (age < 86400 && new Date(now * 1000).getDate() === d.getDate()) {
    return "today " + String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0");
  }
  if (days <= 1) return "yesterday";
  if (days < 14) return days + "d ago";
  return d.toISOString().slice(0, 10);
}
