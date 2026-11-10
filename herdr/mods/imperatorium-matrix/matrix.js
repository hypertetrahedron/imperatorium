// Pure helpers for the Chat Matrix; node tests them (herdr/tests).

// Columns and rows for n cells: as square as the cells allow, wider than tall.
export function grid(n) {
  if (n <= 0) return { cols: 1, rows: 0 };
  const cols = Math.ceil(Math.sqrt(n));
  return { cols, rows: Math.ceil(n / cols) };
}

export function money(usd) {
  if (typeof usd !== "number" || !isFinite(usd)) return "";
  return usd >= 100 ? "$" + Math.round(usd) : "$" + usd.toFixed(2);
}

// The last `n` replies' text, oldest first.
export function lastReplies(replies, n) {
  return (replies ?? []).slice(-n).map((r) => String(r?.text ?? r ?? "")).filter(Boolean);
}

// The step a tile shows: "Bash git status", "thinking: ...", "(+2 more)".
export function stepLine(cur) {
  if (!cur) return "";
  const head = cur.kind === "thinking" ? "thinking: " + (cur.text ?? "")
    : (cur.name ?? "tool") + " " + (cur.summary ?? "");
  return head.trim() + (cur.more ? "  (+" + cur.more + " more)" : "");
}
