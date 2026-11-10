import { when } from "../mods/imperatorium-history/history.js";
import assert from "node:assert/strict";
const now = Date.parse("2026-11-09T22:00:00") / 1000;
assert.match(when(now - 3600, now), /^today 21:00$/);
assert.equal(when((now - 86400 * 3) * 1000, now), "3d ago");
assert.equal(when("2026-10-01T10:00:00Z", now), "2026-10-01");
assert.equal(when(null, now), "");
console.log("history.js: all ok");
