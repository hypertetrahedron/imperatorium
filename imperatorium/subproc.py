"""Child processes that cannot outlive their welcome.

`subprocess.run(..., timeout=)` kills only the process it started. Most of
what this package runs is a launcher for something else: `claude agents`
runs git, git's `cmd\\git.exe` shim runs the real git, gh runs git, a venv's
python.exe runs the real interpreter. On a timeout the grandchildren were
left behind - a user found suspended git.exe processes piling up, one set
per `claude agents` poll that ran long.

So every child here gets its own job object on Windows (its own session,
and so process group, elsewhere), and a timeout kills the whole tree. The
job is created before the child runs a single instruction - it starts
suspended - so nothing it spawns can escape it by being quick. And because
the job is KILL_ON_JOB_CLOSE while it is held, a dispatcher that is killed
mid-call takes its children with it.

A child that exits normally is let go, not cleaned up after: `claude --bg`
must leave its session running, and git may leave its fsmonitor daemon.

Standard library only: setup_machine.py uses it before anything is installed.
"""
import os
import signal
import subprocess
import sys
import types

WINDOWS = sys.platform == "win32"

CREATE_NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)
CREATE_SUSPENDED = 0x4
KILL_ON_JOB_CLOSE = 0x2000
# A child that asks to break away may - one that does would otherwise fail
# to start at all. Nothing that does not ask leaves the job.
BREAKAWAY_OK = 0x800
EXTENDED_LIMIT_INFORMATION = 9


def popen(cmd, **kwargs):
    """subprocess.Popen, in a job (Windows) or session (POSIX) of its own.

    End it with `kill_tree`. On Windows no console window is shown unless
    `creationflags` says otherwise.
    """
    if not WINDOWS:
        kwargs.setdefault("start_new_session", True)
        return subprocess.Popen(cmd, **kwargs)
    flags = kwargs.pop("creationflags", CREATE_NO_WINDOW)
    proc = subprocess.Popen(cmd, creationflags=flags | CREATE_SUSPENDED, **kwargs)
    proc._job = _assign_job(proc)
    if not _resume(proc):
        # Never leave a child suspended: that is the very leak this prevents.
        kill_tree(proc)
        raise OSError("could not resume %s" % (cmd[0] if cmd else cmd,))
    return proc


def run(cmd, input=None, timeout=None, capture_output=False, check=False, **kwargs):
    """subprocess.run, except that a timeout kills the whole tree."""
    if capture_output:
        kwargs["stdout"] = kwargs["stderr"] = subprocess.PIPE
    if input is not None:
        kwargs["stdin"] = subprocess.PIPE
    proc = popen(cmd, **kwargs)
    try:
        out, err = proc.communicate(input, timeout=timeout)
    except subprocess.TimeoutExpired as exc:
        kill_tree(proc)
        try:
            exc.stdout, exc.stderr = proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            pass  # something outside the tree holds the pipe; do not hang on it
        raise
    except BaseException:
        kill_tree(proc)
        raise
    release(proc)
    done = subprocess.CompletedProcess(cmd, proc.returncode, out, err)
    if check:
        done.check_returncode()
    return done


def kill_tree(proc):
    """Kill `proc` and everything it started. Safe to call more than once."""
    job = getattr(proc, "_job", None)
    if job:
        proc._job = None
        k32 = _kernel32()
        k32.TerminateJobObject(job, 1)
        k32.CloseHandle(job)
    elif not WINDOWS:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except (OSError, AttributeError):
            pass
    try:
        if proc.poll() is None:
            proc.kill()
        proc.wait(timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        pass


def release(proc):
    """Let whatever `proc` left running carry on without us."""
    job = getattr(proc, "_job", None)
    if job:
        proc._job = None
        _set_limits(job, 0)
        _kernel32().CloseHandle(job)


# -- third-party code ------------------------------------------------------
_safe_subprocess = types.ModuleType("subprocess")
_safe_subprocess.__dict__.update(
    {k: v for k, v in vars(subprocess).items() if not k.startswith("__")})
_safe_subprocess.run = run


def guard(module):
    """Point `module`'s `subprocess.run` at `run`. Best effort.

    For libraries that shell out with a timeout of their own - the Agent
    SDK's session reader runs `git worktree list` with timeout=5.
    """
    if getattr(module, "subprocess", None) is subprocess:
        module.subprocess = _safe_subprocess


def guard_sdk():
    try:
        from claude_agent_sdk._internal import sessions
    except Exception:
        return
    guard(sessions)


# -- Windows ---------------------------------------------------------------
_k32 = None


def _kernel32():
    global _k32
    if _k32 is None:
        import ctypes
        from ctypes import wintypes

        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.CreateJobObjectW.restype = wintypes.HANDLE
        k32.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        k32.SetInformationJobObject.argtypes = [
            wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        k32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        k32.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
        k32.CloseHandle.argtypes = [wintypes.HANDLE]
        _k32 = k32
    return _k32


def _limit_info(flags):
    import ctypes
    from ctypes import wintypes

    class BASIC(ctypes.Structure):
        _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64),
                    ("PerJobUserTimeLimit", ctypes.c_int64),
                    ("LimitFlags", wintypes.DWORD),
                    ("MinimumWorkingSetSize", ctypes.c_size_t),
                    ("MaximumWorkingSetSize", ctypes.c_size_t),
                    ("ActiveProcessLimit", wintypes.DWORD),
                    ("Affinity", ctypes.c_size_t),
                    ("PriorityClass", wintypes.DWORD),
                    ("SchedulingClass", wintypes.DWORD)]

    class IO(ctypes.Structure):
        _fields_ = [(n, ctypes.c_uint64) for n in (
            "ReadOperationCount", "WriteOperationCount",
            "OtherOperationCount", "ReadTransferCount",
            "WriteTransferCount", "OtherTransferCount")]

    class EXTENDED(ctypes.Structure):
        _fields_ = [("BasicLimitInformation", BASIC),
                    ("IoInfo", IO),
                    ("ProcessMemoryLimit", ctypes.c_size_t),
                    ("JobMemoryLimit", ctypes.c_size_t),
                    ("PeakProcessMemoryUsed", ctypes.c_size_t),
                    ("PeakJobMemoryUsed", ctypes.c_size_t)]

    info = EXTENDED()
    info.BasicLimitInformation.LimitFlags = flags
    return info


def _set_limits(job, flags):
    import ctypes

    info = _limit_info(flags)
    return _kernel32().SetInformationJobObject(
        job, EXTENDED_LIMIT_INFORMATION, ctypes.byref(info), ctypes.sizeof(info))


def _assign_job(proc):
    """A fresh kill-on-close job holding `proc`, or None if that failed."""
    try:
        k32 = _kernel32()
        job = k32.CreateJobObjectW(None, None)
        if not job:
            return None
        if (_set_limits(job, KILL_ON_JOB_CLOSE | BREAKAWAY_OK)
                and k32.AssignProcessToJobObject(job, int(proc._handle))):
            return job
        k32.CloseHandle(job)
    except Exception:
        pass
    # Without a job a timeout still kills `proc`, as subprocess.run would.
    return None


def _resume(proc):
    try:
        import ctypes
        from ctypes import wintypes

        ntdll = ctypes.WinDLL("ntdll")
        ntdll.NtResumeProcess.argtypes = [wintypes.HANDLE]
        ntdll.NtResumeProcess.restype = ctypes.c_long
        return ntdll.NtResumeProcess(int(proc._handle)) == 0
    except Exception:
        return False
