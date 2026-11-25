// A dispatcher under test that cannot reach anything real: its own temp
// --home, --state, CLAUDE_CONFIG_DIR, LOCALAPPDATA/APPDATA, a PATH whose
// `claude`, `wt` and `ssh` are the persona stub, and every outward URL
// pointed at a port nothing serves. Mirrors rust/imperatorium/tests/common
// (clean_env, isolate, assert_launches_are_fakes) so a persona run is as
// safe as the integration tests.
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import crypto from "node:crypto";
import { spawn, spawnSync, execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO = path.resolve(HERE, "..", "..");
const STRIPPED = ["CLAUDE", "ANTHROPIC_", "IMPERATORIUM_", "AI_AGENT"];
const REAL = ["wt.exe", "claude.exe", "wt", "claude",
  "wt.cmd", "claude.cmd", "claude.bat"];
const NOWHERE = "http://127.0.0.1:9";

export function binary() {
  const p = process.env.IMPERATORIUM_TEST_BIN || path.join(REPO, "rust", "target", "debug", "imperatorium.exe");
  if (!fs.existsSync(p)) throw new Error(`no dispatcher build at ${p}: cargo build -p imperatorium (in rust/)`);
  return p;
}

/** The stub, compiled once per source change into the OS temp folder. */
export function stub() {
  const src = path.join(HERE, "..", "stub", "simclaude.rs");
  const text = fs.readFileSync(src);
  const hash = crypto.createHash("sha256").update(text).digest("hex").slice(0, 12);
  const dir = path.join(os.tmpdir(), "imp-persona-stub");
  fs.mkdirSync(dir, { recursive: true });
  const exe = path.join(dir, `simclaude-${hash}.exe`);
  if (!fs.existsSync(exe)) {
    const cargoBin = path.join(os.homedir(), ".cargo", "bin", "rustc.exe");
    const rustc = process.env.RUSTC || (fs.existsSync(cargoBin) ? cargoBin : "rustc");
    const r = spawnSync(rustc, ["--edition", "2021", "-O", "-o", exe + ".part", src], { encoding: "utf8" });
    if (r.status !== 0) throw new Error("rustc failed: " + r.stderr);
    fs.renameSync(exe + ".part", exe);
  }
  return exe;
}

function standInPath(binDir) {
  const keep = [binDir];
  for (const d of (process.env.PATH || "").split(path.delimiter)) {
    if (!d) continue;
    if (REAL.some(n => { try { return fs.statSync(path.join(d, n)).isFile(); } catch { return false; } })) continue;
    // ssh: the stub must win, so drop any folder with a real ssh as well.
    if (["ssh.exe", "ssh"].some(n => { try { return fs.statSync(path.join(d, n)).isFile(); } catch { return false; } })) continue;
    keep.push(d);
  }
  return keep.join(path.delimiter);
}

function assertFakes(binDir, envPath, stubBytes) {
  const exts = (process.env.PATHEXT || ".COM;.EXE;.BAT;.CMD").split(";").filter(Boolean);
  for (const name of ["claude", "wt", "ssh"]) {
    let found = null;
    for (const d of envPath.split(path.delimiter)) {
      for (const e of ["", ...exts]) {
        const p = path.join(d, name + e);
        try { if (fs.statSync(p).isFile()) { found = p; break; } } catch {}
      }
      if (found) break;
    }
    if (!found || path.dirname(found).toLowerCase() !== binDir.toLowerCase())
      throw new Error(`SAFETY: ${name} resolves to ${found}, not the stub in ${binDir}`);
    if (!fs.readFileSync(found).equals(stubBytes)) throw new Error(`SAFETY: ${found} is not the stub`);
  }
  const first = [path.dirname(binary())];
  const win = process.env.SystemRoot || process.env.windir;
  if (win) first.push(path.join(win, "System32"), win);
  for (const d of first) for (const n of ["wt.exe", "claude.exe"])
    if (fs.existsSync(path.join(d, n))) throw new Error(`SAFETY: ${path.join(d, n)} would be found before PATH`);
}

function git(cwd, ...args) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, GIT_AUTHOR_NAME: "persona", GIT_AUTHOR_EMAIL: "persona@example.invalid",
      GIT_COMMITTER_NAME: "persona", GIT_COMMITTER_EMAIL: "persona@example.invalid" } }).trim();
}

/** A small git project with a few commits and, optionally, uncommitted edits. */
export function makeProject(root, name, { files = 6, commits = 3, dirty = false, git: isGit = true } = {}) {
  const dir = path.join(root, name);
  fs.mkdirSync(path.join(dir, "src"), { recursive: true });
  fs.writeFileSync(path.join(dir, "README.md"), `# ${name}\n\nA persona-run project.\n`);
  for (let i = 0; i < files; i++) fs.writeFileSync(path.join(dir, "src", `mod${i}.py`), `def f${i}():\n    return ${i}\n`);
  if (!isGit) return dir;
  git(dir, "init", "-q", "-b", "main");
  git(dir, "add", "-A"); git(dir, "commit", "-q", "-m", "initial");
  for (let c = 1; c < commits; c++) {
    fs.appendFileSync(path.join(dir, "src", `mod${c % files}.py`), `# change ${c}\n`);
    git(dir, "commit", "-qam", `change ${c}`);
  }
  if (dirty) fs.appendFileSync(path.join(dir, "src", "mod0.py"), "# uncommitted\n");
  return dir;
}

export { git };

export class Sandbox {
  constructor(label, { config = {}, policy = null, keep = false } = {}) {
    this.label = label;
    this.keep = keep;
    this.root = fs.mkdtempSync(path.join(os.tmpdir(), `imp-persona-${label}-`));
    this.home = path.join(this.root, "home");
    this.stateFile = path.join(this.root, "state", "state.json");
    this.claude = path.join(this.root, "claude");
    this.bin = path.join(this.root, "bin");
    this.projects = path.join(this.root, "projects");
    this.userHome = path.join(this.root, "user");
    fs.mkdirSync(this.userHome, { recursive: true });
    fs.writeFileSync(path.join(this.userHome, ".gitconfig"), "[user]\n\tname = persona\n\temail = persona@example.invalid\n[safe]\n\tdirectory = *\n");
    for (const d of [this.home, path.dirname(this.stateFile), path.join(this.claude, "sessions"),
      path.join(this.claude, "projects"), this.bin, this.projects]) fs.mkdirSync(d, { recursive: true });
    this.config = { roots: [this.projects], ...config };
    this.policy = policy;
    this.children = [];
  }

  /** The stub under every name, the remote build and probe answer. */
  prepareBin() {
    const s = stub();
    this.stubBytes = fs.readFileSync(s);
    for (const n of ["claude.exe", "wt.exe", "ssh.exe"]) fs.copyFileSync(s, path.join(this.bin, n));
    this.holder = path.join(this.bin, "simhold.exe");
    fs.copyFileSync(s, this.holder);
    fs.writeFileSync(path.join(this.bin, "imperatorium.path"), binary());
    const build = path.join(this.root, "builds", "x86_64-unknown-linux-musl");
    fs.mkdirSync(build, { recursive: true });
    const bytes = Buffer.from("not really a linux binary");
    fs.writeFileSync(path.join(build, "imperatorium"), bytes);
    const hash = crypto.createHash("sha256").update(bytes).digest("hex");
    fs.writeFileSync(path.join(this.bin, "probe.out"), `x86_64\n${hash}  imperatorium\n`);
  }

  env() {
    const env = {};
    for (const [k, v] of Object.entries(process.env)) {
      const K = k.toUpperCase();
      if (K === "PATH" || STRIPPED.some(p => K.startsWith(p))) continue;
      env[k] = v;
    }
    env.PATH = standInPath(this.bin);
    Object.assign(env, {
      LOCALAPPDATA: path.join(this.root, "local"), APPDATA: path.join(this.root, "roaming"),
      // The user's home too: ~/.ssh (key login) and ~/.claude must be the sandbox's.
      USERPROFILE: this.userHome, HOME: this.userHome,
      XDG_STATE_HOME: path.join(this.root, "xstate"), XDG_CONFIG_HOME: path.join(this.root, "xconfig"),
      CLAUDE_CONFIG_DIR: this.claude,
      IMPERATORIUM_POLICY: path.join(this.root, "policy.json"),
      IMPERATORIUM_PARK_ENDPOINT: `${NOWHERE}/permission/park`,
      IMPERATORIUM_ENDPOINT: `${NOWHERE}/hook`,
      IMPERATORIUM_TEST_DEEPLINK_LOG: path.join(this.root, "deeplinks.log"),
      IMPERATORIUM_TEST_OPEN_LOG: path.join(this.root, "opened.log"),
      IMPERATORIUM_RELEASES: `${NOWHERE}/releases`,
      IMPERATORIUM_ACCOUNT_SPEND_URL: `${NOWHERE}/spend`,
      IMPERATORIUM_SSH: path.join(this.bin, "ssh.exe"),
      IMPERATORIUM_REMOTE_BUILDS: path.join(this.root, "builds"),
      HC_SESSION: "imptest",
      // Ask the (stub) claude for its models and commands, as the live dispatcher does.
      IMPERATORIUM_CLAUDE_PROBE: "1",
    });
    return env;
  }

  async start() {
    this.prepareBin();
    fs.writeFileSync(path.join(this.home, "config.json"), JSON.stringify(this.config, null, 1));
    if (this.policy) fs.writeFileSync(path.join(this.root, "policy.json"), JSON.stringify(this.policy));
    return this.launch(0);
  }

  /** As a person restarts the dispatcher: the same port, home and state; config.json as saved. */
  async restart() {
    if (this.proc && this.proc.exitCode === null) {
      try { execFileSync("taskkill", ["/T", "/F", "/PID", String(this.proc.pid)], { stdio: "ignore" }); } catch {}
    }
    await sleep(500);
    return this.launch(this.port);
  }

  async launch(port) {
    const env = this.env();
    assertFakes(this.bin, env.PATH, this.stubBytes);
    this.logFile = path.join(this.root, port ? "serve-restart.log" : "serve.log");
    this.port = undefined;
    const out = fs.openSync(this.logFile, "w");
    this.proc = spawn(binary(), ["serve", "--port", String(port), "--host", "127.0.0.1", "--home", this.home, "--state", this.stateFile],
      { cwd: this.root, env, stdio: ["ignore", out, out], windowsHide: true });
    this.startedAt = Date.now();
    const deadline = Date.now() + 60000;
    while (Date.now() < deadline) {
      const m = fs.readFileSync(this.logFile, "utf8").match(/listening on http:\/\/[^\s]*:(\d+)/);
      if (m) { this.port = +m[1]; break; }
      if (this.proc.exitCode !== null) throw new Error("dispatcher exited: " + fs.readFileSync(this.logFile, "utf8"));
      await sleep(50);
    }
    if (!this.port) throw new Error("dispatcher never listened: " + fs.readFileSync(this.logFile, "utf8"));
    this.url = `http://127.0.0.1:${this.port}`;
    while (Date.now() < deadline) {
      try { if ((await fetch(this.url + "/api/health")).ok) break; } catch {}
      await sleep(50);
    }
    this.bootMs = Date.now() - this.startedAt;
    this.modToken = () => fs.readFileSync(path.join(path.dirname(this.stateFile), "mod-token"), "utf8").trim();
    return this;
  }

  /** A process that stays alive: the pid a simulated session runs on. */
  holdProcess() {
    const p = spawn(this.holder, ["hold"], { stdio: "ignore", windowsHide: true, detached: false });
    this.children.push(p);
    return p;
  }

  /** Working set and CPU seconds of the dispatcher, from tasklist/wmic-free PowerShell-free query. */
  procStats() {
    try {
      const out = execFileSync("tasklist", ["/FI", `PID eq ${this.proc.pid}`, "/FO", "CSV", "/NH"], { encoding: "utf8" });
      const m = out.match(/"([\d,.]+) K"/);
      return { workingSetMB: m ? Math.round(+m[1].replace(/[,.]/g, "") / 1024) : null };
    } catch { return { workingSetMB: null }; }
  }

  launches() {
    try {
      return fs.readFileSync(path.join(this.bin, "sim.log"), "utf8").split("\n").filter(Boolean).map(l => JSON.parse(l));
    } catch { return []; }
  }

  outwardLeaks() {
    const read = f => { try { return fs.readFileSync(f, "utf8").split("\n").filter(Boolean); } catch { return []; } };
    return { deepLinks: read(path.join(this.root, "deeplinks.log")), opened: read(path.join(this.root, "opened.log")) };
  }

  async stop() {
    for (const c of this.children) { try { c.kill(); } catch {} }
    if (this.proc && this.proc.exitCode === null) {
      try { execFileSync("taskkill", ["/T", "/F", "/PID", String(this.proc.pid)], { stdio: "ignore" }); } catch {}
    }
    // Anything still running from the stub folder (tunnels, holders).
    try {
      execFileSync("powershell", ["-NoProfile", "-Command",
        `Get-Process | Where-Object { $_.Path -like '${this.bin.replace(/'/g, "''")}*' } | Stop-Process -Force`], { stdio: "ignore" });
    } catch {}
    await sleep(300);
    if (!this.keep) { try { fs.rmSync(this.root, { recursive: true, force: true, maxRetries: 5 }); } catch {} }
  }
}

export const sleep = ms => new Promise(r => setTimeout(r, ms));
