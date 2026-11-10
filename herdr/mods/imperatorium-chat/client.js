// Talking to the Imperatorium dispatcher.
//
// <app home>/imperatorium.json: {"url": "http://127.0.0.1:8792",
// "token_file": "<path>"}. The token file is written by the dispatcher at
// start (readable by this account only); a prompt sent with it counts as
// the person's own words, like one from the Imperatorium window.

const DEFAULT = { url: "http://127.0.0.1:8792", token_file: null };

export async function loadConfig(appHome, readTextFile) {
  let cfg = { ...DEFAULT };
  try {
    const raw = await readTextFile(appHome() + "/imperatorium.json");
    if (raw) cfg = { ...cfg, ...JSON.parse(raw) };
  } catch { /* defaults */ }
  cfg.url = String(cfg.url).replace(/\/+$/, "");
  cfg.readTextFile = readTextFile;
  cfg.token = null;
  await refreshToken(cfg);
  return cfg;
}

// The dispatcher makes a new token each start, so a 403 rereads it once.
export async function refreshToken(cfg) {
  if (!cfg.token_file) return;
  try {
    const t = await cfg.readTextFile(cfg.token_file);
    cfg.token = t ? t.trim() : null;
  } catch { cfg.token = null; }
}

function headers(cfg, json) {
  const h = {};
  if (json) h["Content-Type"] = "application/json";
  if (cfg.token) h["X-Imperatorium-Herdr"] = cfg.token;
  return h;
}

export async function api(cfg, path) {
  const r = await fetch(cfg.url + path, { headers: headers(cfg, false) });
  if (!r.ok) throw new Error("dispatcher said " + r.status + " for " + path.split("?")[0]);
  return await r.json();
}

export async function post(cfg, path, body) {
  const send = () => fetch(cfg.url + path, { method: "POST", headers: headers(cfg, true), body: JSON.stringify(body) });
  let r = await send();
  if (r.status === 403 && cfg.token_file) { await refreshToken(cfg); r = await send(); }
  let out;
  try { out = await r.json(); } catch { out = {}; }
  if (!r.ok && out.ok === undefined) out.ok = false;
  return out;
}
