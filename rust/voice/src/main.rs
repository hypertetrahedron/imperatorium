//! `imperatorium-voice --worker SETTINGS_JSON`: the dictation child, in
//! place of `python -m imperatorium.voice --worker` (phase 5). Same
//! JSON-lines protocol on stdin/stdout:
//!
//!   {"op": "load"}                                 -> {"device", "model", "ok"}
//!   {"op": "transcribe", "hint", "bytes": n} + n bytes
//!                                                  -> {"device", "model", "ok", "text"}
//!
//! whisper.cpp instead of faster-whisper, with the same choices: the
//! settings' model on CUDA, falling back to the CPU model; greedy decoding
//! at beam_size 1; Silero VAD with faster-whisper's own defaults; nothing
//! returned for near-silence. Audio arrives as WAV (the window decodes what
//! it recorded and resamples to 16 kHz mono before sending); other rates and
//! channel counts are mixed down and resampled here.
//!
//! Models are ggml files in the models folder (`%LOCALAPPDATA%\imperatorium\
//! models`, `~/.local/share/imperatorium/models`, or IMPERATORIUM_MODELS):
//! `ggml-<name>.bin`, and `ggml-silero-v6.2.0.bin` for the VAD.

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;

use serde_json::{json, Map, Value};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadParams,
};

const SAMPLE_RATE: u32 = 16_000;
const VAD_MODEL: &str = "ggml-silero-v6.2.0.bin";

fn models_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("IMPERATORIUM_MODELS").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
    base.join("imperatorium").join("models")
}

/// A model setting as a file: a path as given, else ggml-<name>.bin.
pub fn model_file(name: &str) -> PathBuf {
    let p = PathBuf::from(name);
    if p.extension().is_some() && (p.is_absolute() || name.contains(['/', '\\'])) {
        return p;
    }
    models_dir().join(format!("ggml-{name}.bin"))
}

fn s<'a>(settings: &'a Map<String, Value>, k: &str, d: &'a str) -> &'a str {
    settings.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).unwrap_or(d)
}

struct Transcriber {
    settings: Map<String, Value>,
    /// What runs: (device, model name), once loaded.
    device: String,
    model: String,
    ctx: Option<WhisperContext>,
}

impl Transcriber {
    fn new(settings: Map<String, Value>) -> Self {
        let device = s(&settings, "device", "cuda").to_string();
        let model = s(&settings, "model", "large-v3-turbo").to_string();
        Transcriber { settings, device, model, ctx: None }
    }

    fn build(model: &str, gpu: bool) -> Result<WhisperContext, String> {
        let path = model_file(model);
        if !path.is_file() {
            return Err(format!("no model file {}", path.display()));
        }
        let mut params = WhisperContextParameters::default();
        params.use_gpu(gpu);
        params.flash_attn(gpu);
        WhisperContext::new_with_params(&path, params)
            .map_err(|e| format!("loading {}: {e:?}", path.display()))
    }

    /// Transcriber.load: the model on its device, or the CPU model on the
    /// CPU when the GPU cannot have it.
    fn load(&mut self) -> Result<(), String> {
        if self.ctx.is_some() {
            return Ok(());
        }
        let want_gpu = self.device != "cpu";
        let first = if want_gpu && gpu_present() {
            Self::build(&self.model, true)
        } else {
            Err("no GPU device".into())
        };
        match first {
            Ok(c) => {
                self.ctx = Some(c);
                // The settings' "cuda" only means "a GPU"; report the one
                // this build really runs on (nothing reads more than "cpu").
                self.device = gpu_backend().into();
            }
            Err(e) if want_gpu => {
                eprintln!("imperatorium-voice: {e}; falling back to the CPU");
                let cpu = s(&self.settings, "cpu_model", "small").to_string();
                self.ctx = Some(Self::build(&cpu, false)?);
                self.device = "cpu".into();
                self.model = cpu;
            }
            Err(_) => {
                self.ctx = Some(Self::build(&self.model, false)?);
            }
        }
        Ok(())
    }

    fn transcribe(&mut self, audio: &[f32], hint: Option<&str>) -> Result<String, String> {
        if audio.is_empty() {
            return Ok(String::new());
        }
        // Near-silence is a fumbled key, not an utterance: transcribing it
        // invents words, which is worse than returning nothing.
        if audio.iter().fold(0f32, |m, x| m.max(x.abs())) < 0.005 {
            return Ok(String::new());
        }
        self.load()?;
        let beam = self.settings.get("beam_size").and_then(Value::as_i64).unwrap_or(1).max(1) as i32;
        let strategy = if beam > 1 {
            SamplingStrategy::BeamSearch { beam_size: beam, patience: -1.0 }
        } else {
            SamplingStrategy::Greedy { best_of: 1 }
        };
        let mut p = FullParams::new(strategy);
        let lang = s(&self.settings, "language", "en").to_string();
        p.set_language(Some(&lang));
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        p.set_no_timestamps(true);
        p.set_suppress_blank(true);
        p.set_suppress_nst(true);
        p.set_n_threads(std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4) as i32);
        if let Some(h) = hint.filter(|h| !h.is_empty()) {
            p.set_initial_prompt(h);
        }
        let vad = models_dir().join(VAD_MODEL);
        let vad_path = vad.to_string_lossy().into_owned();
        if vad.is_file() {
            // faster-whisper's VadOptions defaults, which vad_filter=True ran.
            let mut v = WhisperVadParams::new();
            v.set_threshold(0.5);
            v.set_min_speech_duration(0);
            v.set_min_silence_duration(2000);
            v.set_speech_pad(400);
            p.set_vad_model_path(Some(&vad_path));
            p.set_vad_params(v);
            p.enable_vad(true);
        }
        let ctx = self.ctx.as_ref().unwrap();
        let mut state = ctx.create_state().map_err(|e| format!("{e:?}"))?;
        state.full(p, audio).map_err(|e| format!("transcribing: {e:?}"))?;
        // " ".join(segment.text ...).strip(), as voice.py does.
        let mut parts = Vec::new();
        for seg in state.as_iter() {
            parts.push(seg.to_str_lossy().map_err(|e| format!("{e:?}"))?.into_owned());
        }
        Ok(parts.join(" ").trim().to_string())
    }
}

/// Is there a GPU to use? whisper.cpp would quietly run on the CPU without
/// one, and the window says which it is. Asked of ggml's own device list,
/// so it is the same question for the CUDA build and the Vulkan one.
/// The GPU backend this build was compiled with ("cpu": none).
fn gpu_backend() -> &'static str {
    if cfg!(feature = "vulkan") {
        "vulkan"
    } else if cfg!(feature = "cuda") {
        "cuda"
    } else {
        "cpu"
    }
}

fn gpu_present() -> bool {
    if std::env::var_os("IMPERATORIUM_VOICE_CPU").is_some() {
        return false;
    }
    gpu_name().is_some()
}

/// The first GPU ggml sees (a discrete one before an integrated one).
fn gpu_name() -> Option<String> {
    use whisper_rs::whisper_rs_sys as sys;
    let mut integrated = None;
    // SAFETY: the registry is ggml's static list; the pointers it returns
    // live for the process, and the strings are NUL-terminated.
    unsafe {
        for i in 0..sys::ggml_backend_dev_count() {
            let dev = sys::ggml_backend_dev_get(i);
            let kind = sys::ggml_backend_dev_type(dev);
            let desc = sys::ggml_backend_dev_description(dev);
            let name = if desc.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(desc).to_string_lossy().into_owned()
            };
            if kind == sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU {
                return Some(name);
            }
            if kind == sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU && integrated.is_none() {
                integrated = Some(name);
            }
        }
    }
    integrated
}

// -- audio ----------------------------------------------------------------

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// A WAV file as 16 kHz mono f32: PCM 8/16/24/32-bit or float 32/64, any
/// rate, any channel count.
pub fn decode_wav(data: &[u8]) -> Result<Vec<f32>, String> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err("the audio is not WAV (reload the window: it now sends WAV)".into());
    }
    let (mut fmt, mut body) = (None, None);
    let mut i = 12;
    while i + 8 <= data.len() {
        let id = &data[i..i + 4];
        let len = u32le(data, i + 4) as usize;
        let start = i + 8;
        let end = (start + len).min(data.len());
        match id {
            b"fmt " if end - start >= 16 => fmt = Some(&data[start..end]),
            b"data" => body = Some(&data[start..end]),
            _ => {}
        }
        i = start + len + (len & 1);
    }
    let fmt = fmt.ok_or("WAV without a fmt chunk")?;
    let body = body.ok_or("WAV without a data chunk")?;
    let mut tag = u16le(fmt, 0);
    let channels = u16le(fmt, 2).max(1) as usize;
    let rate = u32le(fmt, 4);
    let bits = u16le(fmt, 14) as usize;
    if tag == 0xFFFE && fmt.len() >= 26 {
        tag = u16le(fmt, 24); // the extensible format's subformat
    }
    let width = bits / 8;
    if width == 0 || rate == 0 {
        return Err("WAV with no sample size or rate".into());
    }
    let frame = width * channels;
    let frames = body.len() / frame;
    let mut mono = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut sum = 0f32;
        for c in 0..channels {
            let o = f * frame + c * width;
            let x = &body[o..o + width];
            sum += match (tag, bits) {
                (1, 8) => (x[0] as f32 - 128.0) / 128.0,
                (1, 16) => i16::from_le_bytes([x[0], x[1]]) as f32 / 32768.0,
                (1, 24) => (i32::from_le_bytes([0, x[0], x[1], x[2]]) >> 8) as f32 / 8_388_608.0,
                (1, 32) => i32::from_le_bytes([x[0], x[1], x[2], x[3]]) as f32 / 2_147_483_648.0,
                (3, 32) => f32::from_le_bytes([x[0], x[1], x[2], x[3]]),
                (3, 64) => f64::from_le_bytes(x[..8].try_into().unwrap()) as f32,
                _ => return Err(format!("WAV format {tag} at {bits} bits is not supported")),
            };
        }
        mono.push(sum / channels as f32);
    }
    Ok(resample(&mono, rate, SAMPLE_RATE))
}

/// Linear resampling; the window already sends 16 kHz, so this is the rare
/// path (a WAV from elsewhere).
fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let n = (x.len() as u64 * to as u64 / from as u64) as usize;
    let step = from as f64 / to as f64;
    (0..n)
        .map(|i| {
            let t = i as f64 * step;
            let j = t as usize;
            let k = (j + 1).min(x.len() - 1);
            let f = (t - j as f64) as f32;
            x[j] * (1.0 - f) + x[k] * f
        })
        .collect()
}

// -- the worker loop ------------------------------------------------------

fn worker(settings: Map<String, Value>) -> i32 {
    let mut t = Transcriber::new(settings);
    let stdin = std::io::stdin();
    let mut inp = stdin.lock();
    let mut out = std::io::stdout();
    let reply = |out: &mut std::io::Stdout, v: Value| {
        let _ = writeln!(out, "{}", v).and_then(|_| out.flush());
    };
    let mut line = String::new();
    loop {
        line.clear();
        match inp.read_line(&mut line) {
            Ok(0) | Err(_) => return 0,
            Ok(_) => {}
        }
        let req: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => {
                reply(&mut out, json!({"ok": false, "error": format!("JSONDecodeError: {e}")}));
                continue;
            }
        };
        let got = match req.get("op").and_then(Value::as_str) {
            // Errors named as the Python worker's exceptions are: the
            // dispatcher shows them as "VoiceError: <type>: <message>".
            Some("load") => t.load().map(|_| None).map_err(|e| format!("RuntimeError: {e}")),
            Some("transcribe") => {
                let n = req.get("bytes").and_then(Value::as_u64).unwrap_or(0) as usize;
                let mut data = vec![0u8; n];
                if inp.read_exact(&mut data).is_err() {
                    return 0;
                }
                let hint = req.get("hint").and_then(Value::as_str);
                decode_wav(&data)
                    .map_err(|e| format!("ValueError: {e}"))
                    .and_then(|a| t.transcribe(&a, hint).map_err(|e| format!("RuntimeError: {e}")))
                    .map(Some)
            }
            other => {
                reply(&mut out, json!({"ok": false, "error": format!("unknown op {}",
                    other.map(|o| format!("'{o}'")).unwrap_or_else(|| "None".into()))}));
                continue;
            }
        };
        // dict(where(), ok=True[, text=...]): device and model first.
        let mut r = Map::new();
        match got {
            Ok(text) => {
                r.insert("device".into(), json!(t.device));
                r.insert("model".into(), json!(t.model));
                r.insert("ok".into(), json!(true));
                if let Some(text) = text {
                    r.insert("text".into(), json!(text));
                }
            }
            Err(e) => {
                r.insert("ok".into(), json!(false));
                r.insert("error".into(), json!(e));
            }
        }
        reply(&mut out, Value::Object(r));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // whisper.cpp and ggml log to stderr by default; quiet them unless asked.
    if std::env::var_os("IMPERATORIUM_VOICE_DEBUG").is_none() {
        whisper_rs::install_logging_hooks();
    }
    let code = match args.first().map(String::as_str) {
        Some("--worker") => {
            let settings = args
                .get(1)
                .and_then(|a| serde_json::from_str::<Value>(a).ok())
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            worker(settings)
        }
        Some("--models") => {
            println!("{}", models_dir().display());
            0
        }
        Some("--devices") => {
            // What a load would run on: {"gpu": "<name>" | null, "backend": ...}.
            println!("{}", json!({"gpu": if gpu_present() { gpu_name() } else { None }, "backend": gpu_backend()}));
            0
        }
        Some("--transcribe") => {
            // A file straight through, for testing: --transcribe FILE.wav [SETTINGS]
            let settings = args
                .get(2)
                .and_then(|a| serde_json::from_str::<Value>(a).ok())
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            let mut t = Transcriber::new(settings);
            let started = std::time::Instant::now();
            let got = std::fs::read(args.get(1).cloned().unwrap_or_default())
                .map_err(|e| e.to_string())
                .and_then(|d| decode_wav(&d))
                .and_then(|a| {
                    t.load()?;
                    let loaded = started.elapsed();
                    let s2 = std::time::Instant::now();
                    let text = t.transcribe(&a, None)?;
                    Ok((text, loaded, s2.elapsed()))
                });
            match got {
                Ok((text, l, x)) => {
                    println!("{}", json!({"text": text, "device": t.device, "model": t.model,
                        "load_s": l.as_secs_f64(), "transcribe_s": x.as_secs_f64()}));
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        _ => {
            eprintln!("usage: imperatorium-voice --worker SETTINGS_JSON | --transcribe FILE.wav [SETTINGS] | --models | --devices");
            2
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut v = Vec::new();
        v.extend(b"RIFF");
        v.extend(((36 + data.len()) as u32).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(channels.to_le_bytes());
        v.extend(rate.to_le_bytes());
        v.extend((rate * channels as u32 * 2).to_le_bytes());
        v.extend((channels * 2).to_le_bytes());
        v.extend(16u16.to_le_bytes());
        v.extend(b"LIST");
        v.extend(4u32.to_le_bytes());
        v.extend(b"INFO");
        v.extend(b"data");
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        v
    }

    #[test]
    fn reads_16k_mono() {
        let a = decode_wav(&wav(16000, 1, &[0, 16384, -32768])).unwrap();
        assert_eq!(a, vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn mixes_and_resamples() {
        // 32 kHz stereo, both channels equal: half as many mono samples.
        let a = decode_wav(&wav(32000, 2, &[100, 100, 200, 200, 300, 300, 400, 400])).unwrap();
        assert_eq!(a.len(), 2);
        assert!((a[0] - 100.0 / 32768.0).abs() < 1e-6);
    }

    #[test]
    fn refuses_what_is_not_wav() {
        assert!(decode_wav(b"\x1aE\xdf\xa3webm").unwrap_err().contains("not WAV"));
    }
}
