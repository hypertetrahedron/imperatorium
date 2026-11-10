// Files every mod carries a copy of must stay identical.
import { readdirSync, readFileSync, existsSync } from "node:fs";
import assert from "node:assert/strict";
const root = new URL("../mods/", import.meta.url);
const mods = readdirSync(root).filter((n) => n.startsWith("imperatorium-"));
for (const shared of ["client.js", "text.js"]) {
  const copies = mods.map((m) => new URL(m + "/" + shared, root)).filter((u) => existsSync(u));
  assert.ok(copies.length >= 2, shared + " should be shared");
  const first = readFileSync(copies[0], "utf-8");
  for (const c of copies) assert.equal(readFileSync(c, "utf-8"), first, c.pathname + " differs");
}
console.log("shared files: all ok");
