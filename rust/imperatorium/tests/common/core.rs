//! Helpers for the core_* integration tests (the ports of test_convcache.py,
//! test_rename.py, test_quick.py and the HTTP parts of test_core.py):
//! transcripts written where the dispatcher looks for them, and the same
//! transcript rewritten behind the cache's back.
#![allow(dead_code)]

use super::*;

/// A session id the transcript reader accepts (36 characters).
pub fn sid(n: u32) -> String {
    format!("c0c0{n:04x}-0000-4000-8000-000000000{n:03x}")
}

/// One transcript line: `kind` "user" or "assistant", chained to `parent`.
pub fn line(session: &str, uuid: &str, parent: Option<&str>, kind: &str, text: &str) -> String {
    let message = if kind == "user" {
        json!({"role": "user", "content": text})
    } else {
        json!({"role": "assistant", "content": [{"type": "text", "text": text}]})
    };
    let mut entry = json!({"parentUuid": parent, "isSidechain": false, "userType": "external",
                           "cwd": r"C:\work\core", "sessionId": session, "version": "2.1.0",
                           "type": kind, "message": message, "uuid": uuid,
                           "timestamp": "2026-01-01T00:00:00.000Z"});
    if kind == "assistant" {
        entry["message"]["model"] = json!("claude-sonnet-5-5");
        entry["message"]["stop_reason"] = json!("end_turn");
    }
    entry.to_string()
}

/// The transcript text for a conversation: (kind, text) in order.
pub fn transcript_text(session: &str, turns: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut parent: Option<String> = None;
    for (i, (kind, text)) in turns.iter().enumerate() {
        let uuid = format!("{i:08x}-0000-4000-8000-{}", &session[session.len() - 12..]);
        out.push_str(&line(session, &uuid, parent.as_deref(), kind, text));
        out.push('\n');
        parent = Some(uuid);
    }
    out
}

/// Write a session's transcript under CLAUDE_CONFIG_DIR/projects/<folder>/.
pub fn write_transcript(d: &Dispatcher, session: &str, turns: &[(&str, &str)]) -> PathBuf {
    let dir = d.claude.join("projects").join("C--work-core");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{session}.jsonl"));
    std::fs::write(&path, transcript_text(session, turns)).unwrap();
    path
}

pub fn mtime(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

/// Replace a transcript's bytes and put its modified time back, so its size
/// and mtime are what a cache last saw: a cache that looks at them cannot
/// tell.
pub fn rewrite_unseen(path: &Path, text: &str) {
    let was = mtime(path);
    std::fs::write(path, text).unwrap();
    let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(was).unwrap();
}

/// The conversation the window would be shown: the answer of
/// GET /api/conversation, with every text block joined per item.
pub fn conversation(d: &Dispatcher, query: &str) -> Value {
    let (code, body) = d.get(&format!("/api/conversation?{query}"));
    assert_eq!(code, 200, "{body}");
    body
}

/// Each item as "speaker: text | text".
pub fn shown(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|i| {
            let texts: Vec<&str> = i["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|b| b["text"].as_str())
                .collect();
            format!("{}: {}", i["speaker"].as_str().unwrap_or("?"), texts.join(" | "))
        })
        .collect()
}
