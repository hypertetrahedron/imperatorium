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
