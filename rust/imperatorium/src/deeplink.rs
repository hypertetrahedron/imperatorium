//! Open a project in Claude Code with a prompt pre-filled, from
//! `imperatorium/deeplink.py`: `claude-cli://open?cwd=<abs>&q=<prompt>`
//! opens a session in that directory with the prompt typed but NOT sent -
//! the confirmation step that makes launching on a probable match safe.

pub const MAX_Q: usize = 5000;

/// urllib.parse.quote(s, safe=""): every byte but A-Z a-z 0-9 _ . - ~.
pub fn quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// deeplink.build.
pub fn build(cwd: &str, prompt: &str) -> Result<String, String> {
    if cwd.is_empty() {
        return Err("cwd is required".into());
    }
    let path = crate::paths::abspath(cwd).ok_or_else(|| format!("not a path: {cwd}"))?;
    if path.starts_with("\\\\") || path.starts_with("//") {
        return Err(format!("deep links reject UNC paths: {path}"));
    }
    if path.replace('\\', "/").split('/').any(|seg| seg == "..") {
        return Err(format!("deep links reject '..' segments: {path}"));
    }
    let n = prompt.chars().count();
    if n > MAX_Q {
        return Err(format!("prompt is {n} chars; the cap is {MAX_Q}"));
    }
    let mut url = format!("claude-cli://open?cwd={}", quote(&path));
    if !prompt.is_empty() {
        url.push_str(&format!("&q={}", quote(prompt)));
    }
    Ok(url)
}

/// deeplink.open_url: hand it to the OS handler.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        // Through cmd, so the shell resolves the protocol handler; the empty
        // title is needed or a quoted URL is read as the title.
        let mut c = std::process::Command::new("cmd");
        c.args(["/c", "start", "", url]);
        let _ = crate::proc::no_window(&mut c).status();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).status();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).status();
    }
}

/// deeplink.launch.
pub fn launch(cwd: &str, prompt: &str) -> Result<String, String> {
    let url = build(cwd, prompt)?;
    open_url(&url);
    Ok(url)
}
