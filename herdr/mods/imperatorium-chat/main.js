// Imperatorium chat: one Claude Code session's conversation, read from the
// Imperatorium dispatcher, with a prompt box that sends through it.
//
// Which session: props().session if a layout or pane.split gave one, else
// the session whose hc terminal is the focused one, else the one picked in
// the header. The dispatcher's address and token come from
// <app home>/imperatorium.json, written by `setup_machine.py herdr install`.
import { View, div, list } from "gpui-kit";
import { v_flex, h_flex, TextView, TextareaState, Textarea, Scrollbar,
         VirtualListScrollHandle } from "gpui-base";
import { props, theme, focused, appHome, readTextFile, notify } from "herdr";
import { loadConfig, api, post } from "./client.js";
import { rowsOf, KINDS, MODES, nextMode, firstLine, toolLine } from "./rows.js";

const POLL_MS = 1500;
const DRAWN_TURNS = 60;

export default class ImperatoriumChat extends View {
  init(_props, cx) {
    this.cfg = null;
    this.sessions = [];
    this.pinned = props()?.session ?? null;  // given by the layout or split
    this.picked = null;                        // chosen in the header
    this.items = [];
    this.sig = null;
    this.error = null;
    this.open = {};                            // row key -> opened from Compact
    this.modes = loadModes();
    this.drawn = DRAWN_TURNS;
    this.follow = true;
    this.scroll = VirtualListScrollHandle.new();
    this.box = TextareaState.new({ placeholder: "Message this session", rows: 2 });
    this.box.set_auto_grow(2, 10);
    this.box.on("change", (_e, cx) => cx.notify());
    this.sending = false;
    this.drafts = {};
    cx.spawn(async (cx) => {
      this.cfg = await loadConfig(appHome, readTextFile);
      await this.tick(cx);
      cx.timer.every(POLL_MS, (cx) => this.tick(cx));
    });
  }

  // The session this copy shows.
  current() {
    if (this.pinned) return this.sessions.find((s) => s.sessionId === this.pinned) ?? { sessionId: this.pinned };
    const f = focused();
    if (f?.terminal) {
      const t = this.sessions.find((s) => s.hc && s.hc.address === f.terminal);
      if (t) return t;
    }
    if (this.picked) {
      const p = this.sessions.find((s) => s.sessionId === this.picked);
      if (p) return p;
    }
    return this.sessions[0] ?? null;
  }

  async tick(cx) {
    if (!this.cfg) return;
    try {
      const s = await api(this.cfg, "/api/sessions");
      this.sessions = (s.sessions ?? []).filter((r) => r.kind !== "background" || r.pid);
      const cur = this.current();
      if (!cur) { this.items = []; this.error = null; cx.notify(); return; }
      if (cur.sessionId !== this.shownId) { this.sig = null; this.items = []; this.open = {}; this.drawn = DRAWN_TURNS; this.follow = true; this.swapDraft(cur.sessionId); }
      const q = "/api/conversation?session=" + encodeURIComponent(cur.sessionId) + (this.sig ? "&known=" + this.sig : "");
      const c = await api(this.cfg, q);
      this.shownId = cur.sessionId;
      this.error = c.error ?? null;
      if (!c.unchanged) {
        this.items = c.items ?? [];
        this.sig = c.sig;
        if (this.follow) this.scroll.scroll_to_bottom();
      }
    } catch (e) {
      this.error = String(e?.message ?? e);
    }
    cx.notify();
  }

  swapDraft(id) {
    if (this.shownId) this.drafts[this.shownId] = this.box.value();
    this.box.set_value(this.drafts[id] ?? "");
  }

  async send(cx) {
    const cur = this.current();
    const text = this.box.value().trim();
    if (!cur || !text || this.sending) return;
    this.sending = true; cx.notify();
    try {
      const r = await post(this.cfg, "/api/dispatch", { session_id: cur.sessionId, prompt: text, mode: "auto" });
      if (r.ok === false || r.error) notify({ title: "Not sent", body: r.error ?? JSON.stringify(r), level: "error" });
      else { this.box.set_value(""); this.drafts[cur.sessionId] = ""; this.follow = true; }
    } catch (e) {
      notify({ title: "Not sent", body: String(e?.message ?? e), level: "error" });
    }
    this.sending = false; cx.notify();
  }

  cycle(kind, cx) {
    this.modes[kind] = nextMode(this.modes[kind]);
    saveModes(this.modes);
    cx.notify();
  }

  // ---- drawing -------------------------------------------------------------

  render() {
    const t = theme();
    const cur = this.current();
    const rows = rowsOf(this.items, this.modes, this.drawn);
    this.rows = rows;
    return v_flex().size_full().bg(t.background).text_color(t.foreground)
      .child(this.header(t, cur))
      .child(this.strip(t, rows))
      .child(v_flex().relative().flex_1().min_h(0)
        .child(list("imp-log", rows.length, (i) => rows[i].key, (i) => this.row(t, rows[i]))
          .track_scroll(this.scroll).size_full()
          .on_item_click((key, cx) => { this.open[key] = !this.open[key]; cx.notify(); }))
        .child(Scrollbar.vertical("imp-log").absolute().inset_0()))
      .when(Boolean(this.error), (el) => el.child(div().px(12).py(4).text_size(12).text_color(t.blocked).child(this.error)))
      .child(this.composer(t, cur));
  }

  header(t, cur) {
    const name = cur ? (cur.display_name || cur.name || cur.project || cur.sessionId.slice(0, 8)) : "No sessions";
    const state = cur?.attention ? (cur.attention_reason || "needs you") : (cur?.status ?? "");
    const dot = cur?.attention ? t.blocked : cur?.status === "busy" ? t.working : t.idle;
    const pick = this.sessions.slice(0, 12).map((s, i) =>
      div().id("imp-pick-" + i).px(8).py(2).rounded(6).text_size(11.5).cursor_pointer()
        .bg(cur && s.sessionId === cur.sessionId ? t.selected : t.chrome)
        .text_color(s.attention ? t.blocked : t.muted)
        .hover((st) => st.bg(t.hover))
        .on_click((_e, cx) => { this.pinned = null; this.picked = s.sessionId; this.tick(cx); })
        .child((s.short_name || s.project || s.sessionId.slice(0, 8)).slice(0, 22)));
    return v_flex().bg(t.chrome).border_b(1).border_color(t.border)
      .child(h_flex().h(34).px(12).gap(8).items_center()
        .child(div().w(8).h(8).rounded(4).bg(dot))
        .child(div().flex_1().truncate().text_size(13).font_semibold().child(name))
        .child(div().text_size(11.5).text_color(t.dim).child(state)))
      .when(!this.pinned && this.sessions.length > 1, (el) =>
        el.child(h_flex().px(10).pb(6).gap(4).flex_wrap().children(pick)));
  }

  strip(t, rows) {
    const chips = KINDS.map(([kind, label]) =>
      div().id("imp-mode-" + kind).px(7).py(1).rounded(5).text_size(11).cursor_pointer()
        .bg(this.modes[kind] === "full" ? t.raised : t.background)
        .text_color(this.modes[kind] === "hidden" ? t.dim : t.muted)
        .border_1().border_color(t.border)
        .on_click((_e, cx) => this.cycle(kind, cx))
        .child(label + " " + MODES[this.modes[kind]]));
    const more = rows.earlier > 0
      ? [div().id("imp-earlier").px(7).py(1).rounded(5).text_size(11).cursor_pointer().text_color(t.accent)
          .on_click((_e, cx) => { this.drawn += DRAWN_TURNS; this.follow = false; cx.notify(); })
          .child("show " + Math.min(rows.earlier, DRAWN_TURNS) + " earlier")]
      : [];
    return h_flex().px(10).py(4).gap(4).flex_wrap().border_b(1).border_color(t.border)
      .children(more.concat(chips))
      .child(div().flex_1())
      .child(div().id("imp-end").px(7).py(1).rounded(5).text_size(11).cursor_pointer()
        .text_color(this.follow ? t.dim : t.accent)
        .on_click((_e, cx) => { this.follow = true; this.scroll.scroll_to_bottom(); cx.notify(); })
        .child(this.follow ? "following" : "show most recent"));
  }

  row(t, r) {
    const open = this.open[r.key];
    const compact = r.mode === "compact" && !open;
    let body;
    if (r.kind === "speaker") {
      const color = r.speaker === "you" ? t.accent : r.speaker === "claude" ? t.foreground : t.muted;
      return div().px(14).pt(12).pb(2).text_size(11).font_semibold().text_color(color)
        .child(r.label);
    }
    if (r.kind === "text") {
      body = compact
        ? div().truncate().text_color(t.muted).child(firstLine(r.text))
        : TextView.markdown("imp-md-" + r.key, r.text).selectable(true);
      return div().px(14).py(3).text_size(this.fontSize()).child(body);
    }
    if (r.kind === "thinking") {
      body = compact
        ? div().truncate().child("thinking: " + firstLine(r.text))
        : TextView.markdown("imp-th-" + r.key, r.text).selectable(true);
      return div().px(14).py(2).text_size(this.fontSize() - 1).italic().text_color(t.dim).child(body);
    }
    if (r.kind === "tool") {
      return h_flex().px(14).py(1).gap(6).text_size(12).font_family(this.mono())
        .child(div().text_color(t.ansi4).child("▸ " + r.name))
        .child(div().flex_1().truncate().text_color(t.muted).child(toolLine(r)));
    }
    if (r.kind === "result") {
      const color = r.is_error ? t.blocked : t.dim;
      if (compact) {
        return div().px(26).py(1).text_size(12).font_family(this.mono()).truncate().text_color(color)
          .child("└ " + firstLine(r.text) + (r.lines > 1 ? "  (+" + (r.lines - 1) + " lines)" : ""));
      }
      return div().px(26).py(2).text_size(12).text_color(color)
        .child(TextView.markdown("imp-rs-" + r.key, "```\n" + r.text.replace(/```/g, "``​`") + "\n```").selectable(true));
    }
    if (r.kind === "note") {
      return div().px(14).py(2).text_size(11.5).italic().text_color(t.dim).truncate().child(r.label + (r.text ? ": " + firstLine(r.text) : ""));
    }
    return div();
  }

  composer(t, cur) {
    const can = Boolean(cur) && cur.dispatchable !== false && !this.sending;
    return h_flex().p(8).gap(8).items_end().border_t(1).border_color(t.border).bg(t.chrome)
      .child(Textarea.new(this.box).flex_1().px(8).py(6).rounded(8).border_1().border_color(t.input)
        .bg(t.background).text_size(13))
      .child(h_flex().id("imp-send").h(32).px(14).items_center().rounded(8)
        .bg(can ? t.solid : t.raised).text_color(can ? t.solid_foreground : t.dim)
        .text_size(12.5).font_medium().cursor_pointer()
        .on_click((_e, cx) => this.send(cx))
        .child(this.sending ? "Sending…" : "Send"));
  }

  fontSize() { return this.cfg?.look?.fonts?.chat ?? 14; }
  mono() { return this.cfg?.mono ?? "Cascadia Mono"; }
}

function loadModes() {
  const out = {};
  let saved = {};
  try { saved = JSON.parse(localStorage.getItem("modes") ?? "{}"); } catch { saved = {}; }
  for (const [k] of KINDS) out[k] = MODES[saved[k]] ? saved[k] : (k === "result" || k === "thinking" ? "compact" : "full");
  return out;
}

function saveModes(m) {
  try { localStorage.setItem("modes", JSON.stringify(m)); } catch { /* per-copy convenience */ }
}
