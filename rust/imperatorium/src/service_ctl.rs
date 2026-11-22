//! Starting and stopping the service's units: what `imperatorium start | stop
//! | restart` do (impctl's `start`, `stop` and `restart`).
//!
//! Like install and uninstall, each step is built as data (a [`Cmd`]) first,
//! so the exact command lines are tested on any platform. On Windows the
//! tasks are found by what they run - this exe, with the unit's subcommand -
//! not by name, the way `update` finds them: an install made with another
//! `--task-prefix` is found, and a task that runs another copy of
//! imperatorium is never touched.

use std::path::Path;

use crate::service::{cmd, run, systemctl, Backend, Cmd, Service};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Start,
    Stop,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to do here (the hotkey off Windows, a unit that is not installed, on stop).
    Skipped,
    /// Start: no unit or task runs this binary.
    Missing,
    Done,
}

/// The PowerShell that finds the tasks running `exe` with subcommand `sub`
/// and starts (enabling them first) or stops (disabling them, so the
/// one-minute heartbeat does not bring them back, then ending any process
/// started by hand from the same exe) them. Start prints `missing` or `ok`.
/// `only` narrows the tasks to a name prefix.
pub fn windows_control_script(exe: &Path, sub: &str, op: Op, only: Option<&str>) -> String {
    let lit = |s: &str| s.replace('\'', "''");
    let image = exe.file_name().and_then(|n| n.to_str()).unwrap_or("imperatorium.exe");
    let filter = only
        .map(|p| format!(" -and $_.TaskName -like '{}*'", lit(p)))
        .unwrap_or_default();
    let find = format!(
        "$p = '{p}'.ToLower()\n\
         $sub = '{sub}'\n\
         $mine = @(Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object {{ $a = (($_.Actions | ForEach-Object {{ \"$($_.Execute) $($_.Arguments)\" }}) -join ' ').ToLower(); $a.Contains($p) -and $a -match (' ' + $sub + '( |$)'){filter} }})\n",
        p = lit(&exe.to_string_lossy()),
        sub = lit(sub),
    );
    match op {
        Op::Start => format!(
            "{find}if (-not $mine) {{ 'missing' }} else {{\n\
             foreach ($t in $mine) {{ Enable-ScheduledTask -TaskPath $t.TaskPath -TaskName $t.TaskName | Out-Null; Start-ScheduledTask -TaskPath $t.TaskPath -TaskName $t.TaskName }}\n\
             'ok' }}"
        ),
        // The processes ended are those running the subcommand itself: the
        // exe's path, then the subcommand as its first argument. Never the
        // caller (this script's parent): `imperatorium restart hotkey` has
        // " hotkey" in its own command line, and ending it ended the restart
        // before its start, leaving the tasks Disabled (2026-11-21).
        Op::Stop => format!(
            "{find}foreach ($t in $mine) {{ Disable-ScheduledTask -TaskPath $t.TaskPath -TaskName $t.TaskName | Out-Null; Stop-ScheduledTask -TaskPath $t.TaskPath -TaskName $t.TaskName }}\n\
             $caller = (Get-CimInstance Win32_Process -Filter \"ProcessId=$PID\").ParentProcessId\n\
             $runs = [regex]::Escape($p) + '\"?\\s+' + [regex]::Escape($sub) + '(\\s|$)'\n\
             Get-CimInstance Win32_Process -Filter \"Name='{i}'\" | Where-Object {{ $_.ProcessId -ne $caller -and $_.CommandLine -and $_.CommandLine.ToLower() -match $runs }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}",
            i = lit(image),
        ),
    }
}

impl Service {
    /// The commands that start or stop one unit (`name` is "Dispatcher",
    /// "StreamDeck" or "Hotkey"). Pure: nothing is run.
    pub fn control_cmds(&self, name: &str, op: Op, only: Option<&str>) -> Vec<Cmd> {
        let id = self.id(name);
        match self.backend {
            Backend::Windows => {
                let script = windows_control_script(&self.exe, Backend::subcommand(name), op, only);
                vec![cmd("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script])]
            }
            Backend::Systemd => vec![systemctl(&[if op == Op::Start { "start" } else { "stop" }, &id])],
            Backend::Launchd => {
                let file = self.unit_file(name).unwrap_or_default();
                let domain = format!("gui/{}", self.ctx.uid);
                vec![cmd(
                    "launchctl",
                    &[if op == Op::Start { "bootstrap" } else { "bootout" }, &domain, &file.to_string_lossy()],
                )]
            }
        }
    }

    /// Whether `control` has nothing to do for this unit before running
    /// anything: the hotkey is Windows-only, and a unit file that does not
    /// exist cannot be started or stopped.
    pub fn precheck(&self, name: &str, op: Op) -> Option<Outcome> {
        if self.backend == Backend::Windows {
            return None;
        }
        if name == "Hotkey" {
            return Some(Outcome::Skipped);
        }
        if !self.unit_file(name).is_some_and(|f| f.exists()) {
            return Some(if op == Op::Start { Outcome::Missing } else { Outcome::Skipped });
        }
        None
    }

    /// Start or stop one unit.
    pub fn control(&self, name: &str, op: Op, only: Option<&str>) -> Outcome {
        if let Some(done) = self.precheck(name, op) {
            return done;
        }
        let mut missing = false;
        for c in self.control_cmds(name, op, only) {
            let (ok, said) = run(&c);
            if self.backend == Backend::Windows {
                missing |= said.contains("missing");
            } else if !ok && !said.trim().is_empty() {
                eprintln!("{}", said.trim());
            }
        }
        if missing && op == Op::Start {
            Outcome::Missing
        } else {
            Outcome::Done
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::Ctx;
    use std::path::PathBuf;

    fn svc(backend: Backend, home: &Path) -> Service {
        Service {
            prefix: "Imperatorium".into(),
            exe: PathBuf::from("/b/it's.exe"),
            home: PathBuf::from("/h"),
            serve_args: vec![],
            backend,
            ctx: Ctx { home: home.to_path_buf(), xdg_config: None, uid: 501, temp: PathBuf::from("/t") },
        }
    }

    fn show(c: &Cmd) -> String {
        format!("{} {}", c.prog, c.args.join(" "))
    }

    #[test]
    fn windows_tasks_are_found_by_what_they_run() {
        let s = svc(Backend::Windows, Path::new("/u"));
        let c = s.control_cmds("Dispatcher", Op::Stop, None);
        assert_eq!(c.len(), 1);
        assert_eq!(&c[0].args[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
        let script = &c[0].args[3];
        assert!(script.starts_with("$p = '/b/it''s.exe'.ToLower()\n$sub = 'serve'\n"), "{script}");
        assert!(script.contains("$a.Contains($p) -and $a -match (' ' + $sub + '( |$)') }"), "{script}");
        assert!(!script.contains("TaskName -like"), "no prefix filter unless asked: {script}");
        // Stop disables first (the heartbeat would revive it), then ends strays of this exe.
        let disable = script.find("Disable-ScheduledTask").unwrap();
        let stop = script.find("Stop-ScheduledTask").unwrap();
        let stray = script.find("Get-CimInstance Win32_Process -Filter \"Name='it''s.exe'\"").unwrap();
        assert!(disable < stop && stop < stray);
        assert!(!script.contains("Enable-ScheduledTask"));
    }

    /// 2026-11-21: `imperatorium restart hotkey` ended itself at its stop
    /// (its own command line has " hotkey"), so the tasks stayed Disabled.
    /// The stray match, run by PowerShell itself: the helper, quoted or not,
    /// is ended; the CLI asking for it, or another subcommand, is not.
    #[cfg(windows)]
    #[test]
    fn stop_ends_the_helper_but_not_the_restart_asking_for_it() {
        let exe = Path::new(r"C:\Apps\imperatorium.exe");
        let script = windows_control_script(exe, "hotkey", Op::Stop, None);
        assert!(script.contains("$_.ProcessId -ne $caller"), "{script}");
        let keep: Vec<&str> = script.lines().filter(|l| l.starts_with("$p =") || l.starts_with("$sub =") || l.starts_with("$runs =")).collect();
        assert_eq!(keep.len(), 3, "{script}");
        let lines = [
            r#""C:\Apps\imperatorium.exe" hotkey --home C:\h"#,
            r"C:\Apps\imperatorium.exe hotkey",
            r"C:\Apps\imperatorium.exe restart hotkey",
            r"C:\Apps\imperatorium.exe stop hotkey",
            r"C:\Apps\imperatorium.exe hotkeys",
            r"C:\Apps\imperatorium.exe serve --live",
        ];
        let probe = format!(
            "{}\n{}",
            keep.join("\n"),
            lines.iter().map(|l| format!("[int]('{}'.ToLower() -match $runs)", l.replace('\'', "''"))).collect::<Vec<_>>().join("\n")
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &probe])
            .output()
            .expect("powershell");
        let got: Vec<String> = String::from_utf8_lossy(&out.stdout).split_whitespace().map(str::to_string).collect();
        assert_eq!(got, ["1", "1", "0", "0", "0", "0"], "{}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn windows_start_enables_then_starts_and_reports_missing() {
        let s = svc(Backend::Windows, Path::new("/u"));
        let script = &s.control_cmds("StreamDeck", Op::Start, Some("Imp'x"))[0].args[3];
        assert!(script.contains("$sub = 'streamdeck'"));
        assert!(script.contains("-and $_.TaskName -like 'Imp''x*'"));
        let en = script.find("Enable-ScheduledTask").unwrap();
        let st = script.find("Start-ScheduledTask").unwrap();
        assert!(en < st);
        assert!(script.contains("if (-not $mine) { 'missing' }") && script.ends_with("'ok' }"));
        assert!(!script.contains("Stop-Process"));
        assert!(s.control_cmds("Hotkey", Op::Start, None)[0].args[3].contains("$sub = 'hotkey'"));
    }

    #[test]
    fn systemd_and_launchd_command_lines() {
        let s = svc(Backend::Systemd, Path::new("/u"));
        assert_eq!(show(&s.control_cmds("Dispatcher", Op::Start, None)[0]), "systemctl --user start imperatorium-dispatcher.service");
        assert_eq!(show(&s.control_cmds("StreamDeck", Op::Stop, None)[0]), "systemctl --user stop imperatorium-streamdeck.service");
        let l = svc(Backend::Launchd, Path::new("/u"));
        let plist = "/u/Library/LaunchAgents/local.imperatorium.dispatcher.plist";
        assert_eq!(show(&l.control_cmds("Dispatcher", Op::Start, None)[0]).replace('\\', "/"), format!("launchctl bootstrap gui/501 {plist}"));
        assert_eq!(show(&l.control_cmds("Dispatcher", Op::Stop, None)[0]).replace('\\', "/"), format!("launchctl bootout gui/501 {plist}"));
    }

    #[test]
    fn what_is_not_there_is_skipped_or_missing() {
        let dir = std::env::temp_dir().join(format!("imp-svcctl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = svc(Backend::Systemd, &dir);
        // The hotkey is Windows-only; a unit that is not installed is missing to start, nothing to stop.
        assert_eq!(s.precheck("Hotkey", Op::Start), Some(Outcome::Skipped));
        assert_eq!(s.precheck("Dispatcher", Op::Start), Some(Outcome::Missing));
        assert_eq!(s.precheck("Dispatcher", Op::Stop), Some(Outcome::Skipped));
        let unit = s.unit_file("Dispatcher").unwrap();
        std::fs::create_dir_all(unit.parent().unwrap()).unwrap();
        std::fs::write(&unit, "x").unwrap();
        assert_eq!(s.precheck("Dispatcher", Op::Start), None);
        assert_eq!(s.precheck("Dispatcher", Op::Stop), None);
        // Windows always runs the script, which says whether it found a task.
        assert_eq!(svc(Backend::Windows, &dir).precheck("Hotkey", Op::Start), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
