//! End a local session the way a person at its terminal would, from
//! `imperatorium/closing.py`: type Ctrl+C, `/exit` and Enter into its
//! console, and kill it only if it is still there afterwards. Killing alone
//! would leave its Windows Terminal tab open on "[process exited with code
//! 1]"; a clean exit closes the tab. A shell that is the tab's own is sent
//! `exit` too, once `claude` has gone.
//!
//! Typing means attaching to the session's console, and a process can only
//! be attached to one, so a short-lived child (`imperatorium type-into`)
//! does it.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TERMINALS: &[&str] = &["windowsterminal"];
const SHELLS: &[&str] = &["pwsh", "powershell", "cmd", "bash", "sh", "zsh", "fish"];
pub const CTRL_C: &str = "\u{3}";
pub const ENTER: &str = "\r";

fn alive(pid: u32) -> bool {
    crate::procs::process_table().iter().any(|p| p.0 == pid)
}

fn wait_gone(pid: u32, wait: f64) -> bool {
    let end = Instant::now() + Duration::from_secs_f64(wait);
    while Instant::now() < end {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    !alive(pid)
}

/// The shell `pid` was started from, when that shell is a terminal tab's own.
pub fn tab_shell(pid: u32, table: &[crate::procs::Proc]) -> Option<u32> {
    let find = |p: u32| table.iter().find(|r| r.0 == p);
    let parent = find(pid)?.1;
    let shell = find(parent)?;
    if !SHELLS.contains(&shell.2.as_str()) {
        return None;
    }
    let host = find(shell.1)?;
    TERMINALS.contains(&host.2.as_str()).then_some(parent)
}

/// closing._type: the child types `chunks` into `pid`'s console.
fn type_keys(pid: u32, chunks: &[&str]) -> bool {
    let Ok(me) = std::env::current_exe() else {
        return false;
    };
    let names: Vec<String> = chunks
        .iter()
        .map(|c| match *c {
            CTRL_C => "<ctrl-c>".to_string(),
            ENTER => "<enter>".to_string(),
            other => other.to_string(),
        })
        .collect();
    let mut cmd = std::process::Command::new(me);
    cmd.arg("type-into").arg(pid.to_string()).args(&names);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console of its own
    }
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
}

fn kill(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .status();
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

/// closing.close: {ok, how: exit | killed | gone, tab_closed}.
pub fn close(pid: &Value) -> Value {
    let Some(pid) = pid.as_u64().map(|p| p as u32) else {
        return json!({"ok": false, "error": "no process to close"});
    };
    if !alive(pid) {
        return json!({"ok": true, "how": "gone", "tab_closed": false});
    }
    #[cfg(windows)]
    {
        let shell = tab_shell(pid, &crate::procs::process_table());
        type_keys(pid, &[CTRL_C, "/exit", ENTER]);
        if wait_gone(pid, 8.0) {
            let mut closed = true;
            if let Some(s) = shell.filter(|s| alive(*s)) {
                closed = type_keys(s, &["exit", ENTER]) && wait_gone(s, 3.0);
            }
            return json!({"ok": true, "how": "exit", "tab_closed": closed});
        }
    }
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        if wait_gone(pid, 8.0) {
            return json!({"ok": true, "how": "exit", "tab_closed": false});
        }
    }
    kill(pid);
    if wait_gone(pid, 3.0) {
        return json!({"ok": true, "how": "killed", "tab_closed": false});
    }
    json!({"ok": false, "error": format!("it would not exit (pid {pid})")})
}

/// `imperatorium type-into PID CHUNK...`: attach to `pid`'s console and type
/// each chunk, 0.4 s apart. <ctrl-c> and <enter> name those keys.
#[cfg(windows)]
pub fn type_into_main(args: &[String]) -> i32 {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        AttachConsole, FreeConsole, WriteConsoleInputW, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT,
        KEY_EVENT_RECORD, KEY_EVENT_RECORD_0,
    };
    let Some(pid) = args.first().and_then(|p| p.parse::<u32>().ok()) else {
        return 1;
    };
    let chunks: Vec<String> = args[1..]
        .iter()
        .map(|a| match a.as_str() {
            "<ctrl-c>" => CTRL_C.to_string(),
            "<enter>" => ENTER.to_string(),
            other => other.to_string(),
        })
        .collect();
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return 2;
        }
        let name: Vec<u16> = "CONIN$\0".encode_utf16().collect();
        let conin = CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if conin.is_null() || conin == INVALID_HANDLE_VALUE {
            FreeConsole();
            return 3;
        }
        let mut code = 0;
        for (i, chunk) in chunks.iter().enumerate() {
            if i > 0 {
                std::thread::sleep(Duration::from_millis(400));
            }
            let mut records: Vec<INPUT_RECORD> = Vec::new();
            // One key event per UTF-16 unit: an emoji goes as its surrogate pair.
            for unit in chunk.encode_utf16() {
                let (vk, state) = match unit {
                    0x0D => (0x0D, 0),
                    0x03 => (0x43, 0x0008), // 'C', LEFT_CTRL_PRESSED
                    _ => (0, 0),
                };
                for down in [1, 0] {
                    records.push(INPUT_RECORD {
                        EventType: KEY_EVENT as u16,
                        Event: INPUT_RECORD_0 {
                            KeyEvent: KEY_EVENT_RECORD {
                                bKeyDown: down,
                                wRepeatCount: 1,
                                wVirtualKeyCode: vk,
                                wVirtualScanCode: 0,
                                uChar: KEY_EVENT_RECORD_0 { UnicodeChar: unit },
                                dwControlKeyState: state,
                            },
                        },
                    });
                }
            }
            let mut n = 0u32;
            if WriteConsoleInputW(conin, records.as_ptr(), records.len() as u32, &mut n) == 0 {
                code = 4;
                break;
            }
        }
        CloseHandle(conin);
        FreeConsole();
        code
    }
}

#[cfg(not(windows))]
pub fn type_into_main(_args: &[String]) -> i32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_shell_only_under_a_terminal() {
        let t = |rows: &[(u32, u32, &str)]| -> Vec<crate::procs::Proc> {
            rows.iter()
                .map(|(p, pp, e)| (*p, *pp, e.to_string(), None))
                .collect()
        };
        let tab = t(&[
            (30, 20, "claude"),
            (20, 10, "pwsh"),
            (10, 1, "windowsterminal"),
        ]);
        assert_eq!(tab_shell(30, &tab), Some(20));
        let ide = t(&[(30, 20, "claude"), (20, 10, "pwsh"), (10, 1, "code")]);
        assert_eq!(tab_shell(30, &ide), None);
        let direct = t(&[(30, 10, "claude"), (10, 1, "windowsterminal")]);
        assert_eq!(tab_shell(30, &direct), None);
    }
}
