//! Python's semantics where the port has to reproduce them exactly.
//!
//! The Python hooks are the reference until the conformance suite passes
//! against this binary, and several of their behaviours are Python-specific:
//! truthiness, `str()` of a non-string, `json.dumps`' exact bytes, what `\s`
//! and `str.strip()` count as whitespace, banker's rounding. Each lives here
//! once, so the ports read like the code they replace.

use serde_json::{Map, Value};

/// Python truthiness of a JSON value (`None` for a missing key).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `a or b` on JSON values: the first truthy one, else `b`.
pub fn or<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    if truthy(a) {
        a
    } else {
        b
    }
}

/// Python's whitespace for `str.split()`/`str.strip()` and the `re` module's
/// `\s`: Unicode White_Space plus the four separators U+001C..U+001F, which
/// Python counts and Rust does not.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

pub fn split_ws(s: &str) -> Vec<&str> {
    s.split(is_space).filter(|t| !t.is_empty()).collect()
}

/// `str.casefold()`: lowercasing per character (no final-sigma context,
/// as casefold has none) plus the full foldings Unicode adds on top of
/// lowercase that plausibly occur in a title.
pub fn casefold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{df}' | '\u{1e9e}' => out.push_str("ss"),
            '\u{17f}' => out.push('s'),
            '\u{3c2}' => out.push('\u{3c3}'),
            '\u{fb00}' => out.push_str("ff"),
            '\u{fb01}' => out.push_str("fi"),
            '\u{fb02}' => out.push_str("fl"),
            '\u{fb03}' => out.push_str("ffi"),
            '\u{fb04}' => out.push_str("ffl"),
            '\u{fb05}' | '\u{fb06}' => out.push_str("st"),
            '\u{149}' => out.push_str("\u{2bc}n"),
            '\u{3c9}' | '\u{3a9}' | '\u{2126}' => out.push('\u{3c9}'),
            '\u{b5}' => out.push('\u{3bc}'),
            '\u{1e9b}' => out.push('\u{1e61}'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// `s[:n]`: the first n code points.
pub fn take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Translate a Python `re` pattern for the `regex` crate where the two
/// differ in ways the policy can see: `\s`/`\S` gain Python's four extra
/// separators. Everything else used here means the same in both.
pub fn py_regex(pattern: &str) -> String {
    const EXTRA: &str = r"\x1c-\x1f";
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;
    let mut class_start = false;
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('s') if in_class => {
                    out.push_str(r"\s");
                    out.push_str(EXTRA);
                }
                Some('s') => out.push_str(&format!(r"[\s{EXTRA}]")),
                Some('S') if !in_class => out.push_str(&format!(r"[^\s{EXTRA}]")),
                Some(n) => {
                    out.push('\\');
                    out.push(n);
                }
                None => out.push('\\'),
            }
            class_start = false;
            continue;
        }
        if in_class {
            // A `]` straight after `[` or `[^` is a literal, as in Python.
            if c == ']' && !class_start {
                in_class = false;
            }
            class_start = class_start && c == '^';
        } else if c == '[' {
            in_class = true;
            class_start = true;
        }
        out.push(c);
    }
    out
}

/// Python's `round()` for the statusline: halves go to the even neighbour.
pub fn py_round(f: f64) -> i64 {
    f.round_ties_even() as i64
}

/// `str(value)` as Python prints a json-loaded value.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => py_repr(v),
    }
}

/// `repr(value)` as Python prints a json-loaded value.
pub fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => number_repr(n),
        Value::String(s) => str_repr(s),
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().map(py_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(o) => {
            let parts: Vec<String> = o
                .iter()
                .map(|(k, v)| format!("{}: {}", str_repr(k), py_repr(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || (0x7f..0xa0).contains(&(c as u32)) => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn number_repr(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    // With arbitrary_precision the number keeps its source text. An integer
    // literal of any size is a Python int, printed exactly; `-0` is 0.
    let text = n.to_string();
    let digits = text.strip_prefix('-').unwrap_or(&text);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        let trimmed = digits.trim_start_matches('0');
        return match (trimmed.is_empty(), text.starts_with('-')) {
            (true, _) => "0".into(),
            (false, true) => format!("-{trimmed}"),
            (false, false) => trimmed.to_string(),
        };
    }
    float_repr(n.as_f64().unwrap_or(f64::NAN))
}

/// `repr(float)`: shortest round-trip digits, fixed notation for exponents
/// -4..16, otherwise `d.ddde+XX`.
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // `{:e}` gives the shortest round-trip digits: "-1.2345e-5".
    let e = format!("{:e}", f);
    let (mantissa, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    if (-4..16).contains(&exp) {
        let point = exp + 1; // digits before the decimal point
        let body = if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point as usize >= digits.len() {
            format!("{}{}.0", digits, "0".repeat(point as usize - digits.len()))
        } else {
            let (a, b) = digits.split_at(point as usize);
            format!("{a}.{b}")
        };
        format!("{sign}{body}")
    } else {
        let mut m = digits[..1].to_string();
        if digits.len() > 1 {
            m.push('.');
            m.push_str(&digits[1..]);
        }
        let esign = if exp < 0 { '-' } else { '+' };
        format!("{sign}{m}e{esign}{:02}", exp.abs())
    }
}

/// `json.dumps(value)` byte for byte: `", "` and `": "` separators, and with
/// `ensure_ascii` every character outside space..`~` escaped.
pub fn json_dumps(v: &Value, ensure_ascii: bool) -> String {
    let mut out = String::new();
    write_json(&mut out, v, ensure_ascii, false);
    out
}

/// `json.dumps(value, sort_keys=True)`: keys in code point order, at every
/// level. What convcache.signature hashes.
pub fn json_dumps_sorted(v: &Value) -> String {
    let mut out = String::new();
    write_json(&mut out, v, true, true);
    out
}

fn write_json(out: &mut String, v: &Value, ascii: bool, sort: bool) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number_repr(n)),
        Value::String(s) => write_json_str(out, s, ascii),
        Value::Array(a) => {
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_json(out, item, ascii, sort);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            let mut pairs: Vec<(&String, &Value)> = o.iter().collect();
            if sort {
                // Rust's str order is byte order, which for UTF-8 is code
                // point order: what Python's sort_keys uses.
                pairs.sort_by(|a, b| a.0.cmp(b.0));
            }
            for (i, (k, item)) in pairs.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_json_str(out, k, ascii);
                out.push_str(": ");
                write_json(out, item, ascii, sort);
            }
            out.push('}');
        }
    }
}

fn write_json_str(out: &mut String, s: &str, ascii: bool) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ascii && (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `json.loads` for a hook's stdin: `{}` for blank input, `None` for
/// anything that is not JSON.
pub fn loads_or_empty(raw: &str) -> Option<Value> {
    if strip(raw).is_empty() {
        return Some(Value::Object(Map::new()));
    }
    serde_json::from_str(raw).ok()
}

/// `float(x)` for a json-loaded value; `None` where Python would raise.
pub fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            // float() takes an underscore only between two digits.
            let raw = strip(s);
            let b = raw.as_bytes();
            let between = |i: usize| {
                i > 0 && i + 1 < b.len() && b[i - 1].is_ascii_digit() && b[i + 1].is_ascii_digit()
            };
            if b.iter().enumerate().any(|(i, c)| *c == b'_' && !between(i)) {
                return None;
            }
            let t = raw.replace('_', "");
            match t.to_ascii_lowercase().as_str() {
                "inf" | "+inf" | "infinity" | "+infinity" => Some(f64::INFINITY),
                "-inf" | "-infinity" => Some(f64::NEG_INFINITY),
                "nan" | "+nan" | "-nan" => Some(f64::NAN),
                _ => t.parse().ok(),
            }
        }
        _ => None,
    }
}

/// `isinstance(v, (int, float))`, as a float (bool is an int in Python).
pub fn py_number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dumps_matches_python() {
        let v = json!({"a": [1, 2.5, null, true], "b": "é\n\"x\"", "c": {}});
        assert_eq!(
            json_dumps(&v, true),
            r#"{"a": [1, 2.5, null, true], "b": "\u00e9\n\"x\"", "c": {}}"#
        );
        assert_eq!(json_dumps(&json!("😀"), true), r#""\ud83d\ude00""#);
        assert_eq!(json_dumps(&json!("é"), false), "\"é\"");
    }

    #[test]
    fn float_repr_matches_python() {
        for (f, want) in [
            (20.0, "20.0"),
            (0.5, "0.5"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (-2.25, "-2.25"),
            (123.456, "123.456"),
            (1e100, "1e+100"),
        ] {
            assert_eq!(float_repr(f), want, "{f}");
        }
    }

    #[test]
    fn repr_matches_python() {
        assert_eq!(
            py_repr(&json!(["a", 1, null, {"k": false}])),
            "['a', 1, None, {'k': False}]"
        );
        assert_eq!(py_repr(&json!("it's")), "\"it's\"");
        assert_eq!(py_str(&json!("plain")), "plain");
    }

    #[test]
    fn python_whitespace() {
        assert_eq!(strip("\u{1c} x \u{1f}"), "x");
        assert_eq!(split_ws("a\u{1d}b  c"), vec!["a", "b", "c"]);
        let re = regex::Regex::new(&py_regex(r"git\s+push")).unwrap();
        assert!(re.is_match("git\u{1c}push"));
        let re = regex::Regex::new(&py_regex(r#"(^|[\s\\/=:"'])x"#)).unwrap();
        assert!(re.is_match("a\u{1e}x"));
        let re = regex::Regex::new(&py_regex(r"^\S+$")).unwrap();
        assert!(!re.is_match("a\u{1e}b"));
    }

    #[test]
    fn sum_is_compensated_as_in_python_312() {
        // python3.12 -c "print(repr(sum([0.1]*10)), repr(sum([1e100, 1.0, -1e100])))"
        assert_eq!(py_fsum_builtin([0.1; 10]), 1.0);
        assert_eq!(py_fsum_builtin([1e100, 1.0, -1e100]), 1.0);
        assert_eq!(py_fsum_builtin(std::iter::empty()), 0.0);
    }

    #[test]
    fn bankers_rounding() {
        assert_eq!(py_round(2.5), 2);
        assert_eq!(py_round(3.5), 4);
        assert_eq!(py_round(41.4), 41);
    }
}

/// Python 3.12+ `sum()` of floats (start 0): Neumaier's compensated
/// summation, as CPython does it - the first item as the running total, a
/// compensation term kept beside it, added back at the end when finite.
pub fn py_fsum_builtin(items: impl IntoIterator<Item = f64>) -> f64 {
    let mut it = items.into_iter();
    let Some(first) = it.next() else { return 0.0 };
    // `0 + first` in Python's int path, then the float path from there.
    let (mut f, mut c) = (0.0 + first, 0.0f64);
    for x in it {
        let t = f + x;
        if f.abs() >= x.abs() {
            c += (f - t) + x;
        } else {
            c += (x - t) + f;
        }
        f = t;
    }
    if c != 0.0 && c.is_finite() {
        f += c;
    }
    f
}

/// `json.dumps(value, indent=n)`: ensure_ascii, "," and ": " separators,
/// each item on its own line; empty containers stay `[]` and `{}`.
pub fn json_dumps_indent(v: &Value, indent: usize) -> String {
    fn pad(out: &mut String, indent: usize, level: usize) {
        out.push('\n');
        out.push_str(&" ".repeat(indent * level));
    }
    fn go(out: &mut String, v: &Value, indent: usize, level: usize) {
        match v {
            Value::Array(a) if !a.is_empty() => {
                out.push('[');
                for (i, item) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    pad(out, indent, level + 1);
                    go(out, item, indent, level + 1);
                }
                pad(out, indent, level);
                out.push(']');
            }
            Value::Object(o) if !o.is_empty() => {
                out.push('{');
                for (i, (k, item)) in o.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    pad(out, indent, level + 1);
                    write_json_str(out, k, true);
                    out.push_str(": ");
                    go(out, item, indent, level + 1);
                }
                pad(out, indent, level);
                out.push('}');
            }
            other => write_json(out, other, true, false),
        }
    }
    let mut out = String::new();
    go(&mut out, v, indent, 0);
    out
}
