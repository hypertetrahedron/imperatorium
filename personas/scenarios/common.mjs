import fs from "node:fs";
import path from "node:path";
import { git } from "../lib/sandbox.mjs";

/** The first session the fleet brought up at or after `t0`. */
export async function newSession(d, label, t0, { timeout = 30000, bg = false } = {}) {
  const e = await d.fleetEvent(label, e => e.kind === "session-up" && (!bg || e.bg), { since: t0, timeout, sits: false });
  return e.sid;
}

/** Waits for a session's tile; returns ms from `t0`. */
export async function tileShows(d, sid, t0, { timeout = 30000, label = "its tile shows" } = {}) {
  await d.waitFor(label, d.tile(sid), { timeout });
  return Date.now() - t0;
}

/** What a session would have done in its folder: edit files and commit them. */
export function workIn(dir, files, msg) {
  for (const f of files) {
    fs.mkdirSync(path.dirname(path.join(dir, f)), { recursive: true });
    fs.appendFileSync(path.join(dir, f), `# ${msg}\n`);
  }
  git(dir, "add", "-A");
  git(dir, "-c", "user.name=persona", "-c", "user.email=persona@example.invalid", "commit", "-qm", msg);
}

export function worktrees(dir) {
  return git(dir, "worktree", "list", "--porcelain").split("\n").filter(l => l.startsWith("worktree ")).map(l => l.slice(9))
    .filter(p => path.resolve(p).toLowerCase() !== path.resolve(dir).toLowerCase());
}

/** The tile count right now. */
export const tileCount = d => d.page.locator("#sessions .s").count();
