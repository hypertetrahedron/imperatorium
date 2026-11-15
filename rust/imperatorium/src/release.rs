//! `imperatorium release-package`: a release folder from built binaries -
//! each asset named by its target triple, SHA256SUMS, and manifest.json,
//! which `imperatorium update` and the install scripts read. What
//! the former rust/release/package.py did, byte for byte.
//!
//!   imperatorium release-package --version 0.3.1 --out dist/0.3.1
//!       [--channel stable|preview] [--notes FILE] [--scripts DIR]
//!       KEY=PATH...
//!
//! A key starting `voice-` is the dictation worker for that triple. A
//! missing file is an error: a release with a hole in it is worse than
//! none. The install scripts (install.ps1, install.sh, install.cmd) are
//! copied from `--scripts DIR` when it is given and has them.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub fn asset_name(key: &str) -> String {
    let (triple, name) = match key.strip_prefix("voice-") {
        Some(t) => (t, format!("imperatorium-voice-{t}")),
        None => (key, format!("imperatorium-{key}")),
    };
    if triple.contains("windows") {
        format!("{name}.exe")
    } else {
        name
    }
}

/// Python's text-mode read (newlines translated to \n) then str.strip().
fn read_notes(path: &str) -> Result<String, String> {
    let raw = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let text = String::from_utf8(raw).map_err(|e| format!("{path}: {e}"))?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n").trim().to_string())
}

pub struct Args {
    version: String,
    channel: Option<String>,
    out: PathBuf,
    notes: Option<String>,
    scripts: Option<PathBuf>,
    assets: Vec<String>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let (mut version, mut channel, mut out, mut notes, mut scripts) = (None, None, None, None, None);
    let mut assets = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let mut value = || inline.clone().or_else(|| it.next().cloned()).ok_or(format!("{name} needs a value"));
        match name {
            "--version" => version = Some(value()?),
            "--channel" => channel = Some(value()?),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--notes" => notes = Some(value()?),
            "--scripts" => scripts = Some(PathBuf::from(value()?)),
            n if n.starts_with("--") => return Err(format!("unknown option {n}")),
            _ => assets.push(a.clone()),
        }
    }
    if assets.is_empty() {
        return Err("no assets given (KEY=PATH)".into());
    }
    Ok(Args {
        version: version.ok_or("--version is required")?,
        channel,
        out: out.ok_or("--out is required")?,
        notes,
        scripts,
        assets,
    })
}

pub fn run(a: &Args) -> Result<(), String> {
    let channel = a
        .channel
        .clone()
        .unwrap_or_else(|| if a.version.contains('-') { "preview" } else { "stable" }.to_string());
    std::fs::create_dir_all(&a.out).map_err(|e| format!("{}: {e}", a.out.display()))?;
    let mut manifest = Map::new();
    manifest.insert("version".into(), json!(a.version.trim_start_matches('v')));
    manifest.insert("channel".into(), json!(channel));
    let mut assets = Map::new();
    let mut sums: Vec<String> = vec![];
    for spec in &a.assets {
        let (key, path) = spec.split_once('=').unwrap_or((spec.as_str(), ""));
        if !Path::new(path).is_file() {
            return Err(format!("missing: {path} ({key})"));
        }
        let name = asset_name(key);
        let dst = a.out.join(&name);
        std::fs::copy(path, &dst).map_err(|e| format!("{path}: {e}"))?;
        let bytes = std::fs::read(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        let digest = crate::remote::sha256_hex(&bytes);
        assets.insert(key.to_string(), json!({"file": name, "sha256": digest, "size": bytes.len()}));
        sums.push(format!("{digest}  {name}"));
        println!("{:<44} {}  {} bytes", name, &digest[..12], bytes.len());
    }
    manifest.insert("assets".into(), Value::Object(assets));
    // After the assets, as package.py's dict had it.
    if let Some(n) = &a.notes {
        manifest.insert("notes".into(), json!(read_notes(n)?));
    }
    let w = |name: &str, text: String| {
        std::fs::write(a.out.join(name), text).map_err(|e| format!("{name}: {e}"))
    };
    w("SHA256SUMS", sums.join("\n") + "\n")?;
    w("manifest.json", crate::pyish::json_dumps_indent(&Value::Object(manifest.clone()), 2) + "\n")?;
    if let Some(dir) = &a.scripts {
        for script in ["install.ps1", "install.sh", "install.cmd"] {
            let src = dir.join(script);
            if src.is_file() {
                std::fs::copy(&src, a.out.join(script)).map_err(|e| format!("{script}: {e}"))?;
            }
        }
    }
    println!("{channel} {} -> {}", manifest["version"].as_str().unwrap_or(""), a.out.display());
    Ok(())
}

pub fn main(args: &[String]) -> i32 {
    match parse(args).and_then(|a| run(&a)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(asset_name("x86_64-pc-windows-msvc"), "imperatorium-x86_64-pc-windows-msvc.exe");
        assert_eq!(asset_name("voice-x86_64-pc-windows-msvc"), "imperatorium-voice-x86_64-pc-windows-msvc.exe");
        assert_eq!(asset_name("aarch64-unknown-linux-musl"), "imperatorium-aarch64-unknown-linux-musl");
    }
}
