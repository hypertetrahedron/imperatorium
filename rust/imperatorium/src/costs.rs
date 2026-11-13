//! List prices and what a cold cache costs: the parts of
//! `imperatorium/costs.py` the session rows use (`resolve`, `rewrite_cost`).
//! The spend views (windows, model mix) come with their endpoints.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use crate::pyish::{py_str, truthy};

/// (model id, input $/Mtok, output $/Mtok), in costs.PRICES' order.
pub const PRICES: &[(&str, f64, f64)] = &[
    ("claude-fable-5-1", 10.0, 50.0),
    ("claude-fable-5", 10.0, 50.0),
    ("claude-mythos-5-1", 10.0, 50.0),
    ("claude-opus-5-5", 4.0, 20.0),
    ("claude-opus-5", 5.0, 25.0),
    ("claude-opus-4-8", 5.0, 25.0),
    ("claude-opus-4-7", 5.0, 25.0),
    ("claude-opus-4-6", 5.0, 25.0),
    ("claude-opus-4-5", 5.0, 25.0),
    ("claude-opus-4-1", 15.0, 75.0),
    ("claude-opus-4", 15.0, 75.0),
    ("claude-sonnet-5-5", 2.0, 10.0),
    ("claude-sonnet-5", 2.0, 10.0),
    ("claude-sonnet-4-6", 3.0, 15.0),
    ("claude-sonnet-4-5", 3.0, 15.0),
    ("claude-sonnet-4", 3.0, 15.0),
    ("claude-haiku-5-5", 0.10, 0.50),
    ("claude-haiku-4-5", 1.0, 5.0),
    ("claude-3-5-haiku", 0.80, 4.0),
];
/// Newest of each family, for an id the table has not heard of.
const FAMILIES: &[(&str, &str)] = &[
    ("fable", "claude-fable-5-1"),
    ("opus", "claude-opus-5-5"),
    ("sonnet", "claude-sonnet-5-5"),
    ("haiku", "claude-haiku-5-5"),
];
const WRITE_5M: f64 = 1.25;
const WRITE_1H: f64 = 2.0;
const UNKNOWN: &str = "(unknown)";

/// (price key, priced) for a model id as a transcript spells it.
pub fn resolve(model: Option<&Value>) -> (String, bool) {
    static BRACKETS: OnceLock<Regex> = OnceLock::new();
    let re = BRACKETS.get_or_init(|| Regex::new(r"\[.*?\]").unwrap());
    let empty = json!("");
    let raw = py_str(if truthy(model) {
        model.unwrap()
    } else {
        &empty
    });
    let name = crate::pyish::strip(&re.replace_all(&raw, "")).to_lowercase();
    let mut best: Option<&str> = None;
    for (known, _, _) in PRICES {
        let hit = name == *known || name.starts_with(&format!("{known}-"));
        if hit && best.is_none_or(|b| known.chars().count() > b.chars().count()) {
            best = Some(known);
        }
    }
    if let Some(b) = best {
        return (b.to_string(), true);
    }
    for (family, known) in FAMILIES {
        if name.contains(family) {
            return (known.to_string(), true);
        }
    }
    (
        if name.is_empty() {
            UNKNOWN.to_string()
        } else {
            name
        },
        false,
    )
}

/// Dollars to write `tokens` of context back into the cache; None for a
/// model with no known price or no tokens.
pub fn rewrite_cost(model: Option<&Value>, tokens: Option<&Value>, ttl: Option<&Value>) -> Value {
    let (key, priced) = resolve(model);
    if !priced || !truthy(tokens) {
        return Value::Null;
    }
    let Some(t) = tokens.and_then(Value::as_f64) else {
        return Value::Null;
    };
    let pin = PRICES
        .iter()
        .find(|p| p.0 == key)
        .map(|p| p.1)
        .unwrap_or(0.0);
    let ttl = ttl.and_then(Value::as_f64).unwrap_or(0.0);
    let write = if ttl >= 3600.0 { WRITE_1H } else { WRITE_5M };
    json!(t * pin * write / 1e6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids() {
        assert_eq!(
            resolve(Some(&json!("claude-opus-5-5[1m]"))),
            ("claude-opus-5-5".into(), true)
        );
        assert_eq!(
            resolve(Some(&json!("claude-opus-4-1-20250805"))),
            ("claude-opus-4-1".into(), true)
        );
        assert_eq!(
            resolve(Some(&json!("us.anthropic.claude-sonnet-x"))),
            ("claude-sonnet-5-5".into(), true)
        );
        assert_eq!(resolve(Some(&json!("gpt"))), ("gpt".into(), false));
        assert_eq!(resolve(None), ("(unknown)".into(), false));
    }

    #[test]
    fn a_cold_cache() {
        let c = rewrite_cost(
            Some(&json!("claude-opus-5-5")),
            Some(&json!(100000)),
            Some(&json!(3600.0)),
        );
        assert!((c.as_f64().unwrap() - 0.8).abs() < 1e-12);
        assert_eq!(
            rewrite_cost(Some(&json!("gpt")), Some(&json!(1)), None),
            Value::Null
        );
    }
}
