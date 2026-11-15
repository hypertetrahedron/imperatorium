//! The state folder's format version (docs/releasing.md, section 9).
//!
//! `state_version` in the state folder says which layout the files in it
//! have: `{"version": N, "by": "<the imperatorium version that wrote it>"}`.
//! The live dispatcher checks it before anything else reads the folder:
//!
//! * the same version: nothing to do;
//! * older (or no file: every folder from before this existed is version
//!   1): the folder is copied to a dated backup beside it, then migrated
//!   forward one version at a time;
//! * newer: this binary is older than the one that last ran, and it stops
//!   rather than misread or overwrite files it does not understand. That is
//!   what makes `imperatorium update --rollback` safe.
//!
//! Bump STATE_VERSION, and add the step to MIGRATIONS, only when a change
//! means an older binary would misread the folder. That is a breaking
//! change: a minor version while at 0.x, with a line in CHANGELOG.md.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const STATE_VERSION: u64 = 1;
const FILE: &str = "state_version";

/// One step: MIGRATIONS[i] takes a folder from version i+1 to i+2.
type Migration = fn(&Path) -> Result<(), String>;
const MIGRATIONS: &[Migration] = &[];

/// Check (and if need be migrate) the live state folder.
pub fn ensure(dir: &Path) -> Result<(), String> {
    ensure_at(dir, STATE_VERSION, MIGRATIONS, crate::update::version())
}

fn read(dir: &Path) -> Result<(u64, String), String> {
    let p = dir.join(FILE);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((1, String::new())),
        Err(e) => return Err(format!("{}: {e}", p.display())),
    };
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
    let n = v.get("version").and_then(Value::as_u64).ok_or(format!("{}: no version", p.display()))?;
    Ok((n, v.get("by").and_then(Value::as_str).unwrap_or("").to_string()))
}

fn write(dir: &Path, n: u64, by: &str) -> Result<(), String> {
    let p = dir.join(FILE);
    std::fs::write(&p, json!({"version": n, "by": by}).to_string() + "\n").map_err(|e| format!("{}: {e}", p.display()))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let t = e.file_type()?;
        let dst = to.join(e.file_name());
        if t.is_dir() {
            copy_dir(&e.path(), &dst)?;
        } else if t.is_file() {
            std::fs::copy(e.path(), dst)?;
        }
    }
    Ok(())
}

fn backup_path(dir: &Path, found: u64) -> PathBuf {
    let stamp = chrono_stamp();
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "imperatorium".into());
    dir.with_file_name(format!("{name}-backup-state-v{found}-{stamp}"))
}

fn chrono_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    secs.to_string()
}

fn ensure_at(dir: &Path, current: u64, migrations: &[Migration], me: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (found, by) = read(dir)?;
    if found > current {
        let who = if by.is_empty() { "a newer Imperatorium".to_string() } else { format!("Imperatorium {by}") };
        return Err(format!(
            "the state folder {} was written by {who} (state version {found}); this is {me}, which knows up to \
             version {current}. Install {} again (`imperatorium update --version {}`), or restore the \
             backup a migration left beside the folder.",
            dir.display(),
            if by.is_empty() { "the newer version" } else { by.as_str() },
            if by.is_empty() { "<version>" } else { by.as_str() },
        ));
    }
    if found < current {
        let backup = backup_path(dir, found);
        copy_dir(dir, &backup).map_err(|e| format!("backing up {} to {}: {e}", dir.display(), backup.display()))?;
        eprintln!("state folder: version {found} -> {current}; backup in {}", backup.display());
        for v in found..current {
            let step = migrations
                .get((v - 1) as usize)
                .ok_or(format!("no migration from state version {v} to {}", v + 1))?;
            step(dir).map_err(|e| format!("migrating the state folder from version {v}: {e} (backup in {})", backup.display()))?;
            write(dir, v + 1, me)?;
        }
        return Ok(());
    }
    if !dir.join(FILE).is_file() {
        write(dir, current, me)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("imp-statever-{tag}-{}", std::process::id())).join("imperatorium");
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_folder_without_the_file_is_version_1_and_gets_one() {
        let d = folder("new");
        std::fs::write(d.join("state.json"), "{}").unwrap();
        ensure_at(&d, 1, &[], "0.4.0").unwrap();
        assert_eq!(read(&d).unwrap(), (1, "0.4.0".into()));
        // Again: untouched (the writer stays the first one).
        ensure_at(&d, 1, &[], "0.4.1").unwrap();
        assert_eq!(read(&d).unwrap(), (1, "0.4.0".into()));
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn a_newer_folder_is_refused_and_left_alone() {
        let d = folder("newer");
        write(&d, 3, "0.6.0").unwrap();
        let e = ensure_at(&d, 2, &[], "0.5.0").unwrap_err();
        assert!(e.contains("Imperatorium 0.6.0") && e.contains("--version 0.6.0"), "{e}");
        assert_eq!(read(&d).unwrap(), (3, "0.6.0".into()));
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    fn rename_a_to_b(dir: &Path) -> Result<(), String> {
        std::fs::rename(dir.join("a.json"), dir.join("b.json")).map_err(|e| e.to_string())
    }

    #[test]
    fn an_older_folder_is_backed_up_then_migrated() {
        let d = folder("older");
        std::fs::write(d.join("a.json"), "1").unwrap();
        std::fs::create_dir_all(d.join("mod")).unwrap();
        std::fs::write(d.join("mod").join("x"), "y").unwrap();
        ensure_at(&d, 2, &[rename_a_to_b], "0.5.0").unwrap();
        assert_eq!(read(&d).unwrap(), (2, "0.5.0".into()));
        assert!(d.join("b.json").is_file() && !d.join("a.json").exists());
        let backups: Vec<PathBuf> = std::fs::read_dir(d.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("imperatorium-backup-state-v1-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(backups[0].join("a.json").is_file() && backups[0].join("mod").join("x").is_file());
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn a_missing_step_is_an_error() {
        let d = folder("gap");
        assert!(ensure_at(&d, 2, &[], "0.5.0").unwrap_err().contains("no migration"));
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }
}
