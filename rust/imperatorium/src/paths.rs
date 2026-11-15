//! Where things live, and Python's `os.path` where the policy depends on it.

use std::env;
use std::io;
use std::path::{Path, PathBuf};

/// The value of an environment variable, treating an empty one as unset
/// (Python's `os.environ.get(X) or ...`).
pub fn env_nonempty(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.is_empty())
}

/// The user's home as the OS names it (USERPROFILE on Windows, HOME
/// elsewhere); empty when unset, like `os.environ.get(...) or ""`.
pub fn user_home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    PathBuf::from(env::var(var).unwrap_or_default())
}

/// Claude Code's config folder: CLAUDE_CONFIG_DIR, else ~/.claude (Python's
/// `registry`, `costs` and `sessionstat` do exactly this, no normalisation).
pub fn claude_home() -> PathBuf {
    match env_nonempty("CLAUDE_CONFIG_DIR") {
        Some(d) => PathBuf::from(d),
        None => user_home().join(".claude"),
    }
}

/// `_get_projects_dir()` of the Agent SDK: the config folder, NFC-normalised
/// (as the SDK does, so transcript folder names match), then `projects`.
/// Only this one is normalised; `claude_home()` stays as given.
pub fn projects_dir() -> PathBuf {
    use unicode_normalization::UnicodeNormalization;
    let base: String = claude_home().to_string_lossy().nfc().collect();
    PathBuf::from(base).join("projects")
}

/// Write `bytes` to `path` through a temporary file beside it and a rename,
/// so a reader never sees half a file. The rename is retried (Windows
/// refuses while a reader has the target open); the temporary file is
/// removed on any failure and the last error is returned.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(
        ".{name}.{}-{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let mut last = None;
    for attempt in 0..5 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last.expect("five failed renames"))
}

fn home_dir() -> PathBuf {
    #[cfg(windows)]
    let found = env_nonempty("USERPROFILE");
    #[cfg(not(windows))]
    let found = env_nonempty("HOME");
    found
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The dispatcher's state folder, as `cc_permission.state_dir` decides it.
pub fn state_dir() -> PathBuf {
    let base = env_nonempty("LOCALAPPDATA")
        .or_else(|| env_nonempty("XDG_STATE_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local").join("state"));
    let new = base.join("imperatorium");
    let old = base.join("ccontrol");
    // The dispatcher moves the pre-2026-10-20 folder on start; until it
    // has, the audit log goes where the rest of the state is.
    if !new.exists() && old.is_dir() {
        old
    } else {
        new
    }
}

/// Where config.json and policy.json live: `--home`, then
/// IMPERATORIUM_HOME, then the platform's config folder. During the port the
/// installer passes `--home <checkout>`, so both implementations read the
/// same files.
pub fn config_home(arg: Option<&str>) -> PathBuf {
    if let Some(a) = arg.filter(|a| !a.is_empty()) {
        return PathBuf::from(a);
    }
    if let Some(h) = env_nonempty("IMPERATORIUM_HOME") {
        return PathBuf::from(h);
    }
    #[cfg(windows)]
    {
        if let Some(a) = env_nonempty("APPDATA") {
            return PathBuf::from(a).join("imperatorium");
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(x) = env_nonempty("XDG_CONFIG_HOME") {
            return PathBuf::from(x).join("imperatorium");
        }
    }
    home_dir().join(".config").join("imperatorium")
}

pub fn policy_path(home_arg: Option<&str>) -> PathBuf {
    match env_nonempty("IMPERATORIUM_POLICY") {
        Some(p) => PathBuf::from(p),
        None => config_home(home_arg).join("policy.json"),
    }
}

pub const SEP: &str = if cfg!(windows) { "\\" } else { "/" };

/// `os.path.normcase(os.path.abspath(p))`, or None where Python would raise.
pub fn abs_normcase(p: &str) -> Option<String> {
    imp::abs_normcase(p)
}

/// `os.path.normpath(p)`.
pub fn normpath(p: &str) -> String {
    imp::normpath(p)
}

/// `os.path.abspath(p)`, or None where Python would raise.
pub fn abspath(p: &str) -> Option<String> {
    imp::abspath(p)
}

/// `os.path.basename(p)`.
pub fn basename(p: &str) -> &str {
    #[cfg(windows)]
    {
        // ntpath: drop a drive, then split on either separator.
        let rest = if p.len() >= 2 && p.as_bytes()[1] == b':' {
            &p[2..]
        } else {
            p
        };
        rest.rsplit(['/', '\\']).next().unwrap_or("")
    }
    #[cfg(not(windows))]
    {
        p.rsplit('/').next().unwrap_or("")
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Globalization::{LCMapStringEx, LCMAP_LOWERCASE};
    use windows_sys::Win32::Storage::FileSystem::GetFullPathNameW;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// ntpath.abspath (GetFullPathNameW, which also folds `.`/`..`, both
    /// separators and trailing dots) then ntpath.normcase (an invariant-
    /// locale LCMapStringEx lowercase), the same calls Python makes.
    pub fn abs_normcase(p: &str) -> Option<String> {
        lower(&abspath(p)?.replace('/', "\\"))
    }

    /// ntpath.abspath: normpath, then GetFullPathNameW, else the lexical
    /// fallback Python uses when that call refuses the name.
    pub fn abspath(p: &str) -> Option<String> {
        let norm = normpath(p);
        // _getfullpathname raises on NUL (ValueError) and on names Windows
        // rejects, such as " " (OSError); Python then joins lexically.
        let full = if norm.contains('\0') {
            None
        } else {
            full_path(&norm)
        };
        match full {
            Some(f) => Some(f),
            None => abspath_fallback(p),
        }
    }

    /// ntpath._abspath_fallback: join to the cwd unless isabs, then normpath.
    fn abspath_fallback(p: &str) -> Option<String> {
        let head: String = p.chars().take(3).collect::<String>().replace('/', r"\");
        let isabs = head.starts_with('\\') || head.get(1..).is_some_and(|h| h.starts_with(r":\"));
        if isabs {
            return Some(normpath(p));
        }
        let cwd = std::env::current_dir().ok()?;
        let cwd = cwd.to_str()?;
        let joined = if cwd.ends_with(['\\', '/']) {
            format!("{cwd}{p}")
        } else {
            format!(r"{cwd}\{p}")
        };
        Some(normpath(&joined))
    }

    /// ntpath.splitroot's drive: `C:`, or `\server\share` for UNC.
    fn split_drive(p: &str) -> (&str, &str) {
        let b = p.as_bytes();
        if b.len() >= 2 && b[1] == b':' {
            return p.split_at(2);
        }
        if p.starts_with(r"\\") && !p.starts_with(r"\\?\") && !p.starts_with(r"\\.\") {
            // \\server\share: the drive runs to the separator after share.
            let rest = &p[2..];
            let server_end = rest.find('\\').map(|i| i + 2);
            if let Some(se) = server_end {
                let share_end = p[se + 1..]
                    .find('\\')
                    .map(|i| i + se + 1)
                    .unwrap_or(p.len());
                return p.split_at(share_end);
            }
            return (p, "");
        }
        ("", p)
    }

    /// ntpath.normpath: one separator, `.` dropped, `..` folded, no
    /// trailing separator. Device paths (`\\?\`, `\\.\`) are left as they are.
    pub fn normpath(p: &str) -> String {
        let p = p.replace('/', r"\");
        if p.starts_with(r"\\?\") || p.starts_with(r"\\.\") {
            return p;
        }
        let (drive, rest) = split_drive(&p);
        let root = if rest.starts_with('\\') { r"\" } else { "" };
        let mut parts: Vec<&str> = Vec::new();
        for comp in rest.split('\\') {
            if comp.is_empty() || comp == "." {
                continue;
            }
            if comp == ".." {
                match parts.last() {
                    Some(&last) if last != ".." => {
                        parts.pop();
                    }
                    _ if !root.is_empty() => {}
                    _ => parts.push(comp),
                }
            } else {
                parts.push(comp);
            }
        }
        let out = format!("{drive}{root}{}", parts.join(r"\"));
        if out.is_empty() {
            ".".into()
        } else {
            out
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn normpath_matches_python() {
            use super::normpath;
            assert_eq!(normpath(r"Y:\a\b\"), r"Y:\a\b");
            assert_eq!(normpath("C:/a//b/./c/../d"), r"C:\a\b\d");
            assert_eq!(normpath(r"C:\.."), r"C:\");
            assert_eq!(normpath(r"..\..\x"), r"..\..\x");
            assert_eq!(normpath(r"C:x\..\.."), "C:..");
            assert_eq!(normpath(r"\\srv\share\a\..\b\"), r"\\srv\share\b");
            assert_eq!(normpath(""), ".");
        }
    }

    fn full_path(p: &str) -> Option<String> {
        let src = wide(p);
        let mut buf: Vec<u16> = vec![0; 512];
        loop {
            let n = unsafe {
                GetFullPathNameW(
                    src.as_ptr(),
                    buf.len() as u32,
                    buf.as_mut_ptr(),
                    std::ptr::null_mut(),
                )
            } as usize;
            if n == 0 {
                return None;
            }
            if n < buf.len() {
                return Some(String::from_utf16_lossy(&buf[..n]));
            }
            buf.resize(n + 1, 0);
        }
    }

    fn lower(s: &str) -> Option<String> {
        let src: Vec<u16> = s.encode_utf16().collect();
        if src.is_empty() {
            return Some(String::new());
        }
        let locale = wide(""); // LOCALE_NAME_INVARIANT
        let call = |dst: *mut u16, cap: i32| unsafe {
            LCMapStringEx(
                locale.as_ptr(),
                LCMAP_LOWERCASE,
                src.as_ptr(),
                src.len() as i32,
                dst,
                cap,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
        let need = call(std::ptr::null_mut(), 0);
        if need <= 0 {
            return None;
        }
        let mut out = vec![0u16; need as usize];
        let got = call(out.as_mut_ptr(), need);
        if got <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&out[..got as usize]))
    }
}

#[cfg(not(windows))]
mod imp {
    /// posixpath.abspath: join to the cwd, then normpath. Lexical, like
    /// Python: symlinks are not resolved. normcase is the identity.
    pub fn abs_normcase(p: &str) -> Option<String> {
        abspath(p)
    }

    pub fn abspath(p: &str) -> Option<String> {
        let joined = if p.starts_with('/') {
            p.to_string()
        } else {
            let cwd = std::env::current_dir().ok()?;
            format!("{}/{}", cwd.to_str()?, p)
        };
        Some(normpath(&joined))
    }

    pub fn normpath(p: &str) -> String {
        if p.is_empty() {
            return ".".into();
        }
        let initial = if p.starts_with("//") && !p.starts_with("///") {
            "//"
        } else if p.starts_with('/') {
            "/"
        } else {
            ""
        };
        let mut parts: Vec<&str> = Vec::new();
        for comp in p.split('/') {
            if comp.is_empty() || comp == "." {
                continue;
            }
            if comp != ".."
                || (initial.is_empty() && parts.is_empty())
                || parts.last() == Some(&"..")
            {
                parts.push(comp);
            } else if !parts.is_empty() {
                parts.pop();
            }
        }
        let body = parts.join("/");
        let out = format!("{initial}{body}");
        if out.is_empty() {
            ".".into()
        } else {
            out
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn normpath_matches_python() {
            use super::normpath;
            assert_eq!(normpath("/a/b/../c/./d"), "/a/c/d");
            assert_eq!(normpath("//a//b"), "//a/b");
            assert_eq!(normpath("///a"), "/a");
            assert_eq!(normpath("/.."), "/");
            assert_eq!(normpath("a/../../b"), "../b");
        }
    }
}

/// `time.strftime("%Y-%m-%d %H:%M:%S")` in local time.
pub fn local_stamp() -> String {
    #[cfg(windows)]
    {
        let mut t = unsafe { std::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        )
    }
    #[cfg(unix)]
    {
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
        unsafe { libc::localtime_r(&now, &mut tm) };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

/// The local calendar date of a Unix time, as Python's
/// `datetime.fromtimestamp(t).date()` / `astimezone()` gives it:
/// (year, month, day).
pub fn local_date(t: f64) -> (i64, u32, u32) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows_sys::Win32::System::Time::{
            FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime,
        };
        let ticks = ((t + 11_644_473_600.0) * 1e7) as u64;
        let ft = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        unsafe {
            let mut utc: SYSTEMTIME = std::mem::zeroed();
            let mut local: SYSTEMTIME = std::mem::zeroed();
            if FileTimeToSystemTime(&ft, &mut utc) == 0
                || SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0
            {
                return civil(t.div_euclid(86400.0) as i64);
            }
            (local.wYear as i64, local.wMonth as u32, local.wDay as u32)
        }
    }
    #[cfg(unix)]
    {
        let secs = t.floor() as libc::time_t;
        let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
        unsafe { libc::localtime_r(&secs, &mut tm) };
        (
            tm.tm_year as i64 + 1900,
            tm.tm_mon as u32 + 1,
            tm.tm_mday as u32,
        )
    }
}

/// The civil date of a day number (days since 1970-01-01).
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The day number of a civil date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let (y, m) = if m <= 2 {
        (y - 1, m as i64 + 9)
    } else {
        (y, m as i64 - 3)
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn ymd(d: (i64, u32, u32)) -> String {
    format!("{:04}-{:02}-{:02}", d.0, d.1, d.2)
}

/// `datetime.fromtimestamp(now).replace(day=1, hour=0, minute=0, second=0,
/// microsecond=0).timestamp()`: local midnight on the 1st of this month.
pub fn local_month_start(now: f64) -> f64 {
    let (y, m, _) = local_date(now);
    local_to_epoch(y, m, 1, 0, 0, 0)
}

/// `datetime(y, m, d, h, mi, s).timestamp()` for a naive (local) time.
pub fn local_to_epoch(y: i64, m: u32, d: u32, h: u32, mi: u32, sec: u32) -> f64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows_sys::Win32::System::Time::{
            SystemTimeToFileTime, TzSpecificLocalTimeToSystemTime,
        };
        let local = SYSTEMTIME {
            wYear: y as u16,
            wMonth: m as u16,
            wDayOfWeek: 0,
            wDay: d as u16,
            wHour: h as u16,
            wMinute: mi as u16,
            wSecond: sec as u16,
            wMilliseconds: 0,
        };
        unsafe {
            let mut utc: SYSTEMTIME = std::mem::zeroed();
            let mut ft: FILETIME = std::mem::zeroed();
            if TzSpecificLocalTimeToSystemTime(std::ptr::null(), &local, &mut utc) != 0
                && SystemTimeToFileTime(&utc, &mut ft) != 0
            {
                let ticks = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
                return ticks as f64 / 1e7 - 11_644_473_600.0;
            }
        }
        (days_from_civil(y, m, d) * 86400 + (h * 3600 + mi * 60 + sec) as i64) as f64
    }
    #[cfg(unix)]
    {
        let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
        tm.tm_year = (y - 1900) as i32;
        tm.tm_mon = m as i32 - 1;
        tm.tm_mday = d as i32;
        tm.tm_hour = h as i32;
        tm.tm_min = mi as i32;
        tm.tm_sec = sec as i32;
        tm.tm_isdst = -1;
        unsafe { libc::mktime(&mut tm) as f64 }
    }
}

/// `time.strftime("%H:%M", time.localtime(t))`.
pub fn local_hm(t: f64) -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows_sys::Win32::System::Time::{
            FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime,
        };
        let ticks = ((t + 11_644_473_600.0) * 1e7) as u64;
        let ft = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        unsafe {
            let mut utc: SYSTEMTIME = std::mem::zeroed();
            let mut local: SYSTEMTIME = std::mem::zeroed();
            if FileTimeToSystemTime(&ft, &mut utc) != 0
                && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) != 0
            {
                return format!("{:02}:{:02}", local.wHour, local.wMinute);
            }
        }
        String::new()
    }
    #[cfg(unix)]
    {
        let secs = t.floor() as libc::time_t;
        let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
        unsafe { libc::localtime_r(&secs, &mut tm) };
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

#[cfg(test)]
mod atomic_tests {
    use super::*;

    #[test]
    fn replaces_and_leaves_no_temp() {
        let d = std::env::temp_dir().join(format!("imp-wa-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("x.json");
        write_atomic(&f, b"one").unwrap();
        write_atomic(&f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn failure_leaves_no_temp() {
        let d = std::env::temp_dir().join(format!("imp-wb-{}", std::process::id()));
        std::fs::create_dir_all(d.join("target")).unwrap();
        // The target is a directory: the rename fails.
        assert!(write_atomic(&d.join("target"), b"x").is_err());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
