// The Chat Matrix in herdr-app: one tile per running session with its
// latest replies and the step it's on, plus what they've cost between them.
// Opens as a floating window from the sessions sidebar, or as a tab
// (tab.new {mod: "imperatorium-matrix"}). Click a tile: show that session.
import { View, div } from "gpui-kit";
import { v_flex, h_flex, TextView } from "gpui-base";
import { theme, appHome, readTextFile, run, setShared, props, closeOverlay } from "herdr";
import { loadConfig, api } from "./client.js";
import { grid, money, lastReplies, stepLine } from "./matrix.js";
import { hardBreaks } from "./text.js";

const POLL_MS = 2500;

export default class ImperatoriumMatrix extends View {
  init(_props, cx) {
    this.cfg = null;
    this.data = null;
    this.error = null;
    this.handle = cx.focus_handle();
    cx.spawn(async (cx) => {
      this.cfg = await loadConfig(appHome, readTextFile);
      await this.tick(cx);
      cx.timer.every(POLL_MS, (cx) => this.tick(cx));
    });
  }

  async tick(cx) {
    if (!this.cfg) return;
    try {
      this.data = await api(this.cfg, "/api/matrix");
      this.error = null;
    } catch (e) {
      this.error = String(e?.message ?? e);
    }
    cx.notify();
  }

  open(tile) {
    // The sessions sidebar's own way of showing one: focus its terminal, and
    // point the chat at it.
    setShared("imperatorium.pick", { id: tile.sessionId, at: Date.now() });
    if (tile.hc?.address && !tile.remote) run("pane.focus", { terminal: tile.hc.address });
  }

  render() {
    const t = theme();
    const tiles = this.data?.tiles ?? [];
    const { cols, rows } = grid(tiles.length + 1);
    const cells = tiles.map((tile, i) => this.tile(t, tile, i)).concat([this.spend(t)]);
    const overlay = props()?.overlay;
    const rowEls = [];
    for (let r = 0; r < rows; r++) {
      rowEls.push(h_flex().flex_1().min_h(0).gap(8).overflow_hidden()
        .children(cells.slice(r * cols, (r + 1) * cols)
          .concat(Array.from({ length: Math.max(0, cols - cells.slice(r * cols, (r + 1) * cols).length) }, () => div().flex_1()))));
    }
    return v_flex().size_full().bg(t.canvas).text_color(t.foreground)
      .track_focus(this.handle)
      .on_key_down((e, cx) => { if (e.key === "escape" && overlay) { closeOverlay(overlay); cx.stop_propagation(); } })
      .child(h_flex().h(32).px(12).items_center().bg(t.chrome).border_b(1).border_color(t.border)
        .child(div().flex_1().text_size(12.5).font_semibold().child("Chat Matrix"))
        .when(Boolean(this.error), (el) => el.child(div().text_size(11.5).text_color(t.blocked).child(this.error)))
        .when(Boolean(overlay), (el) => el.child(div().id("imp-mx-close").px(8).text_size(12).cursor_pointer()
          .text_color(t.muted).on_click(() => closeOverlay(overlay)).child("✕"))))
      .child(v_flex().flex_1().min_h(0).p(8).gap(8).children(rowEls));
  }

  tile(t, tile, i) {
    const ring = tile.attention ? t.blocked : tile.status === "busy" ? t.working : t.border;
    const replies = lastReplies(tile.replies, 3);
    const step = stepLine(tile.current);
    return v_flex().id("imp-mx-" + i).flex_1().h_full().min_w(0).min_h(0).rounded(9).border_1().border_color(ring)
      .bg(t.background).overflow_hidden().cursor_pointer()
      .on_click(() => this.open(tile))
      .child(h_flex().px(10).py(5).gap(6).items_center().bg(t.chrome)
        .child(div().w(7).h(7).rounded(4).bg(ring))
        .child(div().flex_1().truncate().text_size(12).font_semibold().child(tile.name || tile.sessionId.slice(0, 8)))
        .child(div().text_size(11).text_color(t.dim).child(money(tile.cost_usd))))
      .when(Boolean(tile.attention), (el) => el.child(div().px(10).py(2).text_size(11.5).text_color(t.blocked)
        .child(tile.attention_reason || "needs you")))
      .child(v_flex().flex_1().min_h(0).overflow_hidden().justify_end().px(10).py(4).gap(4)
        .text_size(this.cfg?.look?.fonts?.matrix ?? 13)
        .children(replies.map((r, j) => TextView.markdown("imp-mx-" + i + "-" + j, hardBreaks(r)).selectable(true))))
      .when(Boolean(step), (el) => el.child(div().px(10).py(3).truncate().text_size(11).text_color(t.working).child(step)));
  }

  spend(t) {
    const s = this.data?.spend;
    return v_flex().flex_1().h_full().min_w(0).min_h(0).overflow_hidden().rounded(9).border_1().border_color(t.border).bg(t.background).p(10).gap(4)
      .child(div().text_size(12).font_semibold().child("Open sessions"))
      .child(div().text_size(20).child(money(s?.cost_usd)))
      .child(div().text_size(11).text_color(t.muted).child((s?.sessions ?? 0) + " sessions · "
        + Math.round((s?.total_tokens ?? 0) / 1e6) + "M tokens"))
      .children((s?.each ?? []).slice(0, 6).map((e) => h_flex().text_size(11).text_color(t.muted)
        .child(div().flex_1().truncate().child(e.name ?? ""))
        .child(div().child(money(e.cost_usd)))));
  }
}
