//! The machine's service manager, behind one type: Windows scheduled tasks,
//! systemd user units, launchd agents. `install`, `uninstall`, `status` and
//! `update` go through [`Service`]; none of them branch on the platform.
//!
//! What each backend would run is built as data first - a [`UnitPlan`] of
//! [`Action`]s, or a [`Cmd`] - and only then executed, so the exact command
//! lines are unit-testable on any platform without touching a service
//! manager. The backends compile everywhere; [`Backend::native`] picks the
//! one this build runs.

use std::path::{Path, PathBuf};

use crate::install::{home_arg, home_dir};

/// The three units, in the order they are listed and removed.
const NAMES: [&str; 3] = ["Dispatcher", "StreamDeck", "Hotkey"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// Scheduled tasks, through `schtasks`.
    Windows,
    /// systemd user units, through `systemctl --user`.
    Systemd,
    /// launchd agents, through `launchctl`.
    Launchd,
}

impl Backend {
    pub fn native() -> Backend {
        if cfg!(windows) {
            Backend::Windows
        } else if cfg!(target_os = "macos") {
            Backend::Launchd
        } else {
            Backend::Systemd
        }
    }

    /// The task name, unit name or agent label of `name` ("Dispatcher",
    /// "StreamDeck", "Hotkey") under `prefix`.
    pub fn unit_id(self, prefix: &str, name: &str) -> String {
        match self {
            Backend::Windows => format!("{prefix}{name}"),
            Backend::Systemd => format!("{}-{}.service", prefix.to_lowercase(), name.to_lowercase()),
            Backend::Launchd => format!("local.{}.{}", prefix.to_lowercase(), name.to_lowercase()),
        }
    }

    /// The `--serve`-style subcommand a unit runs (what its processes are found by).
    fn subcommand(name: &str) -> &'static str {
        match name {
            "Dispatcher" => "serve",
            "StreamDeck" => "streamdeck",
            _ => "hotkey",
        }
    }
}

/// A program and its arguments, not yet run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cmd {
    pub prog: String,
    pub args: Vec<String>,
}

fn cmd(prog: &str, args: &[&str]) -> Cmd {
    Cmd { prog: prog.into(), args: args.iter().map(|a| a.to_string()).collect() }
}

/// One step of a plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Run it. A failure is ignored, unless `fatal` names what was being
    /// done: then it ends the plan with `"{fatal}: {output}"`. `cleanup` is
    /// a file removed after the run, whatever came of it.
    Run { cmd: Cmd, fatal: Option<String>, cleanup: Option<PathBuf> },
    MkDir(PathBuf),
    Write(PathBuf, Vec<u8>),
    Remove(PathBuf),
}

fn run_ignoring(c: Cmd) -> Action {
    Action::Run { cmd: c, fatal: None, cleanup: None }
}

/// What installing or removing one unit says and does.
#[derive(Debug, Default)]
pub struct UnitPlan {
    /// Printed always.
    pub say: Vec<String>,
    /// Printed after `say` under a dry run only; no `actions` then.
    pub dry_say: Vec<String>,
    pub actions: Vec<Action>,
}

/// What a plan needs from the machine, gathered once so plans stay pure.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub home: PathBuf,
    pub xdg_config: Option<PathBuf>,
    pub uid: u32,
    pub temp: PathBuf,
}

impl Ctx {
    pub fn live() -> Ctx {
        Ctx {
            home: home_dir(),
            xdg_config: crate::paths::env_nonempty("XDG_CONFIG_HOME").map(PathBuf::from),
            #[cfg(target_os = "macos")]
            uid: unsafe { libc::getuid() },
            #[cfg(not(target_os = "macos"))]
            uid: 0,
            temp: std::env::temp_dir(),
        }
    }

    fn systemd_dir(&self) -> PathBuf {
        self.xdg_config.clone().unwrap_or_else(|| self.home.join(".config")).join("systemd").join("user")
    }

    fn launch_agents(&self) -> PathBuf {
        self.home.join("Library/LaunchAgents")
    }
}

pub struct Service {
    pub prefix: String,
    pub exe: PathBuf,
    pub home: PathBuf,
    /// Extra `serve` arguments (a test's --port / --state); empty: --live.
    pub serve_args: Vec<String>,
    pub backend: Backend,
    pub ctx: Ctx,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn output_of(c: &Cmd) -> std::io::Result<std::process::Output> {
    let mut p = std::process::Command::new(&c.prog);
    p.args(&c.args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        p.creation_flags(0x0800_0000);
    }
    p.output()
}

/// Run and wait: success, and stdout then stderr.
fn run(c: &Cmd) -> (bool, String) {
    match output_of(c) {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr),
        ),
        Err(e) => (false, e.to_string()),
    }
}

fn execute(actions: Vec<Action>) -> Result<(), String> {
    for a in actions {
        match a {
            Action::Run { cmd, fatal, cleanup } => {
                let (ok, said) = run(&cmd);
                if let Some(f) = cleanup {
                    let _ = std::fs::remove_file(f);
                }
                if let (false, Some(what)) = (ok, fatal) {
                    return Err(format!("{what}: {}", said.trim()));
                }
            }
            Action::MkDir(d) => std::fs::create_dir_all(d).map_err(|e| e.to_string())?,
            Action::Write(p, bytes) => std::fs::write(p, bytes).map_err(|e| e.to_string())?,
            Action::Remove(p) => {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    Ok(())
}

// -- command lines, one per backend -------------------------------------------

/// End any process of ours running `sub` for this service (a task's binary
/// outlives its conhost, so stopping the task is not enough).
fn stop_processes_cmd(exe: &Path, sub: &str, extra: &str) -> Cmd {
    let filter = format!(
        "Name='{}'",
        exe.file_name().and_then(|n| n.to_str()).unwrap_or("imperatorium.exe")
    );
    let script = format!(
        "Get-CimInstance Win32_Process -Filter \"{filter}\" | Where-Object {{ $_.CommandLine -match ' {sub}( |$)' -and $_.CommandLine -match '{extra}' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}"
    );
    cmd("powershell", &["-NoProfile", "-Command", &script])
}

fn systemctl(args: &[&str]) -> Cmd {
    let mut a = vec!["--user"];
    a.extend_from_slice(args);
    cmd("systemctl", &a)
}

/// The PowerShell `update` runs: restarts every task whose action runs `exe`.
pub fn windows_restart_script(exe: &Path, prefix: &str) -> String {
    let lit = |s: &str| s.replace('\'', "''");
    let image = exe.file_name().and_then(|n| n.to_str()).unwrap_or("imperatorium.exe").to_string();
    // The tasks are found by what they run - this exe - not by name:
    // an install made with --task-prefix is restarted too (the window's
    // Update button does not know the prefix), and another install's
    // tasks - the live one, beside a test copy - are left alone, however
    // they are named. Processes likewise: only those started from this
    // exe, by the path in their command line (the task put it there);
    // not by image path, which after the swap names the old, renamed
    // file. The dispatcher goes last: on Linux its restart ends us.
    format!(
        "$p = '{p}'.ToLower()\n\
         $all = @(Get-ScheduledTask -ErrorAction SilentlyContinue)\n\
         $mine = @($all | Where-Object {{ (($_.Actions | ForEach-Object {{ \"$($_.Execute) $($_.Arguments)\" }}) -join ' ').ToLower().Contains($p) }})\n\
         foreach ($t in $all) {{ if ($t.TaskName -like '{prefix}*' -and -not ($mine | Where-Object TaskName -eq $t.TaskName)) {{ \"   $($t.TaskName) runs another copy of imperatorium - left alone\" }} }}\n\
         foreach ($sub in 'streamdeck', 'hotkey', 'serve') {{\n\
           foreach ($t in $mine) {{\n\
             $a = ($t.Actions | ForEach-Object {{ \"$($_.Arguments)\" }}) -join ' '\n\
             if ($a -notmatch \" $sub( |$)\") {{ continue }}\n\
             Stop-ScheduledTask -TaskPath $t.TaskPath -TaskName $t.TaskName\n\
             Get-CimInstance Win32_Process -Filter \"Name='{i}'\" | Where-Object {{ $_.CommandLine -and $_.CommandLine.ToLower().Contains($p) -and $_.CommandLine -match \" $sub( |$)\" }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}\n\
             \"   restarted $($t.TaskName)\"\n\
             Start-ScheduledTask -TaskPath $t.TaskPath -TaskName $t.TaskName\n\
           }}\n\
         }}",
        p = lit(&exe.to_string_lossy()),
        i = lit(&image),
        prefix = lit(prefix),
    )
}

fn windows_restart_cmd(exe: &Path, prefix: &str) -> Cmd {
    cmd("powershell", &["-NoProfile", "-NonInteractive", "-Command", &windows_restart_script(exe, prefix)])
}

/// The `Arguments` of a task's XML export, unescaped.
fn task_arguments(xml: &str) -> Option<String> {
    let start = xml.find("<Arguments>")? + "<Arguments>".len();
    let end = start + xml[start..].find("</Arguments>")?;
    Some(
        xml[start..end]
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

/// The state `schtasks /Query /FO LIST` reports.
fn task_state(ok: bool, said: &str) -> String {
    if !ok {
        return "not registered".into();
    }
    said.lines()
        .find(|l| l.trim_start().starts_with("Status:"))
        .map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string())
        .unwrap_or_else(|| "registered".into())
}

// -- the service ----------------------------------------------------------------

impl Service {
    pub fn new(prefix: String, exe: PathBuf, home: PathBuf, serve_args: Vec<String>) -> Service {
        Service { prefix, exe, home, serve_args, backend: Backend::native(), ctx: Ctx::live() }
    }

    fn units(&self, streamdeck: bool, hotkey: bool) -> Vec<(String, Vec<String>, u32, &'static str)> {
        let home = self.home.to_string_lossy().into_owned();
        let serve = if self.serve_args.is_empty() {
            vec!["serve".into(), "--live".into(), "--home".into(), home.clone()]
        } else {
            let mut a = vec!["serve".to_string()];
            a.extend(self.serve_args.iter().cloned());
            a.extend(["--home".to_string(), home.clone()]);
            a
        };
        let mut out = vec![("Dispatcher".to_string(), serve, 0, "the dispatcher and window")];
        if streamdeck {
            out.push(("StreamDeck".into(), vec!["streamdeck".into(), "--home".into(), home.clone()], 15, "the Stream Deck surface"));
        }
        if hotkey {
            out.push(("Hotkey".into(), vec!["hotkey".into(), "--home".into(), home], 20, "the global hotkey"));
        }
        out
    }

    fn id(&self, name: &str) -> String {
        self.backend.unit_id(&self.prefix, name)
    }

    /// Where a systemd unit or launchd agent file lives (none for tasks).
    fn unit_file(&self, name: &str) -> Option<PathBuf> {
        let id = self.id(name);
        match self.backend {
            Backend::Windows => None,
            Backend::Systemd => Some(self.ctx.systemd_dir().join(id)),
            Backend::Launchd => Some(self.ctx.launch_agents().join(format!("{id}.plist"))),
        }
    }

    /// A task as `service/tasks.ps1` shapes it: logon trigger (delayed for
    /// the surfaces), a one-minute heartbeat trigger from now on (a killed
    /// process is otherwise never restarted), IgnoreNew, three restarts, no
    /// time limit; run under a headless conhost so no console window shows.
    pub fn task_xml(&self, args: &[String], delay: u32, start: &str) -> String {
        let user = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN").unwrap_or_default(),
            std::env::var("USERNAME").unwrap_or_default()
        );
        let quote = |a: &str| if a.contains(' ') || a.is_empty() { format!("\"{a}\"") } else { a.to_string() };
        let arguments = std::iter::once(format!("--headless \"{}\"", self.exe.display()))
            .chain(args.iter().map(|a| quote(a)))
            .collect::<Vec<_>>()
            .join(" ");
        let delay = if delay > 0 { format!("<Delay>PT{delay}S</Delay>") } else { String::new() };
        format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Imperatorium - see and dispatch to Claude Code sessions</Description></RegistrationInfo>
  <Triggers>
    <LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId>{delay}</LogonTrigger>
    <TimeTrigger><Repetition><Interval>PT1M</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition><StartBoundary>{start}</StartBoundary><Enabled>true</Enabled></TimeTrigger>
  </Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <Enabled>true</Enabled>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure><Interval>PT1M</Interval><Count>3</Count></RestartOnFailure>
  </Settings>
  <Actions Context="Author"><Exec><Command>C:\Windows\System32\conhost.exe</Command><Arguments>{args}</Arguments><WorkingDirectory>{wd}</WorkingDirectory></Exec></Actions>
</Task>
"#,
            args = xml_escape(&arguments),
            wd = xml_escape(&self.home.to_string_lossy()),
        )
    }

    /// setup_machine.systemd_unit.
    pub fn systemd_unit(&self, args: &[String], what: &str, path: &str) -> String {
        let quoted = std::iter::once(self.exe.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "[Unit]\nDescription=Imperatorium - {what}\nAfter=network-online.target\n\n[Service]\nType=simple\n\
             WorkingDirectory={}\nEnvironment=\"PATH={path}\"\nExecStart={quoted}\nRestart=always\nRestartSec=5\n\n\
             [Install]\nWantedBy=default.target\n",
            self.home.display()
        )
    }

    /// setup_machine.launchd_plist.
    pub fn launchd_plist(&self, label: &str, args: &[String], path: &str, log: &str) -> String {
        let items: String = std::iter::once(self.exe.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(|a| format!("<string>{}</string>", xml_escape(&a)))
            .collect();
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\
             <key>Label</key><string>{}</string><key>ProgramArguments</key><array>{items}</array>\
             <key>WorkingDirectory</key><string>{}</string>\
             <key>EnvironmentVariables</key><dict><key>PATH</key><string>{}</string></dict>\
             <key>StandardOutPath</key><string>{log}</string><key>StandardErrorPath</key><string>{log}</string>\
             <key>RunAtLoad</key><true/><key>KeepAlive</key><true/></dict></plist>\n",
            xml_escape(label),
            xml_escape(&self.home.to_string_lossy()),
            xml_escape(path)
        )
    }

    // -- plans: what each backend does, as data -----------------------------------

    /// Registering and starting one unit. `path` is the PATH it gets and
    /// `start` the task's first heartbeat (`YYYY-MM-DDTHH:MM:SS`).
    pub fn install_plan(&self, name: &str, args: &[String], delay: u32, what: &str, path: &str, start: &str) -> UnitPlan {
        let id = self.id(name);
        match self.backend {
            Backend::Windows => {
                let task = id;
                let file = self.ctx.temp.join(format!("{task}.xml"));
                // schtasks reads the XML as UTF-16, as its header says.
                let mut bytes = vec![0xFF, 0xFE];
                for u in self.task_xml(args, delay, start).encode_utf16() {
                    bytes.extend(u.to_le_bytes());
                }
                UnitPlan {
                    say: vec![format!("   task {task}: {} {}", self.exe.display(), args.join(" "))],
                    dry_say: vec![],
                    actions: vec![
                        run_ignoring(cmd("schtasks", &["/End", "/TN", &task])),
                        run_ignoring(stop_processes_cmd(&self.exe, &args[0], &regex::escape(&self.home.to_string_lossy()))),
                        Action::Write(file.clone(), bytes),
                        Action::Run {
                            cmd: cmd("schtasks", &["/Create", "/F", "/TN", &task, "/XML", &file.to_string_lossy()]),
                            fatal: Some(format!("registering {task}")),
                            cleanup: Some(file),
                        },
                        run_ignoring(cmd("schtasks", &["/Run", "/TN", &task])),
                    ],
                }
            }
            Backend::Systemd => {
                let file = self.unit_file(name).unwrap_or_default();
                let text = self.systemd_unit(args, what, path);
                UnitPlan {
                    say: vec![format!("   unit {}", file.display())],
                    dry_say: text.lines().map(|l| format!("      {l}")).collect(),
                    actions: vec![
                        Action::MkDir(self.ctx.systemd_dir()),
                        Action::Write(file, text.into_bytes()),
                        run_ignoring(systemctl(&["daemon-reload"])),
                        run_ignoring(systemctl(&["enable", "--now", &id])),
                        run_ignoring(systemctl(&["restart", &id])),
                    ],
                }
            }
            Backend::Launchd => {
                let file = self.unit_file(name).unwrap_or_default();
                let log = self.ctx.home.join("Library/Logs").join(format!("imperatorium-{}.log", name.to_lowercase()));
                let plist = self.launchd_plist(&id, args, path, &log.to_string_lossy());
                let domain = format!("gui/{}", self.ctx.uid);
                UnitPlan {
                    say: vec![format!("   agent {}", file.display())],
                    dry_say: vec![],
                    actions: vec![
                        Action::MkDir(file.parent().map(Path::to_path_buf).unwrap_or_default()),
                        Action::Write(file.clone(), plist.into_bytes()),
                        run_ignoring(cmd("launchctl", &["bootout", &domain, &file.to_string_lossy()])),
                        run_ignoring(cmd("launchctl", &["bootstrap", &domain, &file.to_string_lossy()])),
                    ],
                }
            }
        }
    }

    /// Stopping and removing one unit (that is registered).
    pub fn uninstall_plan(&self, name: &str) -> UnitPlan {
        let id = self.id(name);
        match self.backend {
            Backend::Windows => UnitPlan {
                say: vec![format!("   remove task {id}")],
                dry_say: vec![],
                actions: vec![
                    run_ignoring(cmd("schtasks", &["/End", "/TN", &id])),
                    run_ignoring(cmd("schtasks", &["/Delete", "/F", "/TN", &id])),
                    run_ignoring(stop_processes_cmd(
                        &self.exe,
                        Backend::subcommand(name),
                        &regex::escape(&self.home.to_string_lossy()),
                    )),
                ],
            },
            Backend::Systemd => {
                let file = self.unit_file(name).unwrap_or_default();
                UnitPlan {
                    say: vec![format!("   remove {}", file.display())],
                    dry_say: vec![],
                    actions: vec![
                        run_ignoring(systemctl(&["disable", "--now", &id])),
                        Action::Remove(file),
                        run_ignoring(systemctl(&["daemon-reload"])),
                    ],
                }
            }
            Backend::Launchd => {
                let file = self.unit_file(name).unwrap_or_default();
                let domain = format!("gui/{}", self.ctx.uid);
                UnitPlan {
                    say: vec![format!("   remove {}", file.display())],
                    dry_say: vec![],
                    actions: vec![
                        run_ignoring(cmd("launchctl", &["bootout", &domain, &file.to_string_lossy()])),
                        Action::Remove(file),
                    ],
                }
            }
        }
    }

    fn status_cmd(&self, id: &str) -> Cmd {
        match self.backend {
            Backend::Windows => cmd("schtasks", &["/Query", "/TN", id, "/FO", "LIST"]),
            Backend::Systemd => systemctl(&["is-active", id]),
            Backend::Launchd => cmd("launchctl", &["list", id]),
        }
    }

    // -- doing it -----------------------------------------------------------------

    /// Is `name` registered?
    fn is_installed(&self, name: &str) -> bool {
        match self.unit_file(name) {
            Some(file) => file.exists(),
            None => run(&cmd("schtasks", &["/Query", "/TN", &self.id(name)])).0,
        }
    }

    /// Register and start everything (or describe it under dry_run).
    pub fn install(&self, streamdeck: bool, hotkey: bool, dry: bool, say: &mut dyn FnMut(String)) -> Result<(), String> {
        let path = crate::install::service_path();
        for (name, args, delay, what) in self.units(streamdeck, hotkey) {
            let start = crate::paths::local_stamp().replacen(' ', "T", 1); // YYYY-MM-DD HH:MM:SS
            let plan = self.install_plan(&name, &args, delay, what, &path, &start);
            plan.say.into_iter().for_each(&mut *say);
            if dry {
                plan.dry_say.into_iter().for_each(&mut *say);
                continue;
            }
            execute(plan.actions)?;
        }
        Ok(())
    }

    /// Stop and remove everything this prefix registered.
    pub fn uninstall(&self, dry: bool, say: &mut dyn FnMut(String)) {
        for name in NAMES {
            if !self.is_installed(name) {
                continue;
            }
            let plan = self.uninstall_plan(name);
            plan.say.into_iter().for_each(&mut *say);
            if dry {
                continue;
            }
            let _ = execute(plan.actions);
        }
    }

    /// The `--home` the registered dispatcher was installed with, if any:
    /// what `status` reports when it is given none, since that is the
    /// config home actually in use.
    pub fn installed_home(&self) -> Option<PathBuf> {
        match self.backend {
            Backend::Windows => {
                let (ok, xml) = run(&cmd("schtasks", &["/Query", "/TN", &self.id("Dispatcher"), "/XML"]));
                if !ok {
                    return None;
                }
                home_arg(&task_arguments(&xml)?).map(PathBuf::from)
            }
            Backend::Systemd => {
                let text = std::fs::read_to_string(self.unit_file("Dispatcher")?).ok()?;
                let line = text.lines().find_map(|l| l.strip_prefix("ExecStart="))?;
                home_arg(&line.replace("\\\"", "\u{0}")).map(|h| PathBuf::from(h.replace('\u{0}', "\"")))
            }
            Backend::Launchd => None,
        }
    }

    /// Each unit: registered, and its state.
    pub fn status(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for name in NAMES {
            let id = self.id(name);
            match self.backend {
                Backend::Windows => {
                    let (ok, said) = run(&self.status_cmd(&id));
                    out.push((id, task_state(ok, &said)));
                }
                Backend::Systemd => {
                    if !self.is_installed(name) {
                        out.push((id, "not installed".into()));
                        continue;
                    }
                    let (_, said) = run(&self.status_cmd(&id));
                    out.push((id, said.trim().to_string()));
                }
                Backend::Launchd => {
                    let (ok, _) = run(&self.status_cmd(&id));
                    out.push((id, if ok { "loaded".into() } else { "not loaded".into() }));
                }
            }
        }
        out
    }

    /// Restart what runs from this binary, so a new one takes over. The
    /// dispatcher goes last: its restart ends us.
    pub fn restart_running(prefix: &str) {
        let svc = Service::new(prefix.to_string(), std::env::current_exe().unwrap_or_default(), PathBuf::new(), vec![]);
        match svc.backend {
            Backend::Windows => {
                let c = windows_restart_cmd(&svc.exe, prefix);
                // Printed as it goes: once the dispatcher's restart kills the
                // processes, a later line might never arrive.
                let _ = std::io::Write::flush(&mut std::io::stdout());
                let _ = std::process::Command::new(&c.prog)
                    .args(&c.args)
                    .stdin(std::process::Stdio::null())
                    .status();
            }
            Backend::Systemd => {
                for name in ["StreamDeck", "Hotkey", "Dispatcher"] {
                    let unit = svc.id(name);
                    let enabled = output_of(&systemctl(&["is-enabled", &unit])).is_ok_and(|o| o.status.success());
                    // As on Windows: only a unit that runs this exe.
                    let runs = output_of(&systemctl(&["show", "-p", "ExecStart", "--value", &unit]))
                        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&*svc.exe.to_string_lossy()));
                    if enabled && !runs {
                        println!("   {unit} runs another copy of imperatorium - left alone");
                    } else if enabled {
                        let _ = output_of(&systemctl(&["restart", &unit]));
                        println!("   restarted {unit}");
                    }
                }
            }
            Backend::Launchd => {
                for name in ["StreamDeck", "Hotkey", "Dispatcher"] {
                    let target = format!("gui/{}/{}", svc.ctx.uid, svc.id(name));
                    let _ = output_of(&cmd("launchctl", &["kickstart", "-k", &target]));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc(backend: Backend) -> Service {
        let sep = if cfg!(windows) { '\\' } else { '/' };
        let _ = sep;
        Service {
            prefix: "Imperatorium".into(),
            exe: PathBuf::from("/bin/imperatorium"),
            home: PathBuf::from("/h/imp"),
            serve_args: vec![],
            backend,
            ctx: Ctx { home: PathBuf::from("/u"), xdg_config: None, uid: 501, temp: PathBuf::from("/t") },
        }
    }

    fn show(c: &Cmd) -> String {
        format!("{} {}", c.prog, c.args.join(" "))
    }

    /// One line per action, paths with forward slashes.
    fn render(actions: &[Action]) -> Vec<String> {
        let slash = |p: &Path| p.to_string_lossy().replace('\\', "/");
        actions
            .iter()
            .map(|a| match a {
                Action::Run { cmd, fatal, cleanup } => {
                    let mut s = format!("run {}", show(cmd).replace('\\', "/"));
                    if let Some(f) = fatal {
                        s += &format!(" [fatal: {f}]");
                    }
                    if let Some(c) = cleanup {
                        s += &format!(" [then remove {}]", slash(c));
                    }
                    s
                }
                Action::MkDir(d) => format!("mkdir {}", slash(d)),
                Action::Write(p, b) => format!("write {} ({} bytes)", slash(p), b.len()),
                Action::Remove(p) => format!("remove {}", slash(p)),
            })
            .collect()
    }

    fn serve() -> Vec<String> {
        ["serve", "--live", "--home", "/h/imp"].map(String::from).to_vec()
    }

    #[test]
    fn names_per_backend() {
        assert_eq!(Backend::Windows.unit_id("Imperatorium", "StreamDeck"), "ImperatoriumStreamDeck");
        assert_eq!(Backend::Systemd.unit_id("Imperatorium", "StreamDeck"), "imperatorium-streamdeck.service");
        assert_eq!(Backend::Launchd.unit_id("Imperatorium", "Hotkey"), "local.imperatorium.hotkey");
        assert_eq!(Backend::Systemd.unit_id("Imp2", "Dispatcher"), "imp2-dispatcher.service");
    }

    #[test]
    fn units_follow_the_choices() {
        let s = svc(Backend::Windows);
        let names = |d, h| s.units(d, h).into_iter().map(|u| (u.0, u.2)).collect::<Vec<_>>();
        assert_eq!(names(false, false), vec![("Dispatcher".to_string(), 0)]);
        assert_eq!(
            names(true, true),
            vec![("Dispatcher".to_string(), 0), ("StreamDeck".to_string(), 15), ("Hotkey".to_string(), 20)]
        );
        let mut s = svc(Backend::Windows);
        s.serve_args = vec!["--port".into(), "8811".into(), "--state".into(), "/s".into()];
        assert_eq!(s.units(false, false)[0].1, ["serve", "--port", "8811", "--state", "/s", "--home", "/h/imp"]);
    }

    #[test]
    fn windows_install_runs_schtasks_in_order() {
        let s = svc(Backend::Windows);
        let p = s.install_plan("Dispatcher", &serve(), 0, "the dispatcher and window", "PATH", "2026-01-02T03:04:05");
        assert_eq!(p.say, [format!("   task ImperatoriumDispatcher: {} serve --live --home /h/imp", s.exe.display())]);
        assert!(p.dry_say.is_empty());
        let r = render(&p.actions);
        assert_eq!(r.len(), 5);
        assert_eq!(r[0], "run schtasks /End /TN ImperatoriumDispatcher");
        assert!(r[1].starts_with("run powershell -NoProfile -Command Get-CimInstance Win32_Process -Filter \"Name='imperatorium'\""), "{}", r[1]);
        assert!(r[1].contains("-match ' serve( |$)' -and $_.CommandLine -match '/h/imp'"), "{}", r[1]);
        assert_eq!(r[2].split(" (").next().unwrap(), "write /t/ImperatoriumDispatcher.xml");
        assert_eq!(
            r[3],
            "run schtasks /Create /F /TN ImperatoriumDispatcher /XML /t/ImperatoriumDispatcher.xml \
             [fatal: registering ImperatoriumDispatcher] [then remove /t/ImperatoriumDispatcher.xml]"
        );
        assert_eq!(r[4], "run schtasks /Run /TN ImperatoriumDispatcher");
        // The file is UTF-16 with a BOM, and holds the task.
        let Action::Write(_, bytes) = &p.actions[2] else { panic!() };
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
        let units: Vec<u16> = bytes[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16(&units).unwrap(), s.task_xml(&serve(), 0, "2026-01-02T03:04:05"));
    }

    #[test]
    fn windows_surfaces_find_their_processes_by_subcommand() {
        let s = svc(Backend::Windows);
        let args = ["streamdeck", "--home", "/h/imp"].map(String::from);
        let p = s.install_plan("StreamDeck", &args, 15, "the Stream Deck surface", "PATH", "t");
        let r = render(&p.actions);
        assert_eq!(r[0], "run schtasks /End /TN ImperatoriumStreamDeck");
        assert!(r[1].contains("-match ' streamdeck( |$)'"));
        assert!(s.task_xml(&args, 15, "t").contains("<Delay>PT15S</Delay>"));
        assert!(!s.task_xml(&serve(), 0, "t").contains("<Delay>"));
    }

    #[test]
    fn windows_uninstall() {
        let s = svc(Backend::Windows);
        for (name, sub) in [("Dispatcher", "serve"), ("StreamDeck", "streamdeck"), ("Hotkey", "hotkey")] {
            let p = s.uninstall_plan(name);
            assert_eq!(p.say, [format!("   remove task Imperatorium{name}")]);
            let r = render(&p.actions);
            assert_eq!(r[0], format!("run schtasks /End /TN Imperatorium{name}"));
            assert_eq!(r[1], format!("run schtasks /Delete /F /TN Imperatorium{name}"));
            assert!(r[2].contains(&format!("-match ' {sub}( |$)'")), "{}", r[2]);
            assert_eq!(r.len(), 3);
        }
    }

    #[test]
    fn systemd_install_writes_the_unit_then_enables_it() {
        let s = svc(Backend::Systemd);
        let p = s.install_plan("Dispatcher", &serve(), 0, "the dispatcher and window", "/usr/bin", "t");
        assert_eq!(p.say[0].replace('\\', "/"), "   unit /u/.config/systemd/user/imperatorium-dispatcher.service");
        let unit = s.systemd_unit(&serve(), "the dispatcher and window", "/usr/bin");
        assert_eq!(p.dry_say.len(), unit.lines().count());
        assert_eq!(p.dry_say[0], "      [Unit]");
        assert!(unit.contains("ExecStart=\"/bin/imperatorium\" \"serve\" \"--live\" \"--home\" \"/h/imp\"\n"));
        let r = render(&p.actions);
        assert_eq!(r[0], "mkdir /u/.config/systemd/user");
        assert_eq!(r[1].split(" (").next().unwrap(), "write /u/.config/systemd/user/imperatorium-dispatcher.service");
        assert_eq!(
            r[2..],
            [
                "run systemctl --user daemon-reload",
                "run systemctl --user enable --now imperatorium-dispatcher.service",
                "run systemctl --user restart imperatorium-dispatcher.service",
            ]
        );
    }

    #[test]
    fn systemd_honours_xdg_config_home() {
        let mut s = svc(Backend::Systemd);
        s.ctx.xdg_config = Some(PathBuf::from("/x"));
        let r = render(&s.install_plan("Hotkey", &serve(), 20, "w", "p", "t").actions);
        assert_eq!(r[0], "mkdir /x/systemd/user");
        let r = render(&s.uninstall_plan("Hotkey").actions);
        assert_eq!(r[1], "remove /x/systemd/user/imperatorium-hotkey.service");
    }

    #[test]
    fn systemd_uninstall() {
        let s = svc(Backend::Systemd);
        let p = s.uninstall_plan("StreamDeck");
        assert_eq!(
            render(&p.actions),
            [
                "run systemctl --user disable --now imperatorium-streamdeck.service",
                "remove /u/.config/systemd/user/imperatorium-streamdeck.service",
                "run systemctl --user daemon-reload",
            ]
        );
    }

    #[test]
    fn launchd_install_and_uninstall() {
        let s = svc(Backend::Launchd);
        let p = s.install_plan("Dispatcher", &serve(), 0, "w", "/usr/bin", "t");
        let plist = "/u/Library/LaunchAgents/local.imperatorium.dispatcher.plist";
        assert_eq!(
            render(&p.actions)
                .into_iter()
                .map(|l| l.split(" (").next().unwrap().to_string())
                .collect::<Vec<_>>(),
            [
                "mkdir /u/Library/LaunchAgents".to_string(),
                format!("write {plist}"),
                format!("run launchctl bootout gui/501 {plist}"),
                format!("run launchctl bootstrap gui/501 {plist}"),
            ]
        );
        let Action::Write(_, bytes) = &p.actions[1] else { panic!() };
        let text = String::from_utf8(bytes.clone()).unwrap().replace('\\', "/");
        assert!(text.contains("<key>Label</key><string>local.imperatorium.dispatcher</string>"));
        assert!(text.contains("<key>StandardOutPath</key><string>/u/Library/Logs/imperatorium-dispatcher.log</string>"));
        assert_eq!(
            render(&s.uninstall_plan("Dispatcher").actions),
            [format!("run launchctl bootout gui/501 {plist}"), format!("remove {plist}")]
        );
    }

    #[test]
    fn status_commands() {
        assert_eq!(show(&svc(Backend::Windows).status_cmd("T")), "schtasks /Query /TN T /FO LIST");
        assert_eq!(show(&svc(Backend::Systemd).status_cmd("u.service")), "systemctl --user is-active u.service");
        assert_eq!(show(&svc(Backend::Launchd).status_cmd("local.x.y")), "launchctl list local.x.y");
    }

    #[test]
    fn task_output_is_read() {
        assert_eq!(task_state(false, ""), "not registered");
        assert_eq!(task_state(true, "TaskName: x\nStatus:                 Running\n"), "Running");
        assert_eq!(task_state(true, "TaskName: x\n"), "registered");
        let xml = "<Exec><Arguments>--headless &quot;C:\\a b\\i.exe&quot; serve --home &quot;C:\\h&quot;</Arguments></Exec>";
        assert_eq!(task_arguments(xml).as_deref(), Some("--headless \"C:\\a b\\i.exe\" serve --home \"C:\\h\""));
        assert_eq!(home_arg(&task_arguments(xml).unwrap()).as_deref(), Some("C:\\h"));
        assert_eq!(task_arguments("<x/>"), None);
    }

    #[test]
    fn restart_commands() {
        let exe = Path::new("/b/it's.exe");
        let c = windows_restart_cmd(exe, "Imp'x");
        assert_eq!((c.prog.as_str(), &c.args[..3]), ("powershell", &["-NoProfile", "-NonInteractive", "-Command"].map(String::from)[..]));
        let script = &c.args[3];
        assert!(script.starts_with("$p = '/b/it''s.exe'.ToLower()\n"));
        assert!(script.contains("$t.TaskName -like 'Imp''x*'"));
        assert!(script.contains("Get-CimInstance Win32_Process -Filter \"Name='it''s.exe'\""));
        assert!(script.contains("foreach ($sub in 'streamdeck', 'hotkey', 'serve')"));
        assert!(script.ends_with("}\n         }") || script.ends_with("}"));
        assert_eq!(show(&systemctl(&["is-enabled", "u"])), "systemctl --user is-enabled u");
        assert_eq!(show(&systemctl(&["show", "-p", "ExecStart", "--value", "u"])), "systemctl --user show -p ExecStart --value u");
        assert_eq!(show(&systemctl(&["restart", "u"])), "systemctl --user restart u");
    }
}
