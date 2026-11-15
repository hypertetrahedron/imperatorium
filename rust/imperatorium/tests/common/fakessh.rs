//! The fake ssh (fakecli copied to `ssh.exe`, named by IMPERATORIUM_SSH):
//! a folder of its own with the program and its log, everything running
//! from it stopped on drop (a tunnel's `ssh -N` stays until killed).

use serde_json::Value;
use std::path::PathBuf;

pub struct FakeSsh {
    pub dir: super::TempDir,
    /// What IMPERATORIUM_SSH names.
    pub exe: PathBuf,
    /// FAKESSH_DIR: log.jsonl, sha256.
    pub data: PathBuf,
}

impl FakeSsh {
    pub fn new() -> FakeSsh {
        let dir = super::TempDir::new("fakessh");
        let bin = dir.join("bin");
        let data = dir.join("data");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let exe = bin.join(if cfg!(windows) { "ssh.exe" } else { "ssh" });
        std::fs::copy(super::fakecli(), &exe).unwrap();
        FakeSsh { dir, exe, data }
    }

    /// The environment that points a process at it: IMPERATORIUM_SSH and
    /// FAKESSH_DIR (`mode` "sh": FAKESSH_MODE, the remote command run by sh).
    pub fn env(&self, mode: Option<&str>) -> Vec<(String, String)> {
        let mut e = vec![
            ("IMPERATORIUM_SSH".to_string(), self.exe.to_string_lossy().into_owned()),
            ("FAKESSH_DIR".to_string(), self.data.to_string_lossy().into_owned()),
        ];
        if let Some(m) = mode {
            e.push(("FAKESSH_MODE".into(), m.into()));
        }
        e
    }

    /// Every call so far, as fakessh logs it ({host, cmd, stdin} or
    /// {host, tunnel}).
    pub fn calls(&self) -> Vec<Value> {
        std::fs::read_to_string(self.data.join("log.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

impl Drop for FakeSsh {
    fn drop(&mut self) {
        super::stop_under(&self.dir.join("bin"));
    }
}
