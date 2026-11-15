//! Test support only: a stand-in for `ssh` (IMPERATORIUM_SSH) for the remote
//! tests. No network, no host. cargo builds it beside the tests
//! (target/<profile>/examples/fakessh).
//!
//!     fakessh [ssh options] HOST [REMOTE COMMAND...]
//!
//! $FAKESSH_DIR holds
//! - `rules.json`: an array, re-read on every call; the first rule whose
//!   `match` is a substring of the remote command (and whose `host`, when
//!   given, is the host) answers. Fields: `code`, `stdout`, `stderr`,
//!   `sleep_ms` (before answering), and for a long-lived command either
//!   `canned` ({op: reply fields}: every JSON line on stdin gets its op's
//!   fields plus the request's id and ok:true) or `never_reply` (read
//!   stdin, answer nothing) or `run` ([program, args...] executed with
//!   stdio inherited, `env` added: the real `imperatorium remote-channel`
//!   on a stand-in host). `no_stdin` answers without waiting for stdin to
//!   close (a channel that dies at once).
//! - `log.jsonl`: one record per call: {host, cmd, tunnel, stdin, args}.
//!
//! No rule matches: exit 3 with a message, so a test notices a command it
//! did not expect.

use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::PathBuf::from(std::env::var("FAKESSH_DIR").expect("FAKESSH_DIR"));
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if matches!(a.as_str(), "-o" | "-R" | "-L") {
            it.next();
            continue;
        }
        rest.push(a.clone());
    }
    let tunnel = rest.iter().any(|a| a == "-N");
    rest.retain(|a| a != "-N");
    let host = rest.first().cloned().unwrap_or_default();
    let cmd = rest[1.min(rest.len())..].join(" ");
    let log = |rec: Value| {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(dir.join("log.jsonl")) {
            let _ = writeln!(f, "{rec}");
        }
    };
    let rules: Vec<Value> = std::fs::read_to_string(dir.join("rules.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let rule = rules.iter().find(|r| {
        r["match"].as_str().is_none_or(|m| cmd.contains(m)) && r["host"].as_str().is_none_or(|h| h == host)
    });
    if tunnel {
        log(json!({"host": host, "tunnel": true, "args": args}));
        // Held open like a real tunnel, but not forever.
        std::thread::sleep(std::time::Duration::from_secs(20));
        return;
    }
    let streaming = rule.is_some_and(|r| r.get("canned").is_some() || r.get("never_reply").is_some() || r.get("run").is_some() || r.get("no_stdin").is_some());
    let stdin = if streaming {
        String::new()
    } else {
        let mut raw = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut raw);
        String::from_utf8_lossy(&raw).into_owned()
    };
    log(json!({"host": host, "cmd": cmd, "tunnel": false, "stdin": stdin, "args": args}));
    let Some(rule) = rule else {
        eprintln!("fakessh: unexpected command on {host}: {cmd}");
        std::process::exit(3);
    };
    if let Some(ms) = rule["sleep_ms"].as_u64() {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    if let Some(run) = rule["run"].as_array() {
        let words: Vec<&str> = run.iter().filter_map(Value::as_str).collect();
        let mut c = std::process::Command::new(words[0]);
        c.args(&words[1..]);
        for (k, v) in rule["env"].as_object().into_iter().flatten() {
            c.env(k, v.as_str().unwrap_or(""));
        }
        let code = c.status().map(|s| s.code().unwrap_or(1)).unwrap_or(127);
        std::process::exit(code);
    }
    if rule.get("never_reply").is_some() {
        for _ in std::io::stdin().lock().lines() {}
        return;
    }
    if let Some(canned) = rule["canned"].as_object() {
        let out = std::io::stdout();
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
            let mut reply = match canned.get(req["op"].as_str().unwrap_or("")) {
                Some(Value::Object(o)) => Value::Object(o.clone()),
                _ => json!({"ok": false, "error": "ValueError: unknown op"}),
            };
            if reply.get("ok").is_none() {
                reply["ok"] = json!(true);
            }
            reply["id"] = req["id"].clone();
            let mut o = out.lock();
            let _ = writeln!(o, "{reply}");
            let _ = o.flush();
        }
        return;
    }
    print!("{}", rule["stdout"].as_str().unwrap_or(""));
    eprint!("{}", rule["stderr"].as_str().unwrap_or(""));
    let _ = std::io::stdout().flush();
    std::process::exit(rule["code"].as_i64().unwrap_or(0) as i32);
}
