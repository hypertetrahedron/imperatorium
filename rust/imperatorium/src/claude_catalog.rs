//! What Claude Code says about itself: its models (with the effort levels
//! each takes), the slash commands that run without a terminal, its output
//! styles and agents. Asked of a headless `claude -p` with one `initialize`
//! control request, which makes no API call; cached in the state folder by
//! Claude Code version and asked again when a mod reports a newer one.
//!
//! The probe runs in an empty folder of its own, with only that folder's
//! (absent) local settings and no MCP servers, so it loads no hooks, no
//! plugins (so no mod) and reaches no dispatcher. Only the live dispatcher,
//! or one started with IMPERATORIUM_CLAUDE_PROBE set, starts it: a test
//! dispatcher never runs a real claude.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use crate::locks::LockExt;

pub const FILE: &str = "claude-catalog.json";
/// How long the probe may take: Claude Code's own start, nothing more.
const PROBE_FOR: Duration = Duration::from_secs(60);
/// A failed probe is not tried again sooner than this.
const RETRY_AFTER: f64 = 600.0;
const INIT: &str = r#"{"type":"control_request","request_id":"imperatorium-catalog","request":{"subtype":"initialize"}}"#;

/// Pure: the catalog out of a probe's stream-json output, or None.
pub fn parse_init(stdout: &str, now: f64) -> Option<Value> {
    let r = stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|o| o["type"] == "control_response")?;
    let r = if r["response"]["response"].is_object() {
        &r["response"]["response"]
    } else {
        &r["response"]
    };
    let models = r["models"].as_array()?;
    let pick = |o: &Value, keys: &[&str]| {
        let mut out = serde_json::Map::new();
        for k in keys {
            if let Some(v) = o.get(*k).filter(|v| !v.is_null()) {
                out.insert((*k).into(), v.clone());
            }
        }
        Value::Object(out)
    };
    let models: Vec<Value> = models
        .iter()
        .filter(|m| m["value"].is_string())
        .map(|m| {
            pick(m, &["value", "resolvedModel", "displayName", "description", "supportsEffort",
                      "supportedEffortLevels", "supportsFastMode", "supportsAutoMode"])
        })
        .collect();
    let commands: Vec<Value> = r["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["name"].is_string())
        .map(|c| pick(c, &["name", "description", "argumentHint"]))
        .collect();
    let agents: Vec<Value> = r["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a.get("name").cloned())
        .collect();
    Some(json!({
        "version": r["claude_code_version"],
        "models": models,
        "commands": commands,
        "output_styles": r["available_output_styles"],
        "output_style": r["output_style"],
        "agents": agents,
        "permission_mode": r["current_permission_mode"],
        "fast_mode": r["fast_mode_state"],
        "probed_at": now,
    }))
}

/// The commands a catalog lists as running without a terminal.
pub fn text_commands(catalog: &Value) -> std::collections::HashSet<String> {
    catalog["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["name"].as_str().map(str::to_string))
        .collect()
}

/// The probe's environment: the dispatcher's, without a Claude Code
/// session's variables (CLAUDE_CONFIG_DIR stays: it names the account).
fn probe_env() -> Vec<(String, String)> {
    crate::launch::launch_env()
        .into_iter()
        .filter(|(k, _)| {
            let k = k.to_ascii_uppercase();
            !k.starts_with("CLAUDE") || k == "CLAUDE_CONFIG_DIR"
        })
        .collect()
}

/// Run the probe once; the catalog or why not.
pub fn probe(dir: &Path, now: f64) -> Result<Value, String> {
    let exe = std::env::var("IMPERATORIUM_CLAUDE")
        .ok()
        .filter(|e| !e.is_empty())
        .or_else(|| crate::launch::which("claude"))
        .ok_or("claude is not on PATH")?;
    let folder = dir.join("claude-probe");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let input = folder.join("initialize.jsonl");
    std::fs::write(&input, format!("{INIT}\n")).map_err(|e| e.to_string())?;
    let stdin = std::fs::File::open(&input).map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--setting-sources",
        "local",
        "--strict-mcp-config",
        "--no-session-persistence",
    ])
    .current_dir(&folder)
    .env_clear()
    .envs(probe_env());
    let out = crate::proc::run_timeout_stdin(&mut cmd, PROBE_FOR, stdin.into())
        .map_err(|e| format!("claude did not answer: {e}"))?;
    parse_init(&String::from_utf8_lossy(&out.stdout), now)
        .ok_or_else(|| "claude's answer had no catalog".to_string())
}

#[derive(Default)]
struct State {
    current: Option<Value>,
    tried_at: f64,
}

/// The cached catalog, asked again when out of date.
pub struct Catalog {
    path: PathBuf,
    dir: PathBuf,
    may_probe: bool,
    state: Arc<Mutex<State>>,
    running: Arc<AtomicBool>,
}

impl Catalog {
    pub fn new(dir: &Path, may_probe: bool) -> Self {
        let path = dir.join(FILE);
        let current = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(|v| v["models"].is_array());
        Catalog {
            path,
            dir: dir.to_path_buf(),
            may_probe,
            state: Arc::new(Mutex::new(State { current, tried_at: 0.0 })),
            running: Arc::default(),
        }
    }

    pub fn get(&self) -> Option<Value> {
        self.state.lk().current.clone()
    }

    pub fn probing(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Probe in the background when there is no catalog, or it is of
    /// another Claude Code than `version` (a mod's report).
    pub fn ensure(&self, version: Option<&str>, now: f64) {
        if !self.may_probe {
            return;
        }
        {
            let st = self.state.lk();
            let stale = match (&st.current, version.filter(|v| !v.is_empty())) {
                (None, _) => true,
                (Some(c), Some(v)) => c["version"].as_str() != Some(v),
                (Some(_), None) => false,
            };
            if !stale || now - st.tried_at < RETRY_AFTER && st.tried_at > 0.0 {
                return;
            }
        }
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        self.state.lk().tried_at = now;
        let (state, running, dir, path) =
            (Arc::clone(&self.state), Arc::clone(&self.running), self.dir.clone(), self.path.clone());
        let spawned = std::thread::Builder::new().name("claude-catalog".into()).spawn(move || {
            match probe(&dir, now) {
                Ok(c) => {
                    if let Ok(text) = serde_json::to_string_pretty(&c) {
                        let _ = std::fs::write(&path, text);
                    }
                    state.lk().current = Some(c);
                }
                Err(e) => eprintln!("imperatorium: Claude Code catalog: {e}"),
            }
            running.store(false, Ordering::SeqCst);
        });
        if spawned.is_err() {
            self.running.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = concat!(
        r#"{"type":"system","subtype":"noise"}"#, "\n",
        r#"{"type":"control_response","response":{"subtype":"success","request_id":"imperatorium-catalog","response":{"#,
        r#""claude_code_version":"2.1.330","output_style":"default","available_output_styles":["default","Concise"],"#,
        r#""current_permission_mode":"default","fast_mode_state":"off","agents":[{"name":"Explore"}],"#,
        r#""commands":[{"name":"compact","description":"Free up context","argumentHint":"<instructions>","builtin":true}],"#,
        r#""models":[{"value":"opus","displayName":"Opus 5.5","description":"For complex work","supportsEffort":true,"#,
        r#""supportedEffortLevels":["low","high"],"supportsFastMode":true,"secret":"x"},{"displayName":"no value"}]}}}"#,
        "\n"
    );

    #[test]
    fn the_catalog_is_read_out_of_the_initialize_answer() {
        let c = parse_init(OUT, 5.0).unwrap();
        assert_eq!(c["version"], "2.1.330");
        assert_eq!(c["models"].as_array().unwrap().len(), 1);
        assert_eq!(c["models"][0]["supportedEffortLevels"], json!(["low", "high"]));
        assert!(c["models"][0].get("secret").is_none());
        assert_eq!(c["commands"][0], json!({"name": "compact", "description": "Free up context",
                                             "argumentHint": "<instructions>"}));
        assert_eq!(c["output_styles"], json!(["default", "Concise"]));
        assert_eq!(c["agents"], json!(["Explore"]));
        assert!(text_commands(&c).contains("compact"));
    }

    #[test]
    fn no_answer_is_no_catalog() {
        assert_eq!(parse_init("", 0.0), None);
        assert_eq!(parse_init("{\"type\":\"control_response\",\"response\":{}}", 0.0), None);
    }

    #[test]
    fn a_cached_catalog_is_read_and_never_probed_when_not_allowed() {
        let dir = std::env::temp_dir().join(format!("imp-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), serde_json::to_string(&parse_init(OUT, 1.0).unwrap()).unwrap()).unwrap();
        let c = Catalog::new(&dir, false);
        assert_eq!(c.get().unwrap()["version"], "2.1.330");
        c.ensure(Some("9.9.9"), 100.0);
        assert!(!c.probing());
        assert_eq!(c.get().unwrap()["version"], "2.1.330");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_probe_leaves_session_variables_out() {
        std::env::set_var("CLAUDE_CODE_SSE_PORT_TEST_ONLY", "1");
        let env = probe_env();
        assert!(!env.iter().any(|(k, _)| k == "CLAUDE_CODE_SSE_PORT_TEST_ONLY"));
    }
}
