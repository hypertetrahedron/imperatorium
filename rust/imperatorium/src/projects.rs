//! The project catalogue: `projects.discover` from `imperatorium/projects.py`.
//! Every folder under the configured roots, plus every folder a session has
//! run in, one entry per normalised path, sorted by lowercased name.

use std::path::Path;

use serde_json::{json, Value};

use crate::paths;

const SKIP: &[&str] = &[
    ".git",
    ".venv",
    "venv",
    "node_modules",
    "__pycache__",
    ".idea",
    ".vs",
];

/// projects.discover(roots, extra).
pub fn discover(roots: &[String], extra: &[String]) -> Vec<Value> {
    let mut seen: indexmap::IndexMap<String, Value> = indexmap::IndexMap::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        let mut names: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            if SKIP.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            let path = Path::new(root).join(&name);
            if !path.is_dir() {
                continue;
            }
            let p = path.to_string_lossy().into_owned();
            let (Some(key), Some(abs)) = (paths::abs_normcase(&p), paths::abspath(&p)) else {
                continue;
            };
            seen.insert(key, json!({"name": name, "path": abs, "source": "root"}));
        }
    }
    for path in extra {
        if path.is_empty() {
            continue;
        }
        let (Some(key), Some(abs)) = (paths::abs_normcase(path), paths::abspath(path)) else {
            continue;
        };
        if !seen.contains_key(&key) {
            let name = paths::basename(&paths::normpath(path)).to_string();
            seen.insert(key, json!({"name": name, "path": abs, "source": "session"}));
        }
    }
    let mut out: Vec<Value> = seen.into_values().collect();
    out.sort_by_key(|p| p["name"].as_str().unwrap_or("").to_lowercase());
    out
}

// -- matching what was said to a project: projects._norm / resolve ---------

pub const CERTAIN: f64 = 0.87;
pub const MAYBE: f64 = 0.60;

/// Python's `\w` for a str pattern: alphanumeric or `_`.
fn py_word(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// projects._norm: 'laser-ledger', 'LaserLedger' and 'Laser Ledger' agree.
pub fn norm(text: &str) -> String {
    // [_\-]+ -> " "
    let mut a = String::new();
    let mut run = false;
    for c in text.chars() {
        if c == '_' || c == '-' {
            if !run {
                a.push(' ');
            }
            run = true;
        } else {
            a.push(c);
            run = false;
        }
    }
    // (?<=[a-z0-9])(?=[A-Z]) -> " "
    let mut b = String::new();
    let mut prev: Option<char> = None;
    for c in a.chars() {
        if c.is_ascii_uppercase()
            && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
        {
            b.push(' ');
        }
        b.push(c);
        prev = Some(c);
    }
    // [^\w ]+ -> " "
    let mut d = String::new();
    let mut run = false;
    for c in b.chars() {
        if py_word(c) || c == ' ' {
            d.push(c);
            run = false;
        } else {
            if !run {
                d.push(' ');
            }
            run = true;
        }
    }
    crate::pyish::split_ws(&d.to_lowercase()).join(" ")
}

/// difflib.SequenceMatcher(None, a, b).ratio(), with CPython's matching:
/// the longest block earliest in a, then earliest in b, recursively; no junk
/// and, below 200 characters, no "popular" elements.
// The index loop mirrors CPython's find_longest_match, ties and all.
#[allow(clippy::needless_range_loop)]
pub fn ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    let mut b2j: std::collections::HashMap<char, Vec<usize>> = std::collections::HashMap::new();
    for (j, c) in b.iter().enumerate() {
        b2j.entry(*c).or_default().push(j);
    }
    if b.len() >= 200 {
        // autojunk: elements more common than 1% + 1 of b are dropped.
        let ntest = b.len() / 100 + 1;
        b2j.retain(|_, js| js.len() <= ntest);
    }
    let longest = |alo: usize, ahi: usize, blo: usize, bhi: usize| -> (usize, usize, usize) {
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        let mut j2len: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for i in alo..ahi {
            let mut newj2len = std::collections::HashMap::new();
            if let Some(js) = b2j.get(&a[i]) {
                for &j in js {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = j
                        .checked_sub(1)
                        .and_then(|p| j2len.get(&p))
                        .copied()
                        .unwrap_or(0)
                        + 1;
                    newj2len.insert(j, k);
                    if k > bestsize {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            j2len = newj2len;
        }
        (besti, bestj, bestsize)
    };
    let mut matched = 0usize;
    let mut queue = vec![(0usize, a.len(), 0usize, b.len())];
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = longest(alo, ahi, blo, bhi);
        if k > 0 {
            matched += k;
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    2.0 * matched as f64 / total as f64
}

/// `round(x, 3)`.
fn round3(x: f64) -> f64 {
    format!("{x:.3}").parse().unwrap_or(x)
}

/// projects.resolve: the best project for `spoken`, how sure, and the
/// runners-up. Anything short of "certain" is the caller's to confirm.
pub fn resolve(spoken: &str, catalogue: &[Value]) -> Value {
    let target = norm(spoken);
    let mut scored: Vec<(f64, &Value)> = Vec::new();
    if !target.is_empty() {
        for proj in catalogue {
            let name = norm(proj["name"].as_str().unwrap_or(""));
            let mut score = ratio(&target, &name);
            if name == target {
                score = 1.0;
            } else if name.starts_with(&target) || target.starts_with(&name) {
                score = score.max(0.93);
            } else if name.contains(&target) || target.contains(&name) {
                score = score.max(0.88);
            }
            scored.push((score, proj));
        }
    }
    scored.sort_by(|x, y| {
        y.0.partial_cmp(&x.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let n = |p: &Value| p["name"].as_str().unwrap_or("").to_lowercase();
                n(x.1).cmp(&n(y.1))
            })
    });
    let top = &scored[..scored.len().min(5)];
    let (best_score, best) = top
        .first()
        .map(|(s, p)| (*s, Some(*p)))
        .unwrap_or((0.0, None));
    let runner = top.get(1).map(|t| t.0).unwrap_or(0.0);
    let confidence = if best_score >= CERTAIN && best_score - runner >= 0.03 {
        "certain"
    } else if best_score >= MAYBE {
        "maybe"
    } else {
        "none"
    };
    let candidates: Vec<Value> = top
        .iter()
        .map(|(s, p)| json!({"name": p["name"], "path": p["path"], "score": round3(*s)}))
        .collect();
    json!({
        "match": if confidence != "none" { best.cloned().unwrap_or(Value::Null) } else { Value::Null },
        "score": round3(best_score),
        "confidence": confidence,
        "candidates": candidates,
    })
}

/// projects.parse_utterance: 'project: laser ledger, do the thing' as
/// (name, prompt); no name when there is no project prefix.
pub fn parse_utterance(text: &str) -> (Option<String>, String) {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(&crate::pyish::py_regex(
            r"(?is)\A\s*(?:for\s+)?projects?\s*[:,]?\s*(?P<name>.+?)\s*[:,]\s*(?P<prompt>.+)\z",
        ))
        .expect("the utterance pattern")
    });
    match re.captures(text) {
        Some(m) => (
            Some(crate::pyish::strip(&m["name"]).to_string()),
            crate::pyish::strip(&m["prompt"]).to_string(),
        ),
        None => (None, crate::pyish::strip(text).to_string()),
    }
}

/// `imperatorium projects-check`: JSON lines of {spoken, catalogue} or
/// {utterance} in, the answers out - for the conformance differential.
pub fn check_lines() -> i32 {
    use std::io::BufRead;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let case: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let out = if let Some(u) = case.get("utterance") {
            let (name, prompt) = parse_utterance(u.as_str().unwrap_or(""));
            json!([name, prompt])
        } else if let Some(d) = case.get("deeplink") {
            match crate::deeplink::build(d[0].as_str().unwrap_or(""), d[1].as_str().unwrap_or("")) {
                Ok(u) => json!({"ok": u}),
                Err(e) => json!({"error": e}),
            }
        } else if let Some(t) = case.get("norm") {
            json!(norm(t.as_str().unwrap_or("")))
        } else {
            let cat = case["catalogue"].as_array().cloned().unwrap_or_default();
            resolve(case["spoken"].as_str().unwrap_or(""), &cat)
        };
        println!("{}", crate::pyish::json_dumps(&out, true));
    }
    0
}

/// test_core.py's Matching: the spoken forms of a real catalogue, resolved.
#[cfg(test)]
mod core_tests {
    use super::*;
    use serde_json::json;

    fn catalogue() -> Vec<Value> {
        [
            "laser-ledger", "bookoferrantpages", "gem-trip", "amarantha", "amarantha-story",
            "pulse-palette", "diceCounter", "elixir-forge", "fiducials", "rank-craft",
        ]
        .iter()
        .map(|n| json!({"name": n, "path": format!("Y:/projects-software/{n}"), "source": "root"}))
        .collect()
    }

    fn certain(said: &str, expect: &str) {
        let res = resolve(said, &catalogue());
        assert_eq!(res["confidence"], "certain", "{said} -> {res}");
        assert_eq!(res["match"]["name"], expect, "{said}");
    }

    #[test]
    fn spoken_forms_of_real_projects() {
        certain("laser ledger", "laser-ledger");
        certain("book of errant pages", "bookoferrantpages");
        certain("gem trip", "gem-trip");
        certain("dice counter", "diceCounter");
        certain("elixir forge", "elixir-forge");
        certain("rank craft", "rank-craft");
    }

    #[test]
    fn case_and_separators_do_not_matter() {
        certain("LASER-LEDGER", "laser-ledger");
        certain("Laser_Ledger", "laser-ledger");
    }

    #[test]
    fn unknown_project_is_refused_not_guessed() {
        let res = resolve("home assistant", &catalogue());
        assert_eq!(res["confidence"], "none");
        assert!(res["match"].is_null());
    }

    #[test]
    fn near_tie_is_not_certain() {
        // "amarantha" and "amarantha-story" are close enough that acting
        // silently would be a coin flip.
        let res = resolve("amarantha story", &catalogue());
        assert_ne!(res["confidence"], "none");
        if res["confidence"] == "certain" {
            assert_eq!(res["match"]["name"], "amarantha-story");
        }
    }

    #[test]
    fn empty_input_matches_nothing() {
        assert_eq!(resolve("", &catalogue())["confidence"], "none");
    }

    #[test]
    fn candidates_are_ranked_and_bounded() {
        let res = resolve("gem trip", &catalogue());
        let scores: Vec<f64> = res["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["score"].as_f64().unwrap())
            .collect();
        assert!(scores.len() <= 5 && !scores.is_empty());
        assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
    }
}
