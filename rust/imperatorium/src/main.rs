//! `imperatorium`: one binary for every part of Imperatorium.
//!
//! The port is phased (docs/native-port.md). Phase 1 is what Claude Code
//! runs on every event, so it goes first:
//!
//!     imperatorium hook                    # was hooks/cc_hook.py
//!     imperatorium statusline              # was hooks/cc_statusline.py
//!     imperatorium permission [--home DIR] # was hooks/cc_permission.py
//!
//! and, for the conformance suite only:
//!
//!     imperatorium policy-decide           # JSON lines {request, policy} -> [verdict, reason, tier]

mod hooks;
mod http;
mod paths;
mod policy;
mod pyish;

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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let home = home_arg(&args);
    let code = match args.first().map(String::as_str) {
        Some("hook") => hooks::hook_main(),
        Some("statusline") => hooks::statusline_main(),
        Some("permission") => hooks::permission_main(home.as_deref()),
        Some("policy-decide") => policy_decide(),
        Some("version") | Some("--version") => {
            println!("imperatorium {VERSION}");
            0
        }
        _ => usage(),
    };
    std::process::exit(code);
}
