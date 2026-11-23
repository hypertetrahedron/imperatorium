//! Opening a file the chat names, or the folder it is in (`/api/open_path`):
//! what a click on a path in Claude Code's terminal does. The path is as the
//! chat wrote it - relative to the session's folder, `~/`, with a `:line` or
//! `:line:col` on the end - and is resolved here, on this machine only.
//!
//! "Open" hands the file to the program the OS associates with it, except
//! where that would run it: a program, installer or shortcut is only shown in
//! its folder, and a script (whose association is often its interpreter) is
//! opened in an editor instead.

use std::path::{Path, PathBuf};

/// Tests only: when set, what would be opened is appended to this file
/// ("open <path>", "edit <path>" or "reveal <path>") instead of opening it.
pub const TEST_LOG_ENV: &str = "IMPERATORIUM_TEST_OPEN_LOG";

/// Run when opened: never handed to the OS association.
const RUNS: &[&str] = &[
    "exe", "com", "scr", "msi", "msp", "msix", "appx", "lnk", "url", "hta", "cpl", "pif", "reg", "jar",
    "msc", "application", "appref-ms", "gadget", "app", "dll", "sys", "inf",
];
/// Often associated with their interpreter: opened in an editor instead.
const SCRIPTS: &[&str] = &[
    "bat", "cmd", "ps1", "psm1", "psd1", "vbs", "vbe", "js", "jse", "mjs", "cjs", "wsf", "wsh", "py",
    "pyw", "sh", "bash", "command", "pl", "rb",
];

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum How {
    /// The OS's own association.
    Open,
    /// A text editor.
    Edit,
    /// Not opened: shown in its folder.
    Refuse,
}

/// Pure: how "Open" treats `path`. A folder opens.
pub fn how(path: &Path, is_dir: bool) -> How {
    if is_dir {
        return How::Open;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if RUNS.contains(&ext.as_str()) {
        How::Refuse
    } else if SCRIPTS.contains(&ext.as_str()) {
        How::Edit
    } else {
        How::Open
    }
}

/// Pure: `raw` without the `:line`, `:line:col` or `:a-b` the chat puts
/// after a path (never a drive's colon).
pub fn strip_line(raw: &str) -> &str {
    let mut s = raw.trim();
    for _ in 0..2 {
        let Some(i) = s.rfind(':') else { break };
        let tail = &s[i + 1..];
        let digits = !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '-');
        // "C:" is a drive, not a line.
        if !digits || i < 2 {
            break;
        }
        s = &s[..i];
    }
    s
}

/// Pure: where `raw` points, given the session's folder and the home
/// folder. `/c/Users/...` (a Git Bash path) is `C:\Users\...` on Windows.
pub fn resolve(raw: &str, cwd: Option<&Path>, home: Option<&Path>) -> Result<PathBuf, String> {
    let p = resolve_raw(raw, cwd, home)?;
    // One kind of separator, as Windows writes them.
    Ok(if cfg!(windows) { PathBuf::from(p.to_string_lossy().replace('/', "\\")) } else { p })
}

fn resolve_raw(raw: &str, cwd: Option<&Path>, home: Option<&Path>) -> Result<PathBuf, String> {
    let s = strip_line(raw).trim_matches(|c| c == '`' || c == '"' || c == '\'');
    if s.is_empty() {
        return Err("no path given".into());
    }
    if s.contains('\0') {
        return Err("not a path".into());
    }
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        let home = home.ok_or("no home folder to resolve ~ against")?;
        return Ok(home.join(rest));
    }
    if cfg!(windows) {
        let b = s.as_bytes();
        if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b'/' {
            return Ok(PathBuf::from(format!("{}:\\{}", (b[1] as char).to_ascii_uppercase(), &s[3..])));
        }
    }
    let p = Path::new(s);
    if p.is_absolute() || (cfg!(windows) && s.starts_with(['/', '\\'])) {
        return Ok(p.to_path_buf());
    }
    let cwd = cwd.ok_or("a relative path, and no session folder to resolve it against")?;
    Ok(cwd.join(p))
}

/// Open `path` (or with `reveal`, show it selected in its folder). Returns
/// what was done: "open", "edit" or "reveal".
pub fn open(path: &Path, reveal: bool) -> Result<&'static str, String> {
    let meta = std::fs::metadata(path).map_err(|_| format!("not found: {}", path.display()))?;
    let what = if reveal {
        "reveal"
    } else {
        match how(path, meta.is_dir()) {
            How::Open => "open",
            How::Edit => "edit",
            How::Refuse => {
                return Err(format!(
                    "{} is a program: opening it would run it. Open folder shows it instead",
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                ))
            }
        }
    };
    open_as(path, what)
}

/// A folder (a session's checkout or worktree) or a file in the editor: VS
/// Code when `code` is on PATH; else a folder opens as Open would and a
/// file goes to Notepad. What was done.
pub fn open_in_editor(path: &Path) -> Result<&'static str, String> {
    let meta = std::fs::metadata(path).map_err(|_| format!("not found: {}", path.display()))?;
    let testing = crate::paths::env_nonempty(TEST_LOG_ENV).is_some();
    if meta.is_dir() && !testing && crate::launch::which("code").is_none() {
        return open(path, false);
    }
    open_as(path, "edit")
}

fn open_as(path: &Path, what: &'static str) -> Result<&'static str, String> {
    if let Some(log) = crate::paths::env_nonempty(TEST_LOG_ENV) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|e| e.to_string())?;
        // One write, so two opens at once do not interleave.
        f.write_all(format!("{what} {}\n", path.display()).as_bytes()).map_err(|e| e.to_string())?;
        return Ok(what);
    }
    let names = title_names(path, what);
    let before = if cfg!(windows) { crate::winfocus::titled_windows(None) } else { Vec::new() };
    let front = crate::winfocus::foreground();
    launch(path, what)?;
    if cfg!(windows) {
        std::thread::spawn(move || bring_forward(&before, front, &names));
    }
    Ok(what)
}

/// How long to look for the window that opened, how often, and when to
/// settle for a window that does not name the file.
const RAISE_FOR: std::time::Duration = std::time::Duration::from_secs(8);
const RAISE_EVERY: std::time::Duration = std::time::Duration::from_millis(150);
const RAISE_LATE: std::time::Duration = std::time::Duration::from_millis(2500);

/// Pure: what the window that shows `path` is likely to have in its title.
/// A file's name (and its name without the extension, as Word and others
/// show it, when that is long enough to mean something); for a folder, or
/// a file shown in its folder, the folder's name, which is how File
/// Explorer titles its window.
pub fn title_names(path: &Path, what: &str) -> Vec<String> {
    let folder = what == "reveal" || (what == "open" && path.is_dir());
    let of = |p: &Path, stem: bool| {
        let n = if stem { p.file_stem() } else { p.file_name() };
        n.map(|n| crate::pyish::casefold(&n.to_string_lossy())).unwrap_or_default()
    };
    let mut out = Vec::new();
    if folder {
        let dir = if what == "reveal" { path.parent().unwrap_or(path) } else { path };
        out.push(of(dir, false));
    } else {
        out.push(of(path, false));
        let stem = of(path, true);
        if stem.chars().count() >= 4 && !out.contains(&stem) {
            out.push(stem);
        }
    }
    out.retain(|n| !n.is_empty());
    out
}

/// Pure: which window to bring forward, given the windows there were before
/// the open (`before`), the ones there are now (front to back, as
/// EnumWindows lists them), and the window that was in front then (the
/// browser the click came from, never the answer). First a window that has
/// just come to name the file - a new one, or one whose title changed to it
/// (an editor already running opens it in a tab). Once `late`, a new window
/// whatever its title, then one that already named it (a file opened again).
pub fn pick_opened(
    before: &[(isize, String)],
    now: &[(isize, String)],
    front: isize,
    names: &[String],
    late: bool,
) -> Option<isize> {
    let named = |t: &str| {
        let t = crate::pyish::casefold(t);
        names.iter().any(|n| t.contains(n.as_str()))
    };
    let was = |h: isize| before.iter().find(|(b, _)| *b == h).map(|(_, t)| t.as_str());
    let open: Vec<&(isize, String)> = now.iter().filter(|(h, _)| *h != front && *h != 0).collect();
    if let Some((h, _)) = open.iter().find(|(h, t)| named(t) && was(*h).is_none_or(|b| !named(b))) {
        return Some(*h);
    }
    if !late {
        return None;
    }
    if let Some((h, _)) = open.iter().find(|(h, _)| was(*h).is_none()) {
        return Some(*h);
    }
    open.iter().find(|(_, t)| named(t)).map(|(h, _)| *h)
}

/// Watches for the window that opened and raises it: the dispatcher is a
/// background process, so Windows would otherwise leave the program it
/// started behind the browser the click came from.
fn bring_forward(before: &[(isize, String)], front: isize, names: &[String]) {
    let start = std::time::Instant::now();
    let me = std::process::id();
    while start.elapsed() < RAISE_FOR {
        std::thread::sleep(RAISE_EVERY);
        let now: Vec<(isize, String)> = crate::winfocus::titled_windows(None)
            .into_iter()
            .filter(|(h, _)| crate::winfocus::owner_pid(*h) != Some(me))
            .collect();
        if let Some(h) = pick_opened(before, &now, front, names, start.elapsed() >= RAISE_LATE) {
            crate::winfocus::focus_hwnd(h);
            return;
        }
    }
}

#[cfg(windows)]
fn launch(path: &Path, what: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    // A Windows path cannot hold a double quote, so quoting it is enough.
    let p = path.to_string_lossy().replace('/', "\\");
    if p.contains('"') {
        return Err("not a path".into());
    }
    let mut c = match what {
        "reveal" => {
            let mut c = Command::new("explorer.exe");
            c.raw_arg(format!("/select,\"{p}\""));
            c
        }
        "edit" => match crate::launch::which("code") {
            // VS Code's launcher is a .cmd; Rust refuses an argument cmd
            // would misread rather than pass it on.
            Some(code) => {
                let mut c = Command::new(code);
                c.arg(&p);
                c
            }
            None => {
                let mut c = Command::new("notepad.exe");
                c.raw_arg(format!("\"{p}\""));
                c
            }
        },
        _ => {
            let mut c = Command::new("explorer.exe");
            c.raw_arg(format!("\"{p}\""));
            c
        }
    };
    c.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    crate::proc::no_window(&mut c);
    // Explorer answers 1 even when it worked: started is enough.
    c.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn launch(path: &Path, what: &str) -> Result<(), String> {
    use std::process::Command;
    let mac = cfg!(target_os = "macos");
    let mut c = if mac { Command::new("open") } else { Command::new("xdg-open") };
    match (what, mac) {
        ("reveal", true) => c.arg("-R").arg(path),
        ("reveal", false) => c.arg(path.parent().unwrap_or(path)),
        ("edit", true) => c.arg("-t").arg(path),
        _ => c.arg(path),
    };
    c.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    c.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_column_are_dropped_but_not_a_drive() {
        assert_eq!(strip_line("src/a.rs:12"), "src/a.rs");
        assert_eq!(strip_line("src/a.rs:12:4"), "src/a.rs");
        assert_eq!(strip_line("src/a.rs:12-30"), "src/a.rs");
        assert_eq!(strip_line(r"C:\x\a.py:7"), r"C:\x\a.py");
        assert_eq!(strip_line(r"C:\x\a.py"), r"C:\x\a.py");
        assert_eq!(strip_line("C:"), "C:");
        assert_eq!(strip_line("a.txt:note"), "a.txt:note");
    }

    #[test]
    fn relative_paths_are_the_sessions() {
        let cwd = Path::new(if cfg!(windows) { r"C:\w\proj" } else { "/w/proj" });
        let want = if cfg!(windows) { cwd.join(r"docs\a.md") } else { cwd.join("docs/a.md") };
        assert_eq!(resolve("docs/a.md:3", Some(cwd), None).unwrap(), want);
        assert!(resolve("docs/a.md", None, None).is_err());
        assert!(resolve("  ", Some(cwd), None).is_err());
    }

    #[test]
    fn home_and_absolute_paths() {
        let home = Path::new(if cfg!(windows) { r"C:\Users\x" } else { "/home/x" });
        assert_eq!(resolve("~/notes.txt", None, Some(home)).unwrap(), home.join("notes.txt"));
        let abs = if cfg!(windows) { r"D:\a\b.txt" } else { "/a/b.txt" };
        assert_eq!(resolve(abs, None, None).unwrap(), PathBuf::from(abs));
        if cfg!(windows) {
            assert_eq!(resolve("/c/Users/x/a.txt", None, None).unwrap(), PathBuf::from(r"C:\Users\x\a.txt"));
        }
    }

    #[test]
    fn programs_are_never_opened_and_scripts_go_to_an_editor() {
        assert_eq!(how(Path::new("setup.EXE"), false), How::Refuse);
        assert_eq!(how(Path::new("x.lnk"), false), How::Refuse);
        assert_eq!(how(Path::new("app.js"), false), How::Edit);
        assert_eq!(how(Path::new("run.ps1"), false), How::Edit);
        assert_eq!(how(Path::new("a.py"), false), How::Edit);
        assert_eq!(how(Path::new("notes.md"), false), How::Open);
        assert_eq!(how(Path::new("shot.png"), false), How::Open);
        assert_eq!(how(Path::new("tools.exe"), true), How::Open);
    }

    #[test]
    fn the_title_to_look_for() {
        let n = |p: &str, w: &str| title_names(Path::new(p), w);
        assert_eq!(n("/w/proj/Report.docx", "open"), vec!["report.docx", "report"]);
        assert_eq!(n("/w/proj/a.md", "open"), vec!["a.md"]);
        assert_eq!(n("/w/proj/app.js", "edit"), vec!["app.js"]);
        assert_eq!(n("/w/proj/docs/a.md", "reveal"), vec!["docs"]);
    }

    fn w(v: &[(isize, &str)]) -> Vec<(isize, String)> {
        v.iter().map(|(h, t)| (*h, t.to_string())).collect()
    }

    #[test]
    fn the_window_that_opened_is_picked() {
        let names = vec!["a.md".to_string()];
        let before = w(&[(1, "Imperatorium - Chrome"), (2, "notes.txt - Notepad"), (3, "Code")]);
        // A new window naming the file.
        let now = w(&[(9, "a.md - Typora"), (1, "Imperatorium - Chrome"), (2, "notes.txt - Notepad")]);
        assert_eq!(pick_opened(&before, &now, 1, &names, false), Some(9));
        // An editor already running that switched to it.
        let now = w(&[(1, "Imperatorium - Chrome"), (3, "a.md - proj - Code")]);
        assert_eq!(pick_opened(&before, &now, 1, &names, false), Some(3));
        // Nothing yet: wait, rather than guess early.
        assert_eq!(pick_opened(&before, &before, 1, &names, false), None);
        assert_eq!(pick_opened(&before, &before, 1, &names, true), None);
    }

    #[test]
    fn late_it_settles_for_a_new_window_or_one_already_naming_it() {
        let names = vec!["shot.png".to_string()];
        let before = w(&[(1, "Imperatorium - Chrome"), (4, "shot.png - Paint")]);
        let new = w(&[(7, "Photos"), (1, "Imperatorium - Chrome"), (4, "shot.png - Paint")]);
        assert_eq!(pick_opened(&before, &new, 1, &names, false), None);
        assert_eq!(pick_opened(&before, &new, 1, &names, true), Some(7));
        // Opened again where it already was.
        assert_eq!(pick_opened(&before, &before, 1, &names, true), Some(4));
    }

    #[test]
    fn the_window_the_click_came_from_is_never_raised() {
        let names = vec!["a.md".to_string()];
        let before = w(&[(1, "Imperatorium")]);
        let now = w(&[(1, "a.md - Imperatorium")]);
        assert_eq!(pick_opened(&before, &now, 1, &names, true), None);
    }

    #[test]
    fn a_missing_file_is_said() {
        let e = open(Path::new("Z:/certainly/not/here.txt"), false).unwrap_err();
        assert!(e.starts_with("not found"), "{e}");
    }
}
