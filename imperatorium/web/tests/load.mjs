// Loads the window's JavaScript for the tests in this folder.
//
// The window is plain files in imperatorium/web/. A page is assembled from
// them the way the dispatcher does it (shared.js + app.js for the window,
// shared.js + matrix.js for the chat matrix, shared.js + settings.js for
// Settings), and a test takes the piece it wants out of that text by two
// markers - so it runs the shipped code rather than a copy that could drift.
// Every marker the tests use is named in a test file, never here: when a
// marker moves, the test that names it fails with "marker not found".
//
// Pieces run in a fresh vm context with a stub or two, which is what
// `node -` with a script on stdin was: nothing from the page (no document,
// no localStorage) unless the test defines it. A piece writes its answer with
// process.stdout.write, as before.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import assert from "node:assert/strict";

export const WEB = dirname(dirname(fileURLToPath(import.meta.url)));
export const FONTS = join(dirname(WEB), "fonts");

// Bytes as written (LF; see .gitattributes), like the dispatcher's own read.
const web = (name) => readFileSync(join(WEB, name), "utf8");

export const CSS = web("app.css");
export const SHARED = web("shared.js");
export const SCRIPT = SHARED + web("app.js");
export const MATRIX_CSS = web("matrix.css");
export const MATRIX_SCRIPT = SHARED + web("matrix.js");
export const SETTINGS_CSS = web("settings.css");
export const SETTINGS_SCRIPT = SHARED + web("settings.js");

// A page with its {{STYLES}} and {{SCRIPTS}} marks filled in, inline, as the
// dispatcher serves it (without the per-start meta tags).
const page = (name, styles, scripts) =>
  web(name + ".html")
    .replace("{{STYLES}}", () => "<style>" + styles.join("") + "</style>")
    .replace("{{SCRIPTS}}", () => "<script>" + scripts.join("") + "</script>");
export const renderShell = () => page("shell", [CSS], [SCRIPT]);
export const renderMatrix = () => page("matrix", [CSS, MATRIX_CSS], [MATRIX_SCRIPT]);
export const renderSettings = () => page("settings", [CSS, SETTINGS_CSS], [SETTINGS_SCRIPT]);

// -- cutting a script up ------------------------------------------------------

/** Index of `marker` in `s` at or after `from`; a missing marker throws. */
export function find(s, marker, from = 0) {
  const i = s.indexOf(marker, from);
  if (i < 0) throw new Error("marker not found: " + JSON.stringify(marker));
  return i;
}
/** `s` from the start of marker `a` up to (not including) the next `b`. */
export const between = (s, a, b) => {
  const i = find(s, a);
  return s.slice(i, find(s, b, i));
};
/** `s` from the start of marker `a` to its end. */
export const tail = (s, a) => s.slice(find(s, a));

/** The `const esc =` helper line and what the renderer needs after it. */
export const ESC = between(SCRIPT, "const esc =", "\n\n");

// -- running a piece ------------------------------------------------------------

function context(out) {
  return vm.createContext({
    process: { stdout: { write: (s) => void out.push(String(s)) } },
    console,
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
  });
}

/** Run `js` in a fresh context; what it wrote with process.stdout.write. */
export function evalJs(js) {
  const out = [];
  vm.runInContext(js, context(out), { timeout: 30000 });
  return out.join("");
}
/** evalJs, parsed as the JSON the piece wrote. */
export const evalJson = (js) => JSON.parse(evalJs(js));

/** Like evalJs for a piece that answers from a timer: resolves with the
 * first thing written. */
export function evalJsLater(js, ms = 5000) {
  const out = [];
  return new Promise((resolve, reject) => {
    const ctx = vm.createContext({
      process: {
        stdout: {
          write: (s) => {
            out.push(String(s));
            resolve(out.join(""));
          },
        },
      },
      console,
      setTimeout,
      clearTimeout,
    });
    const t = setTimeout(() => reject(new Error("the piece wrote nothing")), ms);
    t.unref();
    try {
      vm.runInContext(js, ctx, { timeout: 30000 });
    } catch (e) {
      reject(e);
    }
  });
}

/** `v` as JavaScript source (Python's json.dumps). */
export const j = (v) => JSON.stringify(v === undefined ? null : v);
/** Adjacent string literals, joined. */
export const cat = (...parts) => parts.join("");

// -- Python-flavoured asserts, so the tests read like the ones they came from ---

const short = (s) => {
  s = typeof s === "string" ? s : JSON.stringify(s);
  return s.length > 600 ? s.slice(0, 600) + "...(" + s.length + " chars)" : s;
};
export function assertIn(needle, hay, msg = "") {
  assert.ok(hay.includes(needle), `${msg} expected ${short(needle)} in ${short(hay)}`);
}
export function assertNotIn(needle, hay, msg = "") {
  assert.ok(!hay.includes(needle), `${msg} did not expect ${short(needle)} in ${short(hay)}`);
}
export function assertLess(a, b, msg = "") {
  assert.ok(a < b, `${msg} expected ${a} < ${b}`);
}
export function assertLessEqual(a, b, msg = "") {
  assert.ok(a <= b, `${msg} expected ${a} <= ${b}`);
}
export function assertGreaterEqual(a, b, msg = "") {
  assert.ok(a >= b, `${msg} expected ${a} >= ${b}`);
}
export function assertAlmostEqual(a, b) {
  assert.ok(Math.abs(a - b) < 5e-8, `expected ${a} ~ ${b}`);
}
export const count = (hay, needle) => hay.split(needle).length - 1;
export const sorted = (a) => [...a].sort((x, y) => (x < y ? -1 : x > y ? 1 : 0));

// -- the text renderer -----------------------------------------------------------
// markdown, tables, highlighting and the teammate-message helpers: from the
// `const TABLE_SEP` marker to renderBlocks, after the escaper.
export const RENDERER = ESC + "\n" + between(SCRIPT, "const TABLE_SEP", "function renderBlocks");
/** `expr` evaluated with the renderer loaded; its JSON. */
export const runRenderer = (expr) =>
  evalJson(RENDERER + "\nprocess.stdout.write(JSON.stringify(" + expr + "));");
/** renderText(text), as the string it returns. */
export const render = (text) =>
  evalJs(RENDERER + "\nprocess.stdout.write(renderText(" + j(text) + "));");

// -- the chat matrix --------------------------------------------------------------
/** The matrix's layout and tile code (from "// Tiles are 1 wide" to
 * `let data =`, after shared.js) with `tail` run after it; the JSON it writes. */
export const matrixJs = (tail) =>
  evalJson(SHARED + between(MATRIX_SCRIPT, "// Tiles are 1 wide", "let data =") + tail);
