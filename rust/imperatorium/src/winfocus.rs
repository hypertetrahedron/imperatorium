//! Bring a session's window to the front, given its process id. A port of
//! `imperatorium/winfocus.py`: no virtual-desktop API (undocumented, and it
//! breaks on feature updates); SetForegroundWindow on a window on another
//! desktop makes Windows switch to it anyway. Everything degrades to
//! false/None rather than failing when there is nothing to focus.

use serde_json::{json, Value};

/// Ancestors that are never the terminal a person means (names as
/// procs::process_table gives them: lowercased, ".exe" dropped).
const STOP_AT: &[&str] = &["explorer", "svchost", "services", "winlogon", "sihost", "wininit", "csrss"];

/// winfocus.normalise_title: the leading status glyph Claude Code puts in
/// the window title stripped (every leading character that is not an ASCII
/// letter or digit), whitespace collapsed, case folded.
pub fn normalise_title(text: &str) -> String {
    let start = text.find(|c: char| c.is_ascii_alphanumeric()).unwrap_or(text.len());
    let rest = &text[start..];
    crate::pyish::casefold(&crate::pyish::split_ws(rest).join(" "))
}

/// winfocus.match_title: the hwnd whose title is this session's. Exactly one
/// exact match, else exactly one prefix match (a title elided to fit its
/// tab), else nothing - guessing between two would raise the wrong one.
pub fn match_title(summary: &str, windows: &[(isize, String)]) -> Option<isize> {
    let want = normalise_title(summary);
    if want.is_empty() {
        return None;
    }
    let pairs: Vec<(isize, String)> = windows.iter().map(|(h, t)| (*h, normalise_title(t))).collect();
    let exact: Vec<isize> = pairs.iter().filter(|(_, t)| *t == want).map(|(h, _)| *h).collect();
    if exact.len() == 1 {
        return Some(exact[0]);
    }
    if !exact.is_empty() {
        return None;
    }
    let partial: Vec<isize> = pairs
        .iter()
        .filter(|(_, t)| !t.is_empty() && (want.starts_with(t.as_str()) || t.starts_with(want.as_str())))
        .map(|(h, _)| *h)
        .collect();
    if partial.len() == 1 {
        Some(partial[0])
    } else {
        None
    }
}

/// winfocus._ancestors: [(pid, exe)] for parents, nearest first, at most 4.
fn ancestors(pid: u32, table: &[crate::procs::Proc]) -> Vec<(u32, String)> {
    let parent = |p: u32| table.iter().find(|r| r.0 == p).map(|r| r.1);
    let name = |p: u32| table.iter().find(|r| r.0 == p).map(|r| r.2.clone()).unwrap_or_default();
    let mut out = Vec::new();
    let mut seen = vec![pid];
    let mut current = pid;
    for _ in 0..4 {
        match parent(current) {
            Some(p) if p != 0 && !seen.contains(&p) => {
                seen.push(p);
                out.push((p, name(p)));
                current = p;
            }
            _ => break,
        }
    }
    out
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsWindow, IsWindowVisible, SetForegroundWindow, ShowWindow,
        SW_RESTORE,
    };

    fn h(hwnd: isize) -> HWND {
        hwnd as HWND
    }

    fn owner(hwnd: HWND) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        pid
    }

    /// [(hwnd, title)] for visible, titled windows, optionally one process's.
    pub fn titled_windows(pid: Option<u32>) -> Vec<(isize, String)> {
        struct Ctx {
            pid: Option<u32>,
            found: Vec<(isize, String)>,
        }
        unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let ctx = &mut *(lparam as *mut Ctx);
            if ctx.pid.is_some_and(|p| owner(hwnd) != p) || IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return 1;
            }
            let mut buf = vec![0u16; len as usize + 1];
            let got = GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);
            buf.truncate(got.max(0) as usize);
            ctx.found.push((hwnd as isize, String::from_utf16_lossy(&buf)));
            1
        }
        let mut ctx = Ctx { pid, found: Vec::new() };
        unsafe { EnumWindows(Some(cb), &mut ctx as *mut Ctx as LPARAM) };
        ctx.found
    }

    pub fn owner_pid(hwnd: isize) -> Option<u32> {
        if hwnd == 0 {
            return None;
        }
        Some(owner(h(hwnd))).filter(|p| *p != 0)
    }

    pub fn is_window(hwnd: isize) -> bool {
        hwnd != 0 && unsafe { IsWindow(h(hwnd)) } != 0
    }

    /// winfocus._raise: SetForegroundWindow, borrowing the foreground
    /// thread's input queue when Windows refuses a background process (the
    /// dispatcher always is one). Detached again at once: two threads left
    /// attached share input state for good.
    pub fn raise(hwnd: isize) -> bool {
        let hwnd = h(hwnd);
        unsafe {
            ShowWindow(hwnd, SW_RESTORE);
            BringWindowToTop(hwnd);
            if SetForegroundWindow(hwnd) != 0 {
                return true;
            }
            let front = GetForegroundWindow();
            if front.is_null() {
                return false;
            }
            let theirs = GetWindowThreadProcessId(front, std::ptr::null_mut());
            let mine = GetCurrentThreadId();
            if theirs == 0 || theirs == mine {
                return false;
            }
            AllowSetForegroundWindow(owner(hwnd));
            if AttachThreadInput(mine, theirs, 1) == 0 {
                return false;
            }
            ShowWindow(hwnd, SW_RESTORE);
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
            AttachThreadInput(mine, theirs, 0);
            GetForegroundWindow() == hwnd
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub fn titled_windows(_pid: Option<u32>) -> Vec<(isize, String)> {
        Vec::new()
    }
    pub fn owner_pid(_hwnd: isize) -> Option<u32> {
        None
    }
    pub fn is_window(_hwnd: isize) -> bool {
        false
    }
    pub fn raise(_hwnd: isize) -> bool {
        false
    }
}

pub use win::{owner_pid, titled_windows};

/// winfocus.focus_hwnd: raise one specific window; true when it came forward.
pub fn focus_hwnd(hwnd: isize) -> bool {
    win::is_window(hwnd) && win::raise(hwnd)
}

/// winfocus._windows_for_pid: the visible, titled windows a process owns.
fn windows_for_pid(pid: u32) -> Vec<isize> {
    titled_windows(Some(pid)).into_iter().map(|(h, _)| h).collect()
}

/// winfocus.terminal_for: the pid of the terminal hosting this session.
/// Several sessions share one terminal process, so this finds the host,
/// never the window.
pub fn terminal_for(pid: u32) -> Option<u32> {
    if !cfg!(windows) || pid == 0 {
        return None;
    }
    let table = crate::procs::process_table();
    for (candidate, exe) in ancestors(pid, &table) {
        if STOP_AT.contains(&exe.as_str()) {
            return None;
        }
        if !windows_for_pid(candidate).is_empty() {
            return Some(candidate);
        }
    }
    None
}

/// winfocus.focus_pid: raise a window of `pid`, or of its nearest ancestor
/// that has one - never past the shell (raising explorer shows the desktop).
pub fn focus_pid(pid: u32) -> bool {
    if !cfg!(windows) || pid == 0 {
        return false;
    }
    let table = crate::procs::process_table();
    let mut chain = vec![(pid, String::new())];
    chain.extend(ancestors(pid, &table));
    for (i, (candidate, exe)) in chain.into_iter().enumerate() {
        if i > 0 && STOP_AT.contains(&exe.as_str()) {
            break;
        }
        for hwnd in windows_for_pid(candidate) {
            if win::raise(hwnd) {
                return true;
            }
        }
    }
    false
}

/// Conformance only: JSON lines in, one answer per line. Nothing here
/// raises a window: the matching and the read-only enumerations.
pub fn check_lines() -> i32 {
    use std::io::BufRead;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let case: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let out = if let Some(t) = case.get("normalise") {
            json!(normalise_title(t.as_str().unwrap_or("")))
        } else if let Some(m) = case.get("match") {
            let windows: Vec<(isize, String)> = m[1]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|w| (w[0].as_i64().unwrap_or(0) as isize, w[1].as_str().unwrap_or("").to_string()))
                        .collect()
                })
                .unwrap_or_default();
            json!(match_title(m[0].as_str().unwrap_or(""), &windows))
        } else if let Some(p) = case.get("terminal_for") {
            json!(terminal_for(p.as_u64().unwrap_or(0) as u32))
        } else if let Some(p) = case.get("titled") {
            let got = titled_windows(p.as_u64().map(|p| p as u32));
            json!(got.into_iter().map(|(h, t)| json!([h, t])).collect::<Vec<_>>())
        } else if let Some(h) = case.get("owner") {
            json!(owner_pid(h.as_i64().unwrap_or(0) as isize))
        } else {
            Value::Null
        };
        println!("{}", crate::pyish::json_dumps(&out, true));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_lose_their_glyph_and_case() {
        assert_eq!(normalise_title("\u{2733} Fix the  Build"), "fix the build");
        assert_eq!(normalise_title("  ...Stra\u{df}e"), "strasse");
        assert_eq!(normalise_title("\u{2733}\u{2733}"), "");
    }

    #[test]
    fn matching_refuses_to_guess() {
        let w = |v: &[(isize, &str)]| v.iter().map(|(h, t)| (*h, t.to_string())).collect::<Vec<_>>();
        assert_eq!(match_title("Fix build", &w(&[(1, "* fix build"), (2, "other")])), Some(1));
        assert_eq!(match_title("Fix build", &w(&[(1, "fix build"), (2, "Fix Build")])), None);
        assert_eq!(match_title("Fix the build please", &w(&[(1, "fix the build")])), Some(1));
        assert_eq!(match_title("Fix", &w(&[(1, "fix a"), (2, "fix b")])), None);
        assert_eq!(match_title("", &w(&[(1, "")])), None);
    }
}
