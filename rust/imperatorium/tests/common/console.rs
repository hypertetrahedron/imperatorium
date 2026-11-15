//! Hidden fake Claude Code consoles (fakecli `--fake-console`), for the
//! tests that close sessions and type into them: each in a console of its
//! own (CREATE_NEW_CONSOLE) started hidden (SW_HIDE, so Windows does not
//! hand it to Windows Terminal as a new tab), killed with its tree on drop -
//! a panic included.

use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};

/// One console process. Dropping it kills it (and its tree) if it is still
/// running.
pub struct Console {
    pub pid: u32,
    #[cfg(windows)]
    handle: isize,
}

/// The environment a console gets: this one minus STRIPPED, PATH the
/// stand-in one (`bin_dir` first), plus `extra`.
pub fn console_env(bin_dir: &Path, extra: &[(&str, &std::ffi::OsStr)]) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(k, _)| {
            let key = k.to_string_lossy().to_uppercase();
            key != "PATH" && !super::STRIPPED.iter().any(|p| key.starts_with(p))
        })
        .collect();
    env.push(("PATH".into(), super::stand_in_path(Some(bin_dir))));
    for (k, v) in extra {
        env.retain(|(a, _)| !a.to_string_lossy().eq_ignore_ascii_case(k));
        env.push((k.into(), v.into()));
    }
    env
}

#[cfg(windows)]
impl Console {
    /// `exe args...`, hidden, in a new console, with exactly `env`.
    pub fn start(exe: &Path, args: &[&str], env: &[(OsString, OsString)], cwd: &Path) -> Console {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            CreateProcessW, CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
            STARTF_USESHOWWINDOW, STARTUPINFOW,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

        let wide = |s: &std::ffi::OsStr| s.encode_wide().collect::<Vec<u16>>();
        let mut line: Vec<u16> = Vec::new();
        line.push('"' as u16);
        line.extend(wide(exe.as_os_str()));
        line.push('"' as u16);
        for a in args {
            assert!(!a.contains([' ', '"']), "a plain argument: {a}");
            line.push(' ' as u16);
            line.extend(a.encode_utf16());
        }
        line.push(0);
        // The block: KEY=VALUE\0 ... \0, sorted as Windows expects.
        let mut pairs: Vec<&(OsString, OsString)> = env.iter().collect();
        pairs.sort_by_key(|(k, _)| k.to_string_lossy().to_uppercase());
        let mut block: Vec<u16> = Vec::new();
        for (k, v) in pairs {
            block.extend(wide(k));
            block.push('=' as u16);
            block.extend(wide(v));
            block.push(0);
        }
        block.push(0);
        let mut dir = wide(cwd.as_os_str());
        dir.push(0);
        let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = SW_HIDE as u16;
        let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            CreateProcessW(
                std::ptr::null(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
                block.as_ptr().cast(),
                dir.as_ptr(),
                &si,
                &mut pi,
            )
        };
        assert!(
            ok != 0,
            "cannot start {}: {}",
            exe.display(),
            std::io::Error::last_os_error()
        );
        unsafe { CloseHandle(pi.hThread) };
        Console {
            pid: pi.dwProcessId,
            handle: pi.hProcess as isize,
        }
    }

    pub fn running(&self) -> bool {
        use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        unsafe { WaitForSingleObject(self.handle as _, 0) == WAIT_TIMEOUT }
    }

    /// True once it has exited, false if `limit` passed first.
    pub fn wait_exit(&self, limit: Duration) -> bool {
        let end = Instant::now() + limit;
        while self.running() {
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    pub fn kill(&self) {
        if !self.running() {
            return;
        }
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &self.pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        unsafe {
            windows_sys::Win32::System::Threading::TerminateProcess(self.handle as _, 1);
        }
        self.wait_exit(Duration::from_secs(5));
    }
}

#[cfg(windows)]
impl Drop for Console {
    fn drop(&mut self) {
        self.kill();
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle as _) };
    }
}
