//! `imperatorium voice fetch | status`: what dictation needs on a machine
//! that got only the binary (phase 6d) - the worker, `imperatorium-voice`,
//! from the release beside the installed exe, and its ggml models into
//! the models folder, each checked against a SHA-256 known here.
//!
//! The models come from whisper.cpp's own repositories on Hugging Face.
//! Their hashes are the ones those repositories list; a model this table
//! does not know is refused rather than taken on trust.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use sha2::Digest;

const HF: &str = "https://huggingface.co";

/// (file, repository, SHA-256, bytes).
const MODELS: &[(&str, &str, &str, u64)] = &[
    ("ggml-large-v3-turbo.bin", "ggerganov/whisper.cpp", "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69", 1_624_555_275),
    ("ggml-large-v3-turbo-q8_0.bin", "ggerganov/whisper.cpp", "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1", 874_188_075),
    ("ggml-large-v3-turbo-q5_0.bin", "ggerganov/whisper.cpp", "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2", 574_041_195),
    ("ggml-small.bin", "ggerganov/whisper.cpp", "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b", 487_601_967),
    ("ggml-small.en.bin", "ggerganov/whisper.cpp", "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d", 487_614_201),
    ("ggml-silero-v6.2.0.bin", "ggml-org/whisper-vad", "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987", 885_098),
];
pub const VAD: &str = "ggml-silero-v6.2.0.bin";

fn known(file: &str) -> Option<(&'static str, &'static str, &'static str, u64)> {
    MODELS.iter().find(|m| m.0 == file).copied()
}

/// A file, streamed to `<dst>.part` while hashed, renamed into place only
/// when the hash is the one expected.
fn download(url: &str, dst: &std::path::Path, sha: &str, size: u64) -> Result<(), String> {
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(120))
        .redirects(8)
        .build();
    let resp = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let part = dst.with_extension("part");
    let mut out = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
    let mut reader = resp.into_reader();
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let (mut got, mut shown) = (0u64, 0u64);
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("{url}: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        if size > (32 << 20) && got - shown >= size / 20 {
            shown = got;
            print!("\r   {:>3}%  {} of {} MB", got * 100 / size, got >> 20, size >> 20);
            let _ = std::io::stdout().flush();
        }
    }
    drop(out);
    if size > (32 << 20) {
        println!();
    }
    let hash: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if hash != sha {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{}: SHA-256 is {hash}, expected {sha} - discarded", dst.display()));
    }
    std::fs::rename(&part, dst).map_err(|e| e.to_string())
}

fn model_file(name: &str) -> String {
    if name.ends_with(".bin") { name.to_string() } else { format!("ggml-{name}.bin") }
}

/// The models the settings call for: the model, the VAD, and with
/// `cpu` the CPU fallback too.
fn wanted(home: Option<&str>, model: Option<&str>, cpu: bool) -> Vec<String> {
    let cfg = std::fs::read_to_string(crate::paths::config_home(home).join("config.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let settings = crate::voice::settings_from(cfg.as_ref().and_then(|c| c.get("voice")));
    let pick = |k: &str| settings.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut out = vec![model_file(model.map(str::to_string).unwrap_or_else(|| pick("model")).as_str()), VAD.to_string()];
    if cpu {
        out.push(model_file(&pick("cpu_model")));
    }
    out.dedup();
    out
}

fn worker_path() -> PathBuf {
    let name = if cfg!(windows) { "imperatorium-voice.exe" } else { "imperatorium-voice" };
    std::env::current_exe().unwrap_or_default().with_file_name(name)
}

pub fn main(args: &[String], home: Option<&str>) -> i32 {
    let mut model = None;
    let mut cpu = false;
    let mut from = None;
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--model" => model = it.next().cloned(),
            "--cpu" => cpu = true,
            "--from" => from = it.next().cloned(),
            _ => {}
        }
    }
    let dir = crate::voice::models_dir();
    match args.first().map(String::as_str) {
        Some("status") => {
            let w = worker_path();
            println!("worker  {} {}", w.display(), if w.is_file() { "ok" } else { "missing" });
            println!("models  {}", dir.display());
            for f in wanted(home, model.as_deref(), true) {
                let p = dir.join(&f);
                println!("   {:<32} {}", f, if p.is_file() { "ok" } else { "missing" });
            }
            0
        }
        Some("fetch") => {
            let mut failed = 0;
            let w = worker_path();
            if !w.is_file() {
                // The worker from the release this binary came from.
                let base = from.or_else(|| crate::paths::env_nonempty("IMPERATORIUM_RELEASES")).unwrap_or_else(|| {
                    format!("{}/latest/download", crate::update::RELEASES)
                });
                let key = format!("voice-{}", crate::update::this_triple());
                println!("worker  {} from {base}", w.display());
                match crate::update::manifest(&base).and_then(|m| crate::update::fetch(&base, &m, &key)) {
                    Ok(Some(body)) => match std::fs::write(&w, body) {
                        Ok(()) => println!("   SHA-256 checked"),
                        Err(e) => {
                            println!("   {e}");
                            failed += 1;
                        }
                    },
                    Ok(None) => {
                        println!("   the release has no dictation worker for {}", crate::update::this_triple());
                        failed += 1;
                    }
                    Err(e) => {
                        println!("   {e}");
                        failed += 1;
                    }
                }
            }
            for f in wanted(home, model.as_deref(), cpu) {
                let dst = dir.join(&f);
                if dst.is_file() {
                    println!("{f}: already there");
                    continue;
                }
                let Some((file, repo, sha, size)) = known(&f) else {
                    println!("{f}: not a model this version knows the hash of - put it in {} by hand", dir.display());
                    failed += 1;
                    continue;
                };
                println!("{file} ({} MB) from {repo}", size >> 20);
                if let Err(e) = download(&format!("{HF}/{repo}/resolve/main/{file}"), &dst, sha, size) {
                    println!("   {e}");
                    failed += 1;
                } else {
                    println!("   SHA-256 checked");
                }
            }
            if failed > 0 { 1 } else { 0 }
        }
        _ => {
            eprintln!("usage: imperatorium voice fetch [--model NAME] [--cpu] [--from URL] | voice status");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(model_file("large-v3-turbo"), "ggml-large-v3-turbo.bin");
        assert_eq!(model_file("ggml-small.bin"), "ggml-small.bin");
        assert!(known("ggml-large-v3-turbo.bin").is_some());
        assert!(known("ggml-medium.bin").is_none());
    }
}
