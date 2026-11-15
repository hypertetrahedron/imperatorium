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
pub fn run_timeout(cmd: &mut Command, limit: Duration) -> io::Result<Output> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    no_window(cmd);
    let mut child = cmd.spawn()?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(s) => break s,
            None if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(10)),
            None => {
                kill_tree(&mut child);
                return Err(io::Error::new(io::ErrorKind::TimedOut, "timed out"));
            }
        }
    };
    Ok(Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
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
    fn a_clean_exit_leaves_what_it_started_running() {
        // `claude --bg` exits and its session must carry on: the binary has
        // no sweep (see DIFFERS for Python's default).
        let pidfile = scratch("keep");
        let _ = std::fs::remove_file(&pidfile);
        let done = run_timeout(&mut parent(&pidfile, false), Duration::from_secs(30)).unwrap();
        assert!(done.status.success());
        let pid = wait_pid(&pidfile);
        std::thread::sleep(Duration::from_millis(500));
        let still = alive(pid);
        cleanup(Some(pid), &pidfile);
        assert!(still, "a clean exit killed the grandchild");
    }

    #[test]
    #[ignore = "DIFFERS: Python's subproc.run killed what a clean exit left behind (job object); the binary's run_timeout does not"]
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
