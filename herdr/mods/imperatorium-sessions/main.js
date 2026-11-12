// Imperatorium sessions: every Claude Code session the dispatcher knows -
// in herdr-core terminals or not, on this machine or a remote one - with
// the ones waiting on you first, and the account's usage meters.
//
// Click a session: its chat opens as an ordinary herdr pane. In an hc
// terminal here, beside that terminal in its tab; anywhere else, as a tab of
// its own. Either splits, zooms, moves and closes like any pane. A chat copy
// a layout places without a session follows the pick (shared
// "imperatorium.pick"). Matrix opens Imperatorium's chat matrix in a window
// of its own, outside herdr's, so it can sit on another screen.
import { View, div } from "gpui-kit";
import { v_flex, h_flex } from "gpui-base";
import { theme, appHome, readTextFile, run, setShared, openOverlay, spaces, panes, notify } from "herdr";
import { loadConfig, api, post } from "./client.js";
import { order, meterRows, ago, tabNames, chatCommands } from "./sessions.js";

const POLL_MS = 3000;

export default class ImperatoriumSessions extends View {
  init(_props, cx) {
    this.cfg = null;
    this.rows = [];
    this.usage = null;
    this.pending = [];
    this.error = null;
    cx.spawn(async (cx) => {
      this.cfg = await loadConfig(appHome, readTextFile);
      await this.tick(cx);
      cx.timer.every(POLL_MS, (cx) => this.tick(cx));
    });
  }

  async tick(cx) {
    if (!this.cfg) return;
    try {
      const [s, u, p] = await Promise.all([api(this.cfg, "/api/sessions"),
        api(this.cfg, "/api/usage"), api(this.cfg, "/api/pending")]);
      this.rows = order(s.sessions ?? []);
      this.usage = u;
      this.pending = p.pending ?? [];
      this.error = null;
      // Tabs Imperatorium started are called "1", "2"... until named: name
      // each once after its session. A name the person gives is kept.
      for (const [id, name] of tabNames(spaces(), this.rows)) run("tab.rename", { id, name });
    } catch (e) {
      this.error = "Imperatorium: " + String(e?.message ?? e);
    }
    cx.notify();
  }

  open(s) {
    setShared("imperatorium.pick", { id: s.sessionId, at: Date.now() });
    try {
      const cmds = chatCommands(s, panes(), spaces());
      if (!cmds.length) notify({ title: "No space yet", body: "Make a space first; the chat opens as a tab in it.", level: "info" });
      for (const [name, args] of cmds) run(name, args);
    } catch (e) {
      notify({ title: "Chat not opened", body: String(e?.message ?? e), level: "error" });
    }
  }

  async matrix() {
    try {
      const r = await post(this.cfg, "/api/open_window", { page: "matrix" });
      if (r.ok === false) notify({ title: "Matrix not opened", body: r.error ?? "", level: "error" });
    } catch (e) {
      notify({ title: "Matrix not opened", body: String(e?.message ?? e), level: "error" });
    }
  }

  render() {
    const t = theme();
    const waiting = this.pending.length;
    // The section's own heading, like herdr's "Spaces" above it; the layout
    // draws the divider between the two.
    return v_flex().size_full().text_color(t.foreground)
      .child(h_flex().h(31).flex_none().pl(16).pr(10).items_center().gap(2)
        .child(div().flex_1().text_size(11.5).text_color(t.dim).child("Imperatorium"))
        .child(div().id("imp-matrix").px(6).py(1).rounded(5).text_size(11).cursor_pointer()
          .text_color(t.muted).hover((s) => s.bg(t.hover))
          .on_click(() => this.matrix())
          .child("Matrix"))
        .child(div().id("imp-history").px(6).py(1).rounded(5).text_size(11).cursor_pointer()
          .text_color(t.muted).hover((s) => s.bg(t.hover))
          .on_click(() => openOverlay({ mod: "imperatorium-history", kind: "dialog", width: 780, height: 580 }))
          .child("History")))
      .child(this.meters(t))
      .when(waiting > 0, (el) => el.child(div().px(14).py(2).text_size(11.5).text_color(t.blocked)
        .child(waiting + (waiting === 1 ? " request waiting" : " requests waiting"))))
      .when(Boolean(this.error), (el) => el.child(div().px(14).py(4).text_size(11.5).text_color(t.blocked).child(this.error)))
      .child(v_flex().id("imp-sessions").flex_1().min_h(0).overflow_y_scrollbar().px(6).pb(6)
        .children(this.rows.map((s, i) => this.row(t, s, i))));
  }

  meters(t) {
    const rows = meterRows(this.usage, Date.now() / 1000);
    if (!rows.length) return div();
    return v_flex().px(14).py(4).gap(4).children(rows.map((m) =>
      v_flex().gap(2)
        .child(h_flex().text_size(11).text_color(t.muted)
          .child(div().flex_1().child(m.label))
          .child(div().child(m.text)))
        .child(div().relative().h(5).rounded(3).bg(t.raised)
          .child(div().absolute().left(0).top(0).bottom(0).rounded(3)
            .w_full().max_w(Math.max(2, m.pct * 2.2))
            .bg(m.pct >= 90 ? t.blocked : m.pct >= 70 ? t.working : t.accent)))));
  }

  row(t, s, i) {
    const dot = s.attention ? t.blocked : s.status === "busy" ? t.working : s.pid ? t.idle : t.dim;
    const name = s.display_name || s.name || s.short_name || s.project || s.sessionId.slice(0, 8);
    const sub = s.attention ? (s.attention_reason || "needs you")
      : s.status === "busy" ? (s.doing || "working") : (s.project || "");
    return v_flex().id("imp-s-" + i).px(8).py(5).rounded(7).cursor_pointer()
      .hover((st) => st.bg(t.hover))
      .on_click(() => this.open(s))
      .child(h_flex().gap(7).items_center()
        .child(div().w(7).h(7).rounded(4).bg(dot))
        .child(div().flex_1().truncate().text_size(12.5).child(name))
        .when(Boolean(s.hc), (el) => el.child(div().text_size(10.5).text_color(t.dim).child(s.hc.address)))
        .when(Boolean(s.remote), (el) => el.child(div().text_size(10.5).text_color(t.dim).child(s.host ?? "remote")))
        .child(div().text_size(10.5).text_color(t.dim).child(ago(s.status_changed_at, Date.now() / 1000))))
      .child(div().pl(14).truncate().text_size(11).text_color(s.attention ? t.blocked : t.muted).child(sub));
  }
}
