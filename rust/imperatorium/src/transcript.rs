//! Finding and reading Claude Code transcripts (`projects/<folder>/<id>.jsonl`,
//! and `<id>/subagents/*.jsonl` beside them). The callers keep their own
//! policies, each as Python has it: `sessions` takes the first folder that
//! holds a non-empty file (strict UTF-8), `sessionstat` the newest by mtime,
//! `costs` walks everything, `timeline` and `taskout` decode lossily.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Map, Value};

use crate::pyish::strip;

/// `<root>/<folder>` for each entry of the projects folder, in directory
/// order (files included; callers filter as they need).
pub fn folders(root: &Path) -> Vec<PathBuf> {
    match fs::read_dir(root) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(_) => vec![],
    }
}

/// `_read_session_file`: the first project folder (directory order) holding a
/// readable, non-empty `<sid>.jsonl`, with its bytes.
pub fn read_first(root: &Path, sid: &str) -> Option<(PathBuf, Vec<u8>)> {
    let name = format!("{sid}.jsonl");
    for dir in folders(root) {
        let path = dir.join(&name);
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }
        return Some((path, bytes));
    }
    None
}

/// sessionstat's look: the newest (by mtime; the first of equals) of
/// `<root>/*/<sid>.jsonl`.
pub fn locate_newest(root: &Path, sid: &str) -> Option<PathBuf> {
    let mut best: Option<(PathBuf, SystemTime)> = None;
    for dir in fs::read_dir(root).ok()?.flatten() {
        let p = dir.path().join(format!("{sid}.jsonl"));
        let Ok(m) = fs::metadata(&p).and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|b| m > b.1) {
            best = Some((p, m));
        }
    }
    best.map(|b| b.0)
}

/// Every transcript under `root`: each folder's `<id>.jsonl` (session `id`)
/// and each `<id>/subagents/*.jsonl` (session `id` too).
pub fn list_files(root: &Path) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let Ok(rd) = fs::read_dir(root) else {
        return found;
    };
    for folder in rd.flatten().filter(|e| e.path().is_dir()) {
        let Ok(entries) = fs::read_dir(folder.path()) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let p = e.path();
            if p.is_file() && name.ends_with(".jsonl") {
                let id = name[..name.len() - 6].to_string();
                found.push((p, id));
            } else if p.is_dir() {
                if let Ok(subs) = fs::read_dir(p.join("subagents")) {
                    for s in subs.flatten() {
                        let sname = s.file_name().to_string_lossy().into_owned();
                        if s.path().is_file() && sname.ends_with(".jsonl") {
                            found.push((s.path(), name.clone()));
                        }
                    }
                }
            }
        }
    }
    found
}

/// The first `n` bytes of `f` (fewer for a shorter file); leaves the cursor
/// after them.
pub fn read_head(f: &mut File, n: usize) -> io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(n);
    f.take(n as u64).read_to_end(&mut head)?;
    Ok(head)
}

/// Everything from byte `offset` on (nothing, for an offset past the end).
pub fn read_from(f: &mut File, offset: u64) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(offset))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;
    Ok(data)
}

/// `read_from` on a path; None when it cannot be opened or read.
pub fn read_delta(path: &Path, offset: u64) -> Option<Vec<u8>> {
    read_from(&mut File::open(path).ok()?, offset).ok()
}

/// How many leading bytes of `data` end in a newline: the part that is whole
/// lines (a record still being written is left for the next read).
pub fn whole_lines(data: &[u8]) -> usize {
    data.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1)
}

/// Python's `read_text` newline translation: "\r\n" and a lone "\r" become
/// "\n". Neither occurs raw inside a JSON string, so it only splits records.
pub fn normalise_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// How `objects` cuts text into records.
#[derive(Clone, Copy)]
pub enum Split {
    /// At "\n", each line stripped as Python's `str.strip` (sessionstat,
    /// sessions).
    Newline,
    /// `str::lines` (also drops a trailing "\r") and Rust's `trim`
    /// (timeline).
    Lines,
}

/// Every line that is a JSON object, in order; anything else is skipped.
pub fn objects(text: &str, split: Split) -> Vec<Map<String, Value>> {
    let one = |l: &str| match serde_json::from_str::<Value>(l) {
        Ok(Value::Object(o)) => Some(o),
        _ => None,
    };
    match split {
        Split::Newline => text
            .split('\n')
            .map(strip)
            .filter(|l| !l.is_empty())
            .filter_map(one)
            .collect(),
        Split::Lines => text.lines().filter_map(|l| one(l.trim())).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_lines_stops_at_the_last_newline() {
        assert_eq!(whole_lines(b"a\nb\nc"), 4);
        assert_eq!(whole_lines(b"abc"), 0);
        assert_eq!(whole_lines(b""), 0);
    }

    #[test]
    fn objects_skips_what_is_not_an_object() {
        let t = "{\"a\":1}\n\n[1]\nnope\r\n {\"b\":2} \n";
        assert_eq!(objects(t, Split::Newline).len(), 2);
        assert_eq!(objects(t, Split::Lines).len(), 2);
    }

    #[test]
    fn newlines_are_translated() {
        assert_eq!(normalise_newlines("a\r\nb\rc"), "a\nb\nc");
    }
}
