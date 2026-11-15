//! The Settings window's fields: settings.FIELDS/SECTIONS, written from
//! `imperatorium/settings.py` by `rust/assets/export_ui.py` and compiled in,
//! with `settings.value` (a field's value in config.json, else its default).

use std::sync::OnceLock;

use serde_json::Value;

const SCHEMA: &str = include_str!("../assets/settings_fields.json");

pub fn schema() -> &'static Value {
    static S: OnceLock<Value> = OnceLock::new();
    S.get_or_init(|| serde_json::from_str(SCHEMA).expect("settings_fields.json is exported JSON"))
}

pub fn field(key: &str) -> Option<&'static Value> {
    schema()["fields"]
        .as_array()?
        .iter()
        .find(|f| f["key"].as_str() == Some(key))
}

/// settings._get: a dotted key, found or not.
pub fn get<'a>(config: &'a Value, key: &str) -> Option<&'a Value> {
    let mut node = config;
    for part in key.split('.') {
        node = node.as_object()?.get(part)?;
    }
    Some(node)
}

/// settings.value: what config.json says, else the field's default.
pub fn value(config: &Value, key: &str) -> Value {
    if let Some(v) = get(config, key) {
        return v.clone();
    }
    let Some(f) = field(key) else {
        return Value::Null;
    };
    match &f["default"] {
        // _voice_on: a voice block that does not say turns dictation on.
        Value::Object(o) if o.get("from").and_then(Value::as_str) == Some("voice_block") => {
            Value::Bool(config.get("voice").is_some_and(Value::is_object))
        }
        d => d.clone(),
    }
}

/// settings.load: config.json as an object; {} when missing; an error when
/// it is there but unreadable or not an object.
pub fn load(path: &std::path::Path) -> Result<Value, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Value::Object(Default::default()))
        }
        Err(e) => return Err(e.to_string()),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v @ Value::Object(_)) => Ok(v),
        Ok(_) => Err("config.json is not a JSON object".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// settings.describe: the form - sections, fields with their values, and
/// the remote hosts (shown, not edited).
pub fn describe(config: &Value) -> Value {
    let mut fields = Vec::new();
    for f in schema()["fields"].as_array().into_iter().flatten() {
        let Some(fo) = f.as_object() else { continue };
        let key = fo.get("key").and_then(Value::as_str).unwrap_or("");
        let mut out: serde_json::Map<String, Value> = fo
            .iter()
            .filter(|(k, _)| *k != "default")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let v = value(config, key);
        out.insert("value".into(), v.clone());
        if fo.get("type").and_then(Value::as_str) == Some("choice") {
            let choices = fo
                .get("choices")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !choices.contains(&v) {
                let mut more = choices;
                more.push(v);
                out.insert("choices".into(), Value::Array(more));
                out.insert("locked".into(), Value::Bool(true));
            }
        }
        fields.push(Value::Object(out));
    }
    let hosts: Vec<Value> = config
        .get("remote")
        .filter(|r| crate::pyish::truthy(Some(r)))
        .and_then(|r| r.get("hosts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|h| match h {
            Value::Object(o) => serde_json::json!({
                "name": o.get("name").cloned().unwrap_or(Value::Null),
                "label": o.get("label").cloned().unwrap_or(Value::Null),
                "tunnel": crate::pyish::truthy(o.get("tunnel")),
            }),
            other => serde_json::json!({"name": other, "label": null, "tunnel": false}),
        })
        .collect();
    let sections: Vec<Value> = schema()["sections"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| serde_json::json!({"id": s[0], "title": s[1]}))
        .collect();
    serde_json::json!({"sections": sections, "fields": fields, "hosts": hosts,
                       "restart": schema()["restart"]})
}

/// notify.DEFAULTS, and notify.settings(config): each channel's settings
/// from config.json's `notify` block over them.
pub fn notify_channels(config: &Value) -> Vec<(&'static str, serde_json::Map<String, Value>)> {
    let defaults = [
        (
            "ntfy",
            serde_json::json!({"enabled": false, "server": "https://ntfy.sh", "topic": "",
                                    "delay": 60, "priority": "default", "token": null}),
        ),
        (
            "speak",
            serde_json::json!({"enabled": false, "delay": 5, "rate": 0}),
        ),
    ];
    let raw = config
        .get("notify")
        .filter(|n| crate::pyish::truthy(Some(n)));
    defaults
        .into_iter()
        .map(|(name, base)| {
            let mut got = base.as_object().unwrap().clone();
            if let Some(Value::Object(c)) = raw.and_then(|r| r.get(name)) {
                for (k, v) in c {
                    if !k.starts_with('_') {
                        got.insert(k.clone(), v.clone());
                    }
                }
            }
            (name, got)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_and_values() {
        assert_eq!(value(&json!({}), "quick.key"), json!("ctrl+space"));
        assert_eq!(
            value(&json!({"quick": {"key": "alt+q"}}), "quick.key"),
            json!("alt+q")
        );
        assert_eq!(value(&json!({}), "voice.enabled"), json!(false));
        assert_eq!(value(&json!({"voice": {}}), "voice.enabled"), json!(true));
        assert!(field("chat.font_size").is_some());
    }
}

// -- saving: settings.check / apply / save / restarts -----------------------

use serde_json::{json, Map};

use crate::pyish::{py_repr, py_str, strip};

/// `re.match(pattern + "$", v)`: a whole match, where Python's `$` also
/// matches before one trailing newline.
fn whole(re: &regex::Regex, v: &str) -> bool {
    re.is_match(v) || v.strip_suffix('\n').is_some_and(|t| re.is_match(t))
}

/// A whole-value pattern, compiled once per pattern (a Regex clone shares
/// the compiled program).
fn rx(pattern: &'static str) -> regex::Regex {
    type Compiled = std::collections::HashMap<&'static str, regex::Regex>;
    static SEEN: OnceLock<std::sync::Mutex<Compiled>> = OnceLock::new();
    use crate::locks::LockExt;
    SEEN.get_or_init(Default::default)
        .lk()
        .entry(pattern)
        .or_insert_with(|| {
            regex::Regex::new(&format!(r"\A(?:{pattern})\z")).expect("a settings pattern")
        })
        .clone()
}

/// settings._lines: one item per line or comma, stripped, blanks dropped.
fn lines(v: &Value) -> Result<Vec<String>, String> {
    let items: Vec<String> = match v {
        Value::String(s) if s.is_empty() => vec![],
        Value::String(s) => crate::activity::splitlines(&s.replace(',', "\n"))
            .into_iter()
            .map(str::to_string)
            .collect(),
        Value::Array(a) => a.iter().map(py_str).collect(),
        _ => return Err("expected a list".into()),
    };
    Ok(items
        .iter()
        .map(|x| strip(x).to_string())
        .filter(|x| !x.is_empty())
        .collect())
}

/// `[x.strip() for x in v.splitlines() if x.strip()]` for text, else _lines.
fn lines_or_list(v: &Value) -> Result<Vec<String>, String> {
    match v {
        Value::Array(_) => lines(v),
        Value::String(s) => Ok(crate::activity::splitlines(s)
            .into_iter()
            .map(|x| strip(x).to_string())
            .filter(|x| !x.is_empty())
            .collect()),
        _ => Err("expected a list".into()),
    }
}

/// `int(v)` for a string: whitespace, a sign, digits with single
/// underscores between them.
fn py_int_str(s: &str) -> Option<i64> {
    let t = strip(s);
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits.chars().all(|c| c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    let n: i64 = digits.replace('_', "").parse().ok()?;
    Some(if neg { -n } else { n })
}

fn num(f: &Value, k: &str) -> f64 {
    f[k].as_f64().unwrap_or(0.0)
}

/// `"%d" % x`.
fn pct_d(v: &Value) -> String {
    match v.as_i64() {
        Some(i) => i.to_string(),
        None => (v.as_f64().unwrap_or(0.0).trunc() as i64).to_string(),
    }
}

fn modifiers() -> Vec<String> {
    schema()["modifiers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// settings.normal_key: modifiers in the page's order.
fn normal_key(combo: &str) -> String {
    let parts: Vec<&str> = combo.split('+').collect();
    let (key, mods) = parts.split_last().unwrap();
    let mut out: Vec<String> = modifiers()
        .into_iter()
        .filter(|m| mods.contains(&m.as_str()))
        .collect();
    out.push(key.to_string());
    out.join("+")
}

const HOTKEY: &str = r"[a-z0-9]+(\+[a-z0-9]+)*";

/// settings.check: `v` as it should be stored, or why not. `current` is what
/// the file holds now, which a choice may keep though the form cannot pick it.
pub fn check(field: &Value, v: &Value, current: &Value) -> Result<Value, String> {
    let t = field["type"].as_str().unwrap_or("");
    let bounds = || (pct_d(&field["min"]), pct_d(&field["max"]));
    match t {
        "bool" => {
            return v
                .as_bool()
                .map(Value::Bool)
                .ok_or_else(|| "expected on or off".into())
        }
        "int" | "optint" => {
            if t == "optint" && (v.is_null() || v.as_str() == Some("")) {
                return Ok(Value::Null);
            }
            let n: Option<i64> = match v {
                Value::Number(n) if n.is_i64() => n.as_i64(),
                Value::Number(n) => n
                    .as_f64()
                    .filter(|f| f.is_finite() && f.fract() == 0.0)
                    .map(|f| f as i64),
                Value::String(s) => py_int_str(s),
                _ => None,
            };
            let Some(n) = n else {
                return Err("expected a whole number".into());
            };
            if !(num(field, "min") <= n as f64 && n as f64 <= num(field, "max")) {
                let (lo, hi) = bounds();
                return Err(format!("must be from {lo} to {hi}"));
            }
            return Ok(json!(n));
        }
        "choice" => {
            let v = if v.is_null() { json!("") } else { v.clone() };
            let choices = field["choices"].as_array().cloned().unwrap_or_default();
            if choices.contains(&v) || (crate::pyish::truthy(Some(&v)) && &v == current) {
                return Ok(v);
            }
            let names: Vec<String> = choices
                .iter()
                .map(|c| match c.as_str() {
                    Some("") => "(blank)".to_string(),
                    _ => py_str(c),
                })
                .collect();
            return Err(format!("must be one of: {}", names.join(", ")));
        }
        _ => {}
    }
    if !matches!(v, Value::String(_) | Value::Array(_)) {
        return Err("expected text".into());
    }
    let text = v.as_str();
    match t {
        "color" => match text.map(|s| strip(s).to_lowercase()) {
            Some(s) if s.is_empty() || whole(&rx("#[0-9a-f]{6}"), &s) => Ok(json!(s)),
            _ => Err("a colour like #3a7bd5, or blank for the theme's".into()),
        },
        "name" => match text.map(strip) {
            Some(s) if s.chars().count() <= 100 && whole(&rx(r"[A-Za-z0-9._:\[\]-]*"), s) => {
                Ok(json!(s))
            }
            _ => Err("letters, digits and . _ - : [ ] only".into()),
        },
        "url" => {
            let s = text
                .map(|s| strip(s).trim_end_matches('/').to_string())
                .unwrap_or_default();
            if whole(&rx(r"https?://[A-Za-z0-9.-]+(:\d+)?(/[\w./-]*)?"), &s) {
                Ok(json!(s))
            } else {
                Err("expected an http(s) address".into())
            }
        }
        "topic" => match text.map(strip) {
            Some(s) if whole(&rx("[A-Za-z0-9_-]{0,64}"), s) => Ok(json!(s)),
            _ => Err("letters, digits, _ and - only, up to 64".into()),
        },
        "lang" => match text.map(|s| strip(s).to_lowercase()) {
            Some(s) if whole(&rx("[a-z]{0,3}"), &s) => Ok(json!(s)),
            _ => Err("a language code such as en".into()),
        },
        "numbers" => {
            let mut out: Vec<f64> = vec![];
            for x in lines(v)? {
                let Some(n) = crate::pyish::py_float(&json!(x)) else {
                    return Err(format!("{} is not a number", py_repr(&json!(x))));
                };
                if !(num(field, "min") <= n && n <= num(field, "max")) {
                    let (lo, hi) = bounds();
                    return Err(format!("each must be from {lo} to {hi}"));
                }
                out.push(n);
            }
            if out.len() > 6 {
                return Err("at most 6".into());
            }
            let mut uniq: Vec<f64> = vec![];
            for n in out {
                if !uniq.contains(&n) {
                    uniq.push(n);
                }
            }
            uniq.sort_by(|a, b| a.partial_cmp(b).unwrap());
            Ok(Value::Array(
                uniq.into_iter()
                    .map(|n| {
                        if n.fract() == 0.0 {
                            json!(n as i64)
                        } else {
                            json!(n)
                        }
                    })
                    .collect(),
            ))
        }
        "paths" => {
            let out = lines_or_list(v)?;
            for p in &out {
                if !std::path::Path::new(p).is_dir() {
                    return Err(format!("no such folder: {p}"));
                }
            }
            Ok(json!(out))
        }
        "hotkeys" => {
            let out: Vec<String> = lines(v)?
                .iter()
                .map(|x| x.to_lowercase().replace(' ', ""))
                .collect();
            for x in &out {
                if !whole(&rx(HOTKEY), x) {
                    return Err(format!(
                        "{} is not a hotkey like ctrl+alt+j",
                        py_repr(&json!(x))
                    ));
                }
            }
            if out.is_empty() {
                return Err("at least one hotkey".into());
            }
            Ok(json!(out))
        }
        "key" => {
            let s = text
                .map(|s| s.to_lowercase().replace(' ', ""))
                .unwrap_or_default();
            let parts: Vec<&str> = s.split('+').collect();
            let mods = modifiers();
            let is_mod = |p: &str| mods.iter().any(|m| m == p);
            if !whole(&rx(HOTKEY), &s) || parts.len() < 2 || is_mod(parts[parts.len() - 1]) {
                return Err("a modifier and a key, like ctrl+space".into());
            }
            let head = &parts[..parts.len() - 1];
            if !head.iter().any(|p| ["ctrl", "alt", "win"].contains(p))
                || !head.iter().all(|p| is_mod(p))
            {
                return Err("needs ctrl, alt or win - shift alone just types".into());
            }
            let k = normal_key(&s);
            if let Some(what) = schema()["reserved_keys"].get(&k).and_then(Value::as_str) {
                return Err(format!("{k} is already {what}"));
            }
            Ok(json!(k))
        }
        "phrases" => {
            let out: Vec<String> = match v {
                Value::Array(a) => a.iter().map(|x| strip(&py_str(x)).to_string()).collect(),
                _ => crate::activity::splitlines(text.unwrap_or(""))
                    .into_iter()
                    .map(|x| strip(x).to_string())
                    .collect(),
            };
            let out: Vec<String> = out.into_iter().filter(|x| !x.is_empty()).collect();
            if out.len() > 50 {
                return Err("at most 50".into());
            }
            if out.iter().any(|x| x.chars().count() > 1000) {
                return Err("each at most 1000 characters".into());
            }
            Ok(json!(out))
        }
        "names" => {
            let out = lines_or_list(v)?;
            for x in &out {
                if x.chars().count() > 100 || x.chars().any(|c| "\\/\"<>".contains(c)) {
                    return Err(format!("{} is not a project name", py_repr(&json!(x))));
                }
            }
            Ok(json!(out))
        }
        _ => Err("unknown field type".into()),
    }
}

/// settings.apply: a new config with `changes` checked and set, or every
/// key's complaint. A blank background option or colour is removed, so the
/// default applies.
pub fn apply(config: &Value, changes: &Map<String, Value>) -> Result<Value, Map<String, Value>> {
    let mut errors = Map::new();
    let mut out = config.clone();
    for (key, v) in changes {
        let Some(f) = field(key) else {
            errors.insert(key.clone(), json!("not a setting"));
            continue;
        };
        let current = get(config, key).cloned().unwrap_or(Value::Null);
        let got = match check(f, v, &current) {
            Ok(g) => g,
            Err(e) => {
                errors.insert(key.clone(), json!(e));
                continue;
            }
        };
        let parts: Vec<&str> = key.split('.').collect();
        let (last, path) = parts.split_last().unwrap();
        let mut node = &mut out;
        for part in path {
            if !node.get(*part).is_some_and(Value::is_object) {
                node[*part] = json!({});
            }
            node = node.get_mut(*part).unwrap();
        }
        let blank = got.as_str() == Some("");
        if (key.starts_with("background.") || f["type"].as_str() == Some("color")) && blank {
            node.as_object_mut().unwrap().shift_remove(*last);
        } else {
            node[*last] = got;
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// settings.save: atomic; the old file kept as config.json.bak.
pub fn save(path: &std::path::Path, config: &Value) -> Result<(), String> {
    if path.exists() {
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        std::fs::copy(path, bak).map_err(|e| e.to_string())?;
    }
    let text = format!("{}\n", crate::pyish::json_dumps_indent(config, 2));
    crate::paths::write_atomic(path, text.as_bytes()).map_err(|e| e.to_string())
}

/// settings.restarts: which processes must restart for `keys` to show.
pub fn restarts<'a>(keys: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let need: Vec<String> = keys
        .into_iter()
        .filter_map(|k| {
            field(k)
                .and_then(|f| f["applies"].as_str())
                .map(str::to_string)
        })
        .collect();
    schema()["restart"]
        .as_object()
        .map(|r| r.keys().filter(|p| need.contains(p)).cloned().collect())
        .unwrap_or_default()
}

/// `imperatorium settings-check`: JSON lines of {key, value, current} in,
/// {ok: value} or {error: message} out - for the conformance differential.
pub fn check_lines() -> i32 {
    use std::io::BufRead;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(case) = serde_json::from_str::<Value>(&line) else {
            println!("{}", json!({"error": "not JSON"}));
            continue;
        };
        let key = case["key"].as_str().unwrap_or("");
        let out = match field(key) {
            None => json!({"error": "not a setting"}),
            Some(f) => match check(f, &case["value"], &case["current"]) {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"error": e}),
            },
        };
        println!("{}", crate::pyish::json_dumps(&out, true));
    }
    0
}
