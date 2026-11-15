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

/// What test_matrix.py pinned down for `matrix.focus` and `matrix.spend`.
#[cfg(test)]
mod tests {
    use super::*;

    fn you(text: &str) -> Value {
        json!({"speaker": "you", "role": "user", "blocks": [{"kind": "text", "text": text}]})
    }
    fn claude(blocks: Vec<Value>) -> Value {
        json!({"speaker": "claude", "role": "assistant", "blocks": blocks})
    }
    fn text(t: &str) -> Value {
        json!({"kind": "text", "text": t})
    }
    fn tool(id: &str, name: &str, summary: &str) -> Value {
        json!({"kind": "tool", "id": id, "name": name, "summary": summary})
    }
    fn result(id: &str) -> Value {
        json!({"kind": "result", "id": id, "text": "ok", "truncated": false, "is_error": false})
    }
    fn thinking(t: &str) -> Value {
        json!({"kind": "thinking", "text": t})
    }
    fn replies(f: &Map<String, Value>) -> Vec<String> {
        f["replies"].as_array().unwrap().iter().map(|r| r["text"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn each_step_shows_the_replies_and_only_the_step_in_flight() {
        let blocks = [
            text("starting jira item"),
            tool("a", "Read", "ticket.md"),
            result("a"),
            tool("b", "Bash", "npm test"),
            result("b"),
            thinking("the failure is in auth"),
            text("fixed and tested"),
        ];
        let seen: Vec<_> = (1..=blocks.len())
            .map(|n| focus(&[you("do the jira item"), claude(blocks[..n].to_vec())], true))
            .collect();
        let current: Vec<Option<String>> = seen
            .iter()
            .map(|f| {
                f["current"].as_object().map(|c| {
                    c.get("name").or_else(|| c.get("kind")).unwrap().as_str().unwrap().to_string()
                })
            })
            .collect();
        let want = [None, Some("Read"), None, Some("Bash"), None, Some("thinking"), None];
        assert_eq!(current, want.map(|w| w.map(String::from)));
        for f in &seen[..6] {
            assert_eq!(replies(f), ["starting jira item"]);
        }
        assert_eq!(replies(&seen[0]), ["starting jira item"]);
        assert_eq!(replies(&seen[6]), ["starting jira item", "fixed and tested"]);
    }

    #[test]
    fn a_tool_shows_its_name_and_one_line() {
        let f = focus(&[claude(vec![tool("a", "Bash", "git status\n--porcelain")])], true);
        assert_eq!(f["current"], json!({"kind": "tool", "name": "Bash", "summary": "git status"}));
    }

    #[test]
    fn the_prompt_and_results_are_not_shown() {
        let f = focus(&[you("hello"), claude(vec![tool("a", "Bash", "git status"), result("a")])], true);
        assert_eq!(Value::Object(f), json!({"replies": [], "current": null}));
    }

    #[test]
    fn a_thought_is_not_live_once_the_turn_is_over() {
        let items = [claude(vec![text("done"), thinking("hmm")])];
        assert_eq!(focus(&items, true)["current"]["kind"], "thinking");
        assert!(focus(&items, false)["current"].is_null());
    }

    #[test]
    fn nothing_is_in_flight_in_a_session_that_is_not() {
        // The dispatcher passes live=true while a session waits on you, so a
        // call held for permission still shows.
        let items = [claude(vec![tool("a", "Bash", "rm -rf build")])];
        assert_eq!(focus(&items, true)["current"]["name"], "Bash");
        assert!(focus(&items, false)["current"].is_null());
    }

    #[test]
    fn a_reply_or_a_prompt_closes_calls_that_never_got_a_result() {
        let f = focus(
            &[claude(vec![tool("a", "WebFetch", "x"), tool("b", "Bash", "x"), result("b"), text("found it")])],
            true,
        );
        assert!(f["current"].is_null());
        let f = focus(
            &[
                claude(vec![tool("a", "WebFetch", "x")]),
                you("next"),
                claude(vec![tool("c", "Read", "git status")]),
            ],
            true,
        );
        assert_eq!(f["current"], json!({"kind": "tool", "name": "Read", "summary": "git status"}));
    }

    #[test]
    fn parallel_calls_show_the_newest_and_count_the_rest() {
        let f = focus(
            &[claude(vec![
                tool("a", "Read", "x"),
                tool("b", "Grep", "x"),
                tool("c", "Glob", "x"),
                result("c"),
            ])],
            true,
        );
        assert_eq!(f["current"]["name"], "Grep");
        assert_eq!(f["current"]["more"], 1);
    }

    #[test]
    fn replies_carry_their_turn() {
        let f = focus(
            &[you("1"), claude(vec![text("a"), text("b")]), you("2"), claude(vec![text("c")])],
            true,
        );
        let turns: Vec<i64> = f["replies"].as_array().unwrap().iter().map(|r| r["turn"].as_i64().unwrap()).collect();
        assert_eq!(turns, [1, 1, 2]);
    }

    #[test]
    fn only_the_newest_replies_are_kept_and_long_ones_cut() {
        // Python took `keep=5`; the binary keeps a fixed REPLIES (12).
        let many: Vec<Value> = (0..30).map(|i| text(&i.to_string())).collect();
        let long = format!("head {} the conclusion", "x".repeat(5000));
        let f = focus(&[claude(many), claude(vec![text(&long)])], true);
        let got = replies(&f);
        assert_eq!(got.len(), REPLIES);
        // 31 replies in all: the last 12 are #19..#29 and the long one.
        assert_eq!(&got[..3], ["19", "20", "21"]);
        let last = got.last().unwrap();
        assert_eq!(last.chars().count(), REPLY_CHARS + 1);
        // Seen live: a 2369-char reply lost its last paragraph, so the tile
        // looked stuck on an earlier line. The end is what the tile shows.
        assert!(last.starts_with('\u{2026}'));
        assert!(last.ends_with(" the conclusion"));
    }

    #[test]
    fn a_result_that_came_back_with_text_still_ends_its_call() {
        // Seen live: Edit results arrive with a reminder attached, so the
        // message reads as the person's - and every Edit looked unfinished.
        let carried = json!({"speaker": "you", "role": "user",
                             "blocks": [result("a"), {"kind": "text", "text": "<system-reminder>"}]});
        let f = focus(
            &[you("go"), claude(vec![text("editing"), tool("a", "Edit", "x")]), carried, claude(vec![text("done")])],
            true,
        );
        assert!(f["current"].is_null());
        let turns: Vec<i64> = f["replies"].as_array().unwrap().iter().map(|r| r["turn"].as_i64().unwrap()).collect();
        assert_eq!(turns, [1, 1]);
    }

    #[test]
    fn system_notes_are_not_replies() {
        let note = json!({"speaker": "system", "role": "user", "note": "task",
                          "blocks": [{"kind": "text", "text": "background task done"}]});
        assert!(focus(&[note], true)["replies"].as_array().unwrap().is_empty());
    }

    #[test]
    fn working_is_a_turn_in_progress() {
        let row = |v: Value| v.as_object().unwrap().clone();
        assert!(working(&row(json!({"status": "busy"}))));
        assert!(working(&row(json!({"status": "shell"}))));
        assert!(working(&row(json!({"status": "idle", "state": "working"}))));
        assert!(!working(&row(json!({"status": "idle"}))));
    }

    #[test]
    fn spend_totals_and_each_session_most_costly_first() {
        let tile = |v: Value| v.as_object().unwrap().clone();
        let got = spend(
            &[
                tile(json!({"name": "a", "cost_usd": 1.5, "tokens": {"input": 10, "output": 5}})),
                tile(json!({"name": "b", "cost_usd": 4.0, "tokens": {"cache_read": 100}})),
                tile(json!({"name": "c", "cost_usd": null, "tokens": null})),
            ],
            &json!({"five_hour": {"used": 40}}),
        );
        assert!((got["cost_usd"].as_f64().unwrap() - 5.5).abs() < 1e-9);
        assert_eq!((got["costed"].as_i64(), got["sessions"].as_i64()), (Some(2), Some(3)));
        assert_eq!(got["tokens"], json!({"input": 10, "output": 5, "cache_read": 100, "cache_write": 0}));
        assert_eq!(got["total_tokens"], 115);
        let names: Vec<&str> = got["each"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["b", "a", "c"]);
        assert_eq!(got["limits"]["five_hour"]["used"], 40);
    }

    #[test]
    fn the_meters_come_back_after_a_restart() {
        use crate::usage::{Usage, DEFAULT_THRESHOLDS};
        let dir = std::env::temp_dir().join(format!("imp-matrix-usage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("usage.json");
        let (now, payload) = (1_800_000_000.0, |used: f64, resets: f64| {
            json!({"rate_limits": {"five_hour": {"used_percentage": used, "resets_at": resets}}})
                .as_object()
                .unwrap()
                .clone()
        });
        let first = Usage::new(DEFAULT_THRESHOLDS.to_vec(), Some(path.clone()), true, now - 120.0);
        let both = json!({"rate_limits": {
            "five_hour": {"used_percentage": 30, "resets_at": now + 3600.0},
            "seven_day": {"used_percentage": 85, "resets_at": now - 60.0}}});
        first.ingest("s", both.as_object().unwrap(), now - 120.0);
        let again = Usage::new(DEFAULT_THRESHOLDS.to_vec(), Some(path), true, now);
        let limits = again.summary(now)["limits"].clone();
        assert_eq!(limits["five_hour"]["used"], 30.0);
        // It reset while we were away: shown at 0%, waiting for a reading.
        assert_eq!((limits["seven_day"]["used"].clone(), limits["seven_day"]["waiting"].clone()), (json!(0.0), json!(true)));
        // A threshold already crossed is not announced again.
        let crossed = again.ingest("s", &payload(81.0, now + 3600.0), now).expect("80% is announced once");
        assert_eq!(crossed["threshold"], 80.0);
        assert!(again.ingest("s", &payload(82.0, now + 3600.0), now).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
