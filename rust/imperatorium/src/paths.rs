//! Where things live, and Python's `os.path` where the policy depends on it.

use std::env;
use std::path::PathBuf;

/// The value of an environment variable, treating an empty one as unset
/// (Python's `os.environ.get(X) or ...`).
pub fn env_nonempty(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.is_empty())
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
        let norm = normpath(p);
        // _getfullpathname raises on NUL (ValueError) and on names Windows
        // rejects, such as " " (OSError); Python then joins lexically.
        let full = if norm.contains('\0') {
            None
        } else {
            full_path(&norm)
        };
        let full = match full {
            Some(f) => f,
            None => abspath_fallback(p)?,
        };
        lower(&full.replace('/', "\\"))
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
