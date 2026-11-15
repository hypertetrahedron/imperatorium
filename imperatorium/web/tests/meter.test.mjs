// The usage meter (shared.js meterHtml) and the matrix's status tile.
// Ported from test_ui.py: PlanInPlaceOfMeters, SpendShown.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SHARED, SCRIPT, CSS, evalJs, matrixJs, j, count, renderShell, assertIn, assertNotIn, assertLess } from "./load.mjs";

describe("PlanInPlaceOfMeters", () => {
  /** Off a Pro or Max plan there are no windows; the meter names the plan. */
  const runJs = (u) => evalJs(SHARED + `\nprocess.stdout.write(meterHtml(${j(u)}));`);

  it("bedrock with no windows shows its name not meters", () => {
    const got = runJs({ limits: {}, plan: { label: "AWS Bedrock", providers: ["bedrock"], alongside: false } });
    assertIn('class="plan"', got);
    assertIn("AWS Bedrock", got);
    assertNotIn('class="bar"', got);
  });

  it("nothing known stays empty", () => {
    assert.equal(runJs({ limits: {}, plan: null }), "");
  });

  it("meters keep a note for sessions they do not cover", () => {
    const got = runJs({ limits: { five_hour: { used: 40, resets_at: null } },
                        plan: { label: "AWS Bedrock", providers: ["bedrock"], alongside: true } });
    assertIn('class="bar"', got);
    assertIn("also AWS Bedrock - no limits", got);
  });

  it("reset time is in the tip not the row", () => {
    // Peer request 2026-10-18: the reset label made rows end at different
    // places; it moved to a hover tip, and the row ends with the bare
    // percentage in its own column.
    const resets = Math.floor(Date.now() / 1000) + 2 * 3600 + 5 * 60;
    const got = runJs({ limits: { five_hour: { used: 42.4, resets_at: resets },
                                  seven_day: { used: 7, resets_at: null } } });
    assert.equal(count(got, 'class="row"'), 2);
    assertIn('<span class="pct">42%</span>', got);
    assertIn('<span class="pct">7%</span>', got);
    let tip = got.slice(got.indexOf('class="tip"'));
    tip = tip.slice(0, tip.indexOf("</div></div>"));
    assertIn("5-hour window", tip);
    assertIn("resets ", tip);
    assert.match(tip, /\(in 2h [45]m\)/);
    assertIn("of the window elapsed", tip);
    const row = got.slice(0, got.indexOf('class="tip"'));
    assertNotIn("·", row);
    assertNotIn("title=", row);
  });

  it("a window waiting after its reset says so", () => {
    const got = runJs({ limits: { five_hour: { used: 0, resets_at: null, reset_at: 1000,
                                               waiting: true, pace: null } } });
    assertIn('class="row waiting"', got);
    assertIn('<span class="pct">0%</span>', got);
    assertIn("waiting for a new reading", got);
    assertNotIn("<b style", got);   // no time tick: the window's span is unknown
  });

  it("every bar gets the same length", () => {
    let rule = CSS.slice(CSS.indexOf(".meters .rows{"));
    rule = rule.slice(0, rule.indexOf("}"));
    assertIn("grid-template-columns:auto 1fr minmax(4ch,auto)", rule);
    assertIn("grid-template-columns:subgrid", CSS);
  });
});

const SPEND = { global: { windows: [
  { name: "five_hour", since: 1, resets_at: 18001, total_tokens: 17200000,
    cost_usd: 8.86, tokens: { input: 1, output: 2, cache_read: 3, cache_write: 4 } },
  { name: "seven_day", since: 1, resets_at: null, total_tokens: 412000000,
    cost_usd: 189.56, tokens: {} }] },
  current: { sessions: 2, total_tokens: 52600000, cost_usd: 27.76, tokens: {},
             each: [{ name: "c5", total_tokens: 37500000, cost_usd: 20.6 }] },
  complete: true };

const MIXED = { global: { windows: [
  { name: "five_hour", since: 1, resets_at: 18001, total_tokens: 9000,
    cost_usd: 1, tokens: {}, models: {
      "claude-haiku-4-5": { output: 1000, total_tokens: 2000, cost_usd: 0.1 },
      "claude-sonnet-4-5": { output: 2000, total_tokens: 3000, cost_usd: 0.3 },
      "claude-sonnet-4-6": { output: 1000, total_tokens: 1000, cost_usd: 0.1 },
      "claude-opus-4-1": { output: 4000, total_tokens: 4000, cost_usd: 0.5 } } }] },
  current: { sessions: 1, total_tokens: 10, cost_usd: 0, tokens: {}, each: [],
             models: { "mystery-1": { output: 10, total_tokens: 10, cost_usd: 0 } } },
  complete: true };

describe("SpendShown", () => {
  /** Global and current-session spend in the meter and the matrix's status
   * tile (peer request 2026-10-18). */
  const meter = (u, opts = null) =>
    evalJs(SHARED + `\nprocess.stdout.write(meterHtml(${j(u)}, ${j(opts)}));`);

  it("the meter shows global and open lines", () => {
    const got = meter({ limits: { five_hour: { used: 30, resets_at: 18001 } }, spend: SPEND });
    assertIn('<span class="k">global</span>', got);
    assertIn("5h ≈$8.86 · 7d ≈$190", got);
    assertIn('<span class="k">open</span>', got);
    assertIn("2 sessions · ≈$27.76 · 53M tokens", got);
    assertIn("c5: 38M tokens · ≈$20.60", got);          // in the Open title
    assertIn("from this machine: 17M tokens · ≈$8.86", got);  // in the 5h tip
    assertIn("reset time not known yet", got);            // the 7d title
  });

  it("without windows the month still shows", () => {
    const sp = { ...SPEND, global: { windows: [{ name: "month", since: 1, resets_at: null,
                                                 total_tokens: 5, cost_usd: 545.15, tokens: {} }] } };
    const got = meter({ limits: {}, plan: { label: "Claude Enterprise" }, spend: sp });
    assertIn("Claude Enterprise", got);
    assertIn("month ≈$545", got);
  });

  it("the status tile has both parts", () => {
    const html = matrixJs("process.stdout.write(JSON.stringify(statusTile(" + j({
      cost_usd: 27.76, costed: 2, sessions: 2, total_tokens: 52600000,
      tokens: {}, each: [], limits: {}, global: SPEND.global }) + ")));");
    assertLess(html.indexOf("Current sessions"), html.indexOf("<h2>Global</h2>"));
    for (const needle of ["each over its whole life", "5-hour window", "7-day window", "$189.56"]) {
      assertIn(needle, html);
    }
  });

  // Peer request 2026-10-18: the sidebar shows the bars alone and a click
  // opens the rest; the matrix tile leads with the meters and model mix.
  it("everything but the bars is extra", () => {
    const got = meter({ limits: { five_hour: { used: 30, resets_at: 18001, pace: { per_hour: 4 } } },
                        spend: MIXED });
    const bars = got.slice(0, got.indexOf('class="tip"'));
    assertIn('<span class="pct">30%</span>', bars);
    // Each piece after the bars is marked extra, which the closed meter hides.
    for (const piece of ['class="pace extra"', 'class="spend extra"', 'class="cap extra"',
                         'class="row mix extra"', 'class="legend extra"']) {
      assertIn(piece, got);
    }
    assertIn("#meter:not(.open) .extra{display:none}", CSS);
  });

  it("the mix stacks output by family cheapest first", () => {
    const got = meter({ limits: {}, spend: MIXED });
    let five = got.slice(got.indexOf("<span>5h</span>"));
    five = five.slice(0, five.indexOf('class="tip"'));
    // haiku 1000, sonnet 3000 (two models), opus 4000 of 8000 output.
    assertLess(five.indexOf('class="i1" style="width:12.50%"'), five.indexOf('class="i2" style="width:37.50%"'));
    assert.ok(five.includes('class="i1" style="width:12.50%"'));
    assertLess(five.indexOf('class="i2"'), five.indexOf('class="i3" style="width:50.00%"'));
    assert.ok(five.includes('class="i3" style="width:50.00%"'));
    assertIn('<span class="pct">8.0k</span>', five);
    assertIn("sonnet: 3.0k out (38%)", got);
    // A model with no known price is grey, last.
    const opened = got.slice(got.indexOf("<span>open</span>"));
    assertIn('class="io"', opened.slice(0, opened.indexOf('class="tip"')));
    const legend = got.slice(got.indexOf('class="legend extra"'));
    assert.deepEqual(["haiku", "sonnet", "opus", "fable", "other"].filter((n) => legend.includes(n)),
                     ["haiku", "sonnet", "opus", "other"]);
  });

  it("the matrix leaves out the spend lines", () => {
    const got = meter({ limits: { five_hour: { used: 30 } }, spend: MIXED }, { lines: false });
    assertNotIn('class="spend', got);
    assertIn('class="row mix extra"', got);
  });

  it("the status tile leads with the meters", () => {
    const html = matrixJs("process.stdout.write(JSON.stringify(statusTile(" + j({
      cost_usd: 1, costed: 1, sessions: 1, total_tokens: 5, tokens: {}, each: [],
      limits: { five_hour: { used: 42 } }, global: MIXED.global,
      models: MIXED.current.models }) + ")));");
    assertLess(html.indexOf('class="meters top"'), html.indexOf("Current sessions"));
    assert.ok(html.includes('class="meters top"'));
    assertIn('<span class="pct">42%</span>', html);
    assertIn("model mix", html);
    assertNotIn("Account limits", html);
  });

  it("a click folds the meter and it is remembered", () => {
    const shell = renderShell();
    assertIn("id=meter class=meters tabindex=0 role=button aria-expanded=false", shell);
    assertIn('localStorage.getItem("imperatorium.meterOpen")', SCRIPT);
    assertIn('$("#meter").addEventListener("click"', SCRIPT);
    // The caret's escape survives the string it is written in (a lost backslash
    // once made it a control character and the literal "5BE").
    assertIn('#meter::after{content:"\\25BE"', CSS);
    assert.deepEqual([...CSS].filter((c) => c.charCodeAt(0) < 32 && c !== "\n" && c !== "\t"), []);
  });
});

describe("MetersThatApply", () => {
  /** 2026-11-15 (user request): only the windows the account has, and a
   *  Remaining Meter for a fixed allocation such as an Enterprise seat's $500. */
  const runJs = (u) => evalJs(SHARED + `\nprocess.stdout.write(meterHtml(${j(u)}));`);
  const soon = () => Math.floor(Date.now() / 1000) + 86400;

  it("a verified ultra account shows only its weekly meter", () => {
    const got = runJs({ limits: { seven_day: { used: 30, resets_at: soon() } } });
    assert.equal(count(got, 'class="row"'), 1);
    assertIn("<span>7d</span>", got);
    assertNotIn("<span>5h</span>", got);
    assertNotIn('class="row remain"', got);
  });

  it("a spend limit with dollars shows what is left", () => {
    const got = runJs({ limits: { spend_limit: { used: 62.8, resets_at: soon(),
                                                 used_usd: 314.12, limit_usd: 500, period: "monthly" } },
                        spend: { global: { windows: [{ name: "month", total_tokens: 2e6, cost_usd: 10 }] } } });
    assertIn("<span>spend</span>", got);
    assertIn('<span class="pct">63%</span>', got);
    const left = got.slice(got.indexOf('class="row remain"'));
    assertIn("<span>left</span>", left);
    assertIn('style="width:37.2', left);
    assertIn('<span class="pct">$186</span>', left);
    assertIn("$186 of $500 left (a month)", left);
    assertIn("$314 used (63%)", left);
    assertIn("≈37M tokens at this machine's recent mix (200k per $)", left);
    assertNotIn("<span>5h</span>", got);
  });

  it("before the dollars come the remaining meter says the percentage", () => {
    const got = runJs({ limits: { spend_limit: { used: 90, resets_at: soon() } } });
    const left = got.slice(got.indexOf('class="row remain"'));
    assertIn('<span class="pct">10%</span>', left);
    assertIn('class="hi"', left);
    assertIn("10% left", left);
    assertNotIn("tokens at this machine", left);
  });

  it("a spend limit that just reset waits without a remaining meter", () => {
    const got = runJs({ limits: { spend_limit: { used: 0, waiting: true, reset_at: 5, resets_at: null } } });
    assertNotIn('class="row remain"', got);
  });
});
