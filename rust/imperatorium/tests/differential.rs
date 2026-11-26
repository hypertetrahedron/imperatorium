//! The binary's policy on the 86k-case generated corpus, against the answers
//! the Python policy gave (recorded at the git tag `last-python`, where
//! rust/conformance/differential.py generated them). The golden
//! (differential/) holds the
//! lists the cases are crossed from (spec.json), Python's distinct answers
//! (answers.json) and one index per case (index.txt: twice the answer's
//! index, plus one when the documented, stricter choices in policy.rs
//! explain a difference). The cases are rebuilt here in differential.py's
//! cases() order, checked against samples.json.
//!
//! As there: a LOOSER verdict (the binary allows, or merely asks, where
//! Python is stricter) always fails; a STRICTER verdict or a different
//! reason fails unless explained.
#![cfg(windows)]

mod common;
use common::*;
use serde_json::{json, Value};

/// A drive-relative path ("C:", "C:x") on another drive than the checkout.
/// Python's recorded answer for one depended on the recording machine: there
/// the current folder on C: was inside the checkout, so a read of "C:" was
/// a read in the project. With the checkout on another drive (GitHub's
/// Windows runners use D:) it is outside, and asking is the right answer.
fn drive_relative_elsewhere(request: &Value) -> bool {
    let drive = |s: &str| s.chars().next().map(|c| c.to_ascii_uppercase());
    let here = drive(request["cwd"].as_str().unwrap_or(""));
    request["tool_input"].as_object().into_iter().flatten().any(|(_, v)| {
        v.as_str().is_some_and(|s| {
            let b = s.as_bytes();
            b.len() >= 2
                && b[0].is_ascii_alphabetic()
                && b[1] == b':'
                && (b.len() == 2 || (b[2] != b'\\' && b[2] != b'/'))
                && drive(s) != here
        })
    })
}

/// A shell command the 2026-11-25 rules ask about where Python allowed it, or
/// ask about for another reason: a lone `&` or a `\r` between commands, a
/// `NAME=value` prefix outside the benign list, PowerShell's `(...)`/`{...}`.
fn stricter_since_2026_11_25(request: &Value, answer: &Value) -> bool {
    let command = request["tool_input"]["command"].as_str().unwrap_or("");
    let lone_amp = command
        .char_indices()
        .any(|(i, c)| c == '&' && !command[..i].ends_with('&') && !command[i + 1..].starts_with('&'));
    matches!(
        answer[1].as_str(),
        Some("sets an environment variable for the command" | "runs an expression or script block")
    ) || (answer[0] == "ask" && (lone_amp || command.contains('\r')))
}

fn arr(v: &Value) -> &Vec<Value> {
    v.as_array().expect("a list")
}

fn cases(spec: &Value) -> Vec<(Value, Value)> {
    let strings = arr(&spec["strings"]);
    let (cwd, on, on_edits) = (&spec["cwd"], &spec["on"], &spec["on_edits"]);
    let (policies, tools, cwds) = (
        arr(&spec["policies"]),
        arr(&spec["tools"]),
        arr(&spec["cwds"]),
    );
    let mut out = Vec::new();
    for command in strings {
        for pol in policies {
            out.push((
                json!({"tool_name": "Bash", "tool_input": {"command": command}, "cwd": cwd}),
                pol.clone(),
            ));
        }
    }
    for command in strings {
        out.push((
            json!({"tool_name": "PowerShell", "tool_input": {"command": command}, "cwd": cwd}),
            on.clone(),
        ));
    }
    let paths: Vec<&Value> = arr(&spec["paths"])
        .iter()
        .chain(strings.iter().take(200))
        .collect();
    for path in &paths {
        for tool in tools {
            for c in cwds {
                for pol in [on, on_edits] {
                    out.push((
                        json!({"tool_name": tool, "tool_input": {"file_path": path}, "cwd": c}),
                        pol.clone(),
                    ));
                }
            }
        }
    }
    for shape in arr(&spec["shapes"]) {
        for tool in tools {
            for pol in policies {
                out.push((
                    json!({"tool_name": tool, "tool_input": shape, "cwd": cwd}),
                    pol.clone(),
                ));
            }
        }
    }
    for bad in arr(&spec["bad"]) {
        for pol in policies {
            out.push((bad.clone(), pol.clone()));
        }
    }
    out
}

fn rank(v: &Value) -> u8 {
    match v.as_str() {
        Some("allow") => 0,
        Some("ask") => 1,
        _ => 2,
    }
}

#[test]
fn generated_corpus_decides_as_python_does() {
    let dir = golden_dir("differential");
    let places = Places::new(&[]);
    let spec = places.put(&read_json(&dir.join("spec.json")));
    let answers = places.put(&read_json(&dir.join("answers.json")));
    let table = arr(&answers["table"]);
    let index: Vec<usize> = std::fs::read_to_string(dir.join("index.txt"))
        .unwrap()
        .split_whitespace()
        .map(|n| n.parse().unwrap())
        .collect();
    let all = cases(&spec);
    assert_eq!(
        all.len(),
        index.len(),
        "rebuilt {} cases; Python recorded {}",
        all.len(),
        index.len()
    );
    assert_eq!(answers["count"].as_u64(), Some(index.len() as u64));
    for s in arr(&places.put(&read_json(&dir.join("samples.json")))) {
        let i = s["i"].as_u64().unwrap() as usize;
        assert_eq!(
            (&all[i].0, &all[i].1),
            (&s["request"], &s["policy"]),
            "case {i} rebuilt differently"
        );
    }
    let lines: Vec<Value> = all
        .iter()
        .map(|(r, p)| json!({"request": r, "policy": p}))
        .collect();
    let got = check_lines(&["policy-decide"], &repo(), &lines);
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    let mut bad = Vec::new();
    for (i, ((request, pol), rs)) in all.iter().zip(&got).enumerate() {
        let py = &table[index[i] / 2];
        let explained = index[i] % 2 == 1;
        if py == rs {
            continue;
        }
        let mut kind = if py[0] == rs[0] {
            "REASON"
        } else if rank(&rs[0]) < rank(&py[0]) {
            "LOOSER"
        } else {
            "STRICTER"
        };
        let explained = explained
            || (kind == "STRICTER" && drive_relative_elsewhere(request))
            || (kind != "LOOSER" && stricter_since_2026_11_25(request, rs));
        if kind != "LOOSER" && !explained {
            kind = if kind == "REASON" {
                "UNEXPLAINED REASON"
            } else {
                "UNEXPLAINED STRICTER"
            };
        }
        *counts.entry(kind).or_default() += 1;
        if kind == "LOOSER" || kind.starts_with("UNEXPLAINED") {
            bad.push(format!(
                "{kind}: python={py} rust={rs}\n     e.g. {request} policy {pol}"
            ));
        }
    }
    eprintln!("differential: {} cases; {:?}", all.len(), counts);
    assert!(
        bad.is_empty(),
        "{} unexplained or looser:\n{}",
        bad.len(),
        bad[..bad.len().min(10)].join("\n")
    );
}
