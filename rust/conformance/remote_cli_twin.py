"""`imperatorium remote add | remove | check` and `check`, end to end against
a fake ssh that runs the remote command with `sh` in a sandbox HOME - so the
probe script, the binary push, the settings edit and the purge are the real
ones, and nothing leaves this machine. Not part of run_all.py: it needs `sh`
and a built binary.

    python remote_cli_twin.py
"""
import hashlib
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from common import binary  # noqa: E402

FAKE_SSH = r'''
import os, shutil, subprocess, sys
args = sys.argv[1:]
rest, i = [], 0
while i < len(args):
    if args[i] in ("-o", "-R", "-L"):
        i += 2
        continue
    rest.append(args[i])
    i += 1
host, cmd = rest[0], " ".join(rest[1:])
if host == "downhost":
    sys.stderr.write("ssh: connect to host downhost port 22: Connection refused\n")
    sys.exit(255)
# Only the shell's own tools on PATH: this machine's claude must not be found.
env = dict(os.environ, HOME=os.environ["FAKEHOST_HOME"], PATH=os.path.dirname(shutil.which("sh")))
sys.exit(subprocess.run(["sh", "-c", cmd], env=env, stdin=sys.stdin).returncode)
'''


def main():
    failures = []

    def check(name, cond, got=None):
        print("%-62s %s" % (name, "ok" if cond else "FAIL"))
        if not cond:
            failures.append(name)
            if got is not None:
                print("    got: %s" % str(got)[:900])

    tmp = tempfile.mkdtemp(prefix="imp-remote-cli-")
    fakehome = os.path.join(tmp, "hosthome")
    home = os.path.join(tmp, "confighome")
    builds = os.path.join(tmp, "builds", "x86_64-unknown-linux-musl")
    for d in (os.path.join(fakehome, ".local", "bin"), os.path.join(fakehome, ".claude"), home, builds):
        os.makedirs(d)
    claude = os.path.join(fakehome, ".local", "bin", "claude")
    with open(claude, "w", newline="\n") as fh:
        fh.write('#!/bin/sh\necho "2.1.300 (Claude Code)"\n')
    os.chmod(claude, 0o755)
    # A shebang, so a Windows shell counts the file executable (`[ -x ]`).
    payload = b"#!/bin/sh\n# not really an ELF\n"
    with open(os.path.join(builds, "imperatorium"), "wb") as fh:
        fh.write(payload)
    settings = os.path.join(fakehome, ".claude", "settings.json")
    with open(settings, "w") as fh:
        json.dump({"theme": "dark", "hooks": {"Stop": [
            {"hooks": [{"type": "command", "command": "python3",
                        "args": [fakehome + "/.claude/imperatorium/cc_hook.py"]}]},
            {"hooks": [{"type": "command", "command": "someone-else"}]}]}}, fh)
    cfg_path = os.path.join(home, "config.json")
    with open(cfg_path, "w") as fh:
        json.dump({"roots": [tmp]}, fh)
    fake = os.path.join(tmp, "fakehost_ssh.py")
    with open(fake, "w") as fh:
        fh.write(FAKE_SSH)
    shim = os.path.join(tmp, "ssh.cmd")
    with open(shim, "w") as fh:
        fh.write('@"%s" "%s" %%*\r\n' % (sys.executable, fake))
    env = dict(os.environ, IMPERATORIUM_SSH=shim, FAKEHOST_HOME=fakehome,
               IMPERATORIUM_REMOTE_BUILDS=os.path.join(tmp, "builds"),
               IMPERATORIUM_HOME=home)
    for k in [k for k in env if k.startswith("CLAUDE")]:
        del env[k]

    def imp(*args):
        done = subprocess.run([binary()] + list(args) + ["--home", home], env=env,
                              capture_output=True, text=True, timeout=120, stdin=subprocess.DEVNULL)
        return done.returncode, done.stdout.replace("\r", ""), done.stderr.replace("\r", "")

    def config():
        with open(cfg_path) as fh:
            return json.load(fh)

    def hooks():
        with open(settings) as fh:
            return json.load(fh)

    rc, out, _ = imp("remote", "check")
    check("check with no hosts says so", rc == 0 and "no hosts in config.json" in out, out)

    rc, out, err = imp("remote", "add", "fakehost", "-y", "--no-restart", "--label", "Fake")
    check("add exits 0", rc == 0, (out, err))
    check("add probes, pushes, hooks, writes config",
          all(s in out for s in ("1/5 SSH", "2/5 Claude Code on fakehost", "3/5 The binary on fakehost",
                                 "4/5 Reporting hooks on fakehost", "5/5 config.json", "added {")), out)
    binpath = os.path.join(fakehome, ".claude", "imperatorium", "bin", "imperatorium")
    with open(binpath, "rb") as fh:
        check("the binary is on the host, byte for byte", fh.read() == payload)
    h = hooks()
    stop = h["hooks"]["Stop"]
    check("the host's Stop hook runs the binary, the stranger stays",
          len(stop) == 2 and stop[0]["hooks"][0]["command"] == "someone-else"
          and stop[1]["hooks"][0]["args"] == ["hook"]
          and stop[1]["hooks"][0]["command"].endswith("/.claude/imperatorium/bin/imperatorium"), stop)
    check("no permission hook, no tool hooks on the host",
          "PermissionRequest" not in h["hooks"] and "PreToolUse" not in h["hooks"], list(h["hooks"]))
    check("other settings untouched", h["theme"] == "dark")
    baks = [f for f in os.listdir(os.path.join(fakehome, ".claude")) if ".imperatorium-" in f]
    check("settings.json was backed up", len(baks) == 1, baks)
    c = config()
    entry = c["remote"]["hosts"][0]
    check("config.json has the host with a tunnel and the label",
          c["remote"]["enabled"] is True and entry["name"] == "fakehost" and entry["label"] == "Fake"
          and entry["tunnel"] is True and entry["claude"].startswith("$HOME/.local/bin/claude"), c)
    check("config.json.bak kept", os.path.exists(cfg_path + ".bak") or os.path.exists(os.path.join(home, "config.json.bak")))

    rc, out, _ = imp("remote", "check")
    check("check: every row ok", rc == 0 and "MISSING" not in out and "ok       hooks" in out and "ok       binary" in out, out)

    rc, out, _ = imp("remote", "add", "fakehost", "-y", "--no-restart")
    check("add again replaces the entry", rc == 0 and "replaced {" in out, out)
    check("still one host in config", len(config()["remote"]["hosts"]) == 1)

    rc, out, err = imp("remote", "add", "downhost", "-y", "--no-restart")
    check("an unreachable host fails with its reason, exit 1",
          rc == 1 and "still cannot reach downhost" in out and "Connection refused" in out and "ssh-keygen" in out, out)
    check("and is not written to config", len(config()["remote"]["hosts"]) == 1)

    rc, out, _ = imp("remote", "check", "downhost")
    check("check of an unreachable host: MISSING ssh, exit 1", rc == 1 and "MISSING  ssh" in out, out)

    rc, out, err = imp("remote", "remove", "fakehost", "--purge", "-y", "--no-restart")
    check("remove exits 0", rc == 0, (out, err))
    check("hooks of ours are gone, the stranger stays",
          hooks()["hooks"]["Stop"] == [{"hooks": [{"type": "command", "command": "someone-else"}]}], hooks())
    check("--purge removed ~/.claude/imperatorium", not os.path.exists(os.path.join(fakehome, ".claude", "imperatorium")))
    check("the host is out of config.json", config()["remote"]["hosts"] == [])
    rc, out, _ = imp("remote", "remove", "fakehost", "-y", "--no-restart")
    check("removing it again says it was not there", rc == 0 and "was not in config.json" in out, out)

    rc, out, err = imp("remote", "add", "fakehost", "-y", "--no-restart", "--no-tunnel")
    check("add --no-tunnel skips the hooks, writes no tunnel",
          rc == 0 and "skipped (--no-tunnel)" in out and "tunnel" not in config()["remote"]["hosts"][0], (out, err))
    rc, out, _ = imp("remote")
    check("remote with no action is check", rc == 0 and "fakehost" in out, out)
    rc, out, _ = imp("remote", "add")
    check("add without a host: usage, exit 2", rc == 2 and "usage: imperatorium remote add HOST" in out, out)

    # No binary build for the host: the add stops with what to do.
    os.remove(os.path.join(builds, "imperatorium"))
    os.remove(os.path.join(fakehome, ".claude", "imperatorium", "bin", "imperatorium"))
    rc, out, _ = imp("remote", "add", "fakehost", "-y", "--no-restart")
    check("no local build: exit 1 and says where to put one",
          rc == 1 and "no x86_64-unknown-linux-musl build" in out and "IMPERATORIUM_REMOTE_BUILDS" in out, out)

    # `check`: changes nothing, says what is there.
    rc, out, _ = imp("check")
    check("check prints its sections", all(s in out for s in ("== Prerequisites", "== This installation",
                                                              "== Claude Code hooks", "== Service")), out)
    check("check exit code follows the required pieces", rc in (0, 1))

    print("\n%d failed" % len(failures) if failures else "\nall ok")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
