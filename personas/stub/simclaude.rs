// Persona runs only. Copied to `claude.exe`, `wt.exe` and `ssh.exe` in the
// sandbox's bin folder, so a dispatcher under test never reaches a real
// terminal, Claude Code or host. Every call is one JSON line in sim.log
// beside the exe; the persona runner (lib/sim.mjs) reads it and plays the
// part of the session that would have started.
//
// - `claude agents ...`        prints agents.json beside the exe ([] if none)
// - `claude --bg ...`          prints a fresh background short id
// - `claude stop <id>`         logged
// - `claude -p ...`            prints a canned answer (Ask, mid-turn forks)
// - `claude --version`         prints a version
// - `simclaude hold`           stays alive: the pid a simulated session lives on
// - `ssh ... -N`               a tunnel: stays alive
// - `ssh host <cmd>`           remote.out / probe.out / inbox-send / remote-channel
// - anything else              logged, exits 0
use std::io::{Read, Write};
use std::path::PathBuf;

fn here() -> PathBuf {
    std::env::current_exe().unwrap().parent().unwrap().to_path_buf()
}

fn esc(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
}

fn log(prog: &str, args: &[String], extra: &str) {
    let cwd = std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    let a: Vec<String> = args.iter().map(|x| esc(x)).collect();
    let line = format!(
        "{{\"t\":{},\"pid\":{},\"prog\":{},\"cwd\":{},\"args\":[{}]{}}}\n",
        now_ms(), std::process::id(), esc(prog), esc(&cwd), a.join(","), extra
    );
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(here().join("sim.log")) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn file(name: &str, default: &str) -> String {
    std::fs::read_to_string(here().join(name)).unwrap_or_else(|_| default.to_string())
}

const CATALOG: &str = r#"{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{"models":[{"value":"default","displayName":"Default (recommended)","description":"Opus 5.5","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"]},{"value":"opus","resolvedModel":"claude-opus-5-5","displayName":"Opus","description":"Opus 5.5 for complex work","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"]},{"value":"sonnet","resolvedModel":"claude-sonnet-5-5","displayName":"Sonnet","description":"Sonnet 5.5 for daily tasks","supportsEffort":true,"supportedEffortLevels":["low","medium","high"]},{"value":"haiku","resolvedModel":"claude-haiku-5-5","displayName":"Haiku","description":"Haiku 5.5 for quick answers","supportsEffort":false}],"commands":[{"name":"clear","description":"Clear conversation history"},{"name":"model","description":"Set the AI model"},{"name":"effort","description":"Set reasoning effort"},{"name":"config","description":"Open config"},{"name":"resume","description":"Resume a conversation"},{"name":"compact","description":"Compact the conversation"},{"name":"cost","description":"Show the session's cost"},{"name":"review","description":"Review a pull request"}],"output_style":"default","available_output_styles":["default","Explanatory","Learning"]}}}"#;

fn hold() -> ! {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let prog = std::path::Path::new(&argv[0])
        .file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let args = &argv[1..];
    if args.first().map(String::as_str) == Some("hold") {
        hold();
    }
    if prog == "ssh" {
        std::process::exit(ssh(args));
    }
    if prog == "claude" {
        match args.first().map(String::as_str) {
            Some("agents") => {
                let t = file("agents.json", "[]");
                print!("{}", if t.trim().is_empty() { "[]" } else { t.as_str() });
                return;
            }
            Some("--version") => {
                println!("2.1.330 (Claude Code)");
                return;
            }
            _ => {}
        }
        if args.iter().any(|a| a == "--bg") {
            let id = format!("{:08x}", (now_ms() as u64 ^ ((std::process::id() as u64) << 20)) & 0xffff_ffff);
            log(&prog, args, &format!(",\"bg\":{}", esc(&id)));
            println!("Background session started: {id}");
            return;
        }
        if args.iter().any(|a| a == "-p" || a == "--print") {
            log(&prog, args, "");
            let mut stdin = String::new();
            let _ = std::io::stdin().read_to_string(&mut stdin);
            if args.iter().any(|a| a == "--input-format") {
                // The catalog probe (claude_catalog::probe): an initialize answer.
                println!("{}", file("catalog.out", CATALOG));
                return;
            }
            println!("{}", file("print.out", "(simulated answer) Everything is on track."));
            return;
        }
    }
    log(&prog, args, "");
}

fn ssh(args: &[String]) -> i32 {
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if ["-o", "-R", "-L", "-p", "-i", "-F"].contains(&args[i].as_str()) {
            i += 2;
            continue;
        }
        rest.push(args[i].clone());
        i += 1;
    }
    let tunnel = rest.iter().any(|a| a == "-N");
    rest.retain(|a| !a.starts_with('-'));
    let host = rest.first().cloned().unwrap_or_default();
    let cmd = rest.get(1..).map(|r| r.join(" ")).unwrap_or_default();
    if tunnel {
        log("ssh", args, ",\"tunnel\":true");
        hold();
    }
    if here().join(format!("auth-{host}")).exists() {
        log("ssh", args, ",\"auth\":true");
        eprintln!("{host}: Permission denied (publickey,password).");
        return 255;
    }
    if here().join(format!("down-{host}")).exists() {
        log("ssh", args, ",\"refused\":true");
        eprintln!("ssh: connect to host {host} port 22: Connection refused");
        return 255;
    }
    if cmd.contains("remote-channel") {
        // The host's side of the conversation channel: the dispatcher binary
        // itself, run here against the host's own Claude folder.
        log("ssh", args, "");
        let bin = file("imperatorium.path", "").trim().to_string();
        let claude = here().join("remote-home").join(&host).join(".claude");
        let st = std::process::Command::new(bin)
            .arg("remote-channel")
            .env("CLAUDE_CONFIG_DIR", &claude)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();
        return st.map(|s| s.code().unwrap_or(1)).unwrap_or(1);
    }
    if cmd.contains("inbox-send") {
        let mut stdin = String::new();
        let _ = std::io::stdin().read_to_string(&mut stdin);
        log("ssh", args, &format!(",\"host\":{},\"stdin\":{}", esc(&host), esc(&stdin)));
        return 0;
    }
    log("ssh", args, &format!(",\"host\":{}", esc(&host)));
    if cmd.contains("imperatorium session files") {
        print!("{}", file(&format!("remote-{host}.out"), "[]\n\n--- imperatorium session files ---\n"));
    } else if cmd.contains("uname -m") {
        print!("{}", file("probe.out", ""));
    } else if cmd.contains("--bg") {
        println!("Background session started: {:08x}", now_ms() as u64 & 0xffffffff);
    }
    0
}
