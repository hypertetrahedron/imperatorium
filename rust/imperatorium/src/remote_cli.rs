//! `imperatorium remote add | remove | check [HOST]`: onboarding another
//! machine reached over SSH (setup_machine.py's `remote`), with the binary
//! instead of Python.
//!
//! The dispatcher already pushes the binary to a host (`remote::Deploy`),
//! holds its tunnel and reads its sessions through it, so `add` only has to
//! probe the host, make sure the binary is there, point the host's Claude
//! Code hooks at it, and write the host into config.json. Nothing runs on
//! the host but `sh` and the binary: no python3, no reader venv.

use std::io::{IsTerminal, Write};

use serde_json::{json, Map, Value};

use crate::cli::{parse, Args, Spec};
use crate::pyish::{json_dumps, json_dumps_indent, truthy};
use crate::remote;

const MIN_REMOTE_CLAUDE: (u32, u32, u32) = (2, 1, 224);
const CLAUDE_INSTALL: &str = "curl -fsSL https://claude.ai/install.sh | bash";
const BIN_REL: &str = ".claude/imperatorium/bin/imperatorium";

/// One round trip that answers everything `remote add` needs to know.
/// `claude` is looked for by absolute path because a non-interactive SSH
/// session on Ubuntu never reaches the PATH line in .bashrc.
const PROBE: &str = r#"printf 'home=%s\n' "$HOME"
for c in "$(command -v claude 2>/dev/null)" "$HOME/.local/bin/claude" "$HOME/.claude/local/claude" /usr/local/bin/claude /opt/homebrew/bin/claude; do
  if [ -n "$c" ] && [ -x "$c" ]; then printf 'claude=%s\n' "$c"; printf 'version=%s\n' "$("$c" --version 2>/dev/null </dev/null | head -n 1)"; break; fi
done
if [ -x "$HOME/.claude/imperatorium/bin/imperatorium" ]; then echo binary=ok; fi
if [ -d "$HOME/.claude/ccontrol" ] && [ ! -e "$HOME/.claude/imperatorium" ]; then echo legacy=ok; fi
if [ -f "$HOME/.claude/settings.json" ] && grep -q 'imperatorium/bin/imperatorium' "$HOME/.claude/settings.json"; then echo hooks=ok; fi
if [ -f "$HOME/.claude/settings.json" ] && grep -q 'imperatorium/cc_hook.py' "$HOME/.claude/settings.json"; then echo pyhooks=ok; fi
true"#;

const SPEC: Spec = Spec {
    valued: &[("", "label")],
    flags: &[("", "no-tunnel"), ("", "install-claude"), ("", "no-restart"), ("y", "yes"), ("", "purge"), ("", "json")],
};

fn say(s: impl AsRef<str>) {
    println!("{}", s.as_ref());
    let _ = std::io::stdout().flush();
}

fn step(title: &str) {
    say(format!("\n== {title}"));
}

fn ver_text(v: (u32, u32, u32)) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

// -- what the probe says -----------------------------------------------------------

#[derive(Debug, Default, PartialEq)]
pub struct Facts {
    pub home: String,
    pub claude: Option<String>,
    pub version: Option<(u32, u32, u32)>,
    /// The binary is on the host.
    pub binary: bool,
    /// ~/.claude/ccontrol from before the rename, with no ~/.claude/imperatorium.
    pub legacy: bool,
    /// settings.json runs our binary.
    pub hooks: bool,
    /// settings.json still runs the Python hook script.
    pub pyhooks: bool,
}

fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let re = regex::Regex::new(r"(\d+)\.(\d+)\.(\d+)").ok()?;
    let c = re.captures(text)?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?))
}

pub fn parse_probe(text: &str) -> Facts {
    let mut f = Facts::default();
    let mut seen: Vec<&str> = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        if seen.contains(&key) {
            continue; // the first answer to a key wins, as setdefault
        }
        let value = value.trim();
        match key {
            "home" => f.home = value.to_string(),
            "claude" => f.claude = Some(value.to_string()),
            "version" => f.version = parse_version(value),
            "binary" => f.binary = true,
            "legacy" => f.legacy = true,
            "hooks" => f.hooks = true,
            "pyhooks" => f.pyhooks = true,
            _ => continue,
        }
        seen.push(key);
    }
    f
}

/// One ssh call, or what went wrong in a line.
fn ssh(host: &str, cmd: &str, input: Option<Vec<u8>>, timeout: f64) -> Result<String, String> {
    let done = remote::run(host, cmd, input, timeout)?;
    if done.code != 0 {
        let said = String::from_utf8_lossy(&done.stderr).trim().to_string();
        return Err(if said.is_empty() { format!("ssh exited {}", done.code) } else { said });
    }
    Ok(String::from_utf8_lossy(&done.stdout).into_owned())
}

fn probe(host: &str) -> Result<Facts, String> {
    // The script goes in on stdin (`sh -s`): a multi-line command line is not
    // something every ssh wrapper passes through.
    ssh(host, "sh -s", Some(PROBE.as_bytes().to_vec()), 30.0).map(|out| parse_probe(&out))
}

// -- config.json -----------------------------------------------------------------------

fn host_name(h: &Value) -> Option<String> {
    match h {
        Value::String(s) => Some(s.clone()),
        other => other.get("name").and_then(Value::as_str).map(str::to_string),
    }
}

/// `$HOME/.local/bin/claude` rather than `/home/me/...`, as config.json writes it.
pub fn home_relative(path: &str, home: &str) -> String {
    let h = home.trim_end_matches('/');
    if !home.is_empty() && (path == home || path.starts_with(&format!("{h}/"))) {
        return format!("$HOME{}", &path[h.len()..]);
    }
    path.to_string()
}

/// The config.json entry for a host.
pub fn host_entry(name: &str, claude: Option<&str>, label: Option<&str>, tunnel: bool) -> Value {
    let mut e = Map::new();
    e.insert("name".into(), json!(name));
    e.insert("claude".into(), json!(claude.filter(|c| !c.is_empty()).unwrap_or(remote::DEFAULT_CLAUDE)));
    if let Some(l) = label.filter(|l| !l.is_empty() && *l != name) {
        e.insert("label".into(), json!(l));
    }
    if tunnel {
        e.insert("tunnel".into(), json!(true));
    }
    Value::Object(e)
}

/// `cfg` with `entry` added, or replacing the host of the same name. Keeps
/// any keys the existing entry had that this one does not set (python,
/// ports, a tunnel's own port numbers...).
pub fn upsert_host(cfg: &Value, entry: &Value) -> (Value, &'static str) {
    let mut cfg = cfg.clone();
    if !cfg.is_object() {
        cfg = json!({});
    }
    let root = cfg.as_object_mut().expect("an object");
    let block = root.entry("remote").or_insert_with(|| json!({}));
    if !block.is_object() {
        *block = json!({});
    }
    let block = block.as_object_mut().expect("an object");
    block.insert("enabled".into(), json!(true));
    let hosts = block.entry("hosts").or_insert_with(|| json!([]));
    if !hosts.is_array() {
        *hosts = json!([]);
    }
    let hosts = hosts.as_array_mut().expect("an array");
    let name = host_name(entry);
    for slot in hosts.iter_mut() {
        let old = match &*slot {
            Value::String(s) => json!({"name": s}),
            other => other.clone(),
        };
        if host_name(&old) == name {
            let mut merged = old.as_object().cloned().unwrap_or_default();
            for (k, v) in entry.as_object().cloned().unwrap_or_default() {
                // Ports the user wrote down stay when the new entry just says `true`.
                if k == "tunnel" && v == json!(true) && merged.get("tunnel").is_some_and(|t| t.is_object() && truthy(Some(t))) {
                    continue;
                }
                merged.insert(k, v);
            }
            if !truthy(entry.get("tunnel")) {
                merged.shift_remove("tunnel");
            }
            *slot = Value::Object(merged);
            return (cfg, "replaced");
        }
    }
    hosts.push(entry.clone());
    (cfg, "added")
}

pub fn remove_host(cfg: &Value, name: &str) -> (Value, bool) {
    let mut cfg = cfg.clone();
    let mut changed = false;
    if let Some(hosts) = cfg.get_mut("remote").and_then(|r| r.get_mut("hosts")).and_then(Value::as_array_mut) {
        let before = hosts.len();
        hosts.retain(|h| host_name(h).as_deref() != Some(name));
        changed = hosts.len() != before;
    }
    (cfg, changed)
}

fn config_path(home: Option<&str>) -> std::path::PathBuf {
    crate::paths::config_home(home).join("config.json")
}

fn load_config(home: Option<&str>) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(config_path(home)) {
        Ok(t) => serde_json::from_str(&t).map(Some).map_err(|e| format!("{} is not valid JSON ({e})", config_path(home).display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", config_path(home).display())),
    }
}

/// Written atomically, keeping a copy of what it replaced.
fn save_config(home: Option<&str>, cfg: &Value) -> Result<(), String> {
    let path = config_path(home);
    if path.exists() {
        let _ = std::fs::copy(&path, path.with_extension("json.bak"));
    }
    crate::paths::write_atomic(&path, (json_dumps_indent(cfg, 2) + "\n").as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    say("   updated config.json (previous copy: config.json.bak)");
    Ok(())
}

// -- questions ---------------------------------------------------------------------------

/// A yes/no question. `assume_yes` (or no terminal to ask on) takes the default.
fn ask(question: &str, default: bool, assume_yes: bool) -> bool {
    if assume_yes || !std::io::stdin().is_terminal() {
        return default;
    }
    loop {
        print!("   {question} [{}] ", if default { "Y/n" } else { "y/N" });
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            return default;
        }
        match line.trim().to_lowercase().as_str() {
            "" => return default,
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => {}
        }
    }
}

fn public_key() -> Option<std::path::PathBuf> {
    let dir = crate::paths::user_home().join(".ssh");
    ["id_ed25519.pub", "id_ecdsa.pub", "id_rsa.pub"].iter().map(|n| dir.join(n)).find(|p| p.exists())
}

/// Walk through key-based SSH login. True if it may work now.
fn offer_key_login(host: &str, yes: bool) -> bool {
    if yes || !std::io::stdin().is_terminal() {
        say("   set up key login first, then run this again:");
        say("     ssh-keygen -t ed25519         (if you have no key)");
        say(format!("     ssh-copy-id {host}                (or append ~/.ssh/id_ed25519.pub to its"));
        say("                                    ~/.ssh/authorized_keys by hand)");
        return false;
    }
    let mut key = public_key();
    if key.is_none() {
        if !ask("You have no SSH key. Create one now (ssh-keygen -t ed25519)?", true, yes) {
            return false;
        }
        let target = crate::paths::user_home().join(".ssh").join("id_ed25519");
        let _ = std::process::Command::new("ssh-keygen").args(["-t", "ed25519", "-f"]).arg(&target).status();
        key = public_key();
    }
    let Some(key) = key else { return false };
    if !ask(&format!("Copy {} to {host}? You will be asked for its password once", key.display()), true, yes) {
        return false;
    }
    let Ok(pubkey) = std::fs::read_to_string(&key) else { return false };
    let pubkey = pubkey.trim().to_string();
    // ssh-copy-id does not exist on Windows; this is what it does.
    let script = format!(
        "umask 077; mkdir -p ~/.ssh && touch ~/.ssh/authorized_keys && (grep -qxF {q} ~/.ssh/authorized_keys || cat >> ~/.ssh/authorized_keys)",
        q = remote::sh_quote(&pubkey)
    );
    let program = crate::paths::env_nonempty("IMPERATORIUM_SSH").unwrap_or_else(|| "ssh".into());
    let child = std::process::Command::new(program)
        .args(["-o", "StrictHostKeyChecking=accept-new", host, &script])
        .stdin(std::process::Stdio::piped())
        .spawn();
    if let Ok(mut c) = child {
        if let Some(mut sin) = c.stdin.take() {
            let _ = sin.write_all(format!("{pubkey}\n").as_bytes());
        }
        let _ = c.wait();
    }
    true
}

// -- the hooks on the host ---------------------------------------------------------------

/// The same reporting hooks as here, on the host, pointing at the binary the
/// dispatcher keeps there. False (having said why) if it could not.
fn install_remote_hooks(host: &str, home: &str, uninstall: bool) -> bool {
    match remote_hooks(host, home, uninstall) {
        Ok(()) => true,
        Err(e) => {
            say(format!("   {e}"));
            false
        }
    }
}

fn remote_hooks(host: &str, home: &str, uninstall: bool) -> Result<(), String> {
    if !home.starts_with('/') {
        return Err(format!("{host}: could not read $HOME (got {home:?})"));
    }
    let path = format!("{home}/.claude/settings.json");
    let raw = ssh(host, &format!("cat {} 2>/dev/null || true", remote::sh_quote(&path)), None, 60.0).map_err(|e| format!("{host}: {e}"))?;
    let mut settings = if raw.trim().is_empty() {
        Map::new()
    } else {
        match serde_json::from_str::<Value>(&raw) {
            Ok(Value::Object(o)) => o,
            Ok(_) => Map::new(),
            Err(e) => return Err(format!("{host}:{path} is not valid JSON ({e}). Fix it by hand first.")),
        }
    };
    let binary = format!("{home}/{BIN_REL}");
    if !uninstall {
        let there = ssh(host, &format!("[ -x {} ] && echo yes || true", remote::sh_quote(&binary)), None, 60.0).map_err(|e| format!("{host}: {e}"))?;
        if there.trim() != "yes" {
            return Err(format!("{host}: {binary} is not there yet. The dispatcher puts it there on first use; to do it now: imperatorium remote add {host}"));
        }
    }
    let changes = crate::install::apply_remote_hooks(&mut settings, uninstall, &binary);
    say(format!("settings : {host}:{path}"));
    say(format!("hooks    : {binary} hook"));
    if changes.is_empty() {
        say("  (nothing to do)");
    }
    for c in &changes {
        say(format!("  {c}"));
    }
    let stamp = crate::install::stamp();
    let body = (json_dumps_indent(&Value::Object(settings), 2) + "\n").into_bytes();
    let q = remote::sh_quote(&path);
    let write = format!(
        "mkdir -p {d}; [ -f {q} ] && cp {q} {q}.imperatorium-{stamp}.bak; cat > {q}.imperatorium.tmp && mv {q}.imperatorium.tmp {q}",
        d = remote::sh_quote(&format!("{home}/.claude")),
    );
    ssh(host, &write, Some(body), 60.0).map_err(|e| format!("{host}: {e}"))?;
    say(format!("backup   : {path}.imperatorium-{stamp}.bak (if there was a file)"));
    say("written. Restart a session there for the hooks to take effect.");
    Ok(())
}

fn restart_dispatcher(yes: bool, no_restart: bool, home: Option<&str>) {
    let svc = crate::service::Service::new("Imperatorium".into(), std::env::current_exe().unwrap_or_default(), crate::paths::config_home(home), vec![]);
    if no_restart || !svc.is_installed("Dispatcher") {
        say("   restart the dispatcher yourself so it picks up the change");
        return;
    }
    if ask("Restart the dispatcher now so it picks this up?", true, yes) {
        crate::cli::restart_dispatcher(home);
    }
}

// -- add, remove, check ---------------------------------------------------------------------

fn remote_add(host: &str, a: &Args, home: Option<&str>) -> i32 {
    let yes = a.flag("yes");
    say(format!("Imperatorium - onboarding {host}"));
    // A host already in config.json may name another ssh destination or a container.
    if let Ok(Some(cfg)) = load_config(home) {
        remote::load_hosts(&cfg);
    }

    step("1/5 SSH");
    let mut facts = match probe(host) {
        Ok(f) => f,
        Err(err) => {
            say(format!("   cannot log in without a password: {err}"));
            let again = if offer_key_login(host, yes) { probe(host) } else { Err(err.clone()) };
            match again {
                Ok(f) => f,
                Err(err) => {
                    say(format!("   still cannot reach {host}: {err}"));
                    return 1;
                }
            }
        }
    };
    say(format!("   ok  {host}, home {}", facts.home));
    if facts.legacy {
        // Set up before the rename: the folder moves across.
        match ssh(host, "mv ~/.claude/ccontrol ~/.claude/imperatorium", None, 60.0) {
            Ok(_) => say("   moved ~/.claude/ccontrol to ~/.claude/imperatorium"),
            Err(e) => say(format!("   could not move ~/.claude/ccontrol: {e}")),
        }
        facts = match probe(host) {
            Ok(f) => f,
            Err(e) => {
                say(format!("   cannot reach {host}: {e}"));
                return 1;
            }
        };
    }

    step(&format!("2/5 Claude Code on {host}"));
    let mut ver = facts.version;
    if facts.claude.is_none() || ver.is_none_or(|v| v < MIN_REMOTE_CLAUDE) {
        match ver {
            Some(v) => say(format!("   Claude Code {} is older than {}", ver_text(v), ver_text(MIN_REMOTE_CLAUDE))),
            None => say("   Claude Code is not installed"),
        }
        if a.flag("install-claude") || ask(&format!("Install/update it there with the official installer ({CLAUDE_INSTALL})?"), false, yes) {
            if let Err(e) = ssh(host, CLAUDE_INSTALL, None, 600.0) {
                let tail: String = e.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
                say(format!("   the installer failed: {tail}"));
                return 1;
            }
            if let Ok(f) = probe(host) {
                facts = f;
            }
            ver = facts.version;
        }
        match ver {
            Some(v) if v >= MIN_REMOTE_CLAUDE && facts.claude.is_some() => {}
            _ => {
                say(format!("   install it there, then run this again:  ssh {host} '{CLAUDE_INSTALL}'"));
                return 1;
            }
        }
        say(format!("   log in once on {host} if you have not:  ssh -t {host} {}   (then /login)", facts.claude.clone().unwrap_or_default()));
    }
    let claude = facts.claude.clone().unwrap_or_default();
    say(format!("   ok  {} at {claude}", ver.map(ver_text).unwrap_or_default()));

    step(&format!("3/5 The binary on {host}"));
    match remote::Deploy::new(host).ensure() {
        Ok(()) => say(format!("   ok  ~/{BIN_REL} matches this machine's build")),
        Err(e) => {
            say(format!("   {e}"));
            say("   the dispatcher pushes the host's build of imperatorium (a static Linux binary): put one beside this");
            say("   program under targets/<triple>/imperatorium, or point IMPERATORIUM_REMOTE_BUILDS at a folder of them");
            return 1;
        }
    }

    let tunnel = !a.flag("no-tunnel");
    step(&format!("4/5 Reporting hooks on {host}"));
    if tunnel {
        if !install_remote_hooks(host, &facts.home, false) {
            return 1;
        }
    } else {
        say("   skipped (--no-tunnel): the host is polled, but prompts can only start new sessions");
    }

    step("5/5 config.json");
    let cfg = match load_config(home) {
        Ok(Some(c)) => c,
        Ok(None) => {
            say(format!("   no config.json yet in {} - run `imperatorium install` first", crate::paths::config_home(home).display()));
            return 1;
        }
        Err(e) => {
            say(format!("   {e}"));
            return 1;
        }
    };
    let entry = host_entry(host, Some(&home_relative(&claude, &facts.home)), a.opt("label"), tunnel);
    let (cfg, how) = upsert_host(&cfg, &entry);
    if let Err(e) = save_config(home, &cfg) {
        say(format!("   {e}"));
        return 1;
    }
    say(format!("   {how} {}", json_dumps(&entry, true)));
    restart_dispatcher(yes, a.flag("no-restart"), home);

    say(format!("\nDone. {host}'s sessions show in the window within ~20s of the dispatcher starting."));
    if tunnel {
        say("Sessions already running there report in on their next turn; restart them to be sure.");
    }
    0
}

fn remote_remove(host: &str, a: &Args, home: Option<&str>) -> i32 {
    let yes = a.flag("yes");
    if let Ok(Some(cfg)) = load_config(home) {
        remote::load_hosts(&cfg);
    }
    step(&format!("Hooks on {host}"));
    match probe(host) {
        Ok(f) => {
            install_remote_hooks(host, &f.home, true);
        }
        Err(e) => say(format!("   cannot reach {host}: {e}")),
    }
    if a.flag("purge") || ask(&format!("Also delete ~/.claude/imperatorium (the binary) on {host}?"), false, yes) {
        match ssh(host, "rm -rf ~/.claude/imperatorium ~/.claude/ccontrol", None, 60.0) {
            Ok(_) => say("   removed ~/.claude/imperatorium"),
            Err(e) => say(format!("   could not remove it: {e}")),
        }
    }
    step("config.json");
    match load_config(home) {
        Ok(Some(cfg)) => {
            let (cfg, changed) = remove_host(&cfg, host);
            if changed {
                if let Err(e) = save_config(home, &cfg) {
                    say(format!("   {e}"));
                    return 1;
                }
                restart_dispatcher(yes, a.flag("no-restart"), home);
            } else {
                say(format!("   {host} was not in config.json"));
            }
        }
        Ok(None) => {}
        Err(e) => say(format!("   {e}")),
    }
    0
}

/// The rows of `remote check` for one probed host: (mark, what, detail).
pub fn check_rows(facts: &Facts, tunnel: bool) -> Vec<(&'static str, &'static str, String)> {
    let claude_ok = facts.version.is_some_and(|v| v >= MIN_REMOTE_CLAUDE);
    let hooks_ok = facts.hooks || !tunnel;
    vec![
        ("ok ", "ssh", "key login works".to_string()),
        (
            if claude_ok { "ok " } else { "MISSING" },
            "claude",
            facts.version.map(|v| format!("{} at {}", ver_text(v), facts.claude.clone().unwrap_or_default())).unwrap_or_else(|| "not found".into()),
        ),
        (
            if facts.binary { "ok " } else { "-- " },
            "binary",
            if facts.binary { "installed".to_string() } else { "not there yet - the dispatcher puts it there on first use".to_string() },
        ),
        (
            if hooks_ok { "ok " } else { "MISSING" },
            "hooks",
            if facts.hooks {
                "installed".to_string()
            } else if facts.pyhooks {
                "the Python hook script - `remote add` moves them to the binary".to_string()
            } else if tunnel {
                "not installed".to_string()
            } else {
                "not used (no tunnel)".to_string()
            },
        ),
    ]
}

fn remote_check(hosts: &[Value]) -> i32 {
    if hosts.is_empty() {
        say("   no hosts in config.json. Add one:  imperatorium remote add HOST");
        return 0;
    }
    let mut bad = 0;
    for entry in hosts {
        let Some(name) = host_name(entry) else { continue };
        step(&name);
        let facts = match probe(&name) {
            Ok(f) => f,
            Err(e) => {
                say(format!("   MISSING  ssh           {e}"));
                bad += 1;
                continue;
            }
        };
        let tunnel = entry.is_object() && truthy(entry.get("tunnel"));
        for (mark, what, detail) in check_rows(&facts, tunnel) {
            say(format!("   {mark:<8} {what:<12} {detail}"));
            if mark == "MISSING" {
                bad += 1;
            }
        }
    }
    if bad > 0 {
        say("\nFix a host with:  imperatorium remote add HOST");
    }
    i32::from(bad > 0)
}

pub fn main(args: &[String], home: Option<&str>) -> i32 {
    let a = match parse(args, &SPEC) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("usage: imperatorium remote add|remove|check [HOST] [options]\nimperatorium remote: error: {e}");
            return 2;
        }
    };
    let home = a.opt("home").or(home);
    let action = a.pos.first().map(String::as_str).unwrap_or("check");
    let host = a.pos.get(1).map(String::as_str);
    match action {
        "check" => {
            let cfg = load_config(home).ok().flatten().unwrap_or(json!({}));
            remote::load_hosts(&cfg);
            let mut hosts = cfg.get("remote").and_then(|r| r.get("hosts")).and_then(Value::as_array).cloned().unwrap_or_default();
            if let Some(h) = host {
                hosts.retain(|x| host_name(x).as_deref() == Some(h));
                if hosts.is_empty() {
                    hosts = vec![json!(h)];
                }
            }
            remote_check(&hosts)
        }
        "add" | "remove" => match host {
            Some(h) if action == "add" => remote_add(h, &a, home),
            Some(h) => remote_remove(h, &a, home),
            None => {
                say(format!("usage: imperatorium remote {action} HOST   (user@host or an ~/.ssh/config alias)"));
                2
            }
        },
        _ => {
            say("usage: imperatorium remote add|remove|check [HOST]");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_is_read() {
        let f = parse_probe("home=/home/me\nclaude=/home/me/.local/bin/claude\nversion=2.1.300 (Claude Code)\nbinary=ok\nhooks=ok\nhome=/other\nnoise\n");
        assert_eq!(f.home, "/home/me");
        assert_eq!(f.claude.as_deref(), Some("/home/me/.local/bin/claude"));
        assert_eq!(f.version, Some((2, 1, 300)));
        assert!(f.binary && f.hooks && !f.legacy && !f.pyhooks);
        assert_eq!(parse_probe("home=/h\nversion=\n").version, None);
        assert_eq!(parse_probe(""), Facts::default());
    }

    #[test]
    fn claude_is_written_home_relative() {
        assert_eq!(home_relative("/home/me/.local/bin/claude", "/home/me"), "$HOME/.local/bin/claude");
        assert_eq!(home_relative("/usr/local/bin/claude", "/home/me"), "/usr/local/bin/claude");
        assert_eq!(home_relative("/home/me2/claude", "/home/me"), "/home/me2/claude");
        assert_eq!(home_relative("/x", ""), "/x");
    }

    #[test]
    fn entries_follow_the_python_shape() {
        assert_eq!(json_dumps(&host_entry("h", Some("$HOME/.local/bin/claude"), None, true), true), r#"{"name": "h", "claude": "$HOME/.local/bin/claude", "tunnel": true}"#);
        assert_eq!(json_dumps(&host_entry("h", None, Some("Box"), false), true), r#"{"name": "h", "claude": "$HOME/.local/bin/claude", "label": "Box"}"#);
        // A label equal to the name is no label.
        assert_eq!(host_entry("h", Some("c"), Some("h"), false).get("label"), None);
    }

    #[test]
    fn a_host_is_added_replaced_and_removed() {
        let cfg = json!({"roots": ["/p"]});
        let (cfg, how) = upsert_host(&cfg, &host_entry("a", Some("c"), None, true));
        assert_eq!(how, "added");
        assert_eq!(cfg["remote"]["enabled"], json!(true));
        assert_eq!(cfg["roots"], json!(["/p"]));
        let (cfg, _) = upsert_host(&cfg, &host_entry("b", Some("c"), None, false));
        assert_eq!(cfg["remote"]["hosts"].as_array().unwrap().len(), 2);
        // Replacing keeps what the entry does not set, and a tunnel's own ports.
        let mut cfg = cfg;
        cfg["remote"]["hosts"][0] = json!({"name": "a", "claude": "old", "python": "python3.11", "tunnel": {"local_port": 9001}});
        let (cfg, how) = upsert_host(&cfg, &host_entry("a", Some("new"), Some("A"), true));
        assert_eq!(how, "replaced");
        assert_eq!(cfg["remote"]["hosts"][0], json!({"name": "a", "claude": "new", "python": "python3.11", "tunnel": {"local_port": 9001}, "label": "A"}));
        // --no-tunnel on an existing host turns its tunnel off.
        let (cfg, _) = upsert_host(&cfg, &host_entry("a", Some("new"), None, false));
        assert!(cfg["remote"]["hosts"][0].get("tunnel").is_none());
        // A host written as a bare string is replaced, too.
        let (cfg, how) = upsert_host(&json!({"remote": {"hosts": ["s"]}}), &host_entry("s", Some("c"), None, true));
        assert_eq!((how, cfg["remote"]["hosts"].as_array().unwrap().len()), ("replaced", 1));
        let (cfg, changed) = remove_host(&cfg, "s");
        assert!(changed && cfg["remote"]["hosts"].as_array().unwrap().is_empty());
        assert!(!remove_host(&cfg, "s").1);
        assert!(!remove_host(&json!({}), "s").1);
    }

    #[test]
    fn the_hooks_on_a_host_are_reporting_only() {
        let bin = "/home/me/.claude/imperatorium/bin/imperatorium";
        let mut s: Map<String, Value> = serde_json::from_value(json!({"theme": "dark", "hooks": {
            "Stop": [{"hooks": [{"type": "command", "command": "python3", "args": ["/home/me/.claude/imperatorium/cc_hook.py"]}]},
                     {"hooks": [{"type": "command", "command": "someone-else"}]}]}})).unwrap();
        let changes = crate::install::apply_remote_hooks(&mut s, false, bin);
        assert!(changes.contains(&"replace Stop".to_string()) && changes.contains(&"add SessionStart".to_string()), "{changes:?}");
        assert!(!changes.iter().any(|c| c.ends_with("PermissionRequest")));
        let hooks = s["hooks"].as_object().unwrap();
        assert!(!hooks.contains_key("PermissionRequest") && !hooks.contains_key("PreToolUse"));
        let stop = hooks["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "the stranger's hook stays");
        assert_eq!(stop[1]["hooks"][0]["command"], json!(bin));
        assert_eq!(stop[1]["hooks"][0]["args"], json!(["hook"]));
        assert_eq!(s["theme"], json!("dark"));
        // Taking them away leaves the stranger's.
        crate::install::apply_remote_hooks(&mut s, true, bin);
        assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(s["hooks"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn check_rows_say_what_is_missing() {
        let mut f = Facts { home: "/h".into(), claude: Some("/h/c".into()), version: Some((2, 1, 300)), binary: true, hooks: true, ..Facts::default() };
        let marks = |f: &Facts, t| check_rows(f, t).iter().map(|r| r.0).collect::<Vec<_>>();
        assert_eq!(marks(&f, true), ["ok ", "ok ", "ok ", "ok "]);
        f.binary = false;
        f.hooks = false;
        assert_eq!(marks(&f, true), ["ok ", "ok ", "-- ", "MISSING"]);
        assert_eq!(marks(&f, false), ["ok ", "ok ", "-- ", "ok "]);
        assert_eq!(check_rows(&f, false)[3].2, "not used (no tunnel)");
        f.pyhooks = true;
        assert!(check_rows(&f, true)[3].2.contains("Python hook script"));
        f.version = Some((2, 1, 100));
        assert_eq!(marks(&f, true)[1], "MISSING");
        f.version = None;
        assert_eq!(check_rows(&f, true)[1].2, "not found");
    }
}

/// test_rename.py: a host from before the rename is noticed.
#[cfg(test)]
mod core_tests {
    use super::*;

    #[test]
    fn a_host_from_before_the_rename_is_noticed() {
        let f = parse_probe("home=/home/les\nlegacy=ok\n");
        assert!(f.legacy);
        assert_eq!(f.home, "/home/les");
        assert!(!parse_probe("home=/home/les\n").legacy);
    }
}
