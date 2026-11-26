//! `imperatorium update [--check] [--channel stable|preview] [--version V]
//! [--from URL] [--rollback]`: self-update from the project's releases
//! (phase 6). The manifest's signature is checked (signing.rs), then each
//! download's SHA-256 against it. `--version` installs one release, newer
//! or older; the binary an update replaces is kept beside it
//! (`imperatorium.previous[.exe]`) for `--rollback`.
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

/// Where a release is read from: `--from` (or IMPERATORIUM_RELEASES), else
/// one version's release (`--version`), else the channel's.
fn base(from: Option<&str>, channel: &str, pinned: Option<&str>) -> String {
    if let Some(f) = from.map(str::to_string).or_else(|| crate::paths::env_nonempty("IMPERATORIUM_RELEASES")) {
        return f.trim_end_matches('/').to_string();
    }
    if let Some(v) = pinned {
        return format!("{RELEASES}/download/v{}", v.trim().trim_start_matches('v'));
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

/// The release's manifest, once its signature is checked (signing.rs): an
/// unsigned or wrongly signed manifest is an error, so nothing it lists is
/// downloaded.
pub fn manifest(base: &str) -> Result<Value, String> {
    let body = get(&format!("{base}/manifest.json"), 1 << 20)?;
    let sig = get(&format!("{base}/manifest.json.sig"), 4 << 10)
        .map_err(|e| format!("the release has no signature ({e}) - nothing installed"))?;
    crate::signing::verify(&body, &String::from_utf8_lossy(&sig))?;
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

/// Where the binary an update replaced is kept: `imperatorium.previous.exe`
/// (`imperatorium.previous` elsewhere), beside it.
pub fn previous_of(exe: &Path) -> PathBuf {
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "imperatorium".into());
    let ext = exe.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    exe.with_file_name(format!("{stem}.previous{ext}"))
}

/// The renamed-aside copies a swap leaves (`imperatorium.old-<stamp>`):
/// removed when nothing runs them any more; one still running stays until
/// the next update. The `.previous` copy is what --rollback uses instead.
fn prune_old(exe: &Path) {
    let (Some(dir), Some(stem)) = (exe.parent(), exe.file_stem()) else { return };
    let prefix = format!("{}.old-", stem.to_string_lossy());
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// `imperatorium update --rollback`: put back the binary the last update
/// replaced, and keep this one in its place, so a second --rollback undoes
/// the first. The services are restarted onto it. An older binary that
/// finds state written by a newer one refuses to start and says which
/// version to install (statever.rs).
fn rollback(prefix: &str) -> i32 {
    let exe = std::env::current_exe().unwrap_or_default();
    let prev = previous_of(&exe);
    if !prev.is_file() {
        eprintln!("update: no {} - nothing to roll back to", prev.display());
        return 1;
    }
    let was = std::process::Command::new(&prev)
        .arg("version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_start_matches("imperatorium ").to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "the previous version".into());
    let hold = exe.with_file_name(format!("{}.rollback", prev.file_name().unwrap_or_default().to_string_lossy()));
    let swapped = std::fs::copy(&exe, &hold)
        .map_err(|e| format!("{}: {e}", hold.display()))
        .and_then(|_| crate::install::place_binary(&prev, &exe))
        .and_then(|_| std::fs::rename(&hold, &prev).map_err(|e| format!("{}: {e}", prev.display())));
    if let Err(e) = swapped {
        let _ = std::fs::remove_file(&hold);
        eprintln!("update: {e}");
        return 1;
    }
    prune_old(&exe);
    println!("rolled back: imperatorium {} -> {was} ({})", version(), exe.display());
    restart_services(prefix);
    0
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
        let base = base(None, &channel, None);
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
    let mut pinned: Option<String> = None;
    let mut roll = false;
    let mut it = args.iter();
    // A flag it does not know, or one missing its value, is an error: a
    // trailing `--from` once fell back to GitHub's channel without a word.
    while let Some(a) = it.next() {
        let mut value = |name: &str| match it.next() {
            Some(v) if !v.starts_with("--") => Ok(v.clone()),
            _ => Err(format!("update: {name} needs a value")),
        };
        let got = match a.as_str() {
            "--check" => {
                check = true;
                Ok(())
            }
            "--channel" => value("--channel").map(|v| channel = v),
            "--from" => value("--from").map(|v| from = Some(v)),
            "--task-prefix" => value("--task-prefix").map(|v| prefix = v),
            "--force" => {
                force = true;
                Ok(())
            }
            "--version" => value("--version").map(|v| pinned = Some(v.trim().trim_start_matches('v').to_string())),
            "--rollback" => {
                roll = true;
                Ok(())
            }
            other => Err(format!("update: unknown option {other:?} (see `imperatorium help`)")),
        };
        if let Err(e) = got {
            eprintln!("{e}");
            return 2;
        }
    }
    if roll {
        return rollback(&prefix);
    }
    if pinned.as_deref().is_some_and(|v| parse(v).is_none()) {
        eprintln!("update: --version needs a version, like 0.4.0 or 0.4.0-preview.2");
        return 1;
    }
    let base = base(from.as_deref(), &channel, pinned.as_deref());
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
    } else if let Some(v) = &pinned {
        format!("v{v}")
    } else {
        channel.clone()
    };
    // --version installs that release whether it is newer or older; it is
    // only "up to date" when it is this one.
    let wanted = match &pinned {
        Some(v) => {
            if parse(&latest) != parse(v) {
                eprintln!("update: the release at {base} says it is {latest}, not {v} - nothing installed");
                return 1;
            }
            latest != mine
        }
        None => newer(&latest, mine),
    };
    if !force && !wanted {
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
    // Everything this release replaces is fetched and checked before any of
    // it is swapped in: a voice or host build that fails its SHA-256 (or
    // cannot be fetched) leaves the whole install as it was, not a new
    // dispatcher beside an old worker.
    let vname = if cfg!(windows) { "imperatorium-voice.exe" } else { "imperatorium-voice" };
    let voice = exe.with_file_name(vname);
    let mut extra: Vec<(PathBuf, Vec<u8>, &str)> = Vec::new();
    if voice.is_file() {
        match fetch(&base, &m, &format!("voice-{triple}")) {
            Ok(Some(b)) => extra.push((voice.clone(), b, "SHA-256 checked")),
            Ok(None) => {}
            Err(e) => {
                eprintln!("update: voice worker: {e} - nothing installed");
                return 1;
            }
        }
    }
    // The hosts' builds, so the dispatcher pushes the matching version.
    let targets: PathBuf = exe.parent().map(|d| d.join("targets")).unwrap_or_default();
    for host_triple in ["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"] {
        if host_triple == triple || !targets.join(host_triple).exists() && !cfg!(windows) {
            continue;
        }
        match fetch(&base, &m, host_triple) {
            Ok(Some(b)) => extra.push((targets.join(host_triple).join("imperatorium"), b, "for remote hosts")),
            Ok(None) => {}
            Err(e) => {
                eprintln!("update: {host_triple}: {e} - nothing installed");
                return 1;
            }
        }
    }
    // The binary being replaced is kept for `--rollback` (a running exe can
    // be copied, just not overwritten).
    if exe.is_file() {
        if let Err(e) = std::fs::copy(&exe, previous_of(&exe)) {
            eprintln!("   (the current binary was not kept for --rollback: {e})");
        }
    }
    if let Err(e) = write_beside_and_swap(&body, &exe) {
        eprintln!("update: {e}");
        return 1;
    }
    prune_old(&exe);
    println!("   {} ({} bytes, SHA-256 checked)", exe.display(), body.len());
    for (dst, b, note) in &extra {
        match write_beside_and_swap(b, dst) {
            Ok(()) => println!("   {} ({note})", dst.display()),
            Err(e) => eprintln!("   {}: {e}", dst.display()),
        }
    }
    restart_services(&prefix);
    println!("updated to {latest}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Refused before anything is fetched: no network in this test.
    #[test]
    fn unknown_options_and_missing_values_are_errors() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(main(&args(&["--bogus"])), 2);
        assert_eq!(main(&args(&["--check", "--from"])), 2);
        assert_eq!(main(&args(&["--from", "--check"])), 2);
        assert_eq!(main(&args(&["--version"])), 2);
    }

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

    #[test]
    fn dev_builds_sort_below_previews() {
        // main carries X.Y.Z-dev (docs/releasing.md); a dev build is never
        // offered as newer than the preview it leads to, and vice versa.
        assert!(newer("0.4.0-preview.1", "0.4.0-dev"));
        assert!(!newer("0.4.0-dev", "0.4.0-preview.1"));
        assert!(newer("0.4.0", "0.4.0-preview.9"));
    }

    #[test]
    fn where_a_release_is_read() {
        std::env::remove_var("IMPERATORIUM_RELEASES");
        assert_eq!(base(None, "stable", None), format!("{RELEASES}/latest/download"));
        assert_eq!(base(None, "preview", None), format!("{RELEASES}/download/preview"));
        assert_eq!(base(None, "preview", Some("v0.4.0")), format!("{RELEASES}/download/v0.4.0"));
        assert_eq!(base(Some("http://127.0.0.1:9/r/"), "stable", Some("0.4.0")), "http://127.0.0.1:9/r");
    }

    #[test]
    fn previous_and_pruning() {
        let d = std::env::temp_dir().join(format!("imp-update-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let exe = d.join(if cfg!(windows) { "imperatorium.exe" } else { "imperatorium" });
        let prev = previous_of(&exe);
        assert_eq!(prev.file_name().unwrap(), if cfg!(windows) { "imperatorium.previous.exe" } else { "imperatorium.previous" });
        for f in ["imperatorium.old-20261115120000", "imperatorium.old-20261116120000", "imperatorium-voice.old-1", "other.txt"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        std::fs::write(&exe, b"x").unwrap();
        std::fs::write(&prev, b"x").unwrap();
        prune_old(&exe);
        let mut left: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        let mut want = vec!["imperatorium-voice.old-1".to_string(), "other.txt".into(),
                            exe.file_name().unwrap().to_string_lossy().into_owned(), prev.file_name().unwrap().to_string_lossy().into_owned()];
        want.sort();
        assert_eq!(left, want);
        let _ = std::fs::remove_dir_all(&d);
    }
}
