//! The chat matrix's pieces: `matrix.focus` (a tile's replies and the step
//! in flight) and `matrix.spend` (the status tile), from
//! `imperatorium/matrix.py`.

use serde_json::{json, Map, Value};

use crate::activity::splitlines;
use crate::pyish::{py_str, strip, take, truthy};

const REPLIES: usize = 12;
const REPLY_CHARS: usize = 2000;
const LINE_CHARS: usize = 200;

/// matrix.working: in a turn, so a trailing thought is still live.
pub fn working(row: &Map<String, Value>) -> bool {
    matches!(
        row.get("status").and_then(Value::as_str),
        Some("busy") | Some("shell")
    ) || row.get("state").and_then(Value::as_str) == Some("working")
}

fn line(text: Option<&Value>) -> String {
    let empty = json!("");
    let s = py_str(if truthy(text) { text.unwrap() } else { &empty });
    for l in splitlines(&s) {
        let l = strip(l);
        if !l.is_empty() {
            let more = l.chars().count() > LINE_CHARS;
            return format!(
                "{}{}",
                take(l, LINE_CHARS),
                if more { "\u{2026}" } else { "" }
            );
        }
    }
    String::new()
}

/// matrix.focus: {replies, current} from a conversation's items.
pub fn focus(items: &[Value], live: bool) -> Map<String, Value> {
    let (mut replies, mut open, mut turn): (Vec<Value>, Vec<Map<String, Value>>, i64) =
        (vec![], vec![], 0);
    let kind_of = |s: &Map<String, Value>| {
        s.get("kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    for item in items {
        let speaker = item.get("speaker").and_then(Value::as_str);
        let blocks = item
            .get("blocks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !matches!(speaker, Some("claude") | Some("tool")) {
            let results: Vec<&Value> = blocks
                .iter()
                .filter(|b| b.get("kind").and_then(Value::as_str) == Some("result"))
                .collect();
            if !results.is_empty() {
                let ids: Vec<Value> = results
                    .iter()
                    .map(|b| b.get("id").cloned().unwrap_or(Value::Null))
                    .collect();
                open.retain(|s| {
                    kind_of(s) != "tool" || !ids.contains(s.get("id").unwrap_or(&Value::Null))
                });
            } else if speaker == Some("you") {
                turn += 1;
                open.clear();
            }
            continue;
        }
        for block in &blocks {
            let kind = block.get("kind").and_then(Value::as_str);
            open.retain(|s| kind_of(s) != "thinking");
            match kind {
                Some("text") => {
                    open.clear();
                    let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                    let n = text.chars().count();
                    let tail: String = text.chars().skip(n.saturating_sub(REPLY_CHARS)).collect();
                    let cut = n > REPLY_CHARS;
                    replies.push(json!({"text": format!("{}{}", if cut { "\u{2026}" } else { "" }, tail), "turn": turn}));
                }
                Some("tool") => {
                    let name = block
                        .get("name")
                        .filter(|v| truthy(Some(v)))
                        .cloned()
                        .unwrap_or(json!("tool"));
                    open.push(
                        json!({"kind": "tool", "id": block.get("id").cloned().unwrap_or(Value::Null),
                               "name": name, "summary": line(block.get("summary"))})
                        .as_object()
                        .unwrap()
                        .clone(),
                    );
                }
                Some("thinking") => {
                    open.push(
                        json!({"kind": "thinking", "text": line(block.get("text"))})
                            .as_object()
                            .unwrap()
                            .clone(),
                    );
                }
                Some("result") => {
                    let id = block.get("id").cloned().unwrap_or(Value::Null);
                    open.retain(|s| {
                        kind_of(s) != "tool" || s.get("id").unwrap_or(&Value::Null) != &id
                    });
                }
                _ => {}
            }
        }
    }
    if !live {
        open.clear();
    }
    let current = open.last().map(|c| {
        let mut c: Map<String, Value> = c
            .iter()
            .filter(|(k, _)| *k != "id")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let tools = open.iter().filter(|s| kind_of(s) == "tool").count();
        if tools > 1 {
            c.insert("more".into(), json!(tools - 1));
        }
        Value::Object(c)
    });
    let start = replies.len().saturating_sub(REPLIES);
    let mut out = Map::new();
    out.insert("replies".into(), Value::Array(replies[start..].to_vec()));
    out.insert("current".into(), current.unwrap_or(Value::Null));
    out
}

/// matrix.spend: the status tile.
pub fn spend(tiles: &[Map<String, Value>], limits: &Value) -> Map<String, Value> {
    let names = ["input", "output", "cache_read", "cache_write"];
    let mut tokens = [0f64; 4];
    let mut int_only = [true; 4];
    let (mut cost, mut costed, mut each) = (0.0f64, 0, Vec::new());
    for tile in tiles {
        let got = tile
            .get("tokens")
            .filter(|v| truthy(Some(v)))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (i, name) in names.iter().enumerate() {
            if let Some(Value::Number(n)) = got.get(*name) {
                tokens[i] += n.as_f64().unwrap_or(0.0);
                int_only[i] &= n.is_i64() || n.is_u64();
            }
        }
        let usd = tile.get("cost_usd").cloned().unwrap_or(Value::Null);
        if let Some(u) = usd.as_f64().filter(|_| usd.is_number()) {
            cost += u;
            costed += 1;
        }
        let total: i64 = got.values().filter_map(|v| v.as_i64()).sum();
        each.push(json!({"name": tile.get("name").cloned().unwrap_or(Value::Null), "cost_usd": usd, "tokens": total}));
    }
    each.sort_by(|a, b| {
        let c = |e: &Value| -(e["cost_usd"].as_f64().unwrap_or(0.0));
        c(a).partial_cmp(&c(b)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let tok = |i: usize| {
        if int_only[i] {
            json!(tokens[i] as i64)
        } else {
            json!(tokens[i])
        }
    };
    let tmap: Map<String, Value> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.to_string(), tok(i)))
        .collect();
    let total: i64 = tokens.iter().map(|t| *t as i64).sum();
    let limits = if truthy(Some(limits)) {
        limits.clone()
    } else {
        json!({})
    };
    json!({"cost_usd": cost, "costed": costed, "sessions": tiles.len(), "tokens": tmap,
           "total_tokens": total, "each": each, "limits": limits})
    .as_object()
    .unwrap()
    .clone()
}
