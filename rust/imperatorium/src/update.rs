//! `imperatorium update [--check] [--channel stable|preview] [--from URL]`:
//! self-update from the project's releases (phase 6), SHA-256 verified.
//!
//! A release is a folder of assets: one binary per target triple
//! (`imperatorium-<triple>[.exe]`, `imperatorium-voice-<triple>.exe`),
//! `SHA256SUMS`, and `manifest.json`:
//!
//!   {"version": "0.3.1", "channel": "stable",
//!    "assets": {"<triple>": {"file": "...", "sha256": "..."},
//!               "voice-<triple>": {...}}}
//!
//! stable reads `.../releases/latest/download/`, preview the rolling
//! `.../releases/download/preview/`; `--from` (or IMPERATORIUM_RELEASES)
//! points at any folder served over HTTP, which is how it is tested.
//! The running binary is renamed aside, the new one put in its place, then
//! the dispatcher, Stream Deck and hotkey are restarted; Claude Code
//! sessions are not touched. The hosts' builds are refreshed too, so the
//! dispatcher's next push matches.

use crate::locks::LockExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

pub const RELEASES: &str = "https://github.com/hypertetrahedron/imperatorium/releases";

/// The version this binary reports: set by the release build from its tag,
/// else Cargo's.
pub fn version() -> &'static str {
    option_env!("IMPERATORIUM_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// This binary's target triple, as release assets are named.
pub fn this_triple() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => "unknown",
    }
}

/// (major, minor, patch, pre): a version without a pre-release part sorts
/// after one with it (0.3.0 > 0.3.0-preview.2 > 0.3.0-preview.1).
pub fn parse(v: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string())),
        None => (v, None),
    };
    let mut it = core.split('.').map(|x| x.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0), pre))
}

pub fn newer(candidate: &str, current: &str) -> bool {
    let (Some(a), Some(b)) = (parse(candidate), parse(current)) else {
        return false;
    };
    if (a.0, a.1, a.2) != (b.0, b.1, b.2) {
        return (a.0, a.1, a.2) > (b.0, b.1, b.2);
    }
    match (&a.3, &b.3) {
        (None, Some(_)) => true,
        (Some(_), None) | (None, None) => false,
        (Some(x), Some(y)) => {
            // Numeric parts compared as numbers: preview.10 > preview.9.
            let key = |s: &str| -> Vec<(u64, String)> {
                s.split('.').map(|p| (p.parse().unwrap_or(0), p.to_string())).collect()
            };
            key(x) > key(y)
        }
    }
}

fn base(from: Option<&str>, channel: &str) -> String {
    if let Some(f) = from.map(str::to_string).or_else(|| crate::paths::env_nonempty("IMPERATORIUM_RELEASES")) {
        return f.trim_end_matches('/').to_string();
    }
    if channel == "preview" {
        format!("{RELEASES}/download/preview")
    } else {
        format!("{RELEASES}/latest/download")
    }
}

fn get(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .redirects(8)
        .build();
    let resp = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let mut body = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(resp.into_reader(), limit), &mut body)
        .map_err(|e| format!("{url}: {e}"))?;
    Ok(body)
}

/// The release's manifest.
pub fn manifest(base: &str) -> Result<Value, String> {
    let body = get(&format!("{base}/manifest.json"), 1 << 20)?;
    serde_json::from_slice(&body).map_err(|e| format!("manifest.json: {e}"))
}

/// One asset, downloaded and checked against the manifest's SHA-256.
pub fn fetch(base: &str, m: &Value, key: &str) -> Result<Option<Vec<u8>>, String> {
    let Some(a) = m.get("assets").and_then(|x| x.get(key)) else {
        return Ok(None);
    };
    let file = a.get("file").and_then(Value::as_str).ok_or("an asset without a file name")?;
    let want = a.get("sha256").and_then(Value::as_str).ok_or("an asset without a sha256")?.to_lowercase();
    let body = get(&format!("{base}/{file}"), 512 << 20)?;
    let got = crate::remote::sha256_hex(&body);
    if got != want {
        return Err(format!("{file}: SHA-256 is {got}, the manifest says {want} - not installed"));
    }
    Ok(Some(body))
}

fn write_beside_and_swap(body: &[u8], dst: &Path) -> Result<(), String> {
    let tmp = dst.with_extension("download");
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&tmp, body).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let r = crate::install::place_binary(&tmp, dst);
    let _ = std::fs::remove_file(&tmp);
    r
}

/// Restart what runs from this binary, so the new one takes over.
fn restart_services(prefix: &str) {
    crate::service::Service::restart_running(prefix);
}

// -- the dispatcher's check and the window's Update button (gap J3) --------

/// Asked once a few minutes after the dispatcher starts, then once a
/// day, when `updates.check` is on (it is by default). Nothing is
/// installed until the window asks; then `imperatorium update` runs as a
/// child that outlives us, swaps the binary and restarts the services
/// (this dispatcher last).
pub struct Checker {
    state: std::sync::Mutex<Value>,
}


impl Default for Checker {
    fn default() -> Self {
        Checker { state: std::sync::Mutex::new(serde_json::json!({})) }
    }
}

fn channel_of(cfg: &Value) -> String {
    match crate::settings::value(cfg, "updates.channel").as_str() {
        Some("preview") => "preview".into(),
        _ => "stable".into(),
    }
}

fn epoch() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

impl Checker {
    /// What the window shows: this version, the newest one the last check
    /// saw, its notes, and whether an update is under way.
    pub fn status(&self, cfg: &Value) -> Value {
        let mut s = self.state.lk().clone();
        if let Some(o) = s.as_object_mut() {
            o.insert("current".into(), Value::from(version()));
            o.insert("channel".into(), Value::from(channel_of(cfg)));
            o.insert("check".into(), Value::Bool(crate::pyish::truthy(Some(&crate::settings::value(cfg, "updates.check")))));
            o.insert("ok".into(), Value::Bool(true));
        }
        s
    }

    /// Ask the release now; the answer replaces the last one.
    pub fn check(&self, cfg: &Value) -> Value {
        let channel = channel_of(cfg);
        let base = base(None, &channel);
        let mut next = serde_json::json!({"checked_at": epoch(), "checked_channel": channel});
        match manifest(&base) {
            Ok(m) => {
                let latest = m.get("version").and_then(Value::as_str).unwrap_or("").to_string();
                next["newer"] = Value::Bool(newer(&latest, version()));
                next["latest"] = Value::from(latest);
                next["notes"] = m.get("notes").cloned().unwrap_or(Value::Null);
                // A release without our build cannot be installed from here.
                next["installable"] = Value::Bool(m.get("assets").and_then(|a| a.get(this_triple())).is_some());
            }
            Err(e) => next["error"] = Value::from(e),
        }
        {
            let mut st = self.state.lk();
            if let Some(a) = st.get("applying").cloned() {
                next["applying"] = a;
            }
            *st = next;
        }
        self.status(cfg)
    }

    /// The background check. `cfg` is None once the dispatcher is gone.
    pub fn start(self: &std::sync::Arc<Self>, cfg: impl Fn() -> Option<Value> + Send + 'static) {
        let me = std::sync::Arc::clone(self);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs_f64(crate::timing::t().update_first_check));
            loop {
                let Some(c) = cfg() else { return };
                if crate::pyish::truthy(Some(&crate::settings::value(&c, "updates.check"))) {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| me.check(&c)));
                }
                std::thread::sleep(Duration::from_secs_f64(crate::timing::t().update_every));
            }
        });
    }

    /// Run `imperatorium update` for the version the last check found. Its
    /// output goes to `update.log` in the state folder.
    pub fn apply(&self, cfg: &Value, log: &Path) -> Value {
        let mut st = self.state.lk();
        if st.get("newer") != Some(&Value::Bool(true)) {
            return serde_json::json!({"ok": false, "error": "no newer version known - check first"});
        }
        if st.get("installable") == Some(&Value::Bool(false)) {
            return serde_json::json!({"ok": false, "error": format!("the release has no build for {}", this_triple())});
        }
        if st.get("applying").and_then(Value::as_f64).is_some_and(|t| epoch() - t < 600.0) {
            return serde_json::json!({"ok": false, "error": "an update is already under way"});
        }
        let latest = st.get("latest").cloned().unwrap_or(Value::Null);
        let channel = st.get("checked_channel").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| channel_of(cfg));
        let out = match std::fs::File::create(log) {
            Ok(f) => f,
            Err(e) => return serde_json::json!({"ok": false, "error": format!("{}: {e}", log.display())}),
        };
        let err = out.try_clone().ok();
        let mut c = std::process::Command::new(std::env::current_exe().unwrap_or_default());
        c.args(["update", "--channel", &channel]).stdin(std::process::Stdio::null()).stdout(out);
        if let Some(e) = err {
            c.stderr(e);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW:
            // it must outlive the dispatcher it is about to restart.
            c.creation_flags(0x0000_0008 | 0x0000_0200 | crate::proc::CREATE_NO_WINDOW);
        }
        match c.spawn() {
            Ok(child) => {
                st["applying"] = Value::from(epoch());
                serde_json::json!({"ok": true, "version": latest, "pid": child.id(), "log": log.to_string_lossy()})
            }
            Err(e) => serde_json::json!({"ok": false, "error": format!("cannot start the update: {e}")}),
        }
    }
}

pub fn main(args: &[String]) -> i32 {
    let mut check = false;
    let mut channel = "stable".to_string();
    let mut from = None;
    let mut prefix = "Imperatorium".to_string();
    let mut force = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" => check = true,
            "--channel" => channel = it.next().cloned().unwrap_or(channel),
            "--from" => from = it.next().cloned(),
            "--task-prefix" => prefix = it.next().cloned().unwrap_or(prefix),
            "--force" => force = true,
            _ => {}
        }
    }
    let base = base(from.as_deref(), &channel);
    let m = match manifest(&base) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("update: {e}");
            return 1;
        }
    };
    let latest = m.get("version").and_then(Value::as_str).unwrap_or("").to_string();
    let mine = version();
    let source = if from.is_some() || crate::paths::env_nonempty("IMPERATORIUM_RELEASES").is_some() {
        base.clone()
    } else {
        channel.clone()
    };
    if !force && !newer(&latest, mine) {
        println!("imperatorium {mine} is up to date ({source}: {latest})");
        return 0;
    }
    println!("imperatorium {mine} -> {latest} ({source})");
    if check {
        return 0;
    }
    let triple = this_triple();
    let exe = std::env::current_exe().unwrap_or_default();
    let body = match fetch(&base, &m, triple) {
        Ok(Some(b)) => b,
        Ok(None) => {
            eprintln!("update: the release has no build for {triple}");
            return 1;
        }
        Err(e) => {
            eprintln!("update: {e}");
            return 1;
        }
    };
    if let Err(e) = write_beside_and_swap(&body, &exe) {
        eprintln!("update: {e}");
        return 1;
    }
    println!("   {} ({} bytes, SHA-256 checked)", exe.display(), body.len());
    // The voice worker, when one is installed beside us.
    let vname = if cfg!(windows) { "imperatorium-voice.exe" } else { "imperatorium-voice" };
    let voice = exe.with_file_name(vname);
    if voice.is_file() {
        match fetch(&base, &m, &format!("voice-{triple}")) {
            Ok(Some(b)) => match write_beside_and_swap(&b, &voice) {
                Ok(()) => println!("   {} (SHA-256 checked)", voice.display()),
                Err(e) => eprintln!("   voice worker: {e}"),
            },
            Ok(None) => {}
            Err(e) => eprintln!("   voice worker: {e}"),
        }
    }
    // The hosts' builds, so the dispatcher pushes the matching version.
    let targets: PathBuf = exe.parent().map(|d| d.join("targets")).unwrap_or_default();
    for host_triple in ["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"] {
        if host_triple == triple || !targets.join(host_triple).exists() && !cfg!(windows) {
            continue;
        }
        match fetch(&base, &m, host_triple) {
            Ok(Some(b)) => {
                let dst = targets.join(host_triple).join("imperatorium");
                if write_beside_and_swap(&b, &dst).is_ok() {
                    println!("   {} (for remote hosts)", dst.display());
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("   {host_triple}: {e}"),
        }
    }
    restart_services(&prefix);
    println!("updated to {latest}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order() {
        assert!(newer("0.3.1", "0.3.0"));
        assert!(newer("0.3.0", "0.3.0-dev"));
        assert!(newer("0.3.0-preview.10", "0.3.0-preview.9"));
        assert!(!newer("0.3.0", "0.3.0"));
        assert!(!newer("0.2.9", "0.3.0-dev"));
        assert!(newer("v1.0", "0.9.9"));
        assert!(!newer("garbage", "0.3.0"));
    }

    #[test]
    fn checker_status_and_refusals() {
        let c = Checker::default();
        let cfg = serde_json::json!({});
        let s = c.status(&cfg);
        assert_eq!(s["current"], version());
        assert_eq!(s["channel"], "stable");
        assert_eq!(s["check"], true);
        assert!(s.get("checked_at").is_none());
        let cfg = serde_json::json!({"updates": {"channel": "preview", "check": false}});
        assert_eq!(c.status(&cfg)["channel"], "preview");
        assert_eq!(c.status(&cfg)["check"], false);
        // Nothing to install before a check has found something newer.
        let log = std::env::temp_dir().join("imperatorium-update-test.log");
        assert_eq!(c.apply(&cfg, &log)["ok"], false);
        *c.state.lk() = serde_json::json!({"newer": true, "latest": "9.9.9", "installable": false});
        assert!(c.apply(&cfg, &log)["error"].as_str().unwrap().contains("no build"));
        *c.state.lk() = serde_json::json!({"newer": true, "latest": "9.9.9", "applying": epoch()});
        assert!(c.apply(&cfg, &log)["error"].as_str().unwrap().contains("under way"));
    }

    #[test]
    fn triple_is_known() {
        assert_ne!(this_triple(), "unknown");
    }
}
