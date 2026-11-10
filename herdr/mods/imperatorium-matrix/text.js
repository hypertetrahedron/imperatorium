// Shared by the Imperatorium mods: each mod folder carries its own copy
// (herdr-app loads a mod from its folder), and herdr/tests checks they match.

// Claude's line breaks are meant: markdown would join single newlines into
// one paragraph ("1\n2\n3" as "1 2 3"). Outside fenced code, a line that is
// followed by another non-blank line gets a hard break.
export function hardBreaks(text) {
  const lines = String(text ?? "").split("\n");
  let fenced = false;
  const out = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (/^\s*(```|~~~)/.test(line)) fenced = !fenced;
    const next = lines[i + 1];
    const breakable = !fenced && next !== undefined && line.trim() !== "" && next.trim() !== ""
      && !/^\s*(```|~~~)/.test(next) && !/^\s*(```|~~~)/.test(line)
      && !/\s{2}$/.test(line) && !/\$/.test(line) && !/^\s*\|/.test(line);
    out.push(breakable ? line + "  " : line);
  }
  return out.join("\n");
}
