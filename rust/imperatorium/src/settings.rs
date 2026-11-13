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
