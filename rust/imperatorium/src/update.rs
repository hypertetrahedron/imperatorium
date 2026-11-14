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
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().unwrap_or_default();
        for (task, sub) in [("Dispatcher", "serve"), ("StreamDeck", "streamdeck"), ("Hotkey", "hotkey")] {
            let task = format!("{prefix}{task}");
            let q = std::process::Command::new("schtasks").args(["/Query", "/TN", &task]).output();
            if !q.is_ok_and(|o| o.status.success()) {
                continue;
            }
            let _ = std::process::Command::new("schtasks").args(["/End", "/TN", &task]).output();
            let filter = format!("Name='{}'", exe.file_name().and_then(|n| n.to_str()).unwrap_or("imperatorium.exe"));
            // Only processes started from this install's exe, by the path in
            // their command line (the task put it there): another install's
            // dispatcher - the live one, beside a test copy - is not ours to
            // stop. Not by image path: after the swap that names the old,
            // renamed file.
            let path = regex::escape(&exe.to_string_lossy()).replace('\'', "''");
            let script = format!(
                "Get-CimInstance Win32_Process -Filter \"{filter}\" | Where-Object {{ $_.CommandLine -match '{path}' -and $_.CommandLine -match ' {sub}( |$)' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}"
            );
            let _ = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &script]).output();
            let _ = std::process::Command::new("schtasks").args(["/Run", "/TN", &task]).output();
            println!("   restarted {task}");
        }
    }
    #[cfg(target_os = "linux")]
    for name in ["dispatcher", "streamdeck", "hotkey"] {
        let unit = format!("{}-{name}.service", prefix.to_lowercase());
        let ok = std::process::Command::new("systemctl").args(["--user", "is-enabled", &unit]).output();
        if ok.is_ok_and(|o| o.status.success()) {
            let _ = std::process::Command::new("systemctl").args(["--user", "restart", &unit]).output();
            println!("   restarted {unit}");
        }
    }
    #[cfg(target_os = "macos")]
    for name in ["dispatcher", "streamdeck", "hotkey"] {
        let label = format!("local.{}.{name}", prefix.to_lowercase());
        let uid = unsafe { libc::getuid() };
        let _ = std::process::Command::new("launchctl").args(["kickstart", "-k", &format!("gui/{uid}/{label}")]).output();
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
    fn triple_is_known() {
        assert_ne!(this_triple(), "unknown");
    }
}
