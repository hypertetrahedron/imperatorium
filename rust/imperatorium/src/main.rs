//! `imperatorium`: one binary for every part of Imperatorium.
//!
//! The port is phased (docs/native-port.md). Phase 1 is what Claude Code
//! runs on every event, so it goes first:
//!
//!     imperatorium hook                    # was hooks/cc_hook.py
//!     imperatorium statusline              # was hooks/cc_statusline.py
//!     imperatorium permission [--home DIR] # was hooks/cc_permission.py
//!
//! Phase 2 reads transcripts without the Agent SDK:
//!
//!     imperatorium sessions chain <id>     # the conversation chain, compactions put back
//!     imperatorium sessions info <id>      # one session's title, prompt, branch...
//!     imperatorium sessions titles <id>... # {id: title}, for every tile at once
//!     imperatorium sessions list           # every session, newest first
//!
//! and, for the conformance suite only:
//!
//!     imperatorium policy-decide           # JSON lines {request, policy} -> [verdict, reason, tier]

mod hooks;
mod http;
mod paths;
mod policy;
mod pyish;
mod sessions;

use std::io::{BufRead, Write};

use serde_json::{json, Value};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage() -> i32 {
    eprintln!(
        "imperatorium {VERSION}\n\nusage:\n  imperatorium hook\n  imperatorium statusline\n  \
         imperatorium permission [--home DIR]\n  imperatorium policy-decide\n  imperatorium version"
    );
    2
}

/// `--home DIR` / `--home=DIR`, anywhere after the subcommand.
fn home_arg(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--home" {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix("--home=") {
            return Some(v.to_string());
        }
    }
    None
}

fn policy_decide() -> i32 {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let result = match serde_json::from_str::<Value>(&line) {
            Ok(case) => {
                let null = Value::Null;
                let d = policy::decide(
                    case.get("request").unwrap_or(&null),
                    case.get("policy").unwrap_or(&null),
                );
                json!([d.verdict, d.reason, d.tier])
            }
            Err(e) => json!({"error": e.to_string()}),
        };
        let _ = writeln!(out, "{result}");
    }
    let _ = out.flush();
    0
}

/// `sessions ...`: JSON on stdout. Exit 1 with a message on stderr only
/// when a transcript cannot be read at all.
fn sessions_main(args: &[String]) -> i32 {
    let id = args.get(2).map(String::as_str).unwrap_or("");
    let result = match args.get(1).map(String::as_str) {
        Some("chain") => sessions::chain_json(id),
        Some("info") => Ok(sessions::info_json(id)),
        Some("titles") => Ok(sessions::titles_json(&args[2..])),
        Some("list") => Ok(sessions::list_json()),
        _ => return usage(),
    };
    match result {
        Ok(value) => {
            let mut out = std::io::BufWriter::new(std::io::stdout().lock());
            if serde_json::to_writer(&mut out, &value).is_err() || out.flush().is_err() {
                return 1;
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let home = home_arg(&args);
    let code = match args.first().map(String::as_str) {
        Some("hook") => hooks::hook_main(),
        Some("statusline") => hooks::statusline_main(),
        Some("permission") => hooks::permission_main(home.as_deref()),
        Some("policy-decide") => policy_decide(),
        Some("sessions") => sessions_main(&args),
        Some("version") | Some("--version") => {
            println!("imperatorium {VERSION}");
            0
        }
        _ => usage(),
    };
    std::process::exit(code);
}
