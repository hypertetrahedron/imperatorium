//! Ported from remote_cli_twin.py (at `last-python`): `imperatorium remote add | remove | check` and
//! `check`, end to end against the fake ssh in its sh mode (fakecli as
//! ssh.exe, FAKESSH_MODE=sh): the remote command runs with `sh` in a
//! sandbox HOME, so the probe script, the binary push, the settings edit and
//! the purge are the real ones, and nothing leaves this machine. Needs `sh`
//! (Git for Windows' on Windows); says it is skipped without one.
//!
//! No golden: each expectation is the script's.
//!
//! Windows-only, as the other twins: fakecli stands in as `ssh.exe` and
//! `claude.exe`. `remote add` from a Linux machine is not covered here.
//!
//! Beyond the script, for safety: `check` gets `--port` (a free port, not
//! the live dispatcher's 8792), `--settings` (the sandbox's, not this
//! machine's ~/.claude/settings.json) and `--task-prefix` (no such tasks);
//! a fakecli claude.exe is first on PATH, so `claude --version` never runs
//! the real one; `--no-restart` everywhere, stdin closed (no prompts).
#![cfg(windows)]

mod common;
use common::fakessh::FakeSsh;
use common::safety::which_on;
use common::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

struct Checks {
    failed: Vec<String>,
    n: usize,
}

impl Checks {
    fn check(&mut self, name: &str, ok: bool, got: impl FnOnce() -> String) {
        self.n += 1;
        eprintln!("{name:<62} {}", if ok { "ok" } else { "FAIL" });
        if !ok {
            let g: String = got().chars().take(900).collect();
            self.failed.push(format!("{name}\n     got: {g}"));
        }
    }
}

struct Sandbox {
    ssh: FakeSsh,
    tmp: TempDir,
    bin: PathBuf,
    home: PathBuf,
    fakehome: PathBuf,
    builds: PathBuf,
}

impl Sandbox {
    /// `imperatorium ARGS --home HOME`: exit code, stdout, stderr (no \r).
    fn imp(&self, args: &[&str]) -> (i32, String, String) {
        let mut cmd = Command::new(bin());
        cmd.args(args).arg("--home").arg(&self.home);
        clean_env(&mut cmd, Some(&self.bin));
        isolate(&mut cmd, Some(&self.tmp.join("iso")));
        for (k, v) in self.ssh.env(Some("sh")) {
            cmd.env(k, v);
        }
        cmd.env("FAKEHOST_HOME", &self.fakehome)
            .env("IMPERATORIUM_REMOTE_BUILDS", &self.builds)
            .env("IMPERATORIUM_HOME", &self.home)
            .current_dir(&self.tmp.path)
            .stdin(Stdio::null());
        let out = cmd.output().expect("run imperatorium");
        let s = |b: &[u8]| String::from_utf8_lossy(b).replace('\r', "");
        (
            out.status.code().unwrap_or(-1),
            s(&out.stdout),
            s(&out.stderr),
        )
    }

    fn config(&self) -> Value {
        read_json(&self.home.join("config.json"))
    }

    fn hooks(&self) -> Value {
        read_json(&self.fakehome.join(".claude").join("settings.json"))
    }
}

#[test]
fn remote_add_remove_check_against_a_sandbox_host() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    if which_on("sh", &path).is_none() {
        eprintln!("remote_cli: skipped (no sh on PATH)");
        return;
    }
    let began = Instant::now();
    let tmp = TempDir::new("remote-cli");
    let fakehome = tmp.join("hosthome");
    let home = tmp.join("confighome");
    let builds = tmp.join("builds");
    let musl = builds.join("x86_64-unknown-linux-musl");
    let bin = tmp.join("bin");
    for d in [
        fakehome.join(".local").join("bin"),
        fakehome.join(".claude"),
        home.clone(),
        musl.clone(),
        bin.clone(),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    // This machine's `claude` (for `check`'s version probe) is fakecli.
    std::fs::copy(fakecli(), bin.join("claude.exe")).unwrap();
    std::fs::write(
        fakehome.join(".local").join("bin").join("claude"),
        "#!/bin/sh\necho \"2.1.300 (Claude Code)\"\n",
    )
    .unwrap();
    // A shebang, so a Windows shell counts the file executable (`[ -x ]`).
    let payload: &[u8] = b"#!/bin/sh\n# not really an ELF\n";
    std::fs::write(musl.join("imperatorium"), payload).unwrap();
    let settings = fakehome.join(".claude").join("settings.json");
    let fh = fakehome.to_string_lossy();
    std::fs::write(
        &settings,
        json!({"theme": "dark", "hooks": {"Stop": [
            {"hooks": [{"type": "command", "command": "python3",
                        "args": [format!("{fh}/.claude/imperatorium/cc_hook.py")]}]},
            {"hooks": [{"type": "command", "command": "someone-else"}]}]}})
        .to_string(),
    )
    .unwrap();
    let cfg_path = home.join("config.json");
    std::fs::write(&cfg_path, json!({"roots": [tmp.path]}).to_string()).unwrap();
    let sb = Sandbox {
        ssh: FakeSsh::new(),
        tmp,
        bin,
        home,
        fakehome: fakehome.clone(),
        builds,
    };
    let mut c = Checks {
        failed: vec![],
        n: 0,
    };

    let (rc, out, _) = sb.imp(&["remote", "check"]);
    c.check(
        "check with no hosts says so",
        rc == 0 && out.contains("no hosts in config.json"),
        || out.clone(),
    );

    let (rc, out, err) = sb.imp(&[
        "remote",
        "add",
        "fakehost",
        "-y",
        "--no-restart",
        "--label",
        "Fake",
    ]);
    c.check("add exits 0", rc == 0, || format!("{out}\n{err}"));
    c.check(
        "add probes, pushes, hooks, writes config",
        [
            "1/5 SSH",
            "2/5 Claude Code on fakehost",
            "3/5 The binary on fakehost",
            "4/5 Reporting hooks on fakehost",
            "5/5 config.json",
            "added {",
        ]
        .iter()
        .all(|s| out.contains(s)),
        || out.clone(),
    );
    let binpath = fakehome
        .join(".claude")
        .join("imperatorium")
        .join("bin")
        .join("imperatorium");
    c.check(
        "the binary is on the host, byte for byte",
        std::fs::read(&binpath).ok().as_deref() == Some(payload),
        || binpath.display().to_string(),
    );
    let h = sb.hooks();
    let stop = h["hooks"]["Stop"].clone();
    c.check(
        "the host's Stop hook runs the binary, the stranger stays",
        stop.as_array().map(Vec::len) == Some(2)
            && stop[0]["hooks"][0]["command"] == "someone-else"
            && stop[1]["hooks"][0]["args"] == json!(["hook"])
            && stop[1]["hooks"][0]["command"]
                .as_str()
                .is_some_and(|s| s.ends_with("/.claude/imperatorium/bin/imperatorium")),
        || stop.to_string(),
    );
    c.check(
        // Since c61634d a host has a permission hook again, but only the
        // park-only one: its requests are answered here, never by policy there.
        "only the park-only permission hook, no tool hooks on the host",
        h["hooks"]["PermissionRequest"].as_array().is_some_and(|b| {
            b.len() == 1 && b[0]["hooks"][0]["args"] == json!(["permission", "--park-only"])
        }) && h["hooks"].get("PreToolUse").is_none(),
        || {
            format!(
                "{:?}",
                h["hooks"].as_object().map(|o| o.keys().collect::<Vec<_>>())
            )
        },
    );
    c.check("other settings untouched", h["theme"] == "dark", || {
        h.to_string()
    });
    let baks: Vec<String> = std::fs::read_dir(fakehome.join(".claude"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".imperatorium-"))
        .collect();
    c.check("settings.json was backed up", baks.len() == 1, || {
        format!("{baks:?}")
    });
    let cfg = sb.config();
    let entry = cfg["remote"]["hosts"][0].clone();
    c.check(
        "config.json has the host with a tunnel and the label",
        cfg["remote"]["enabled"] == json!(true)
            && entry["name"] == "fakehost"
            && entry["label"] == "Fake"
            && entry["tunnel"] == json!(true)
            && entry["claude"]
                .as_str()
                .is_some_and(|s| s.starts_with("$HOME/.local/bin/claude")),
        || cfg.to_string(),
    );
    c.check(
        "config.json.bak kept",
        Path::new(&format!("{}.bak", cfg_path.display())).exists()
            || sb.home.join("config.json.bak").exists(),
        String::new,
    );

    let (rc, out, _) = sb.imp(&["remote", "check"]);
    c.check(
        "check: every row ok",
        rc == 0
            && !out.contains("MISSING")
            && out.contains("ok       hooks")
            && out.contains("ok       binary"),
        || out.clone(),
    );

    let (rc, out, _) = sb.imp(&["remote", "add", "fakehost", "-y", "--no-restart"]);
    c.check(
        "add again replaces the entry",
        rc == 0 && out.contains("replaced {"),
        || out.clone(),
    );
    c.check(
        "still one host in config",
        sb.config()["remote"]["hosts"].as_array().map(Vec::len) == Some(1),
        || sb.config().to_string(),
    );

    let (rc, out, _) = sb.imp(&["remote", "add", "downhost", "-y", "--no-restart"]);
    c.check(
        "an unreachable host fails with its reason, exit 1",
        rc == 1
            && out.contains("still cannot reach downhost")
            && out.contains("Connection refused")
            && out.contains("ssh-keygen"),
        || out.clone(),
    );
    c.check(
        "and is not written to config",
        sb.config()["remote"]["hosts"].as_array().map(Vec::len) == Some(1),
        || sb.config().to_string(),
    );

    let (rc, out, _) = sb.imp(&["remote", "check", "downhost"]);
    c.check(
        "check of an unreachable host: MISSING ssh, exit 1",
        rc == 1 && out.contains("MISSING  ssh"),
        || out.clone(),
    );

    let (rc, out, err) = sb.imp(&[
        "remote",
        "remove",
        "fakehost",
        "--purge",
        "-y",
        "--no-restart",
    ]);
    c.check("remove exits 0", rc == 0, || format!("{out}\n{err}"));
    c.check(
        "hooks of ours are gone, the stranger stays",
        sb.hooks()["hooks"]["Stop"]
            == json!([{"hooks": [{"type": "command", "command": "someone-else"}]}]),
        || sb.hooks().to_string(),
    );
    c.check(
        "--purge removed ~/.claude/imperatorium",
        !fakehome.join(".claude").join("imperatorium").exists(),
        String::new,
    );
    c.check(
        "the host is out of config.json",
        sb.config()["remote"]["hosts"] == json!([]),
        || sb.config().to_string(),
    );
    let (rc, out, _) = sb.imp(&["remote", "remove", "fakehost", "-y", "--no-restart"]);
    c.check(
        "removing it again says it was not there",
        rc == 0 && out.contains("was not in config.json"),
        || out.clone(),
    );

    let (rc, out, err) = sb.imp(&[
        "remote",
        "add",
        "fakehost",
        "-y",
        "--no-restart",
        "--no-tunnel",
    ]);
    c.check(
        "add --no-tunnel skips the hooks, writes no tunnel",
        rc == 0
            && out.contains("skipped (--no-tunnel)")
            && sb.config()["remote"]["hosts"][0].get("tunnel").is_none(),
        || format!("{out}\n{err}"),
    );
    let (rc, out, _) = sb.imp(&["remote"]);
    c.check(
        "remote with no action is check",
        rc == 0 && out.contains("fakehost"),
        || out.clone(),
    );
    let (rc, out, _) = sb.imp(&["remote", "add"]);
    c.check(
        "add without a host: usage, exit 2",
        rc == 2 && out.contains("usage: imperatorium remote add HOST"),
        || format!("{rc} {out}"),
    );

    // No binary build for the host: the add stops with what to do.
    std::fs::remove_file(musl.join("imperatorium")).unwrap();
    std::fs::remove_file(&binpath).unwrap();
    let (rc, out, _) = sb.imp(&["remote", "add", "fakehost", "-y", "--no-restart"]);
    c.check(
        "no local build: exit 1 and says where to put one",
        rc == 1
            && out.contains("no x86_64-unknown-linux-musl build")
            && out.contains("IMPERATORIUM_REMOTE_BUILDS"),
        || out.clone(),
    );

    // `check`: changes nothing, says what is there.
    let port = free_port().to_string();
    let local_settings = sb.tmp.join("iso").join("settings.json");
    let (rc, out, _) = sb.imp(&[
        "check",
        "--port",
        &port,
        "--settings",
        &local_settings.to_string_lossy(),
        "--task-prefix",
        "ImperatoriumTestNoSuchTask",
    ]);
    c.check(
        "check prints its sections",
        [
            "== Prerequisites",
            "== This installation",
            "== Claude Code hooks",
            "== Service",
        ]
        .iter()
        .all(|s| out.contains(s)),
        || out.clone(),
    );
    c.check(
        "check exit code follows the required pieces",
        rc == 0 || rc == 1,
        || rc.to_string(),
    );
    eprintln!("remote_cli: {:.1} s", began.elapsed().as_secs_f64());
    assert!(
        c.failed.is_empty(),
        "remote_cli: {} of {} checks failed:\n  {}",
        c.failed.len(),
        c.n,
        c.failed.join("\n  ")
    );
    eprintln!("remote_cli: {} checks, all passed", c.n);
}
