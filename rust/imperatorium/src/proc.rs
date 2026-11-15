//! Running child processes: no console window on Windows, and a bounded run
//! that drains both pipes.

use std::io::{self, Read};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// CREATE_NO_WINDOW: the child gets no console window of its own.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Keep `cmd` from opening a console window (a no-op off Windows).
pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// SIGKILL `pid`; on Windows `taskkill /T /F`, so its children go too.
pub fn kill_pid_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut tk = Command::new("taskkill");
        tk.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = no_window(&mut tk).status();
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

/// Kill `child` and, on Windows, everything it started, then reap it.
pub fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    kill_pid_tree(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

fn drain<R: Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(mut r) = r {
            let _ = r.read_to_end(&mut b);
        }
        b
    })
}

/// Spawn `cmd` (stdin null, stdout and stderr piped), drain both on threads
/// and wait up to `limit`. A spawn failure comes back as it is (so callers
/// can tell `NotFound`); on timeout the process tree is killed and the error
/// is `ErrorKind::TimedOut`.
///
/// When it exits, whatever it left behind is killed too (Windows; see
/// [`Job`]): `claude agents` runs git through git's `cmd\git.exe` wrapper,
/// and a real git whose wrapper died mid-CreateProcess is never resumed - it
/// stays suspended forever, one set per poll. [`run_timeout_keep`] is for a
/// command whose point is to leave something running.
pub fn run_timeout(cmd: &mut Command, limit: Duration) -> io::Result<Output> {
    run_bounded(cmd, limit, false)
}

/// [`run_timeout`], except that a clean exit leaves what the command started
/// running (a timeout still kills the whole tree).
#[allow(dead_code)] // nothing run through here starts anything to keep yet
pub fn run_timeout_keep(cmd: &mut Command, limit: Duration) -> io::Result<Output> {
    run_bounded(cmd, limit, true)
}

fn run_bounded(cmd: &mut Command, limit: Duration, keep: bool) -> io::Result<Output> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let (mut child, job) = Job::spawn(cmd)?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                job.kill(&mut child);
                return Err(io::Error::new(io::ErrorKind::TimedOut, "timed out"));
            }
            Err(e) => {
                job.kill(&mut child);
                return Err(e);
            }
        }
    };
    // Before the pipes are read to their end: a leftover that inherited them
    // would otherwise hold the read open as long as it lives.
    if keep {
        job.release();
    } else {
        job.kill(&mut child);
    }
    Ok(Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// What one bounded run's tree lives in, so all of it can be ended.
///
/// Windows: a job object of its own. The child is created suspended and
/// assigned before it runs a single instruction, so nothing it starts can
/// escape by being quick; KILL_ON_JOB_CLOSE while held, so a dispatcher
/// killed mid-call takes its children with it; BREAKAWAY_OK, so a child that
/// asks to leave may (one that asks and is refused would fail to start).
/// Ending it seals the job first - an active-process limit of zero, which
/// refuses and terminates any process that tries to join (the late git
/// above can join after a plain kill) - then terminates what is in it.
///
/// Elsewhere: a process group of its own, killed on a timeout. No sweep
/// after a clean exit: once the leader is reaped its group id can be reused,
/// and killpg could reach a stranger.
pub struct Job {
    #[cfg(windows)]
    handle: usize,
}

#[cfg(windows)]
mod win {
    pub use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    pub const CREATE_SUSPENDED: u32 = 0x0000_0004;

    /// The job's limit flags (and an active-process limit of zero).
    pub unsafe fn set_limits(job: usize, flags: u32) -> bool {
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = flags;
        SetInformationJobObject(
            job as _,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) != 0
    }

    /// Resume every thread of a process created suspended (it has one).
    pub unsafe fn resume(pid: u32) -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        };
        use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut te: THREADENTRY32 = std::mem::zeroed();
        te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut resumed = false;
        let mut more = Thread32First(snap, &mut te) != 0;
        while more {
            if te.th32OwnerProcessID == pid {
                let t = OpenThread(THREAD_SUSPEND_RESUME, 0, te.th32ThreadID);
                if !t.is_null() {
                    if ResumeThread(t) != u32::MAX {
                        resumed = true;
                    }
                    CloseHandle(t);
                }
            }
            more = Thread32Next(snap, &mut te) != 0;
        }
        CloseHandle(snap);
        resumed
    }
}

impl Job {
    /// Spawn `cmd` (no console window) inside a job of its own.
    pub fn spawn(cmd: &mut Command) -> io::Result<(Child, Job)> {
        #[cfg(windows)]
        unsafe {
            use std::os::windows::io::AsRawHandle;
            use std::os::windows::process::CommandExt;
            use win::*;
            cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
            let mut child = cmd.spawn()?;
            let mut handle = CreateJobObjectW(std::ptr::null(), std::ptr::null()) as usize;
            if handle != 0
                && !(set_limits(handle, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK)
                    && AssignProcessToJobObject(handle as _, child.as_raw_handle() as _) != 0)
            {
                windows_sys::Win32::Foundation::CloseHandle(handle as _);
                handle = 0;
            }
            // Without a job a timeout still kills the tree (taskkill /T).
            let job = Job { handle };
            if !resume(child.id()) {
                // Never leave a child suspended: that is the very leak this prevents.
                job.kill(&mut child);
                return Err(io::Error::other("could not resume the child process"));
            }
            Ok((child, job))
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
            Ok((cmd.spawn()?, Job {}))
        }
    }

    /// End the child and everything it started, and reap it.
    pub fn kill(self, child: &mut Child) {
        #[cfg(windows)]
        unsafe {
            use win::*;
            if self.handle != 0 {
                self.seal();
                TerminateJobObject(self.handle as _, 1);
                windows_sys::Win32::Foundation::CloseHandle(self.handle as _);
            } else if let Ok(None) = child.try_wait() {
                kill_pid_tree(child.id());
            }
        }
        #[cfg(unix)]
        if let Ok(None) = child.try_wait() {
            // The group is ours while its leader is unreaped.
            unsafe {
                libc::killpg(child.id() as i32, libc::SIGKILL);
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Refuse any process that tries to join from now on: the members stay;
    /// one being created into the job is terminated and its CreateProcess
    /// fails. (A no-op off Windows.)
    pub fn seal(&self) -> bool {
        #[cfg(windows)]
        unsafe {
            use win::*;
            self.handle != 0
                && set_limits(self.handle, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS)
        }
        #[cfg(not(windows))]
        false
    }

    /// Let what the child left running carry on without us.
    pub fn release(self) {
        #[cfg(windows)]
        unsafe {
            if self.handle != 0 {
                win::set_limits(self.handle, 0);
                windows_sys::Win32::Foundation::CloseHandle(self.handle as _);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/c", script]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", script]);
            c
        }
    }

    #[test]
    fn collects_both_streams() {
        let o = run_timeout(&mut shell("echo out& echo err 1>&2"), Duration::from_secs(10)).unwrap();
        assert!(o.status.success());
        assert!(String::from_utf8_lossy(&o.stdout).contains("out"));
        assert!(String::from_utf8_lossy(&o.stderr).contains("err"));
    }

    #[test]
    fn missing_program_is_not_found() {
        let e = run_timeout(&mut Command::new("imperatorium-no-such-program"), Duration::from_secs(1)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn times_out() {
        let slow = if cfg!(windows) { "ping -n 20 127.0.0.1 >nul" } else { "sleep 20" };
        let e = run_timeout(&mut shell(slow), Duration::from_millis(300)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
    }
}

/// What test_subproc.py pinned down: the whole tree goes with a timeout.
#[cfg(all(test, windows))]
mod subproc_cov {
    use super::*;

    /// A PowerShell parent that starts a long-lived grandchild (ping), writes
    /// its pid to `pidfile`, then sleeps (`stay`) or exits.
    fn parent(pidfile: &std::path::Path, stay: bool) -> Command {
        let script = format!(
            "$g = Start-Process -FilePath ping -ArgumentList '-n','60','127.0.0.1' -PassThru -WindowStyle Hidden; \
             Set-Content -Path '{}' -Value $g.Id -Encoding ascii; {}",
            pidfile.display(),
            if stay { "Start-Sleep -Seconds 60" } else { "" }
        );
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        c
    }

    fn pid_from(pidfile: &std::path::Path) -> Option<u32> {
        std::fs::read_to_string(pidfile).ok()?.trim().parse().ok()
    }

    fn alive(pid: u32) -> bool {
        let mut c = Command::new("tasklist");
        c.args(["/FI", &format!("PID eq {pid}"), "/NH"]);
        let out = no_window(&mut c).output().unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    }

    fn gone_within(pid: u32, secs: u64) -> bool {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            if !alive(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        !alive(pid)
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("imp-cov-{tag}-{}.pid", std::process::id()))
    }

    fn wait_pid(pidfile: &std::path::Path) -> u32 {
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end {
            if let Some(p) = pid_from(pidfile) {
                return p;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the child never started its grandchild");
    }

    fn cleanup(pid: Option<u32>, pidfile: &std::path::Path) {
        if let Some(p) = pid {
            kill_pid_tree(p);
        }
        let _ = std::fs::remove_file(pidfile);
    }

    #[test]
    fn a_timeout_kills_the_grandchild_too() {
        let pidfile = scratch("timeout");
        let _ = std::fs::remove_file(&pidfile);
        // Long enough for PowerShell to start the grandchild, short of its sleep.
        let e = run_timeout(&mut parent(&pidfile, true), Duration::from_secs(12)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
        let pid = pid_from(&pidfile);
        let ok = pid.map(|p| gone_within(p, 5));
        cleanup(pid, &pidfile);
        assert_eq!(ok, Some(true), "grandchild {pid:?} outlived the timeout");
    }

    #[test]
    fn keep_leaves_what_a_clean_exit_started_running() {
        // For a command whose point is to leave something running.
        let pidfile = scratch("keep");
        let _ = std::fs::remove_file(&pidfile);
        let done = run_timeout_keep(&mut parent(&pidfile, false), Duration::from_secs(30)).unwrap();
        assert!(done.status.success());
        let pid = wait_pid(&pidfile);
        std::thread::sleep(Duration::from_millis(500));
        let still = alive(pid);
        cleanup(Some(pid), &pidfile);
        assert!(still, "keep killed the grandchild");
    }

    #[test]
    fn a_member_left_suspended_goes_with_the_job() {
        // The leak this exists for: a git created into the job, never
        // resumed, its parent gone. Stood in for by a suspended ping
        // assigned to the job by hand.
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        let mut leader = Command::new("ping");
        leader.args(["-n", "60", "127.0.0.1"]).stdout(Stdio::null()).stderr(Stdio::null());
        let (mut child, job) = Job::spawn(&mut leader).unwrap();
        let mut late = Command::new("ping");
        late.args(["-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW | win::CREATE_SUSPENDED);
        let mut suspended = late.spawn().unwrap();
        let joined = unsafe { win::AssignProcessToJobObject(job.handle as _, suspended.as_raw_handle() as _) != 0 };
        let pid = suspended.id();
        job.kill(&mut child);
        let ok = gone_within(pid, 5);
        let _ = suspended.kill();
        let _ = suspended.wait();
        assert!(joined, "could not put the stand-in in the job");
        assert!(ok, "suspended member {pid} outlived the job");
    }

    #[test]
    fn a_sealed_job_refuses_a_late_child() {
        // The late git joined ~90 ms after claude exited, after a one-shot
        // kill would already have run: sealed first, it cannot start at all.
        let script = "[Console]::Out.WriteLine('ready'); [Console]::Out.Flush(); [void][Console]::In.ReadLine(); \
                      try { $p = Start-Process -FilePath cmd -ArgumentList '/c','exit' -PassThru -WindowStyle Hidden -ErrorAction Stop; 'spawned' } \
                      catch { 'refused' }";
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (mut child, job) = Job::spawn(&mut c).unwrap();
        let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        std::io::BufRead::read_line(&mut out, &mut line).unwrap();
        assert_eq!(line.trim(), "ready");
        assert!(job.seal());
        use std::io::Write;
        let mut stdin = child.stdin.take().unwrap();
        writeln!(stdin, "go").unwrap();
        drop(stdin);
        let mut rest = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut rest);
        job.kill(&mut child);
        assert_eq!(rest.trim(), "refused");
    }

    #[test]
    fn a_clean_exit_kills_what_it_left() {
        let pidfile = scratch("sweep");
        let _ = std::fs::remove_file(&pidfile);
        let done = run_timeout(&mut parent(&pidfile, false), Duration::from_secs(30)).unwrap();
        assert!(done.status.success());
        let pid = wait_pid(&pidfile);
        let ok = gone_within(pid, 5);
        cleanup(Some(pid), &pidfile);
        assert!(ok, "grandchild {pid} outlived a clean exit");
    }

    #[test]
    fn kill_tree_ends_a_long_lived_child_and_its_children_and_twice_is_harmless() {
        let pidfile = scratch("tree");
        let _ = std::fs::remove_file(&pidfile);
        let mut child = parent(&pidfile, true)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = wait_pid(&pidfile);
        kill_tree(&mut child);
        kill_tree(&mut child);
        assert!(child.try_wait().unwrap().is_some());
        let ok = gone_within(pid, 5);
        cleanup(Some(pid), &pidfile);
        assert!(ok);
    }

    #[test]
    fn the_exit_status_comes_back() {
        let mut c = Command::new("cmd");
        c.args(["/c", "echo HI& exit 3"]);
        let o = run_timeout(&mut c, Duration::from_secs(10)).unwrap();
        assert_eq!(o.status.code(), Some(3));
        assert!(String::from_utf8_lossy(&o.stdout).contains("HI"));
    }
}
