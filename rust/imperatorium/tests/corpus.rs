//! The transcript readers against the Python reference
//! (rust/conformance/sessions_compare.py, items_compare.py,
//! stats_compare.py), all through one `imperatorium sessions batch`:
//!
//!   list    the Agent SDK's list_sessions against `list`
//!   info    get_session_info(id) against `info`
//!   chain   the SDK's chain plus conversation.with_compacted, cut to the
//!           keys the binary keeps, against `chain`
//!   items   conversation.to_items over the SDK's reading, plain and merged
//!   stats   sessionstat.summarise at four pinned clocks
//!
//! Two corpora:
//!
//! - the synthetic one committed in tests/fixtures/corpus (goldens in
//!   tests/golden/{sessions,items,stats}_compare), copied to a temp folder
//!   with its mtimes pinned;
//! - LOCAL ONLY: a frozen copy of this machine's ~/.claude/projects with
//!   its goldens, outside the repository (never committed: they are
//!   private conversations). Read from $IMPERATORIUM_CORPUS, else
//!   %LOCALAPPDATA%\imperatorium\conformance-corpus; skipped when absent.
//!   See tests/golden/README.md for how it is made.
//!
//! Where Python raises on an input the binary's answer is not compared.
#![cfg(windows)]

mod common;
use common::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Known differences, kept visible: (part, session, why). Each must still
/// differ - a fix shows up as a failure asking for the entry to go.
const KNOWN: &[(&str, &str, &str)] = &[
    (
        "stats",
        "5e550017-0000-4000-8000-000000000017",
        "token counts beyond u64: Python keeps the exact int, the binary gives a float",
    ),
    (
        "stats",
        "5e550026-0000-4000-8000-000000000026",
        "token sums beyond i64: Python keeps the exact int, the binary saturates",
    ),
];

/// What the summary has gained since the Python reference froze
/// (2026-11-15), left out of `got` before comparing: `last_prompt_at`
/// (2026-11-18, a failure clears once the session goes on), `hooks_ok_at`
/// and each hook error's `hooks` (when the Stop hooks last ran cleanly and
/// which commands ran, for the window's explanation).
fn without_additions(mut got: Value) -> Value {
    if let Some(o) = got.as_object_mut() {
        o.shift_remove("last_prompt_at");
        o.shift_remove("hooks_ok_at");
        if let Some(Value::Array(errs)) = o.get_mut("hook_errors") {
            for e in errs.iter_mut().filter_map(Value::as_object_mut) {
                e.shift_remove("hooks");
            }
        }
    }
    got
}

struct Goldens {
    list: Value,
    sessions: Vec<Value>,
    items: Vec<Value>,
    stats: Vec<Value>,
    clocks: Vec<Value>,
}

/// The goldens as recorded: no placeholders (a transcript's own text may
/// hold "<repo>" literally; see golden.py corpus_setup).
/// The binary keeps when each entry was written (chain `timestamp`, a
/// block's `at`, for View > Display Times, 2026-11-15); the Python reference
/// the goldens came from did not, and is retired, so those are left out of
/// the comparison and everything else still has to match.
fn without_times(mut v: Value, key: &str) -> Value {
    for e in v.as_array_mut().into_iter().flatten() {
        if let Some(o) = e.as_object_mut() {
            o.remove(key);
        }
    }
    v
}

fn without_items_times(mut v: Value) -> Value {
    for item in v.as_array_mut().into_iter().flatten() {
        if let Some(blocks) = item.get_mut("blocks").filter(|b| b.is_array()) {
            *blocks = without_times(blocks.take(), "at");
        }
    }
    v
}

fn load(dir: &Path) -> Goldens {
    let rows = |part: &str, file: &str| -> Vec<Value> { read_jsonl(&dir.join(part).join(file)) };
    Goldens {
        list: read_json(&dir.join("sessions_compare").join("list.json")),
        sessions: rows("sessions_compare", "sessions.jsonl"),
        items: rows("items_compare", "items.jsonl"),
        stats: rows("stats_compare", "stats.jsonl"),
        clocks: read_json(&dir.join("stats_compare").join("clocks.json"))
            .as_array()
            .unwrap()
            .clone(),
    }
}

/// Sessions whose head holds a prompt another session delivered (Claude
/// Code writes it as meta, with `origin.kind` / `turnOrigin` "peer"). Python
/// skipped those, so a session driven only from Imperatorium's window had
/// `/clear` for its first prompt, or no info and no place in the list at
/// all; since 2026-11-21 the binary counts them (sessions.rs,
/// first_prompt_from_head). For these sessions the goldens' `summary` and
/// `first_prompt` - and their absence - are not compared.
fn peer_sessions(root: &Path) -> std::collections::HashSet<String> {
    use std::io::Read;
    let mut out = std::collections::HashSet::new();
    let Ok(projects) = std::fs::read_dir(root.join("projects")) else { return out };
    for dir in projects.flatten() {
        let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let mut head = vec![0u8; 65536];
            let n = std::fs::File::open(&path).and_then(|mut h| h.read(&mut head)).unwrap_or(0);
            let head = String::from_utf8_lossy(&head[..n]);
            if head.contains("\"turnOrigin\":\"peer\"") || head.contains("\"kind\":\"peer\"") {
                if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                    out.insert(id.to_string());
                }
            }
        }
    }
    out
}

/// `v` without the fields a delivered prompt changes, for a peer session.
fn without_prompt_fields(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("summary");
        o.remove("first_prompt");
    }
    v
}

fn raises(v: &Value) -> bool {
    v.get("raises").is_some()
}

/// Every golden checked against one batch run over `root` (CLAUDE_CONFIG_DIR).
fn compare(root: &Path, g: &Goldens, what: &str) {
    // One session at a time, so the batch reads each chain once.
    let n = g.sessions.len();
    assert!(
        g.items.len() == n && g.stats.len() == n,
        "goldens of different corpora"
    );
    let mut ops = vec![json!({"op": "list"})];
    for i in 0..n {
        let id = &g.sessions[i]["id"];
        assert!(
            g.items[i]["id"] == *id && g.stats[i]["id"] == *id,
            "goldens out of step at {i}"
        );
        ops.push(json!({"op": "info", "id": id}));
        ops.push(json!({"op": "chain", "id": id}));
        ops.push(json!({"op": "items", "id": id, "merge": false}));
        ops.push(json!({"op": "items", "id": id, "merge": true}));
        for now in &g.clocks {
            ops.push(json!({"op": "stats", "id": id, "now": now}));
        }
    }
    let started = std::time::Instant::now();
    let got = check_lines_env(
        &["sessions", "batch"],
        &repo(),
        &ops,
        &[("CLAUDE_CONFIG_DIR", root.as_os_str())],
    );
    eprintln!(
        "{what}: {} reads in one process, {:.1}s",
        ops.len(),
        started.elapsed().as_secs_f64()
    );
    let mut it = got.into_iter();
    let mut f = Failures::default();
    let mut known_seen: HashMap<(&str, String), bool> = HashMap::new();
    let mut check = |f: &mut Failures,
                     part: &'static str,
                     id: &str,
                     label: String,
                     want: &Value,
                     got: &Value| {
        if raises(want) {
            return;
        }
        let diff = first_difference(want, got, "$", true);
        if let Some((_, _, why)) = KNOWN.iter().find(|(p, s, _)| *p == part && *s == id) {
            if diff.is_some() {
                if known_seen.insert((part, id.to_string()), true) != Some(true) {
                    eprintln!("{what}: known difference, {part} {id}: {why}");
                }
                return;
            }
            known_seen.entry((part, id.to_string())).or_insert(false);
        }
        f.check(|| label, diff);
    };
    let peers = peer_sessions(root);
    let is_peer = |v: &Value| v["session_id"].as_str().is_some_and(|id| peers.contains(id));
    let list = it.next().unwrap();
    let (want_list, got_list) = if peers.is_empty() {
        (g.list.clone(), list)
    } else {
        let wanted: std::collections::HashSet<&str> = g.list.as_array().into_iter().flatten()
            .filter_map(|v| v["session_id"].as_str()).collect();
        let norm = |v: &Value| if is_peer(v) { without_prompt_fields(v) } else { v.clone() };
        let want: Vec<Value> = g.list.as_array().into_iter().flatten().map(norm).collect();
        let got: Vec<Value> = list.as_array().into_iter().flatten()
            .filter(|v| !is_peer(v) || v["session_id"].as_str().is_some_and(|id| wanted.contains(id)))
            .map(norm).collect();
        (json!(want), json!(got))
    };
    check(&mut f, "list", "", "list".into(), &want_list, &got_list);
    for i in 0..n {
        let (s, items, stats) = (&g.sessions[i], &g.items[i], &g.stats[i]);
        let id = s["id"].as_str().unwrap();
        let (info, chain, plain, merged) = (
            it.next().unwrap(),
            without_times(it.next().unwrap(), "timestamp"),
            without_items_times(it.next().unwrap()),
            without_items_times(it.next().unwrap()),
        );
        if !peers.contains(id) {
            check(&mut f, "sessions", id, format!("info {id}"), &s["info"], &info);
        } else if !s["info"].is_null() {
            check(&mut f, "sessions", id, format!("info {id}"),
                  &without_prompt_fields(&s["info"]), &without_prompt_fields(&info));
        }
        check(
            &mut f,
            "sessions",
            id,
            format!("chain {id}"),
            &s["chain"],
            &chain,
        );
        // sessions_compare's items (to_items, unmerged) and items_compare's.
        check(
            &mut f,
            "items",
            id,
            format!("items {id}"),
            &s["items"],
            &plain,
        );
        check(
            &mut f,
            "items",
            id,
            format!("items plain {id}"),
            &items["plain"],
            &plain,
        );
        check(
            &mut f,
            "items",
            id,
            format!("items merge {id}"),
            &items["merge"],
            &merged,
        );
        for (k, now) in g.clocks.iter().enumerate() {
            let got = without_additions(it.next().unwrap());
            check(
                &mut f,
                "stats",
                id,
                format!("stats {id} now={now}"),
                &stats["stats"][k],
                &got,
            );
        }
    }
    for ((part, id), differed) in &known_seen {
        if !differed && g.stats.iter().any(|s| s["id"] == id.as_str()) {
            f.items.push(format!(
                "{part} {id} is listed in KNOWN but now matches: remove it"
            ));
        }
    }
    eprintln!("{what}: {} sessions", g.sessions.len());
    f.finish(what);
}

#[test]
fn synthetic_corpus_reads_as_python_reads_it() {
    let tmp = TempDir::new("corpus");
    let root = tmp.join("corpus");
    copy_tree(&fixtures().join("corpus"), &root);
    pin_mtimes(&root);
    let g = load(&golden_dir(""));
    assert!(
        g.sessions.len() >= 30,
        "only {} sessions in the goldens",
        g.sessions.len()
    );
    compare(&root, &g, "synthetic corpus");
}

/// The frozen local corpus: a folder holding projects/ (the copy) and
/// golden/ (Python's answers for it).
fn local_corpus() -> Option<PathBuf> {
    let dir = std::env::var_os("IMPERATORIUM_CORPUS")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|l| {
                PathBuf::from(l)
                    .join("imperatorium")
                    .join("conformance-corpus")
            })
        })?;
    (dir.join("projects").is_dir() && dir.join("golden").is_dir()).then_some(dir)
}

#[test]
fn local_corpus_reads_as_python_reads_it() {
    let Some(root) = local_corpus() else {
        eprintln!("local corpus: none here (IMPERATORIUM_CORPUS or %LOCALAPPDATA%\\imperatorium\\conformance-corpus); skipped");
        return;
    };
    // Pinned where it lies (idempotent), as when its goldens were recorded.
    pin_mtimes(&root);
    let g = load(&root.join("golden"));
    compare(&root, &g, "local corpus");
}
