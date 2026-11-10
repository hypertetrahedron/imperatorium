// Imperatorium chat: one Claude Code session's conversation, read from the
// Imperatorium dispatcher, with a prompt box that sends through it.
//
// Which session: props().session if a layout or pane.split gave one, else
// the session whose hc terminal is the focused one, else the one picked in
// the header. The dispatcher's address and token come from
// <app home>/imperatorium.json, written by `setup_machine.py herdr install`.
import { View, div, list } from "gpui-kit";
import { v_flex, h_flex, TextView, TextareaState, Textarea, Scrollbar } from "gpui-base";
import { props, theme, focused, appHome, readTextFile, notify, nextCommand } from "herdr";
import { loadConfig, api, post } from "./client.js";
import { rowsOf, KINDS, MODES, nextMode, firstLine, toolLine, pendingFor, answerFor, hardBreaks } from "./rows.js";

const POLL_MS = 1500;
const DRAWN_TURNS = 60;
const TAIL_ROWS = 40;      // drawn while following; more than a screen's worth
const HISTORY_STEP = 30;   // rows a scroll-back starts with, and each "earlier" adds

export default class ImperatoriumChat extends View {
  init(_props, cx) {
    this.cfg = null;
    this.sessions = [];
    this.pending = [];
    this.pinned = props()?.session ?? null;  // given by the layout or split
    this.picked = null;                        // chosen in the header
    this.items = [];
    this.sig = null;
    this.error = null;
    this.open = {};                            // row key -> opened from Compact
    this.modes = loadModes();
    this.drawn = DRAWN_TURNS;
    // Following, the newest rows sit at the bottom of a clipped column (a
    // tail, like a terminal's). Scrolling up switches to a list() of the
    // rows from `histFrom` on, which can scroll; "Latest" goes back. A
    // list() can't follow an end itself: gpui-kit builds it top-aligned and
    // ignores scroll handles (list.rs, ListAlignment::Top).
    this.follow = true;
    this.gen = 0;
    this.histFrom = 0;
    this.box = TextareaState.new({ placeholder: "Message this session  (Ctrl+Enter sends)", rows: 2 });
    this.box.set_auto_grow(2, 10);
    this.box.on("change", (_e, cx) => cx.notify());
    this.box.on("submit", (_e, cx) => this.send(cx, "send"));
    this.busy = null;                          // "send" | "queue" | "typed" | "mic"
    this.mic = false;
    this.picks = {};                           // pending id -> {question: label}
    this.drafts = {};
    this.handle = cx.focus_handle();
    cx.spawn(async (cx) => {
      this.cfg = await loadConfig(appHome, readTextFile);
      await this.tick(cx);
      cx.timer.every(POLL_MS, (cx) => this.tick(cx));
    });
    cx.spawn(async (cx) => {
      for (;;) {
        const c = await nextCommand();
        if (c === "focus") this.box.focus();
        else if (c === "dictate") await this.dictate(cx);
        cx.notify();
      }
    });
  }

  // The session this copy shows.
  current() {
    if (this.pinned) return this.sessions.find((s) => s.sessionId === this.pinned) ?? { sessionId: this.pinned };
    // The focused terminal's session. Clicking into this pane (or any mod)
    // leaves no terminal focused; the pane then stays on the last one it
    // followed rather than jumping to whichever session needs attention.
    const f = focused();
    if (f?.terminal) {
      const t = this.sessions.find((s) => s.hc && s.hc.address === f.terminal);
      if (t) { this.followed = t.sessionId; this.picked = null; return t; }
    }
    if (this.picked) {
      const p = this.sessions.find((s) => s.sessionId === this.picked);
      if (p) return p;
    }
    if (this.followed) {
      const p = this.sessions.find((s) => s.sessionId === this.followed);
      if (p) return p;
    }
    if (this.picked) {
      const p = this.sessions.find((s) => s.sessionId === this.picked);
      if (p) return p;
    }
    return this.sessions.find((s) => s.attention) ?? this.sessions[0] ?? null;
  }

  async tick(cx) {
    if (!this.cfg) return;
    try {
      const [s, p] = await Promise.all([api(this.cfg, "/api/sessions"), api(this.cfg, "/api/pending")]);
      this.sessions = (s.sessions ?? []).filter((r) => r.pid || r.status);
      this.pending = p.pending ?? [];
      const cur = this.current();
      if (!cur) { this.items = []; this.error = null; cx.notify(); return; }
      if (cur.sessionId !== this.shownId) {
        this.swapDraft(cur.sessionId);
        this.sig = null; this.items = []; this.open = {}; this.drawn = DRAWN_TURNS;
        this.follow = true;
      }
      const q = "/api/conversation?session=" + encodeURIComponent(cur.sessionId) + (this.sig ? "&known=" + this.sig : "");
      const c = await api(this.cfg, q);
      this.shownId = cur.sessionId;
      this.error = c.error ?? null;
      if (!c.unchanged) {
        this.items = c.items ?? []; this.sig = c.sig;
        if (this.follow) this.gen++;
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

  async send(cx, how) {
    const cur = this.current();
    const text = this.box.value().trim();
    if (!cur || !text || this.busy) return;
    this.busy = how; cx.notify();
    const route = how === "queue" ? ["/api/queue", { session_id: cur.sessionId, prompt: text }]
      : how === "typed" ? ["/api/typed", { session_id: cur.sessionId, prompt: text }]
      : ["/api/dispatch", { session_id: cur.sessionId, prompt: text, mode: "auto" }];
    try {
      const r = await post(this.cfg, route[0], route[1]);
      if (r.ok === false || r.error) notify({ title: "Not sent", body: r.error ?? JSON.stringify(r), level: "error" });
      else { this.box.set_value(""); this.drafts[cur.sessionId] = ""; this.toEnd(); }
    } catch (e) {
      notify({ title: "Not sent", body: String(e?.message ?? e), level: "error" });
    }
    this.busy = null; cx.notify();
  }

  async dictate(cx) {
    if (this.busy && this.busy !== "mic") return;
    const starting = !this.mic;
    this.busy = "mic"; cx.notify();
    try {
      const r = await post(this.cfg, "/api/dictate", { action: starting ? "start" : "stop" });
      if (r.ok === false) { notify({ title: "Dictation", body: r.error ?? "failed", level: "error" }); this.mic = false; }
      else {
        this.mic = Boolean(r.recording);
        if (!starting && r.text) {
          const was = this.box.value();
          this.box.set_value(was ? was.replace(/\s*$/, " ") + r.text : r.text);
          this.box.focus();
        }
      }
    } catch (e) {
      notify({ title: "Dictation", body: String(e?.message ?? e), level: "error" });
      this.mic = false;
    }
    this.busy = null; cx.notify();
  }

  async decide(cx, item, verdict) {
    const body = { id: item.id, verdict, who: "herdr" };
    if (item.questions && verdict === "allow") {
      body.answers = answerFor(item, this.picks[item.id] ?? {});
      if (!body.answers) { notify({ title: "Pick an answer for each question", level: "warning" }); return; }
    }
    const r = await post(this.cfg, "/api/decide", body);
    if (!r.ok) notify({ title: "Already answered", body: "It was decided elsewhere, or timed out.", level: "info" });
    this.pending = this.pending.filter((p) => p.id !== item.id);
    cx.notify();
  }

  // Scrolled back: the rows from histFrom on, in a list that scrolls.
  history(el, t, rows, logId) {
    const from = Math.min(this.histFrom, rows.length);
    const shown = rows.slice(from);
    const more = from > 0 ? 1 : 0;
    return el
      .child(list(logId, shown.length + more,
        (i) => (more && i === 0) ? "earlier" : shown[i - more].key,
        (i) => (more && i === 0)
          ? div().px(14).py(6).text_size(11.5).text_color(t.accent).child("Show earlier (" + from + " more rows)")
          : this.row(t, shown[i - more]))
        .size_full()
        .on_item_click((key, cx) => {
          if (key === "earlier") { this.histFrom = Math.max(0, this.histFrom - HISTORY_STEP); }
          else this.open[key] = !this.open[key];
          cx.notify();
        }))
      .child(Scrollbar.vertical(logId).absolute().inset_0());
  }

  toEnd() {
    this.follow = true;
    this.gen++;
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
    const asks = cur ? pendingFor(this.pending, cur.sessionId) : [];
    const logId = "imp-log-" + this.gen;
    return v_flex().size_full().bg(t.background).text_color(t.foreground)
      .track_focus(this.handle)
      .on_key_down((e, cx) => this.key(e, cx))
      .child(this.header(t, cur))
      .child(this.strip(t, rows))
      .child(v_flex().relative().flex_1().min_h(0)
        .on_scroll_wheel((e, cx) => {
          if (this.follow && (e.delta?.y ?? 0) > 0) {
            this.follow = false; this.gen++;
            this.histFrom = Math.max(0, rows.length - HISTORY_STEP);
            cx.notify();
          }
        })
        .when(this.follow, (el) => el.child(v_flex().size_full().overflow_hidden().justify_end()
          .children(rows.slice(-TAIL_ROWS).map((r) => this.row(t, r)
            .id("imp-row-" + r.key).on_click((_e, cx) => { this.open[r.key] = !this.open[r.key]; cx.notify(); })))))
        .when(!this.follow, (el) => this.history(el, t, rows, logId))
        .when(!this.follow, (el) => el.child(h_flex().absolute().bottom(10).right(18)
          .id("imp-latest").px(10).py(4).rounded(12).bg(t.accent).text_color(t.accent_foreground)
          .text_size(11.5).cursor_pointer().on_click((_e, cx) => { this.toEnd(); cx.notify(); })
          .child("Latest ↓"))))
      .children(asks.map((a) => this.ask(t, a)))
      .when(Boolean(this.error), (el) => el.child(div().px(12).py(4).text_size(12).text_color(t.blocked).child(this.error)))
      .child(this.composer(t, cur));
  }

  key(e, cx) {
    const k = e.keystroke ?? e.key;
    if (k === "ctrl-enter" || k === "cmd-enter") { this.send(cx, "send"); cx.stop_propagation(); }
    else if (k === "ctrl-shift-enter") { this.send(cx, "queue"); cx.stop_propagation(); }
    else if (k === "f9") { this.dictate(cx); cx.stop_propagation(); }
  }

  header(t, cur) {
    const name = cur ? (cur.display_name || cur.name || cur.project || cur.sessionId.slice(0, 8)) : "No sessions";
    const state = cur?.attention ? (cur.attention_reason || "needs you") : (cur?.status ?? "");
    const dot = cur?.attention ? t.blocked : cur?.status === "busy" ? t.working : t.idle;
    const pick = this.sessions.slice(0, 16).map((s, i) =>
      div().id("imp-pick-" + i).px(8).py(2).rounded(6).text_size(11.5).cursor_pointer()
        .bg(cur && s.sessionId === cur.sessionId ? t.selected : t.chrome)
        .text_color(s.attention ? t.blocked : t.muted)
        .hover((st) => st.bg(t.hover))
        .on_click((_e, cx) => { this.picked = s.sessionId; this.tick(cx); })
        .child((s.short_name || s.project || s.sessionId.slice(0, 8)).slice(0, 22)));
    const doing = cur?.doing ? firstLine(cur.doing).slice(0, 160) : "";
    return v_flex().bg(t.chrome).border_b(1).border_color(t.border)
      .child(h_flex().h(34).px(12).gap(8).items_center()
        .child(div().w(8).h(8).rounded(4).bg(dot))
        .child(div().flex_1().truncate().text_size(13).font_semibold().child(name))
        .when(Boolean(cur?.hc), (el) => el.child(div().text_size(11).text_color(t.dim).child(cur.hc.address)))
        .child(div().text_size(11.5).text_color(cur?.attention ? t.blocked : t.dim).child(state)))
      .when(Boolean(doing) && cur?.status === "busy", (el) =>
        el.child(div().px(12).pb(4).truncate().text_size(11.5).text_color(t.muted).child(doing)))
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
          .on_click((_e, cx) => { this.drawn += DRAWN_TURNS; cx.notify(); })
          .child("show " + Math.min(rows.earlier, DRAWN_TURNS) + " earlier")]
      : [];
    return h_flex().px(10).py(4).gap(4).flex_wrap().border_b(1).border_color(t.border)
      .children(more.concat(chips));
  }

  row(t, r) {
    const open = this.open[r.key];
    const compact = r.mode === "compact" && !open;
    let body;
    if (r.kind === "speaker") {
      const color = r.speaker === "you" ? t.accent : r.speaker === "claude" ? t.foreground : t.muted;
      return div().px(14).pt(12).pb(2).text_size(11).font_semibold().text_color(color).child(r.label);
    }
    if (r.kind === "text") {
      body = compact
        ? div().truncate().text_color(t.muted).child(firstLine(r.text))
        : TextView.markdown("imp-md-" + r.key, hardBreaks(r.text)).selectable(true);
      return div().px(14).py(3).text_size(this.fontSize()).child(body);
    }
    if (r.kind === "thinking") {
      body = compact
        ? div().truncate().child("thinking: " + firstLine(r.text))
        : TextView.markdown("imp-th-" + r.key, hardBreaks(r.text)).selectable(true);
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
        .child(TextView.markdown("imp-rs-" + r.key, fence(r.text)).selectable(true));
    }
    if (r.kind === "note") {
      return div().px(14).py(2).text_size(11.5).italic().text_color(t.dim).truncate()
        .child(r.label + (r.text ? ": " + firstLine(r.text) : ""));
    }
    return div();
  }

  // A parked permission request or question for this session.
  ask(t, a) {
    const btn = (id, label, color, fn) => h_flex().id(id).h(28).px(12).items_center().rounded(7)
      .bg(color).text_color(t.solid_foreground).text_size(12).font_medium().cursor_pointer()
      .hover((s) => s.opacity(0.88)).on_click(fn).child(label);
    const box = v_flex().mx(10).my(6).p(10).gap(6).rounded(9).border_1().border_color(t.blocked).bg(t.raised);
    if (a.questions) {
      const picks = this.picks[a.id] ?? (this.picks[a.id] = {});
      return box.child(div().text_size(12).font_semibold().text_color(t.blocked).child("Claude is asking"))
        .children(a.questions.map((q, qi) => v_flex().gap(4)
          .child(div().text_size(12.5).child(q.question))
          .child(h_flex().gap(4).flex_wrap().children(q.options.map((o, oi) => {
            const on = (picks[q.question] ?? []).includes(o.label);
            return div().id("imp-q-" + a.id + "-" + qi + "-" + oi).px(9).py(3).rounded(6).text_size(12).cursor_pointer()
              .bg(on ? t.accent : t.background).text_color(on ? t.accent_foreground : t.foreground)
              .border_1().border_color(t.border)
              .on_click((_e, cx) => {
                const cur = picks[q.question] ?? [];
                picks[q.question] = q.multiSelect ? (on ? cur.filter((x) => x !== o.label) : cur.concat([o.label])) : [o.label];
                cx.notify();
              })
              .child(o.label);
          })))))
        .child(h_flex().gap(6).justify_end()
          .child(btn("imp-deny-" + a.id, "Answer in terminal", t.dim, (_e, cx) => this.decide(cx, a, "pass")))
          .child(btn("imp-allow-" + a.id, "Send answers", t.solid, (_e, cx) => this.decide(cx, a, "allow"))));
    }
    return box
      .child(h_flex().gap(6).items_center()
        .child(div().text_size(12).font_semibold().text_color(t.blocked).child(a.tool ?? "Permission"))
        .child(div().flex_1().truncate().text_size(12).font_family(this.mono()).text_color(t.foreground).child(a.input ?? "")))
      .when(Boolean(a.reason), (el) => el.child(div().text_size(11.5).text_color(t.muted).child(a.reason)))
      .child(h_flex().gap(6).justify_end()
        .child(btn("imp-pass-" + a.id, "In terminal", t.dim, (_e, cx) => this.decide(cx, a, "pass")))
        .child(btn("imp-deny-" + a.id, "Deny", t.blocked, (_e, cx) => this.decide(cx, a, "deny")))
        .child(btn("imp-allow-" + a.id, "Allow", t.working, (_e, cx) => this.decide(cx, a, "allow"))));
  }

  composer(t, cur) {
    const can = Boolean(cur) && !this.busy;
    const small = (id, label, active, fn, title) => h_flex().id(id).h(26).px(9).items_center().rounded(7)
      .bg(active ? t.blocked : t.raised).text_color(active ? t.solid_foreground : t.muted)
      .text_size(11.5).cursor_pointer().hover((s) => s.bg(active ? t.blocked : t.hover))
      .on_click(fn).child(label);
    return v_flex().p(8).gap(6).border_t(1).border_color(t.border).bg(t.chrome)
      .child(Textarea.new(this.box).w_full().px(8).py(6).rounded(8).border_1()
        .border_color(this.mic ? t.blocked : t.input).bg(t.background).text_size(13))
      .child(h_flex().gap(6).items_center()
        .child(small("imp-mic", this.mic ? "● Stop dictation" : "Dictate (F9)", this.mic, (_e, cx) => this.dictate(cx)))
        .child(div().flex_1())
        .child(small("imp-typed", "As typed", false, (_e, cx) => this.send(cx, "typed")))
        .child(small("imp-queue", "Queue", false, (_e, cx) => this.send(cx, "queue")))
        .child(h_flex().id("imp-send").h(28).px(14).items_center().rounded(8)
          .bg(can ? t.solid : t.raised).text_color(can ? t.solid_foreground : t.dim)
          .text_size(12.5).font_medium().cursor_pointer()
          .on_click((_e, cx) => this.send(cx, "send"))
          .child(this.busy === "send" ? "Sending…" : "Send")));
  }

  fontSize() { return this.cfg?.look?.fonts?.chat ?? 14; }
  mono() { return this.cfg?.mono ?? "Cascadia Mono"; }
}

function fence(text) {
  return "```\n" + String(text).replace(/```/g, "``​`") + "\n```";
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
