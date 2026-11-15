//! Guards for tests that make the dispatcher launch things: a launch must
//! reach fakecli, never a real Windows Terminal, Claude Code or deep link.

use std::path::{Path, PathBuf};

/// launch.rs `which` (shutil.which): the first `name`, with each PATHEXT
/// extension, on `path`.
pub fn which_on(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        vec![String::new()]
    };
    let has_ext = exts
        .iter()
        .any(|e| !e.is_empty() && name.to_lowercase().ends_with(&e.to_lowercase()));
    for dir in std::env::split_paths(path) {
        let names: Vec<String> = if has_ext || !cfg!(windows) {
            vec![name.to_string()]
        } else {
            exts.iter().map(|e| format!("{name}{e}")).collect()
        };
        for n in names {
            if dir.join(&n).is_file() {
                return Some(dir.join(n));
            }
        }
    }
    None
}

fn same_dir(p: &Path, dir: &Path) -> bool {
    let canon = |x: &Path| std::fs::canonicalize(x).unwrap_or_else(|_| x.to_path_buf());
    p.parent().map(canon) == Some(canon(dir))
}

/// Panics (before anything is launched) unless every way the dispatcher
/// finds `wt` and `claude` ends in `bin_dir`'s fakes: `which` on the PATH it
/// was given (harness clean_env: the stand-in PATH), each PATH folder free
/// of any other wt/claude, and the folders std's Command searches before
/// PATH (the binary's own, System32, Windows) holding neither.
pub fn assert_launches_are_fakes(bin_dir: &Path) {
    let path = super::stand_in_path(Some(bin_dir));
    let fake = std::fs::read(super::fakecli()).expect("fakecli");
    for name in ["wt", "wt.exe", "claude", "claude.exe"] {
        let found = which_on(name, &path)
            .unwrap_or_else(|| panic!("SAFETY: `{name}` is not found on the dispatcher's PATH"));
        assert!(
            same_dir(&found, bin_dir),
            "SAFETY: `{name}` would launch {} - not the fake in {}",
            found.display(),
            bin_dir.display()
        );
        assert_eq!(
            std::fs::read(&found).ok().as_deref(),
            Some(fake.as_slice()),
            "SAFETY: {} is not fakecli",
            found.display()
        );
    }
    let real = ["wt.exe", "claude.exe", "wt.cmd", "claude.cmd", "wt.bat", "claude.bat"];
    for dir in std::env::split_paths(&path).skip(1) {
        for n in real {
            assert!(
                !dir.join(n).is_file(),
                "SAFETY: a real {n} on the dispatcher's PATH: {}",
                dir.display()
            );
        }
    }
    let mut first: Vec<PathBuf> = vec![super::bin().parent().unwrap().to_path_buf()];
    if let Some(w) = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("windir")) {
        let w = PathBuf::from(w);
        first.push(w.join("System32"));
        first.push(w.clone());
    }
    for dir in first {
        for n in ["wt.exe", "claude.exe"] {
            assert!(
                !dir.join(n).is_file(),
                "SAFETY: {} would be found before PATH",
                dir.join(n).display()
            );
        }
    }
    // claude_path()'s fallback when `claude` is not on PATH: it is, so it
    // is never reached; but the fallback folder must not be on PATH either.
    let local_bin = super::home_dir().join(".local").join("bin");
    assert!(
        !std::env::split_paths(&path).any(|d| same_dir(&d.join("x"), &local_bin)),
        "SAFETY: {} is on the dispatcher's PATH",
        local_bin.display()
    );
}

/// The deep links the dispatcher would have opened
/// (IMPERATORIUM_TEST_DEEPLINK_LOG): must stay empty.
pub fn deep_links(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}
