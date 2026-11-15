//! The pure checks, each a `*-check` subcommand fed the cases Python
//! answered (goldens recorded by the matching rust/conformance script with
//! --record):
//!
//!   settings_compare  settings.check over every field and awkward values
//!   slash_compare     slash.parse / refusal / clean_typed / typed_refusal
//!   projects_compare  _norm, resolve, parse_utterance, deeplink.build
//!   focus_compare     normalise_title and match_title (the pure half; the
//!                     live window enumeration is not recorded)
//!
//! As the scripts did, answers are compared as json.dumps(sort_keys=True)
//! would: key order aside, int and float distinct.

mod common;
use common::*;
use serde_json::{json, Value};

#[cfg(windows)]
#[test]
fn settings_check_answers_as_python_did() {
    let places = Places::new(&[("tmp", &system_temp())]);
    let rows: Vec<Value> = read_jsonl(&golden_dir("settings_compare").join("cases.jsonl"))
        .iter()
        .map(|r| places.put(r))
        .collect();
    let cases: Vec<Value> = rows.iter().map(|r| r["case"].clone()).collect();
    let got = check_lines(&["settings-check"], &repo(), &cases);
    let mut f = Failures::default();
    let mut raises = 0;
    for (r, g) in rows.iter().zip(&got) {
        if r["want"].get("raises").is_some() {
            raises += 1; // Python itself fails; nothing to match
            continue;
        }
        f.check(
            || format!("settings {}", r["case"]),
            first_difference(&r["want"], g, "$", false),
        );
    }
    eprintln!(
        "settings_compare: {} cases, {raises} where Python raises",
        rows.len()
    );
    f.finish("settings_compare");
}

#[test]
fn slash_check_answers_as_python_did() {
    let rows = read_jsonl(&golden_dir("slash_compare").join("cases.jsonl"));
    let cases: Vec<Value> = rows.iter().map(|r| json!({"text": r["text"]})).collect();
    let got = check_lines(&["slash-check"], &repo(), &cases);
    let mut f = Failures::default();
    for (r, g) in rows.iter().zip(&got) {
        f.check(
            || format!("slash {}", r["text"]),
            first_difference(&r["want"], g, "$", false),
        );
    }
    f.finish("slash_compare");
}

#[cfg(windows)]
#[test]
fn projects_check_answers_as_python_did() {
    let dir = golden_dir("projects_compare");
    let places = Places::new(&[]);
    let catalogues: std::collections::HashMap<String, Value> =
        read_jsonl(&dir.join("catalogues.jsonl"))
            .into_iter()
            .map(|c| {
                (
                    c["name"].as_str().unwrap().to_string(),
                    places.put(&c["catalogue"]),
                )
            })
            .collect();
    let rows: Vec<Value> = read_jsonl(&dir.join("cases.jsonl"))
        .iter()
        .map(|r| places.put(r))
        .collect();
    let cases: Vec<Value> = rows
        .iter()
        .map(|r| {
            let mut c = r["case"].clone();
            if let Some(name) = c
                .get("catalogue")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                c["catalogue"] = catalogues[&name].clone();
            }
            c
        })
        .collect();
    // "relative\dir" resolves against the checkout, as when recorded.
    let got = check_lines(&["projects-check"], &repo(), &cases);
    let mut f = Failures::default();
    for (r, g) in rows.iter().zip(&got) {
        f.check(
            || format!("projects {}", r["case"]),
            first_difference(&r["want"], g, "$", false),
        );
    }
    f.finish("projects_compare");
}

#[cfg(windows)]
#[test]
fn focus_check_pure_half_answers_as_python_did() {
    let rows = read_jsonl(&golden_dir("focus_compare").join("cases.jsonl"));
    let cases: Vec<Value> = rows.iter().map(|r| r["case"].clone()).collect();
    let got = check_lines(&["focus-check"], &repo(), &cases);
    let mut f = Failures::default();
    for (r, g) in rows.iter().zip(&got) {
        f.check(
            || format!("focus {}", r["case"]),
            first_difference(&r["want"], g, "$", false),
        );
    }
    f.finish("focus_compare");
}
