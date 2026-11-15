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
