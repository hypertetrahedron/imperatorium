//! test_policy.py's calls to `policy.decide`, each with the answer the
//! Python policy gave (golden: policy_tests/calls.jsonl, recorded by
//! rust/conformance/run_policy_tests.py --record), against
//! `imperatorium policy-decide`, run from the checkout as Python was.
#![cfg(windows)]

mod common;
use common::*;
use serde_json::{json, Value};

#[test]
fn test_policy_calls_answer_as_python_did() {
    let places = Places::new(&[]);
    let calls: Vec<Value> = read_jsonl(&golden_dir("policy_tests").join("calls.jsonl"))
        .iter()
        .map(|c| places.put(c))
        .collect();
    assert!(calls.len() > 100, "only {} recorded calls", calls.len());
    let cases: Vec<Value> = calls
        .iter()
        .map(|c| json!({"request": c["request"], "policy": c["policy"]}))
        .collect();
    let got = check_lines(&["policy-decide"], &repo(), &cases);
    let mut f = Failures::default();
    for (c, g) in calls.iter().zip(&got) {
        f.check(
            || format!("decide({})", c["request"]),
            first_difference(&c["want"], g, "$", true),
        );
    }
    f.finish("policy_tests");
}
