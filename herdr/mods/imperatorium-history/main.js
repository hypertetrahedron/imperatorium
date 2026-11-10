// Imperatorium History: search every past Claude Code session on this
// machine and Resume or Fork one. With Settings > Terminals set to hc it
// opens as a herdr tab; otherwise in Windows Terminal, as from the window.
// Opened from the sessions sidebar as a dialog; Esc closes it.
import { View, div } from "gpui-kit";
import { v_flex, h_flex, InputState, Input } from "gpui-base";
import { theme, appHome, readTextFile, props, closeOverlay, notify } from "herdr";
import { loadConfig, api, post } from "./client.js";
import { when } from "./history.js";

export default class ImperatoriumHistory extends View {
  init(_props, cx) {
    this.cfg = null;
    this.results = [];
    this.busy = false;
    this.error = null;
    this.query = "";
    this.input = InputState.new({ placeholder: "Search sessions: words from a prompt, a reply, a folder" });
    // Searching awaits the network (and a debounce), so it runs as a task
    // of its own rather than inside the input's event.
    this.input.on("change", (_e, cx) => { this.query = this.input.value(); cx.spawn(async (cx) => this.search(cx)); });
    this.input.on("submit", (_e, cx) => { cx.spawn(async (cx) => this.search(cx, true)); });
    this.input.focus();
    cx.timer.after(300, () => this.input.focus());
    this.handle = cx.focus_handle();
    this.seq = 0;
    cx.spawn(async (cx) => {
      this.cfg = await loadConfig(appHome, readTextFile);
      await this.search(cx, true);
    });
  }

  // Debounced: each keystroke bumps seq, and only the last one searches.
  async search(cx, now = false) {
    const n = ++this.seq;
    if (!now) await new Promise((res) => cx.timer.after(250, () => res()));
    if (n !== this.seq || !this.cfg) return;
    this.busy = true; cx.notify();
    try {
      const got = await api(this.cfg, "/api/history?q=" + encodeURIComponent(this.query.trim()));
      if (n === this.seq) { this.results = got.results ?? []; this.error = null; }
    } catch (e) {
      if (n === this.seq) this.error = String(e?.message ?? e);
    }
    this.busy = false; cx.notify();
  }

  async open(r, fork, cx) {
    const out = await post(this.cfg, "/api/resume", { session_id: r.session_id, fork, cwd: r.cwd, where: "window" });
    if (out.ok === false) {
      notify({ title: fork ? "Not forked" : "Not resumed", body: out.error ?? "", level: "error" });
      return;
    }
    notify({ title: (fork ? "Forked " : "Resumed ") + (r.title || r.session_id.slice(0, 8)).slice(0, 60),
             body: out.where === "hc" ? "In herdr, terminal " + out.hc?.address : "In Windows Terminal", level: "info" });
    const o = props()?.overlay;
    if (o) closeOverlay(o);
  }

  render() {
    const t = theme();
    const o = props()?.overlay;
    const now = Date.now() / 1000;
    const btn = (id, label, fn) => h_flex().id(id).h(24).px(9).items_center().rounded(6)
      .bg(t.raised).text_size(11.5).cursor_pointer().hover((s) => s.bg(t.hover)).on_click(fn).child(label);
    return v_flex().size_full().bg(t.popover).text_color(t.foreground).p(14).gap(8)
      .track_focus(this.handle)
      .on_key_down((e, cx) => { if (e.key === "escape" && o) { closeOverlay(o); cx.stop_propagation(); } })
      .child(h_flex().items_center()
        .child(div().flex_1().text_size(15).font_semibold().child("History"))
        .child(div().text_size(11).text_color(t.dim).child(this.busy ? "searching…" : this.results.length + " sessions")))
      .child(Input.new(this.input).h(34).px(10).rounded(8).border_1().border_color(t.input).bg(t.background).text_size(13))
      .when(Boolean(this.error), (el) => el.child(div().text_size(12).text_color(t.blocked).child(this.error)))
      .child(v_flex().id("imp-hist").flex_1().min_h(0).overflow_y_scrollbar().gap(2)
        .children(this.results.slice(0, 60).map((r, i) => v_flex().px(8).py(6).rounded(7)
          .hover((s) => s.bg(t.hover))
          .child(h_flex().gap(8).items_center()
            .child(div().flex_1().truncate().text_size(12.5).child(r.title || r.first_prompt || r.session_id))
            .when(Boolean(r.live), (el) => el.child(div().text_size(10.5).text_color(t.working).child("running")))
            .child(div().text_size(10.5).text_color(t.dim).child(when(r.modified, now)))
            .when(!r.live, (el) => el.child(btn("imp-h-r-" + i, "Resume", (_e, cx) => this.open(r, false, cx))))
            .child(btn("imp-h-f-" + i, "Fork", (_e, cx) => this.open(r, true, cx))))
          .child(div().truncate().text_size(11).text_color(t.muted)
            .child([r.cwd, r.branch].filter(Boolean).join(" · ")))
          .when(Boolean(r.snippet), (el) => el.child(div().truncate().text_size(11).text_color(t.dim).child(r.snippet))))));
  }
}
