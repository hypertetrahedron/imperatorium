//! Is an idle session still waiting on something it started? A port of
//! `imperatorium/procs.py`: the shells beneath each live session's process.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash", "fish", "pwsh", "powershell"];
/// A hook or statusline render runs through a shell too, briefly.
const MIN_AGE: f64 = 5.0;

/// (pid, ppid, exe name lowercased without ".exe", started_at)
pub type Proc = (u32, u32, String, Option<f64>);

fn exe(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
    base.strip_suffix(".exe")
        .map(str::to_string)
        .unwrap_or(base)
}

#[cfg(windows)]
pub fn process_table() -> Vec<Proc> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut rows = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap == INVALID_HANDLE_VALUE {
            return rows;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let len = e
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(e.szExeFile.len());
            let name = String::from_utf16_lossy(&e.szExeFile[..len]);
            rows.push((e.th32ProcessID, e.th32ParentProcessID, exe(&name), None));
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    rows
}

#[cfg(windows)]
fn started(pid: u32) -> Option<f64> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut t: [FILETIME; 4] = std::mem::zeroed();
        let ok = GetProcessTimes(h, &mut t[0], &mut t[1], &mut t[2], &mut t[3]);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let ft = ((t[0].dwHighDateTime as u64) << 32) | t[0].dwLowDateTime as u64;
        Some(ft as f64 / 1e7 - 11_644_473_600.0)
    }
}

#[cfg(not(windows))]
pub fn process_table() -> Vec<Proc> {
    let Ok(out) = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,etimes=,comm="])
        .output()
    else {
        return vec![];
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut rows = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let (Ok(pid), Ok(ppid)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) else {
            continue;
        };
        let age = parts[2].parse::<f64>().ok();
        rows.push((pid, ppid, exe(&parts[3..].join(" ")), age.map(|a| now - a)));
    }
    rows
}

#[cfg(not(windows))]
fn started(_pid: u32) -> Option<f64> {
    None
}

/// {pid: shells beneath it that have outlived MIN_AGE}; a shell inside a
/// shell is one task, so only the topmost on each branch counts.
pub fn waiting_shells(pids: &[u32], table: &[Proc], now: f64) -> HashMap<u32, u64> {
    let mut children: HashMap<u32, Vec<&Proc>> = HashMap::new();
    for p in table {
        children.entry(p.1).or_default().push(p);
    }
    let mut out = HashMap::new();
    for &root in pids {
        let mut count = 0u64;
        let mut stack: Vec<&Proc> = children.get(&root).cloned().unwrap_or_default();
        let mut seen: HashSet<u32> = HashSet::from([root]);
        while let Some(p) = stack.pop() {
            if !seen.insert(p.0) {
                continue;
            }
            if SHELLS.contains(&p.2.as_str()) {
                let at = p.3.or_else(|| started(p.0));
                if at.is_none_or(|a| now - a >= MIN_AGE) {
                    count += 1;
                }
                continue;
            }
            if let Some(kids) = children.get(&p.0) {
                stack.extend(kids.iter().copied());
            }
        }
        if count > 0 {
            out.insert(root, count);
        }
    }
    out
}

/// procs.annotate: `shell_processes` on every live local row, and
/// `background_tasks` on idle ones still waiting on a shell.
pub fn annotate(rows: &mut [Map<String, Value>]) {
    let live: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            crate::pyish::truthy(r.get("status"))
                && r.get("pid").is_some_and(|p| p.is_u64() || p.is_i64())
        })
        .map(|(i, _)| i)
        .collect();
    if live.is_empty() {
        return;
    }
    let pids: Vec<u32> = live
        .iter()
        .filter_map(|&i| rows[i]["pid"].as_u64().map(|p| p as u32))
        .collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let counts = waiting_shells(&pids, &process_table(), now);
    for &i in &live {
        let Some(pid) = rows[i]["pid"].as_u64() else {
            continue;
        };
        let n = counts.get(&(pid as u32)).copied().unwrap_or(0);
        rows[i].insert("shell_processes".into(), Value::from(n));
        if n > 0 && rows[i].get("status").and_then(Value::as_str) == Some("idle") {
            rows[i].insert("background_tasks".into(), Value::from(n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_topmost_shell_on_each_branch() {
        let t: Vec<Proc> = vec![
            (10, 1, "claude".into(), Some(0.0)),
            (11, 10, "bash".into(), Some(0.0)),
            (12, 11, "bash".into(), Some(0.0)), // a shell inside a shell
            (13, 10, "node".into(), Some(0.0)),
            (14, 13, "pwsh".into(), Some(0.0)),
            (15, 10, "bash".into(), Some(99.0)), // too young
        ];
        assert_eq!(waiting_shells(&[10], &t, 100.0).get(&10), Some(&2));
        assert!(waiting_shells(&[99], &t, 100.0).is_empty());
    }

    #[test]
    fn exe_names() {
        assert_eq!(exe(r"C:\Program Files\Git\bin\BASH.EXE"), "bash");
        assert_eq!(exe("pwsh"), "pwsh");
    }
}
