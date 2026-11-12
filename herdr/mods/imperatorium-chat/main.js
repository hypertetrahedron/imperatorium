// Imperatorium chat: one Claude Code session's conversation, read from the
// Imperatorium dispatcher, with a prompt box that sends through it.
//
// Which session: props().session when the sessions sidebar opened this copy
// as a pane (beside the session's terminal, or as a tab of its own), so it
// splits, zooms, moves and closes like any herdr pane. A copy placed in a
// layout without one follows the focused hc terminal, or the sidebar's pick.
// The dispatcher's address and token come from <app home>/imperatorium.json,
// written by `setup_machine.py herdr install`.
import { View, div, list } from "gpui-kit";
import { v_flex, h_flex, TextView, TextareaState, Textarea, Scrollbar } from "gpui-base";
import { props, theme, focused, appHome, readTextFile, notify, nextCommand, shared } from "herdr";
import { loadConfig, api, post } from "./client.js";
import { rowsOf, KINDS, MODES, nextMode, firstLine, toolLine, pendingFor, answerFor, hardBreaks, workLine } from "./rows.js";

const POLL_MS = 1500;
const WATCH_MS = 120;      // how soon a change of session is noticed (no network)
const BLOCKS = 400;        // newest blocks asked for; "load earlier" adds more

// Every chat copy runs in herdr's one JS engine. They share one poll of the
// session list and one cache of conversations, so a copy that opens on a
// session another copy has shown draws at once, and twenty panes cost one
// /api/sessions a tick rather than twenty.
const hub = (globalThis.__imperatoriumChat ??= { at: 0, data: null, inflight: null, convs: new Map() });

async function sessionsNow(cfg) {
  if (hub.data && Date.now() - hub.at < POLL_MS - 200) return hub.data;
  if (!hub.inflight) {
    hub.inflight = Promise.all([api(cfg, "/api/sessions"), api(cfg, "/api/pending")])
      .then(([s, p]) => {
        hub.data = { sessions: s.sessions ?? [], pending: p.pending ?? [] };
        hub.at = Date.now();
        return hub.data;
      })
      .finally(() => { hub.inflight = null; });
  }
  return hub.inflight;
}
const DRAWN_TURNS = 60;
const TAIL_ROWS = 40;      // drawn while following; more than a screen's worth
const HISTORY_STEP = 30;   // rows a scroll-back starts with, and each "earlier" adds
const DIFF_LINES = 60;     // of an Edit's diff, before "… N more lines"

export default class ImperatoriumChat extends View {
  init(_props, cx) {
    this.cfg = null;
    this.sessions = [];
    this.pending = [];
    this.pinned = props()?.session ?? null;  // given by the sidebar's split or tab
    this.picked = null;                        // the sidebar's pick, when not pinned
    this.items = [];
    this.base = null;                          // {start, first_block} of a tail
    this.blocks = BLOCKS;
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
      // A new focus or pick is fetched now, not at the next poll.
      cx.timer.every(WATCH_MS, (cx) => {
        const cur = this.current();
        if (cur && cur.sessionId !== this.shownId) this.tick(cx);
      });
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

  // The session this copy shows. A layout or split can pin one. Otherwise:
  // when the focused terminal changes to a session's, follow it; a pick in
  // the header or the sessions sidebar wins until focus moves again; with
  // nothing focused (a click into this pane) stay where we were.
  current() {
    if (this.pinned) return this.sessions.find((s) => s.sessionId === this.pinned) ?? { sessionId: this.pinned };
    const byId = (id) => id ? this.sessions.find((s) => s.sessionId === id) : null;
    const pick = shared("imperatorium.pick");
    if (pick && pick.at !== this.pickAt) { this.pickAt = pick.at; this.picked = pick.id; }
    const term = focused()?.terminal ?? null;
    if (term && term !== this.lastTerm && this.sessions.length) {
      this.lastTerm = term;
      const t = this.sessions.find((s) => s.hc && s.hc.address === term);
      if (t) { this.followed = t.sessionId; this.picked = null; }
    }
    return byId(this.picked) ?? byId(this.followed)
      ?? this.sessions.find((s) => s.attention && (s.pid || s.status))
      ?? this.sessions.find((s) => s.pid || s.status) ?? null;
  }

  async tick(cx) {
    if (!this.cfg || this.ticking) return;
    this.ticking = true;
    try {
      const d = await sessionsNow(this.cfg);
      this.sessions = d.sessions;
      this.pending = d.pending;
      const cur = this.current();
      if (!cur) { this.items = []; this.error = null; return; }
      const id = cur.sessionId;
      if (id !== this.shownId) {
        // Switched: draw what any copy last had for it, then ask what changed.
        this.swapDraft(id);
        this.shownId = id;
        this.open = {}; this.drawn = DRAWN_TURNS; this.blocks = BLOCKS;
        this.follow = true; this.gen++;
        const had = hub.convs.get(id);
        this.items = had?.items ?? []; this.base = had?.base ?? null; this.sig = had?.sig ?? null;
        if (had) this.blocks = Math.max(BLOCKS, had.blocks);
        cx.notify();
      }
      const q = "/api/conversation?session=" + encodeURIComponent(id) + "&blocks=" + this.blocks
        + (this.sig ? "&known=" + this.sig : "");
      const c = await api(this.cfg, q);
      if (id !== this.shownId) return;            // switched again meanwhile
      this.error = c.error ?? null;
      if (!c.unchanged) {
        this.items = c.items ?? []; this.sig = c.sig;
        this.base = { start: c.start ?? 0, first_block: c.first_block ?? 0 };
        hub.convs.set(id, { items: this.items, base: this.base, sig: this.sig, blocks: this.blocks });
        if (hub.convs.size > 40) hub.convs.delete(hub.convs.keys().next().value);
        if (this.follow) this.gen++;
      }
    } catch (e) {
      this.error = String(e?.message ?? e);
    } finally {
      this.ticking = false;
      cx.notify();
    }
  }

  // More of the conversation than the tail the dispatcher sent.
  loadEarlier(cx) {
    this.blocks += BLOCKS;
    this.sig = null;                           // the signature is the whole one's: ask again
    this.tick(cx);
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
    const more = from > 0 || rows.unloaded ? 1 : 0;
    return el
      .child(list(logId, shown.length + more,
        (i) => (more && i === 0) ? "earlier" : shown[i - more].key,
        (i) => (more && i === 0)
          ? div().px(14).py(6).text_size(11.5).text_color(t.accent)
              .child(from > 0 ? "Show earlier (" + from + " more rows)" : "Load earlier")
          : this.row(t, shown[i - more]))
        .size_full()
        .on_item_click((key, cx) => {
          if (key === "earlier") {
            if (from > 0) this.histFrom = Math.max(0, this.histFrom - HISTORY_STEP);
            else this.loadEarlier(cx);
          }
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
    const rows = rowsOf(this.items, this.modes, this.drawn, this.base);
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
    const doing = cur?.doing ? firstLine(cur.doing).slice(0, 160) : "";
    const facts = workLine(cur, Date.now() / 1000);
    return v_flex().bg(t.chrome).border_b(1).border_color(t.border)
      .child(h_flex().h(34).px(12).gap(8).items_center()
        .child(div().w(8).h(8).rounded(4).bg(dot))
        .child(div().flex_1().truncate().text_size(13).font_semibold().child(name))
        .when(Boolean(cur?.hc), (el) => el.child(div().text_size(11).text_color(t.dim).child(cur.hc.address)))
        .child(div().text_size(11.5).text_color(cur?.attention ? t.blocked : t.dim).child(state)))
      .when(Boolean(facts), (el) => el.child(div().px(12).pb(4).truncate().text_size(11).text_color(t.dim).child(facts)))
      .when(Boolean(doing) && cur?.status === "busy", (el) =>
        el.child(div().px(12).pb(4).truncate().text_size(11.5).text_color(t.muted).child(doing)));
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
      : rows.unloaded
      ? [div().id("imp-load").px(7).py(1).rounded(5).text_size(11).cursor_pointer().text_color(t.accent)
          .on_click((_e, cx) => this.loadEarlier(cx))
          .child("load earlier")]
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
      const line = h_flex().px(14).py(1).gap(6).text_size(12).font_family(this.mono())
        .child(div().text_color(t.ansi4).child((r.diff && !compact ? "▾ " : "▸ ") + r.name))
        .child(div().flex_1().truncate().text_color(t.muted).child(toolLine(r)));
      // An Edit's change, as the window shows it; Compact folds it away.
      if (!r.diff || compact) return line;
      // herdr's markdown view doesn't colour a diff block, so the lines are
      // drawn here: removed red, added green, hunk breaks dim.
      const lines = String(r.diff).split("\n");
      const shown = lines.slice(0, DIFF_LINES);
      return v_flex().child(line)
        .child(v_flex().ml(26).mr(14).my(2).px(8).py(4).rounded(6).bg(t.raised)
          .text_size(12).font_family(this.mono())
          .children(shown.map((l) => div().whitespace_nowrap().truncate()
            .text_color(l.startsWith("+") ? t.ansi2 : l.startsWith("-") ? t.ansi1 : t.dim)
            .child(l === "@@" ? "⋯" : (l || " "))))
          .when(lines.length > shown.length, (el) => el.child(div().text_color(t.dim)
            .child("… " + (lines.length - shown.length) + " more lines"))));
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

function fence(text, lang = "") {
  return "```" + lang + "\n" + String(text).replace(/```/g, "``​`") + "\n```";
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
