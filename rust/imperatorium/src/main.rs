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
//!     imperatorium sessions items <id> [--merge]  # to_items: what the window renders
//!     imperatorium sessions list           # every session, newest first
//!
//! Phase 3 is the dispatcher itself, endpoint by endpoint, as a shadow beside
//! the Python one until the cutover:
//!
//!     imperatorium serve [--port 8811] [--home DIR]
//!
//! and, for the conformance suite only:
//!
//!     imperatorium policy-decide           # JSON lines {request, policy} -> [verdict, reason, tier]

mod activity;
mod changes;
mod conversation;
mod costs;
mod history;
mod hooks;
mod http;
mod paths;
mod policy;
mod procs;
mod projects;
mod pyish;
mod registry;
mod server;
mod sessions;
mod sessionstat;
mod settings;
mod taskout;
mod ui;

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

/// `--name VALUE` / `--name=VALUE`, anywhere after the subcommand.
fn flag(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix(&format!("{name}=")) {
            return Some(v.to_string());
        }
    }
    None
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
        Some("stats") => {
            let now = flag(args, "--now")
                .and_then(|n| n.parse().ok())
                .unwrap_or(0.0);
            Ok(sessionstat::summarise_file(id, now).unwrap_or(serde_json::Value::Null))
        }
        Some("items") => sessions::chain_json(id).map(|chain| {
            let chain = chain.as_array().cloned().unwrap_or_default();
            let merge = args.iter().any(|a| a == "--merge");
            serde_json::Value::Array(conversation::to_items(
                &conversation::messages(&chain),
                0,
                merge,
            ))
        }),
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
        Some("serve") => {
            let port = flag(&args, "--port")
                .and_then(|p| p.parse().ok())
                .unwrap_or(8811);
            let host = flag(&args, "--host").unwrap_or_else(|| "127.0.0.1".into());
            server::serve(&host, port, home.as_deref())
        }
        Some("version") | Some("--version") => {
            println!("imperatorium {VERSION}");
            0
        }
        _ => usage(),
    };
    std::process::exit(code);
}
