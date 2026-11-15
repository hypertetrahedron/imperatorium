//! `imperatorium install | uninstall | status`: setting a machine up from the
//! binary alone (phase 6) - what `setup_machine.py` and `hooks/install.py`
//! do, with no checkout and no Python.
//!
//! * **Settings.** `~/.claude/settings.json` gets the hooks, the mod and
//!   (without the mod) the statusline, exactly as `hooks/install.py
//!   --native` writes them: `apply_hooks`, `apply_statusline` and
//!   `apply_mod` are ports, checked against it byte for byte.
//! * **Config.** `config.json` and `policy.json` in the config home, from
//!   the examples embedded at build time, written only when absent.
//! * **The mod.** Its files, embedded too, written to the state folder.
//! * **Service.** Scheduled tasks on Windows (shaped as `service/tasks.ps1`
//!   shapes them), systemd user units on Linux, launchd agents on macOS.
//!
//! Every step says what it does; `--dry-run` says it and does nothing.
//! `--task-prefix`, `--port`, `--state` and `--settings` exist so a test can
//! install a second, separate copy without touching the real one.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::pyish::{json_dumps_indent, truthy};
use crate::service::Service;

const MARK: &str = "imperatorium";
const LEGACY_MARKS: &[&str] = &["ccontrol"];
const PLUGIN_DIRS: &str = "CLAUDE_CODE_PLUGIN_DIRS";
const MIN_CLAUDE: (u32, u32, u32) = if cfg!(windows) { (2, 1, 234) } else { (2, 1, 224) };
const MIN_CLAUDE_MOD: (u32, u32, u32) = (2, 1, 287);

const CONFIG_EXAMPLE: &str = include_str!("../../../config.example.json");
const POLICY_EXAMPLE: &str = include_str!("../../../policy.example.json");
const MOD_FILES: &[(&str, &str)] = &[
    (".claude-plugin/plugin.json", include_str!("../../../mod/imperatorium/.claude-plugin/plugin.json")),
    ("hooks/hooks.json", include_str!("../../../mod/imperatorium/hooks/hooks.json")),
    ("hooks/register.ts", include_str!("../../../mod/imperatorium/hooks/register.ts")),
    ("tsconfig.json", include_str!("../../../mod/imperatorium/tsconfig.json")),
];

/// The reporting hooks and the permission hook: (event, matcher, subcommand, async).
const EVENTS: &[(&str, Option<&str>, &str, bool)] = &[
    ("SessionStart", Some("startup|resume|clear"), "hook", true),
    ("SessionEnd", None, "hook", true),
    (
        "Notification",
        Some("permission_prompt|idle_prompt|agent_needs_input|elicitation_dialog|elicitation_url_dialog"),
        "hook",
        true,
    ),
    ("Stop", None, "hook", true),
    ("StopFailure", None, "hook", true),
    ("PermissionRequest", None, "permission", false),
];
const TOOL_EVENTS: &[(&str, Option<&str>, &str, bool)] = &[
    ("PreToolUse", None, "hook", true),
    ("PostToolUse", None, "hook", true),
    ("PostToolUseFailure", None, "hook", true),
];

// -- settings.json ----------------------------------------------------------

/// install.permission_timeout: outlast a parked decision, with slack.
pub fn permission_timeout(home: &Path) -> i64 {
    let park = std::fs::read_to_string(home.join("policy.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|p| p.get("park").cloned())
        .unwrap_or(json!({}));
    if !truthy(park.get("enabled")) {
        return 8;
    }
    let n = |k: &str| park.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    n("wait_seconds").max(n("background_wait_seconds")) as i64 + 20
}

fn entry(matcher: Option<&str>, sub: &str, is_async: bool, native: &str, home: &Path) -> Value {
    let args: Vec<Value> = if sub == "permission" {
        vec![json!("permission"), json!("--home"), json!(home.to_string_lossy())]
    } else {
        vec![json!(sub)]
    };
    let mut hook = Map::new();
    hook.insert("type".into(), json!("command"));
    hook.insert("command".into(), json!(native));
    hook.insert("args".into(), Value::Array(args));
    hook.insert("timeout".into(), json!(if is_async { 5 } else { permission_timeout(home) }));
    hook.insert("statusMessage".into(), json!(MARK));
    if is_async {
        hook.insert("async".into(), json!(true));
    }
    let mut block = Map::new();
    block.insert("hooks".into(), json!([Value::Object(hook)]));
    if let Some(m) = matcher {
        block.insert("matcher".into(), json!(m));
    }
    Value::Object(block)
}

/// install.is_ours: stamped by us (now or under the old name), or running
/// one of our hook scripts.
fn is_ours(block: &Value) -> bool {
    block
        .get("hooks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|h| {
            let mark = h.get("statusMessage").and_then(Value::as_str);
            if mark.is_some_and(|m| m == MARK || LEGACY_MARKS.contains(&m)) {
                return true;
            }
            h.get("args").and_then(Value::as_array).into_iter().flatten().any(|a| {
                let a = a.as_str().unwrap_or("").to_lowercase();
                a.ends_with("cc_hook.py") || a.ends_with("cc_permission.py")
            })
        })
}

/// install.apply: the hooks block. Returns the changes, as Python words them.
pub fn apply_hooks(settings: &mut Map<String, Value>, uninstall: bool, tool_hooks: bool, native: &str, home: &Path) -> Vec<String> {
    let mut changes = Vec::new();
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        return vec!["SKIP hooks: existing value is not an object".into()];
    };
    let events: Vec<_> = EVENTS
        .iter()
        .chain(if tool_hooks || uninstall { TOOL_EVENTS } else { &[] }.iter())
        .collect();
    for (event, matcher, sub, is_async) in events {
        let blocks = hooks.get(*event).cloned().unwrap_or(json!([]));
        let Some(blocks) = blocks.as_array() else {
            changes.push(format!("SKIP {event}: existing value is not a list"));
            continue;
        };
        let mut kept: Vec<Value> = blocks.iter().filter(|b| !is_ours(b)).cloned().collect();
        let removed = blocks.len() - kept.len();
        if uninstall {
            if removed > 0 {
                changes.push(format!("remove {event} ({removed} entry)"));
            }
            if kept.is_empty() {
                hooks.shift_remove(*event);
            } else {
                hooks.insert(event.to_string(), Value::Array(kept));
            }
            continue;
        }
        kept.push(entry(*matcher, sub, *is_async, native, home));
        hooks.insert(event.to_string(), Value::Array(kept));
        changes.push(if removed > 0 { format!("replace {event}") } else { format!("add {event}") });
    }
    if hooks.is_empty() {
        settings.shift_remove("hooks");
    }
    changes
}

fn is_our_statusline(v: Option<&Value>) -> bool {
    let Some(Value::Object(o)) = v else { return false };
    let cmd = o.get("command").map(crate::pyish::py_str).unwrap_or_default();
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r#"(?i)imperatorium(\.exe)?"?\s+statusline\b"#).unwrap());
    cmd.contains("cc_statusline.py") || re.is_match(&cmd)
}

/// install.apply_statusline.
pub fn apply_statusline(settings: &mut Map<String, Value>, uninstall: bool, native: &str) -> Vec<String> {
    let cur = settings.get("statusLine").cloned();
    let ours = is_our_statusline(cur.as_ref());
    if uninstall {
        if ours {
            settings.shift_remove("statusLine");
            return vec!["remove statusLine".into()];
        }
        return vec![];
    }
    if truthy(cur.as_ref()) && !ours {
        let shown = match &cur {
            Some(Value::Object(o)) => o.get("command").map(crate::pyish::py_str).unwrap_or_else(|| "None".into()),
            Some(v) => crate::pyish::py_str(v),
            None => String::new(),
        };
        return vec![format!(
            "SKIP statusLine: one is already configured and it is not ours ({})",
            crate::pyish::take(&shown, 80)
        )];
    }
    settings.insert(
        "statusLine".into(),
        json!({"type": "command", "command": format!("\"{native}\" statusline"), "padding": 0}),
    );
    vec![if truthy(cur.as_ref()) { "replace statusLine".into() } else { "add statusLine".into() }]
}

fn same_dir(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        let p = if let Some(rest) = p.strip_prefix('~') {
            format!("{}{rest}", home_dir().display())
        } else {
            p.to_string()
        };
        crate::paths::normpath(&p).to_lowercase()
    };
    if cfg!(windows) { norm(a) == norm(b) } else { crate::paths::normpath(a) == crate::paths::normpath(b) }
}

/// A folder holding the Imperatorium mod (its plugin.json says so), wherever
/// it is: a checkout's mod/imperatorium or the one an install wrote.
fn is_imperatorium_mod(dir: &str) -> bool {
    std::fs::read_to_string(Path::new(dir).join(".claude-plugin").join("plugin.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .is_some_and(|p| p.get("name").and_then(Value::as_str) == Some("imperatorium"))
}

/// install.apply_mod: our folder in CLAUDE_CODE_PLUGIN_DIRS, keeping the
/// folders anything else put there. Unlike Python it also drops another
/// copy of the Imperatorium mod (a checkout's), so a session never loads two.
pub fn apply_mod(settings: &mut Map<String, Value>, uninstall: bool, mod_dir: &str) -> Vec<String> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    if settings.get("env").is_some_and(|e| !e.is_object() && !e.is_null()) {
        return vec!["SKIP mod: settings `env` is not an object".into()];
    }
    let mut env = settings.get("env").and_then(Value::as_object).cloned().unwrap_or_default();
    let listed = env.get(PLUGIN_DIRS).map(crate::pyish::py_str).unwrap_or_default();
    let dirs: Vec<String> = listed.split(sep).filter(|d| !d.trim().is_empty()).map(str::to_string).collect();
    let ours = |d: &String| same_dir(d, mod_dir) || (!uninstall && is_imperatorium_mod(d));
    let mine: Vec<&String> = dirs.iter().filter(|d| same_dir(d, mod_dir)).collect();
    let others: Vec<String> = dirs.iter().filter(|d| !ours(d)).cloned().collect();
    if uninstall {
        if mine.is_empty() {
            return vec![];
        }
        if others.is_empty() {
            env.shift_remove(PLUGIN_DIRS);
        } else {
            env.insert(PLUGIN_DIRS.into(), json!(others.join(&sep.to_string())));
        }
        if env.is_empty() {
            settings.shift_remove("env");
        } else {
            settings.insert("env".into(), Value::Object(env));
        }
        return vec![format!("remove the mod from env.{PLUGIN_DIRS}")];
    }
    if !mine.is_empty() && others.len() + 1 == dirs.len() {
        return vec![];
    }
    let mut out = others;
    out.push(mod_dir.to_string());
    env.insert(PLUGIN_DIRS.into(), json!(out.join(&sep.to_string())));
    settings.insert("env".into(), Value::Object(env));
    vec![format!("add the mod to env.{PLUGIN_DIRS}: {mod_dir}")]
}

pub fn home_dir() -> PathBuf {
    crate::paths::user_home()
}

/// install.settings_path: CLAUDE_CONFIG_DIR, else ~/.claude.
pub fn settings_path() -> PathBuf {
    crate::paths::claude_home().join("settings.json")
}

fn load_settings(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
        Ok(t) => match serde_json::from_str::<Value>(&t) {
            Ok(Value::Object(o)) => Ok(o),
            Ok(_) => Ok(Map::new()),
            Err(e) => Err(format!(
                "{} is not valid JSON ({e}). Fix it by hand before installing.",
                path.display()
            )),
        },
    }
}

/// Backed up beside itself, then replaced whole, as install.py writes it.
fn write_settings(path: &Path, settings: &Map<String, Value>, say: &mut dyn FnMut(String)) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        let backup = format!("{}.imperatorium-{}.bak", path.display(), stamp());
        std::fs::copy(path, &backup).map_err(|e| e.to_string())?;
        say(format!("backup   : {backup}"));
    }
    let tmp = path.with_extension("json.imperatorium.tmp");
    let body = json_dumps_indent(&Value::Object(settings.clone()), 2) + "\n";
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// time.strftime("%Y%m%d-%H%M%S"), local time.
fn stamp() -> String {
    let s = crate::paths::local_stamp(); // "YYYY-MM-DD HH:MM:SS"
    s.replace('-', "").replace(':', "").replacen(' ', "-", 1)
}

// -- config -----------------------------------------------------------------

/// setup_machine.build_config.
pub fn build_config(roots: &[String], voice: bool, ntfy: Option<&str>) -> Value {
    let mut cfg: Value = serde_json::from_str(CONFIG_EXAMPLE).unwrap_or(json!({}));
    cfg["roots"] = json!(roots);
    if !cfg.get("voice").is_some_and(Value::is_object) {
        cfg["voice"] = json!({});
    }
    cfg["voice"]["enabled"] = json!(voice);
    if let Some(topic) = ntfy {
        if !cfg.get("notify").is_some_and(Value::is_object) {
            cfg["notify"] = json!({});
        }
        if !cfg["notify"].get("ntfy").is_some_and(Value::is_object) {
            cfg["notify"]["ntfy"] = json!({});
        }
        cfg["notify"]["ntfy"]["topic"] = json!(topic);
        cfg["notify"]["ntfy"]["enabled"] = json!(true);
    }
    cfg
}

/// setup_machine.build_policy.
pub fn build_policy(auto_approve: bool) -> Value {
    let mut pol: Value = serde_json::from_str(POLICY_EXAMPLE).unwrap_or(json!({}));
    pol["enabled"] = json!(auto_approve);
    pol
}

/// setup_machine.ntfy_topic: on ntfy.sh the topic name is the only secret.
fn ntfy_topic() -> String {
    format!("imperatorium-{}", crate::server::token_urlsafe(12))
}

/// setup_machine.default_roots: where repositories usually live.
fn default_roots() -> Vec<String> {
    let home = home_dir();
    let places: &[&[&str]] = &[
        &["src"], &["code"], &["projects"], &["repos"], &["dev"], &["github_src"],
        &["Desktop", "github_src"], &["source", "repos"],
    ];
    for parts in places {
        let p = parts.iter().fold(home.clone(), |p, part| p.join(part));
        if p.is_dir() {
            return vec![p.to_string_lossy().into_owned()];
        }
    }
    vec![home.to_string_lossy().into_owned()]
}

// -- versions ---------------------------------------------------------------

fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let re = regex::Regex::new(r"(\d+)\.(\d+)\.(\d+)").ok()?;
    let c = re.captures(text)?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?))
}

/// `claude`: on PATH, else where Claude Code's own installer puts it
/// (~/.local/bin), which a non-interactive shell's PATH often lacks.
pub fn claude_path() -> Option<PathBuf> {
    let name = if cfg!(windows) { "claude.exe" } else { "claude" };
    crate::launch::which("claude").map(PathBuf::from).or_else(|| {
        let p = home_dir().join(".local").join("bin").join(name);
        p.is_file().then_some(p)
    })
}

/// The PATH a service gets: this one, plus claude's folder when it is not
/// on it (a user service starts with a bare PATH, and the dispatcher calls
/// `claude`, `git` and `gh`).
pub(crate) fn service_path() -> String {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut path = std::env::var("PATH").unwrap_or_default();
    if crate::launch::which("claude").is_none() {
        if let Some(dir) = claude_path().and_then(|p| p.parent().map(Path::to_path_buf)) {
            path = format!("{}{sep}{path}", dir.display());
        }
    }
    path
}

fn claude_version() -> Option<(u32, u32, u32)> {
    let mut c = std::process::Command::new(claude_path()?);
    c.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().ok()?;
    parse_version(&String::from_utf8_lossy(&out.stdout))
}

fn v(t: (u32, u32, u32)) -> String {
    format!("{}.{}.{}", t.0, t.1, t.2)
}

// -- where things go --------------------------------------------------------

/// Where the binary is installed: %LOCALAPPDATA%\Programs\imperatorium on
/// Windows, ~/.local/bin elsewhere.
pub fn install_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(crate::paths::env_nonempty("LOCALAPPDATA").unwrap_or_default())
            .join("Programs")
            .join("imperatorium")
    } else {
        home_dir().join(".local").join("bin")
    }
}

pub fn exe_name() -> &'static str {
    if cfg!(windows) { "imperatorium.exe" } else { "imperatorium" }
}

// -- the commands -----------------------------------------------------------

struct Opts {
    yes: bool,
    dry: bool,
    home: PathBuf,
    settings: PathBuf,
    prefix: String,
    port: Option<String>,
    state: Option<String>,
    no_service: bool,
    no_copy: bool,
    roots: Vec<String>,
    flags: Map<String, Value>,
}

fn opts(args: &[String], home: Option<&str>) -> Opts {
    let mut o = Opts {
        yes: false,
        dry: false,
        home: crate::paths::config_home(home),
        settings: settings_path(),
        prefix: "Imperatorium".into(),
        port: None,
        state: None,
        no_service: false,
        no_copy: false,
        roots: vec![],
        flags: Map::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--yes" | "-y" => o.yes = true,
            "--dry-run" => o.dry = true,
            "--home" => {
                it.next();
            }
            "--settings" => o.settings = it.next().map(PathBuf::from).unwrap_or(o.settings),
            "--task-prefix" => o.prefix = it.next().cloned().unwrap_or(o.prefix),
            "--port" => o.port = it.next().cloned(),
            "--state" => o.state = it.next().cloned(),
            "--no-service" => o.no_service = true,
            "--no-copy" => o.no_copy = true,
            "--root" => o.roots.extend(it.next().cloned()),
            flag if flag.starts_with("--no-") => {
                o.flags.insert(flag[5..].to_string(), json!(false));
            }
            flag if flag.starts_with("--") => {
                o.flags.insert(flag[2..].to_string(), json!(true));
            }
            _ => {}
        }
    }
    o
}

/// A yes/no choice: a flag decides it; else asked at a terminal; else (no
/// terminal, or --yes) the default.
fn choose(o: &Opts, key: &str, question: &str, default: bool) -> bool {
    if let Some(v) = o.flags.get(key) {
        return v.as_bool().unwrap_or(default);
    }
    if o.yes || !std::io::stdin().is_terminal() {
        return default;
    }
    print!("   {question} [{}] ", if default { "Y/n" } else { "y/N" });
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    match line.trim().to_lowercase().as_str() {
        "" => default,
        a => a.starts_with('y'),
    }
}

fn wait_for_dispatcher(port: &str, seconds: u64) -> Option<Value> {
    let url = format!("http://127.0.0.1:{port}/api/health");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    loop {
        if let Ok(r) = ureq::get(&url).timeout(std::time::Duration::from_secs(2)).call() {
            if let Some(v) = r.into_string().ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                return Some(v);
            }
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// The value after `--home` in a command line, quoted or not.
pub(crate) fn home_arg(cmdline: &str) -> Option<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut any) = (false, false);
    for ch in cmdline.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    words.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        words.push(cur);
    }
    let i = words.iter().position(|w| w == "--home")?;
    words.get(i + 1).filter(|w| !w.is_empty()).cloned()
}

fn service_for(o: &Opts, exe: PathBuf) -> Service {
    let mut serve_args = Vec::new();
    if let Some(p) = &o.port {
        serve_args.extend(["--port".to_string(), p.clone()]);
    }
    if let Some(s) = &o.state {
        serve_args.extend(["--state".to_string(), s.clone()]);
    }
    Service::new(o.prefix.clone(), exe, o.home.clone(), serve_args)
}

/// A step that failed and has said why; `install_main` ends with 1.
struct Failed;

type Say<'a> = &'a mut dyn FnMut(String);

/// The answers of step 2.
struct Choices {
    roots: Vec<String>,
    voice: bool,
    deck: bool,
    auto: bool,
    use_mod: bool,
    statusline: bool,
    tool_hooks: bool,
    push: bool,
    hotkey: bool,
    service: bool,
}

fn heading(s: &str) {
    println!("\n== {s}");
}

/// Step 1: Claude Code is there and new enough; git and gh are noted.
fn check_prerequisites(say: Say) -> Result<Option<(u32, u32, u32)>, Failed> {
    let ver = claude_version();
    match ver {
        Some(v_) if v_ >= MIN_CLAUDE => say(format!("   ok       Claude Code {}", v(v_))),
        Some(v_) => {
            say(format!("   MISSING  Claude Code {}+ (this machine has {})", v(MIN_CLAUDE), v(v_)));
            return Err(Failed);
        }
        None => {
            say("   MISSING  Claude Code (`claude` is not on PATH) - https://claude.com/claude-code".into());
            return Err(Failed);
        }
    }
    for (tool, why) in [("git", "worktree actions and PR badges"), ("gh", "PR badges and 'open PR'")] {
        let found = crate::launch::which(tool);
        say(format!("   {:<8} {tool:<12} {}", if found.is_some() { "ok" } else { "--" },
                    found.unwrap_or_else(|| format!("optional - {why}"))));
    }
    Ok(ver)
}

/// Step 2: what to set up (flags, then questions, then defaults).
fn make_choices(o: &Opts, ver: Option<(u32, u32, u32)>, say: Say) -> Choices {
    let roots = if o.roots.is_empty() { default_roots() } else { o.roots.clone() };
    say(format!("   projects under: {}", roots.join(", ")));
    let mod_ok = ver.is_some_and(|x| x >= MIN_CLAUDE_MOD);
    let voice = choose(o, "voice", "Local dictation (F9)? Fetches a ~1.6 GB model; a GPU helps", false);
    let deck = choose(o, "streamdeck", "Elgato Stream Deck surface?", false);
    let auto = choose(o, "auto-approve", "Auto-approve cheap permission prompts (reads, git status, tests)?", false);
    let use_mod = mod_ok && choose(o, "mod", "The Imperatorium mod (prompts arrive as your own words; live status)?", true);
    let statusline = choose(o, "statusline", "Statusline (usage meter)?", !use_mod);
    let tool_hooks = choose(o, "tool-hooks", "Tool hooks (live, timed activity)?", true);
    let push = choose(o, "ntfy", "Phone push when a session has waited on you (ntfy.sh)?", false);
    let hotkey = cfg!(windows) && choose(o, "hotkey", "Global hotkey to bring the window up?", true);
    let service = !o.no_service && choose(o, "service", "Start Imperatorium at logon and keep it running?", true);
    Choices { roots, voice, deck, auto, use_mod, statusline, tool_hooks, push, hotkey, service }
}

/// Step 3: the binary the hooks and services will run (copied into place
/// unless it is there already or --no-copy).
fn install_binary(o: &Opts, say: Say) -> Result<PathBuf, Failed> {
    let here = std::env::current_exe().unwrap_or_default();
    let target = install_dir().join(exe_name());
    if o.no_copy || same_dir(&here.to_string_lossy(), &target.to_string_lossy()) {
        say(format!("   {}", here.display()));
        return Ok(here);
    }
    say(format!("   copy {} -> {}", here.display(), target.display()));
    if !o.dry {
        if let Err(e) = place_binary(&here, &target) {
            say(format!("   could not: {e}"));
            return Err(Failed);
        }
        // The voice worker travels with it when it sits beside us.
        let vname = if cfg!(windows) { "imperatorium-voice.exe" } else { "imperatorium-voice" };
        let vsrc = here.with_file_name(vname);
        if vsrc.is_file() {
            let _ = place_binary(&vsrc, &target.with_file_name(vname));
        }
    }
    Ok(target)
}

/// Step 4: config.json and policy.json, each only when absent.
fn write_config(o: &Opts, c: &Choices, say: Say) -> Result<(), Failed> {
    say(format!("   config home: {}", o.home.display()));
    let topic = c.push.then(ntfy_topic);
    for (name, body) in [
        ("config.json", build_config(&c.roots, c.voice, topic.as_deref())),
        ("policy.json", build_policy(c.auto)),
    ] {
        let path = o.home.join(name);
        if path.exists() {
            say(format!("   {name} exists - left as it is"));
            continue;
        }
        say(format!("   write {}", path.display()));
        if !o.dry {
            let _ = std::fs::create_dir_all(&o.home);
            if let Err(e) = std::fs::write(&path, json_dumps_indent(&body, 2) + "\n") {
                say(format!("   could not: {e}"));
                return Err(Failed);
            }
        }
    }
    if let Some(t) = &topic {
        say(format!("   phone push: subscribe to {t} in the ntfy app (treat the name like a password)"));
    }
    Ok(())
}

/// Step 5: the mod's files, then settings.json's hooks, statusline and mod.
fn install_claude_hooks(o: &Opts, c: &Choices, native: &str, say: Say) -> Result<(), Failed> {
    let mod_dir = crate::paths::state_dir().join("mod").join("imperatorium");
    if c.use_mod {
        say(format!("   the mod: {}", mod_dir.display()));
        if !o.dry {
            for (rel, body) in MOD_FILES {
                let p = mod_dir.join(rel);
                let _ = std::fs::create_dir_all(p.parent().unwrap());
                if let Err(e) = std::fs::write(&p, body) {
                    say(format!("   could not write {}: {e}", p.display()));
                    return Err(Failed);
                }
            }
        }
    }
    let mut settings = match load_settings(&o.settings) {
        Ok(s) => s,
        Err(e) => {
            say(format!("   {e}"));
            return Err(Failed);
        }
    };
    let mut changes = apply_hooks(&mut settings, false, c.tool_hooks, native, &o.home);
    if c.statusline {
        changes.extend(apply_statusline(&mut settings, false, native));
    }
    if c.use_mod {
        changes.extend(apply_mod(&mut settings, false, &mod_dir.to_string_lossy()));
    }
    say(format!("   settings: {}", o.settings.display()));
    for ch in &changes {
        say(format!("     {ch}"));
    }
    if !o.dry {
        if let Err(e) = write_settings(&o.settings, &settings, say) {
            say(format!("   could not write settings: {e}"));
            return Err(Failed);
        }
    }
    Ok(())
}

/// Step 6: register and start the service, or say how to start by hand.
fn install_service(o: &Opts, c: &Choices, exe: &Path, say: Say) -> Result<(), Failed> {
    if c.service {
        let svc = service_for(o, exe.to_path_buf());
        if let Err(e) = svc.install(c.deck, c.hotkey, o.dry, say) {
            say(format!("   {e}"));
            return Err(Failed);
        }
    } else {
        say(format!(
            "   skipped - start it with:  \"{}\" serve --live --home \"{}\"",
            exe.display(),
            o.home.display()
        ));
    }
    Ok(())
}

/// Step 7: wait for the dispatcher and say what is left to do.
fn final_check(o: &Opts, c: &Choices, say: Say) {
    let port = o.port.clone().unwrap_or_else(|| "8792".into());
    if o.dry {
        say("   dry run: nothing was changed".into());
        return;
    }
    match wait_for_dispatcher(&port, if c.service { 30 } else { 2 }) {
        Some(h) => {
            say(format!("   dispatcher is up (pid {})", h.get("pid").cloned().unwrap_or(Value::Null)));
            say(format!("\nDone. Open the window: http://127.0.0.1:{port}/"));
        }
        None => say(format!("   dispatcher is not answering yet on 127.0.0.1:{port}")),
    }
    if c.voice {
        say("Dictation: `imperatorium voice fetch` downloads the worker and the model (~1.6 GB), SHA-256 checked.".into());
    }
    say("Restart running Claude Code sessions so they pick up the hooks.".into());
}

/// `imperatorium install`.
pub fn install_main(args: &[String], home: Option<&str>) -> i32 {
    let o = opts(args, home);
    match install_steps(&o) {
        Ok(()) => 0,
        Err(Failed) => 1,
    }
}

fn install_steps(o: &Opts) -> Result<(), Failed> {
    let mut say = |s: String| println!("{s}");
    println!("Imperatorium {} setup{}", crate::update::version(), if o.dry { " (dry run: nothing is changed)" } else { "" });

    heading("1/7 Prerequisites");
    let ver = check_prerequisites(&mut say)?;
    heading("2/7 Choices");
    let choices = make_choices(o, ver, &mut say);
    heading("3/7 The binary");
    let exe = install_binary(o, &mut say)?;
    let native = exe.to_string_lossy().into_owned();
    heading("4/7 Settings");
    write_config(o, &choices, &mut say)?;
    heading("5/7 Claude Code hooks");
    install_claude_hooks(o, &choices, &native, &mut say)?;
    heading("6/7 Keep it running");
    install_service(o, &choices, &exe, &mut say)?;
    heading("7/7 Check");
    final_check(o, &choices, &mut say);
    Ok(())
}

/// Copy beside, then rename into place: a running copy (a hook holds it)
/// is renamed out of the way first, which Windows allows.
pub fn place_binary(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = dst.with_extension("new");
    std::fs::copy(src, &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    if dst.exists() {
        let old = dst.with_extension(format!("old-{}", stamp()));
        std::fs::rename(dst, &old).map_err(|e| format!("{}: {e}", dst.display()))?;
    }
    std::fs::rename(&tmp, dst).map_err(|e| format!("{}: {e}", dst.display()))
}

/// `imperatorium uninstall`: hooks, mod, statusline and service; config,
/// policy and state are left.
pub fn uninstall_main(args: &[String], home: Option<&str>) -> i32 {
    let o = opts(args, home);
    let mut say = |s: String| println!("{s}");
    println!("== Claude Code hooks");
    let mut settings = match load_settings(&o.settings) {
        Ok(s) => s,
        Err(e) => {
            println!("   {e}");
            return 1;
        }
    };
    let exe = std::env::current_exe().unwrap_or_default();
    let mod_dir = crate::paths::state_dir().join("mod").join("imperatorium");
    let mut changes = apply_hooks(&mut settings, true, true, "", &o.home);
    changes.extend(apply_statusline(&mut settings, true, ""));
    changes.extend(apply_mod(&mut settings, true, &mod_dir.to_string_lossy()));
    for c in changes.iter().chain(changes.is_empty().then(|| "(nothing to do)".to_string()).iter()) {
        say(format!("   {c}"));
    }
    if !o.dry && !changes.is_empty() {
        if let Err(e) = write_settings(&o.settings, &settings, &mut say) {
            say(format!("   could not write settings: {e}"));
            return 1;
        }
    }
    println!("== Service");
    service_for(&o, exe).uninstall(o.dry, &mut say);
    println!(
        "\nRemoved. config.json and policy.json ({}) and your state ({}) were left in place.",
        o.home.display(),
        crate::paths::state_dir().display()
    );
    0
}

/// `imperatorium install-hooks --settings F --native BIN [--home H]
/// [--tool-hooks] [--statusline] [--mod DIR] [--uninstall]`: only the
/// settings.json edit, with `hooks/install.py`'s flags, so conformance can
/// compare the two files byte for byte. No backup is made.
pub fn install_hooks_main(args: &[String]) -> i32 {
    let mut settings_file = settings_path();
    let (mut native, mut home) = (String::new(), PathBuf::from("."));
    let (mut tool, mut line, mut uninstall, mut mod_dir) = (false, false, false, None);
    let mut reporting_only = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--settings" => settings_file = it.next().map(PathBuf::from).unwrap_or(settings_file),
            "--native" => native = it.next().cloned().unwrap_or_default(),
            "--home" => home = it.next().map(PathBuf::from).unwrap_or(home),
            "--tool-hooks" => tool = true,
            "--statusline" => line = true,
            "--uninstall" => uninstall = true,
            // A remote host or container: its permission prompts stay there.
            "--reporting-only" => reporting_only = true,
            "--mod" => mod_dir = it.next().cloned(),
            _ => {}
        }
    }
    let mut s = match load_settings(&settings_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let mut changes = apply_hooks(&mut s, uninstall, tool, &native, &home);
    if reporting_only && !uninstall {
        if let Some(hooks) = s.get_mut("hooks").and_then(Value::as_object_mut) {
            let kept: Vec<Value> = hooks
                .get("PermissionRequest")
                .and_then(Value::as_array)
                .map(|b| b.iter().filter(|x| !is_ours(x)).cloned().collect())
                .unwrap_or_default();
            if kept.is_empty() {
                hooks.shift_remove("PermissionRequest");
            } else {
                hooks.insert("PermissionRequest".into(), Value::Array(kept));
            }
            changes.retain(|c| !c.ends_with(" PermissionRequest"));
        }
    }
    if line || uninstall {
        changes.extend(apply_statusline(&mut s, uninstall, &native));
    }
    if let Some(m) = mod_dir {
        changes.extend(apply_mod(&mut s, uninstall, &m));
    }
    for c in &changes {
        println!("  {c}");
    }
    let body = json_dumps_indent(&Value::Object(s), 2) + "\n";
    match std::fs::write(&settings_file, body) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// `imperatorium status`.
pub fn status_main(args: &[String], home: Option<&str>) -> i32 {
    let mut o = opts(args, home);
    println!("imperatorium {}  {}", crate::update::version(), std::env::current_exe().unwrap_or_default().display());
    // Without --home (or IMPERATORIUM_HOME), the home that matters is the
    // one the installed dispatcher was given, not the platform default.
    let chosen = home.is_some_and(|h| !h.is_empty()) || crate::paths::env_nonempty("IMPERATORIUM_HOME").is_some();
    let installed = if chosen {
        None
    } else {
        service_for(&o, std::env::current_exe().unwrap_or_default()).installed_home()
    };
    match installed {
        Some(h) => {
            o.home = h;
            println!("config home   {}  (the installed dispatcher's)", o.home.display());
        }
        None => println!("config home   {}", o.home.display()),
    }
    println!("state         {}", crate::paths::state_dir().display());
    let settings = load_settings(&o.settings).unwrap_or_default();
    let hooks = settings.get("hooks").and_then(Value::as_object).map(|h| {
        h.values().flat_map(|b| b.as_array().cloned().unwrap_or_default()).filter(is_ours).count()
    });
    println!("hooks         {} of ours in {}", hooks.unwrap_or(0), o.settings.display());
    let dirs = settings
        .get("env")
        .and_then(|e| e.get(PLUGIN_DIRS))
        .map(crate::pyish::py_str)
        .unwrap_or_default();
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mods: Vec<&str> = dirs.split(sep).filter(|d| is_imperatorium_mod(d)).collect();
    println!("mod           {}", if mods.is_empty() { "not loaded".to_string() } else { mods.join(", ") });
    for (name, state) in service_for(&o, std::env::current_exe().unwrap_or_default()).status() {
        println!("{name:<26} {state}");
    }
    let port = o.port.clone().unwrap_or_else(|| "8792".into());
    match wait_for_dispatcher(&port, 2) {
        Some(h) => println!("dispatcher    responding on 127.0.0.1:{port}, pid {}", h.get("pid").cloned().unwrap_or(Value::Null)),
        None => println!("dispatcher    NOT responding on 127.0.0.1:{port}"),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_from_a_command_line() {
        assert_eq!(
            home_arg(r#"--headless "C:\P\imperatorium.exe" serve --live --home "C:\Users\A B\imp""#).as_deref(),
            Some(r"C:\Users\A B\imp")
        );
        assert_eq!(home_arg(r#""/u/bin/imperatorium" "serve" "--live" "--home" "/home/a/imp""#).as_deref(), Some("/home/a/imp"));
        assert_eq!(home_arg("imperatorium serve --home /x/y --port 1").as_deref(), Some("/x/y"));
        assert_eq!(home_arg("imperatorium serve --live"), None);
        assert_eq!(home_arg("imperatorium serve --home"), None);
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("2.1.323 (Claude Code)"), Some((2, 1, 323)));
        assert!(Some((2, 1, 300)) >= Some(MIN_CLAUDE_MOD));
        assert_eq!(parse_version("nope"), None);
    }

    #[test]
    fn hooks_replace_ours_and_keep_others() {
        let mut s: Map<String, Value> = serde_json::from_value(json!({"hooks": {"Stop": [
            {"hooks": [{"type": "command", "command": "python", "args": ["x/cc_hook.py"]}]},
            {"hooks": [{"type": "command", "command": "someone-else"}]}
        ]}})).unwrap();
        let ch = apply_hooks(&mut s, false, false, "C:/bin/imperatorium.exe", Path::new("C:/h"));
        assert!(ch.contains(&"replace Stop".to_string()));
        let stop = s["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "someone-else");
        assert_eq!(stop[1]["hooks"][0]["args"], json!(["hook"]));
        assert_eq!(s["hooks"]["PermissionRequest"][0]["hooks"][0]["args"], json!(["permission", "--home", "C:/h"]));
        let ch = apply_hooks(&mut s, true, false, "", Path::new("C:/h"));
        assert!(ch.iter().any(|c| c == "remove Stop (1 entry)"));
        assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn statusline_refuses_a_stranger() {
        let mut s: Map<String, Value> = serde_json::from_value(json!({"statusLine": {"command": "my-line"}})).unwrap();
        assert!(apply_statusline(&mut s, false, "x")[0].starts_with("SKIP"));
        let mut s = Map::new();
        apply_statusline(&mut s, false, "C:/b/imperatorium.exe");
        assert!(is_our_statusline(s.get("statusLine")));
    }

    #[test]
    fn mod_dirs_are_kept() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let mut s: Map<String, Value> =
            serde_json::from_value(json!({"env": {PLUGIN_DIRS: format!("/a{sep}/b"), "X": "1"}})).unwrap();
        apply_mod(&mut s, false, "/m");
        assert_eq!(s["env"][PLUGIN_DIRS], format!("/a{sep}/b{sep}/m"));
        assert!(apply_mod(&mut s, false, "/m").is_empty());
        apply_mod(&mut s, true, "/m");
        assert_eq!(s["env"][PLUGIN_DIRS], format!("/a{sep}/b"));
    }
}
