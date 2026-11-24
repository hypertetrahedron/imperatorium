// The simulated fleet: plays Claude Code behind a sandboxed dispatcher.
// Each session is a held pid, a record in CLAUDE_CONFIG_DIR/sessions, a
// transcript under projects/, hooks posted to /hook, and the Imperatorium
// mod's beat (POST /api/mod/beat every 2 s, as mod/imperatorium does) which
// is how the window's prompts reach it. Launches the dispatcher makes (wt,
// claude --bg, claude stop, ssh inbox-send) are read from the stub's log and
// answered as the real thing would, after a startup delay.
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { sleep } from "./sandbox.mjs";

export const slug = p => p.replace(/[^a-zA-Z0-9]/g, "-");
const uuid = () => crypto.randomUUID();
const iso = t => new Date(t).toISOString();
const MODELS = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];

export class Fleet {
  constructor(sb, { beatMs = 2000, startupMs = 2500, workMs = [4000, 12000], replyWords = 60 } = {}) {
    this.sb = sb;
    this.beatMs = beatMs;
    this.startupMs = startupMs;
    this.workMs = workMs;
    this.replyWords = replyWords;
    this.sessions = new Map(); // sid -> session
    this.events = [];          // {t, kind, sid, ...}: for latency metrics
    this.logPos = 0;
    this.seenLaunch = new Set();
    this.agentsRows = [];
    this.remote = {};          // host -> {sessions: Map}
    this.running = true;
    this.permits = [];
  }

  note(kind, sid, extra = {}) { this.events.push({ t: Date.now(), kind, sid, ...extra }); }

  async hook(event, s, extra = {}) {
    const body = { hook_event_name: event, session_id: s.sid, cwd: s.cwd, transcript_path: s.transcript,
      session_name: s.name, ...extra };
    try { await fetch(this.sb.url + "/hook", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }); }
    catch (e) { this.note("hook-error", s.sid, { event, error: String(e) }); }
  }

  writeRecord(s) {
    if (s.kind !== "interactive" || s.gone) return;
    const rec = { pid: s.pid, sessionId: s.sid, cwd: s.cwd, startedAt: s.startedAt, version: "2.1.330", kind: "interactive",
      entrypoint: "cli", name: s.name, nameSource: "derived", status: s.status, updatedAt: Date.now(), statusUpdatedAt: Date.now() };
    fs.writeFileSync(path.join(this.sb.claude, "sessions", `${s.pid}.json`), JSON.stringify(rec));
  }

  writeAgents() {
    const rows = [...this.sessions.values()].filter(s => s.kind === "background").map(s => {
      const r = { sessionId: s.sid, id: s.short, kind: "background", state: s.bgState, status: s.status, cwd: s.cwd,
        name: s.name, key: s.sid, startedAt: s.startedAt };
      if (!s.gone) r.pid = s.pid;
      return r;
    });
    fs.writeFileSync(path.join(this.sb.bin, "agents.json"), JSON.stringify(rows));
  }

  append(s, entry) {
    fs.mkdirSync(path.dirname(s.transcript), { recursive: true });
    const base = { parentUuid: s.last || null, isSidechain: false, userType: "external", cwd: s.cwd, sessionId: s.sid,
      version: "2.1.330", gitBranch: s.branch || "main", uuid: uuid(), timestamp: iso(entry._t || Date.now()) };
    delete entry._t;
    const e = { ...base, ...entry };
    s.last = e.uuid;
    fs.appendFileSync(s.transcript, JSON.stringify(e) + "\n");
    return e;
  }

  userTurn(s, text, t) { this.append(s, { type: "user", message: { role: "user", content: text }, promptId: uuid(), _t: t }); }

  assistantTurn(s, { text, tools = 0, t, model } = {}) {
    for (let i = 0; i < tools; i++) {
      const id = "toolu_" + crypto.randomBytes(8).toString("hex");
      const file = `src/mod${i % 6}.py`;
      const name = i % 3 === 0 ? "Read" : i % 3 === 1 ? "Edit" : "Bash";
      const input = name === "Bash" ? { command: "python -m pytest -q", description: "Run the tests" } :
        name === "Edit" ? { file_path: path.join(s.cwd, file), old_string: "return", new_string: "return  " } : { file_path: path.join(s.cwd, file) };
      this.append(s, { type: "assistant", message: { id: "msg_" + uuid(), type: "message", role: "assistant", model: model || s.model,
        content: [{ type: "tool_use", id, name, input }], usage: { input_tokens: 1200, output_tokens: 80, cache_read_input_tokens: 30000, cache_creation_input_tokens: 400 } }, _t: t });
      this.append(s, { type: "user", message: { role: "user", content: [{ type: "tool_result", tool_use_id: id, content: name === "Bash" ? "12 passed in 0.4s" : "ok" }] }, _t: t });
    }
    const words = text || lorem(this.replyWords);
    this.append(s, { type: "assistant", message: { id: "msg_" + uuid(), type: "message", role: "assistant", model: model || s.model,
      content: [{ type: "text", text: words }], stop_reason: "end_turn",
      usage: { input_tokens: 2400, output_tokens: 350, cache_read_input_tokens: 42000, cache_creation_input_tokens: 900 } }, _t: t });
  }

  /** A session that is already running when the persona sits down. */
  async addInteractive(cwd, { name, status = "idle", history = 0, model, sid = uuid(), host, beat = true, branch } = {}) {
    const holder = this.sb.holdProcess();
    const now = Date.now();
    const s = { sid, cwd, name: name || path.basename(cwd) + "-" + sid.slice(0, 2), kind: "interactive", pid: holder.pid, holder,
      status, busy: status === "busy", startedAt: now - history * 60000, model: model || MODELS[sid.charCodeAt(0) % 2],
      transcript: path.join(this.sb.claude, "projects", slug(cwd), `${sid}.jsonl`), acks: [], stopped: [], results: [], beat, branch };
    this.sessions.set(sid, s);
    holder.on("exit", () => this.gone(s));
    this.append(s, { type: "permission-mode", permissionMode: "bypassPermissions" });
    for (let i = 0; i < history; i++) {
      const t = s.startedAt + i * 60000;
      this.userTurn(s, `Earlier task ${i + 1}: ${lorem(12)}`, t);
      this.assistantTurn(s, { tools: i % 4, t: t + 20000 });
    }
    this.writeRecord(s);
    await this.hook("SessionStart", s, { source: "startup" });
    this.note("session-up", sid, { cwd });
    if (beat) this.beatLoop(s);
    return s;
  }

  async addBackground(cwd, { short, prompt, sid = uuid(), name } = {}) {
    const holder = this.sb.holdProcess();
    const s = { sid, short: short || crypto.randomBytes(4).toString("hex"), cwd, name: name || "bg-" + path.basename(cwd), kind: "background",
      pid: holder.pid, holder, status: "busy", bgState: "working", busy: true, startedAt: Date.now(), model: MODELS[1],
      transcript: path.join(this.sb.claude, "projects", slug(cwd), `${sid}.jsonl`), acks: [], stopped: [], results: [], beat: true };
    this.sessions.set(sid, s);
    holder.on("exit", () => this.gone(s));
    this.append(s, { type: "permission-mode", permissionMode: "acceptEdits" });
    this.writeAgents();
    await this.hook("SessionStart", s, { source: "startup" });
    this.note("session-up", sid, { cwd, bg: s.short });
    this.beatLoop(s);
    if (prompt) this.runTurn(s, prompt);
    else { s.busy = false; s.status = "idle"; s.bgState = "idle"; this.writeAgents(); }
    return s;
  }

  gone(s) {
    if (s.gone) return;
    s.gone = true;
    s.status = "gone";
    if (s.kind === "interactive") { try { fs.rmSync(path.join(this.sb.claude, "sessions", `${s.pid}.json`)); } catch {} }
    else { s.bgState = "stopped"; this.writeAgents(); }
    this.note("session-gone", s.sid);
  }

  async runTurn(s, text, { workMs, tools } = {}) {
    if (s.gone) return;
    if (text.trim() === "/clear") return this.clear(s);
    const t0 = Date.now();
    s.busy = true; s.status = "busy"; s.turnStarted = t0;
    if (s.kind === "background") { s.bgState = "working"; this.writeAgents(); }
    this.userTurn(s, text, t0);
    this.writeRecord(s);
    this.kick(s);
    await this.hook("UserPromptSubmit", s, { prompt: text });
    this.note("turn-start", s.sid, { text: text.slice(0, 60) });
    const ms = workMs ?? s.workMs ?? (this.workMs[0] + Math.random() * (this.workMs[1] - this.workMs[0]));
    const turn = s.turn = { stopped: false };
    const end = Date.now() + ms;
    while (Date.now() < end && !turn.stopped && !s.gone && this.running) await sleep(100);
    if (s.gone) return;
    this.assistantTurn(s, { tools: tools ?? Math.floor(ms / 3000), text: turn.stopped ? "[Request interrupted by user]" : undefined });
    s.busy = false; s.status = "idle"; s.turn = null;
    if (s.kind === "background") { s.bgState = "idle"; this.writeAgents(); }
    this.writeRecord(s);
    this.kick(s);
    await this.hook("Stop", s, {});
    this.note("turn-end", s.sid, { ms: Date.now() - t0, stopped: turn.stopped });
  }

  /** /clear: the same process under a new session id and a new transcript. */
  async clear(s) {
    const old = s.sid;
    this.sessions.delete(old);
    s.sid = uuid();
    s.transcript = path.join(this.sb.claude, "projects", slug(s.cwd), `${s.sid}.jsonl`);
    s.last = null;
    this.sessions.set(s.sid, s);
    this.append(s, { type: "permission-mode", permissionMode: "bypassPermissions" });
    this.writeRecord(s);
    await this.hook("SessionStart", s, { source: "clear" });
    this.note("cleared", s.sid, { from: old });
  }

  /** A beat now, as the mod beats on every change (once one is not already in flight). */
  kick(s) { if (s.beat && !s.gone && s.kind !== undefined && !s.remoteHost) this.beatOnce(s); }

  async beatLoop(s) {
    while (this.running && !s.gone) {
      if (s.beat) await this.beatOnce(s);
      await sleep(this.beatMs);
    }
  }

  async beatOnce(s) {
    if (s.beating) { s.again = true; return; }
    s.beating = true;
    try { await this.beatInner(s); } finally { s.beating = false; }
    if (s.again) { s.again = false; this.beatOnce(s); }
  }

  async beatInner(s) {
    const body = { session_id: s.sid, cwd: s.cwd, busy: s.busy, turn_started: s.turnStarted || null,
      interactive: s.kind === "interactive", version: "2.1.330", mod_version: "0.4.0", acks: s.acks };
    if (s.stopped.length) body.stopped = s.stopped;
    if (s.results.length) body.results = s.results;
    if (!s.sentCommands) body.commands = [{ name: "clear", description: "Clear conversation history", source: "builtin" },
      { name: "model", description: "Set the AI model", source: "builtin" }, { name: "compact", description: "Compact", source: "builtin" },
      { name: "effort", description: "Set reasoning effort", source: "builtin" }, { name: "config", description: "Open config", source: "builtin" },
      { name: "resume", description: "Resume a conversation", source: "builtin" }, { name: "cost", description: "Show the session's cost", source: "builtin" },
      { name: "context", description: "Show context usage", source: "builtin" }, { name: "review", description: "Review a pull request", source: "skill" },
      { name: "changelog-draft", description: "Draft the changelog", source: "skill", argumentHint: "[version]" }];
    body.usage = { rateLimits: [{ kind: "five_hour", percentUsed: 37, resetsAt: Date.now() / 1000 + 7200 },
      { kind: "seven_day", percentUsed: 52, resetsAt: Date.now() / 1000 + 3 * 86400 }],
      context: { percent: 18, window: 200000, tokens: 36000 } };
    const sending = s.acks; s.acks = []; const stopped = s.stopped; s.stopped = []; const results = s.results; s.results = [];
    let reply;
    try {
      const r = await fetch(this.sb.url + "/api/mod/beat", { method: "POST",
        headers: { "Content-Type": "application/json", "X-Imperatorium-Mod": this.sb.modToken() }, body: JSON.stringify(body) });
      reply = await r.json();
      s.sentCommands = true;
    } catch (e) { s.acks = sending.concat(s.acks); s.stopped = stopped; s.results = results; return; }
    if (!reply?.ok) { s.acks = sending.concat(s.acks); return; }
    if (reply.stop) {
      const id = reply.stop.id ?? reply.stop;
      if (!(s.stopsSeen ||= new Set()).has(id)) {
        s.stopsSeen.add(id);
        const outcome = s.turn ? "aborted" : "idle";
        if (s.turn) s.turn.stopped = true;
        s.stopped.push({ id, outcome });
        this.note("stop", s.sid, { outcome });
        this.kick(s);
      }
    }
    for (const d of reply.deliver || []) {
      if (s.busy) continue; // as the mod does: offered again once idle
      s.acks.push({ id: d.id, outcome: "accepted" }, { id: d.id, outcome: "submitted" });
      this.note("delivered", s.sid, { text: String(d.text).slice(0, 60), as_user: d.as_user });
      this.runTurn(s, String(d.text));
    }
    for (const op of reply.ops || []) {
      if ((s.opsSeen ||= new Set()).has(op.id)) continue;
      s.opsSeen.add(op.id);
      const cmd = op.args?.command;
      this.note("op", s.sid, { op: op.op, command: cmd });
      if (op.op === "command" && cmd === "clear") {
        await this.clear(s);
        s.results.push({ id: op.id, ok: true, data: { text: "" } });
      } else if (op.op === "config") s.results.push({ id: op.id, ok: true, data: { rows: CONFIG_ROWS } });
      else s.results.push({ id: op.id, ok: true, data: { text: `(simulated) ${op.op}${cmd ? " /" + cmd : ""} done` } });
      this.kick(s);
    }
  }

  /** A tool call waiting on the person: parked at the dispatcher until decided. */
  async park(s, tool = "Bash", input = { command: "rm -rf build && make" }, { waitS = 600 } = {}) {
    s.status = "waiting"; this.writeRecord(s);
    await this.hook("Notification", s, { notification_type: "permission_prompt", message: `Claude needs your permission to use ${tool}` });
    const t0 = Date.now();
    this.note("parked", s.sid, { tool });
    const p = fetch(this.sb.url + "/permission/park", { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ session_id: s.sid, cwd: s.cwd, tool_name: tool, tool_input: input, wait: waitS, _hook_pid: s.pid }) })
      .then(r => r.json()).catch(e => ({ error: String(e) }))
      .then(async v => {
        this.note("decided", s.sid, { tool, ms: Date.now() - t0, verdict: v?.verdict ?? null });
        s.status = s.busy ? "busy" : "idle"; this.writeRecord(s);
        // As Claude Code goes on: the tool runs (allowed) or the turn carries on without it.
        if (v?.verdict === "allow") {
          // The turn goes on: the tool runs, then a few seconds of work, then the turn ends.
          await this.hook("PostToolUse", s, { tool_name: tool, tool_input: input, tool_response: { ok: true } });
          if (!s.turn) {
            s.busy = true; s.status = "busy"; this.writeRecord(s); this.kick(s);
            await sleep(5000);
            this.assistantTurn(s, { tools: 1 });
            s.busy = false; s.status = "idle"; this.writeRecord(s); this.kick(s);
            await this.hook("Stop", s, {});
          }
        } else await this.hook("Stop", s, {});
        return v;
      });
    this.permits.push(p);
    return p;
  }

  /** Reads the stub's log: what the dispatcher launched, and answers it. */
  async watchLaunches() {
    const file = path.join(this.sb.bin, "sim.log");
    while (this.running) {
      let text = "";
      try { text = fs.readFileSync(file, "utf8"); } catch {}
      const upto = text.lastIndexOf("\n") + 1;
      const complete = upto > this.logPos ? text.slice(this.logPos, upto).split("\n") : [];
      if (upto > this.logPos) this.logPos = upto;
      for (const line of complete) {
        if (!line.trim()) continue;
        let e; try { e = JSON.parse(line); } catch { continue; }
        this.onLaunch(e);
      }
      await sleep(150);
    }
  }

  onLaunch(e) {
    const a = e.args || [];
    const at = (flag) => { const i = a.indexOf(flag); return i >= 0 ? a[i + 1] : undefined; };
    this.note("launch", null, { prog: e.prog, args: a.slice(0, 12) });
    if (e.prog === "wt" || (e.prog === "claude" && !e.bg && a.includes("--session-id"))) {
      const ci = a.indexOf("claude");
      const cargs = e.prog === "wt" ? (ci >= 0 ? a.slice(ci + 1) : []) : a;
      if (e.prog === "wt" && ci < 0) return;
      const cwd = at("-d") || e.cwd;
      const ai = (f) => { const i = cargs.indexOf(f); return i >= 0 ? cargs[i + 1] : undefined; };
      const sid = ai("--session-id") || (!cargs.includes("--fork-session") && ai("--resume")) || uuid();
      const resumed = ai("--resume");
      const prompt = cargs.length && !cargs[cargs.length - 1].startsWith("-") && cargs[cargs.length - 2] !== "--session-id"
        && cargs[cargs.length - 2] !== "--resume" && cargs[cargs.length - 2] !== "--model" && cargs[cargs.length - 2] !== "--effort"
        && cargs[cargs.length - 2] !== "--permission-mode" ? cargs[cargs.length - 1] : null;
      setTimeout(async () => {
        const s = await this.addInteractive(cwd, { sid });
        if (resumed && resumed !== sid) this.note("forked", sid, { from: resumed });
        if (prompt) this.runTurn(s, prompt);
      }, this.startupMs);
    } else if (e.prog === "claude" && e.bg) {
      const prompt = a.filter(x => !x.startsWith("-")).pop();
      setTimeout(() => this.addBackground(e.cwd, { short: e.bg, prompt: prompt && prompt !== e.cwd ? prompt : null }), this.startupMs);
    } else if (e.prog === "claude" && a[0] === "stop") {
      const s = [...this.sessions.values()].find(x => x.short === a[1]);
      if (s) { try { s.holder.kill(); } catch {} this.gone(s); }
    } else if (e.prog === "ssh" && e.stdin) {
      this.onRemoteInject(e.host, e.stdin);
    }
  }

  // ------------------------------------------------------------ remote hosts
  remoteHost(host, { sessions = 1, cwdBase = "/home/dev/projects", tunnelPort } = {}) {
    const h = this.remote[host] = { host, sessions: new Map(), tunnelPort };
    for (let i = 0; i < sessions; i++) {
      const sid = uuid();
      const cwd = `${cwdBase}/svc-${i}`;
      const claude = path.join(this.sb.bin, "remote-home", host, ".claude");
      const s = { sid, cwd, name: `svc-${i}`, kind: "interactive", pid: 4000 + i, status: "idle", busy: false, startedAt: Date.now() - 3600e3,
        model: MODELS[1], host, socket: `/tmp/claude-1000/cc-msg-${sid.slice(0, 8)}.sock`,
        transcript: path.join(claude, "projects", slug(cwd), `${sid}.jsonl`) };
      h.sessions.set(sid, s);
      for (let k = 0; k < 6; k++) { this.userTurn(s, `remote task ${k}: ${lorem(10)}`, s.startedAt + k * 60000); this.assistantTurn(s, { tools: 2, t: s.startedAt + k * 60000 + 20000 }); }
    }
    this.writeRemote(host);
    if (tunnelPort) for (const s of h.sessions.values()) this.remoteHook(h, "SessionStart", s, { source: "startup" }, 30000);
    return h;
  }

  /** A host session's hook, as it arrives through the reverse tunnel: retried until the listener is up. */
  async remoteHook(h, event, s, extra = {}, patienceMs = 5000) {
    const body = { hook_event_name: event, session_id: s.sid, cwd: s.cwd, session_name: s.name,
      messaging_socket: s.socket, messaging_token: "tok-" + s.sid.slice(0, 8), ...extra };
    const end = Date.now() + patienceMs;
    while (this.running) {
      try {
        const r = await fetch(`http://127.0.0.1:${h.tunnelPort}/hook`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
        if (r.ok) { this.note("remote-hook", s.sid, { event }); return; }
      } catch {}
      if (Date.now() > end) { this.note("remote-hook-failed", s.sid, { event }); return; }
      await sleep(250);
    }
  }

  writeRemote(host) {
    const h = this.remote[host];
    const rows = [...h.sessions.values()].map(s => ({ sessionId: s.sid, kind: "interactive", status: s.status, cwd: s.cwd, pid: s.pid, name: s.name }));
    const files = [...h.sessions.values()].map(s => JSON.stringify({ pid: s.pid, sessionId: s.sid, cwd: s.cwd, startedAt: s.startedAt, version: "2.1.330",
      kind: "interactive", name: s.name, status: s.status, messagingSocketPath: s.socket, updatedAt: Date.now() }));
    fs.writeFileSync(path.join(this.sb.bin, `remote-${host}.out`), JSON.stringify(rows) + "\n\n--- imperatorium session files ---\n" + files.join("\n") + "\n");
  }

  async onRemoteInject(host, stdin) {
    const h = this.remote[host]; if (!h) return;
    let text = stdin;
    // inbox-send's stdin: an auth line, then the message (its text in message.content or content).
    for (const line of stdin.split("\n").filter(Boolean)) {
      try {
        const j = JSON.parse(line);
        if (j.type === "auth") continue;
        const c = j.message?.content ?? j.content ?? j.text;
        if (c) { text = typeof c === "string" ? c : JSON.stringify(c); break; }
      } catch {}
    }
    const s = [...h.sessions.values()][0];
    this.note("delivered", s.sid, { remote: host, text: String(text).slice(0, 60) });
    s.status = "busy"; s.busy = true; this.writeRemote(host);
    this.userTurn(s, String(text));
    if (h.tunnelPort) this.remoteHook(h, "UserPromptSubmit", s, { prompt: String(text) });
    await sleep(this.workMs[0]);
    this.assistantTurn(s, { tools: 2 });
    s.status = "idle"; s.busy = false; this.writeRemote(host);
    if (h.tunnelPort) this.remoteHook(h, "Stop", s, {});
    this.note("turn-end", s.sid, { remote: host });
  }

  /** Past sessions with no process: what History searches. */
  pastSessions(cwd, n, { daysBack = 60, turns = 4 } = {}) {
    for (let i = 0; i < n; i++) {
      const sid = uuid();
      const t = Date.now() - Math.random() * daysBack * 864e5;
      const s = { sid, cwd, transcript: path.join(this.sb.claude, "projects", slug(cwd), `${sid}.jsonl`), model: MODELS[i % 3] };
      this.append(s, { type: "permission-mode", permissionMode: "default" });
      const topic = TOPICS[i % TOPICS.length];
      for (let k = 0; k < turns; k++) { this.userTurn(s, `${topic} - step ${k + 1}: ${lorem(8)}`, t + k * 90000); this.assistantTurn(s, { tools: 1, t: t + k * 90000 + 30000 }); }
      const mt = new Date(t + turns * 90000);
      fs.utimesSync(s.transcript, mt, mt);
    }
  }

  async stop() {
    this.running = false;
    await sleep(200);
  }
}

// What the mod's config op answers: the rows Claude Code's /config menu draws.
const CONFIG_ROWS = [
  { key: "autoCompactEnabled", label: "Auto-compact", kind: "boolean", value: true },
  { key: "verbose", label: "Verbose output", kind: "boolean", value: false },
  { key: "theme", label: "Theme", kind: "choice", value: "dark", options: ["dark", "light", "dark-daltonized", "light-daltonized"] },
  { key: "outputStyle", label: "Output style", kind: "choice", value: "default", options: ["default", "Explanatory", "Learning"] },
  { key: "editorMode", label: "Editor mode", kind: "choice", value: "normal", options: ["normal", "vim"] },
  { key: "todoFeatureEnabled", label: "Use todo list", kind: "boolean", value: true },
  { key: "preferredNotifChannel", label: "Notifications", kind: "choice", value: "auto", options: ["auto", "terminal_bell", "notifications_disabled"] },
  { key: "cleanupPeriodDays", label: "Keep transcripts (days)", kind: "number", value: 30 },
];

const TOPICS = ["fix the flaky login test", "add retry to the fetch helper", "migrate the settings page", "write the release notes",
  "profile the slow import", "refactor the parser", "bump dependencies", "add dark mode", "investigate the memory leak", "document the API"];
const WORDS = "the change keeps the helper small and the tests now cover the retry path while the old behaviour stays for callers that pass no timeout so nothing else moves".split(" ");
export function lorem(n) { const o = []; for (let i = 0; i < n; i++) o.push(WORDS[(i * 7 + n) % WORDS.length]); return o.join(" ") + "."; }
