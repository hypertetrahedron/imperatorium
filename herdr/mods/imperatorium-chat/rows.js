// Conversation items (the dispatcher's /api/conversation) to the rows the
// chat draws. Pure: no herdr or gpui imports, so node can test it.

// [kind, label] in strip order. The kinds are the window's Display Shapes.
export const KINDS = [
  ["prompt", "Prompts"],
  ["text", "Replies"],
  ["thinking", "Thinking"],
  ["tool", "Tools"],
  ["result", "Results"],
  ["note", "System"],
];

// mode -> the label the strip shows.
export const MODES = { full: "Full", compact: "Compact", hidden: "Hidden" };
const ORDER = ["full", "compact", "hidden"];

export function nextMode(m) {
  return ORDER[(ORDER.indexOf(m) + 1) % ORDER.length];
}

export function firstLine(text) {
  const s = String(text ?? "").replace(/^\s+/, "");
  const i = s.indexOf("\n");
  return (i < 0 ? s : s.slice(0, i)).slice(0, 400);
}

export function toolLine(r) {
  return String(r.summary ?? "").replace(/\s+/g, " ").slice(0, 300);
}

const LABELS = { you: "You", claude: "Claude", tool: "Tools", system: "System" };

// A turn starts at each prompt. Returns the rows for the last `drawn` turns,
// with `earlier` (turns not drawn) as a property on the array.
export function rowsOf(items, modes, drawn) {
  const turns = [];
  for (const it of items ?? []) {
    const isPrompt = it.speaker === "you" && !it.note;
    if (isPrompt || turns.length === 0) turns.push([]);
    turns[turns.length - 1].push(it);
  }
  const from = Math.max(0, turns.length - (drawn || turns.length));
  const rows = [];
  let last = null;
  for (let ti = from; ti < turns.length; ti++) {
    for (const it of turns[ti]) {
      const idx = items.indexOf(it);
      const speaker = it.note ? "system" : (it.speaker ?? it.role);
      const blocks = (it.blocks ?? []).map((b, bi) => ({ b, bi }))
        .filter(({ b }) => modeFor(b.kind, speaker, it, modes) !== "hidden");
      if (it.note && modes.note !== "hidden") {
        rows.push({ key: idx + ":n", kind: "note", label: it.note, text: textOf(it), mode: modes.note });
        last = "system";
        continue;
      }
      if (blocks.length === 0) continue;
      if (speaker !== last && speaker !== "tool") {
        rows.push({ key: idx + ":h", kind: "speaker", speaker, label: it.via ? LABELS[speaker] + " · " + it.via : (LABELS[speaker] ?? speaker) });
        last = speaker;
      }
      for (const { b, bi } of blocks) {
        const key = idx + ":" + bi;
        const mode = modeFor(b.kind, speaker, it, modes);
        if (b.kind === "text") rows.push({ key, kind: "text", text: b.text ?? "", mode });
        else if (b.kind === "thinking") rows.push({ key, kind: "thinking", text: b.text ?? "", mode });
        else if (b.kind === "tool") rows.push({ key, kind: "tool", name: b.name ?? "tool", summary: b.summary ?? "", diff: b.diff ?? "", mode });
        else if (b.kind === "result") {
          const text = String(b.text ?? "");
          rows.push({ key, kind: "result", text, is_error: Boolean(b.is_error), lines: text.split("\n").length, mode });
        }
      }
    }
  }
  rows.earlier = from;
  return rows;
}

function modeFor(kind, speaker, it, modes) {
  if (kind === "text") return speaker === "you" ? modes.prompt : modes.text;
  return modes[kind] ?? "full";
}

function textOf(it) {
  return (it.blocks ?? []).filter((b) => b.kind === "text").map((b) => b.text).join("\n");
}

// Parked requests (/api/pending) for one session, oldest first.
export function pendingFor(pending, sessionId) {
  return (pending ?? []).filter((p) => p.session_id === sessionId);
}

// {question: answer} for /api/decide from the labels picked per question,
// or null while a question has none. A multi-select answer is its labels
// joined with ", ", as the terminal sends it.
export function answerFor(item, picks) {
  const out = {};
  for (const q of item.questions ?? []) {
    const got = (picks[q.question] ?? []).filter(Boolean);
    if (!got.length) return null;
    out[q.question] = got.join(", ");
  }
  return out;
}

export { hardBreaks } from "./text.js";
