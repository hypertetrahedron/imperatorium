//! `imperatorium hotkey`: the global hotkey that raises the Imperatorium
//! window, or opens it - `hotkey.py` without Python (phase 6). No window of
//! its own: RegisterHotKey binds to the calling thread, so this is one
//! message loop. `--once` raises or opens and exits.

use serde_json::Value;

/// Conflicts are the norm (ctrl+alt+space and win+k were taken on the
/// machine this was written on), so the first one Windows grants wins.
pub const DEFAULT_HOTKEYS: &[&str] = &["ctrl+alt+backquote", "ctrl+alt+j", "ctrl+shift+space", "ctrl+shift+f12"];

const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;
const WINDOW_TITLE: &str = "Imperatorium";

/// hotkey.parse_hotkey: "ctrl+alt+space" -> (modifiers, virtual key).
pub fn parse_hotkey(spec: &str) -> Result<(u32, u32), String> {
    let (mut mods, mut key) = (0u32, None::<String>);
    for part in spec.to_lowercase().split('+') {
        match part.trim() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" | "super" => mods |= MOD_WIN,
            "" => {}
            k => key = Some(k.to_string()),
        }
    }
    let key = key.ok_or_else(|| format!("no key in hotkey {spec:?}"))?;
    let vk = match key.as_str() {
        "space" => 0x20,
        "enter" => 0x0D,
        "tab" => 0x09,
        "escape" => 0x1B,
        "backquote" | "`" => 0xC0,
        k if k.chars().count() == 1 => k.to_uppercase().chars().next().unwrap() as u32,
        k if k.starts_with('f') && k[1..].parse::<u32>().is_ok() => 0x6F + k[1..].parse::<u32>().unwrap(),
        k => return Err(format!("unrecognised key {k:?}")),
    };
    Ok((mods, vk))
}

/// hotkey.candidates: --hotkey, else config.json `palette.hotkey` (a string
/// or a list), else the defaults.
pub fn candidates(over: Option<&str>, config: &Value) -> Vec<String> {
    if let Some(h) = over {
        return vec![h.to_string()];
    }
    match config.get("palette").and_then(|p| p.get("hotkey")) {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) if !a.is_empty() => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => DEFAULT_HOTKEYS.iter().map(|s| s.to_string()).collect(),
    }
}

/// hotkey.is_app_title: exact, so a browser tab on the GitHub repo is not
/// raised instead; "(2) Imperatorium" (sessions waiting) is allowed.
pub fn is_app_title(title: &str) -> bool {
    let t = title.trim();
    let t = match regex::Regex::new(r"^\(\d+\)\s+").unwrap().find(t) {
        Some(m) => &t[m.end()..],
        None => t,
    };
    t.eq_ignore_ascii_case(WINDOW_TITLE)
}

/// Front the window if it is open, otherwise open it.
pub fn raise_or_open(port: &str) -> &'static str {
    let found = crate::winfocus::titled_windows(None).into_iter().find(|(_, t)| is_app_title(t));
    if let Some((hwnd, _)) = found {
        if crate::winfocus::focus_hwnd(hwnd) {
            return "raised";
        }
    }
    let _ = crate::voice::open_window(&format!("http://127.0.0.1:{port}/"), "1280,820");
    "opened"
}

pub fn main(args: &[String], home: Option<&str>) -> i32 {
    let mut over = None;
    let mut once = false;
    let mut port = "8792".to_string();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--hotkey" => over = it.next().cloned(),
            "--once" => once = true,
            "--port" => port = it.next().cloned().unwrap_or(port),
            _ => {}
        }
    }
    if once {
        println!("{}", raise_or_open(&port));
        return 0;
    }
    let config = std::fs::read_to_string(crate::paths::config_home(home).join("config.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    run(&candidates(over.as_deref(), &config), &port)
}

#[cfg(windows)]
fn run(specs: &[String], port: &str) -> i32 {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::RegisterHotKey;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
    let mut bound = None;
    for spec in specs {
        let Ok((mods, vk)) = parse_hotkey(spec) else { continue };
        if unsafe { RegisterHotKey(std::ptr::null_mut(), 1, mods, vk) } != 0 {
            bound = Some(spec.clone());
            break;
        }
    }
    let Some(bound) = bound else {
        eprintln!("no hotkey could be registered; open the window with: imperatorium hotkey --once");
        return 1;
    };
    println!("hotkey {bound} raises the Imperatorium window");
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        if msg.message == WM_HOTKEY {
            // A daemon that dies on one bad press is worse than one that
            // occasionally does nothing.
            let _ = std::panic::catch_unwind(|| raise_or_open(port));
        }
    }
    0
}

#[cfg(not(windows))]
fn run(_specs: &[String], _port: &str) -> i32 {
    eprintln!("global hotkeys are Windows-only here");
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_like_python() {
        assert_eq!(parse_hotkey("ctrl+alt+backquote"), Ok((MOD_CONTROL | MOD_ALT, 0xC0)));
        assert_eq!(parse_hotkey("ctrl+shift+f12"), Ok((MOD_CONTROL | MOD_SHIFT, 0x7B)));
        assert_eq!(parse_hotkey("Win+K"), Ok((MOD_WIN, 'K' as u32)));
        assert!(parse_hotkey("ctrl+alt").is_err());
        assert!(parse_hotkey("ctrl+pagedown").is_err());
    }

    #[test]
    fn titles() {
        assert!(is_app_title("Imperatorium"));
        assert!(is_app_title("(2) Imperatorium"));
        assert!(!is_app_title("hypertetrahedron/imperatorium: x - Google Chrome"));
    }

    #[test]
    fn candidate_order() {
        assert_eq!(candidates(None, &json!({"palette": {"hotkey": "ctrl+k"}})), ["ctrl+k"]);
        assert_eq!(candidates(None, &json!({"palette": {"hotkey": ["a+b", "c+d"]}})), ["a+b", "c+d"]);
        assert_eq!(candidates(Some("x+y"), &json!({})).len(), 1);
        assert_eq!(candidates(None, &json!({})).len(), 4);
    }
}
