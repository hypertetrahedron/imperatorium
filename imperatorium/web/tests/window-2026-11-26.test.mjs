// The window changes of 2026-11-26 (user request): Settings > Updates can
// look for and install a preview build by hand. The daily check and the
// badge stay on releases; the Channel setting is gone.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SETTINGS_SCRIPT, SETTINGS_CSS, SCRIPT, ESC, between, evalJson, cat, assertIn, assertNotIn } from "./load.mjs";

describe("PreviewBuild", () => {
  const body = () => between(SETTINGS_SCRIPT, "function agoText(", "async function drawUpdates(");
  const run = (code) => evalJson(cat(ESC, "\n", body(), "\n", code));

  it("the release line no longer names a channel, and says what an update is installing", () => {
    const [up, going] = run(cat(
      "process.stdout.write(JSON.stringify([",
      " updateLine({ok: true, current: '0.4.0', check: true, checked_at: 100, newer: false}, 160).text,",
      " updateLine({ok: true, current: '0.4.0', applying: 1, applying_to: '0.5.0-preview.1', latest: '0.4.1'}, 0).text]));"));
    assert.equal(up, "Imperatorium 0.4.0 · up to date (checked 1 min ago)");
    assert.equal(going, "Imperatorium 0.4.0 · updating to 0.5.0-preview.1…");
  });

  it("the preview line: unchecked, found, current, older, failed, no build here", () => {
    const lines = run(cat(
      "const d = (current, preview) => ({ok: true, current, preview});",
      "process.stdout.write(JSON.stringify([",
      " previewLine(d('0.4.0'), 0),",
      " previewLine(d('0.5.0-preview.1'), 0),",
      " previewLine(d('0.4.0', {checked_at: 10, newer: true, latest: '0.5.0-preview.2', installable: true}), 20),",
      " previewLine(d('0.5.0-preview.2', {checked_at: 10, newer: false, latest: '0.5.0-preview.2'}), 20),",
      " previewLine(d('0.5.0', {checked_at: 10, newer: false, latest: '0.5.0-preview.2'}), 20),",
      " previewLine(d('0.4.0', {checked_at: 10, error: 'offline'}), 20),",
      " previewLine(d('0.4.0', {checked_at: 10, newer: true, latest: '0.5.0-preview.2', installable: false}), 20)]));"));
    assert.deepEqual(lines.map(l => l.bad), [false, false, false, false, false, true, false]);
    assertIn("never offers a preview", lines[0].text);
    assertIn("This is a preview build", lines[1].text);
    assert.equal(lines[2].text, "Preview 0.5.0-preview.2 is available (checked 10s ago)");
    assertIn("You are on the newest preview, 0.5.0-preview.2", lines[3].text);
    assertIn("is not newer than this version", lines[4].text);
    assert.equal(lines[5].text, "The check 10s ago failed: offline");
    assertIn("no build for this machine", lines[6].text);
  });

  it("the buttons ask for a preview by hand, and the badge still only shows a release", () => {
    for (const needle of ['id="prevcheck"', 'id="prevapply"', 'postUpdate("/api/update/check", {preview: true})',
                          'postUpdate("/api/update/apply", preview ? {preview: true} : {})',
                          "imperatorium update --rollback"]) {
      assertIn(needle, SETTINGS_SCRIPT);
    }
    assertNotIn("d.channel", SETTINGS_SCRIPT);
    // the preview's notes wrap like the release's, not widening the pane
    assertIn("#set #updnotes,#set #prevnotes{white-space:pre-wrap;", SETTINGS_CSS);
    // app.js's badge reads the release answer, not d.preview.
    assertIn("d.check && d.newer && !d.applying", SCRIPT);
    assertNotIn("preview", between(SCRIPT, "async function showUpdate(", "showUpdate();"));
  });
});
