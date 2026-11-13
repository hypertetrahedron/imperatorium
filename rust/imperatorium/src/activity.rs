//! The one-line "what is it doing" for a tile: `activity.doing` and
//! `first_line` from `imperatorium/activity.py`.

use serde_json::{json, Value};

use crate::pyish::{py_str, strip, take};

const ACTIVITY_CHARS: usize = 140;

/// Python's `str.splitlines()`: every line boundary it knows, `\r\n` as one.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let boundary = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !boundary {
            continue;
        }
        out.push(&text[start..i]);
        let mut next = i + c.len_utf8();
        if c == '\r' {
            if let Some(&(j, '\n')) = chars.peek() {
                chars.next();
                next = j + 1;
            }
        }
        start = next;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// The first non-blank line, cut to `limit` with an ellipsis.
pub fn first_line(text: Option<&Value>) -> String {
    let empty = json!("");
    let s = py_str(if crate::pyish::truthy(text) {
        text.unwrap()
    } else {
        &empty
    });
    for line in splitlines(&s) {
        let line = strip(line);
        if !line.is_empty() {
            let more = line.chars().count() > ACTIVITY_CHARS;
            return format!(
                "{}{}",
                take(line, ACTIVITY_CHARS),
                if more { "\u{2026}" } else { "" }
            );
        }
    }
    String::new()
}

/// activity.doing: the newest tool call, else the newest thing said.
pub fn doing(items: &[Value]) -> String {
    for item in items.iter().rev() {
        let speaker = item.get("speaker").and_then(Value::as_str);
        let blocks = item
            .get("blocks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for b in blocks.iter().rev() {
            match b.get("kind").and_then(Value::as_str) {
                Some("tool") => {
                    let name = b.get("name").map(py_str).unwrap_or_else(|| "None".into());
                    return format!("{} {}", name, first_line(b.get("summary")));
                }
                Some("text") if speaker == Some("claude") => return first_line(b.get("text")),
                Some("text") if speaker == Some("you") => {
                    return format!("asked: {}", first_line(b.get("text")))
                }
                _ => {}
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_as_python_does() {
        assert_eq!(splitlines("a\r\nb\rc\u{2028}d\n"), vec!["a", "b", "c", "d"]);
        assert_eq!(splitlines(""), Vec::<&str>::new());
        assert_eq!(first_line(Some(&json!("\n  \n  hello  \nworld"))), "hello");
    }

    #[test]
    fn the_newest_thing_wins() {
        let items = vec![
            json!({"speaker": "you", "blocks": [{"kind": "text", "text": "fix it"}]}),
            json!({"speaker": "claude", "blocks": [{"kind": "text", "text": "on it"},
                   {"kind": "tool", "name": "Bash", "summary": "make\nmore"}]}),
        ];
        assert_eq!(doing(&items), "Bash make");
        assert_eq!(doing(&items[..1]), "asked: fix it");
    }
}
