//! The Settings form: reading config.json, applying and saving a change.

use super::*;

impl Shared {
    /// The config as applied now.
    pub(super) fn config(&self) -> Value {
        self.config_live.rd().clone()
    }

    /// Control.save_settings: check every change, write config.json once,
    /// and apply at once what can be applied; the rest waits for a restart,
    /// which the answer names.
    pub(super) fn save_settings(&self, changes: Option<&Value>) -> Value {
        let Some(changes) = changes.and_then(Value::as_object).filter(|c| !c.is_empty()) else {
            return json!({"ok": false, "error": "nothing to save"});
        };
        let path = paths::config_home(self.home.as_deref()).join("config.json");
        let need = {
            let _held = self.settings_lock.lk();
            let cfg = match settings::load(&path) {
                Ok(c) => c,
                Err(e) => {
                    return json!({"ok": false, "error": format!("config.json cannot be read: {e}")})
                }
            };
            let new = match settings::apply(&cfg, changes) {
                Ok(n) => n,
                Err(errors) => {
                    return json!({"ok": false, "errors": errors, "error": "some values were not accepted"})
                }
            };
            if let Err(e) = settings::save(&path, &new) {
                return json!({"ok": false, "error": format!("config.json cannot be written: {e}")});
            }
            self.apply_settings(&new, changes);
            let need = settings::restarts(changes.keys());
            self.restart_needed
                .lk()
                .extend(need.iter().cloned());
            need
        };
        let mut out = self.api_settings();
        let mut saved: Vec<&String> = changes.keys().collect();
        saved.sort();
        out["saved"] = json!(saved);
        out["restart"] = json!(need);
        out
    }

    /// Control._apply_settings: what the running dispatcher takes without a
    /// restart. Everything else keeps its value from start.
    fn apply_settings(&self, new: &Value, changes: &Map<String, Value>) {
        const NOW: &[&str] = &[
            "chat.send_as_typed",
            "chat.cold_cache_tokens",
            "chat.conversation_cache_mb",
            "terminal.host",
            "terminal.hc_session",
            "chat.matrix_status",
            "quick.key",
            "quick.phrases",
            "budget.thresholds",
            "list.hide_finished_hours",
            "worktrees.confirm",
            "roots",
        ];
        let mut cfg = self.config_live.wr();
        let mut copy = |key: &str| {
            let parts: Vec<&str> = key.split('.').collect();
            let Some((last, path)) = parts.split_last() else {
                return;
            };
            // config_live is always an object, and each step is made one.
            let Some(mut node) = cfg.as_object_mut() else {
                return;
            };
            for part in path {
                let child = node.entry(part.to_string()).or_insert_with(|| json!({}));
                if !child.is_object() {
                    *child = json!({});
                }
                let Some(o) = child.as_object_mut() else {
                    return;
                };
                node = o;
            }
            match settings::get(new, key) {
                Some(v) => {
                    node.insert(last.to_string(), v.clone());
                }
                None => {
                    node.shift_remove(*last);
                }
            }
        };
        for key in changes.keys() {
            if NOW.contains(&key.as_str()) {
                copy(key);
            }
        }
        // The text sizes and highlights are redrawn from the new file whole.
        if changes.keys().any(|k| {
            k == "chat.font_size" || k == "chat.matrix_font_size" || k.starts_with("highlight.")
        }) {
            copy("chat.font_size");
            copy("chat.matrix_font_size");
            copy("highlight");
        }
        if changes.keys().any(|k| k.starts_with("background.")) {
            copy("background");
        }
        // The notifier always takes its channels from the new file.
        copy("notify");
        drop(cfg);
        let flips: Map<String, Value> = changes
            .iter()
            .filter(|(k, _)| *k == "notify.ntfy.enabled" || *k == "notify.speak.enabled")
            .map(|(k, v)| {
                (
                    k.split('.').nth(1).unwrap().to_string(),
                    json!(truthy(Some(v))),
                )
            })
            .collect();
        if let (Some(store), false) = (&self.store, flips.is_empty()) {
            // Kept where /api/notify keeps its switches, which win at start.
            let mut saved = self
                .state()
                .get("prefs")
                .and_then(|x| x.get("notify"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            saved.extend(flips);
            let mut change = Map::new();
            change.insert("notify".into(), Value::Object(saved));
            store.set_prefs(&change);
        }
        if changes.contains_key("budget.thresholds") {
            let t: Vec<f64> = new
                .get("budget")
                .and_then(|b| b.get("thresholds"))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(crate::pyish::py_float).collect())
                .unwrap_or_default();
            self.usage.set_thresholds(t);
        }
        if changes.contains_key("list.hide_finished_hours") {
            self.invalidate();
        }
    }

    /// GET /api/settings: Control.settings_view.
    pub(super) fn api_settings(&self) -> Value {
        let path = paths::config_home(self.home.as_deref()).join("config.json");
        let cfg = match settings::load(&path) {
            Ok(c) => c,
            Err(e) => {
                return json!({"ok": false, "error": format!("config.json cannot be read: {e}")})
            }
        };
        let mut out = settings::describe(&cfg);
        let enabled: HashMap<&str, bool> = self
            .notify_enabled()
            .into_iter()
            .map(|(n, _, on)| (n, on))
            .collect();
        if let Some(Value::Array(fields)) = out.get_mut("fields") {
            for f in fields.iter_mut() {
                let key = f["key"].as_str().unwrap_or("").to_string();
                if key == "notify.ntfy.enabled" || key == "notify.speak.enabled" {
                    let channel = key.split('.').nth(1).unwrap_or("");
                    f["value"] = json!(enabled.get(channel).copied().unwrap_or(false));
                }
            }
        }
        out["ok"] = json!(true);
        out["path"] = json!(path.to_string_lossy());
        out["waiting"] = json!(self
            .restart_needed
            .lk()
            .iter()
            .collect::<Vec<_>>());
        out["notify_error"] = json!(self.notifier.last_error());
        out["shortcuts"] = settings::schema()["shortcuts"].clone();
        out
    }
}

pub(super) fn load_config(home: Option<&str>) -> Value {
    std::fs::read_to_string(paths::config_home(home).join("config.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// settings._get: a dotted key, found or not.
pub(super) fn get<'a>(config: &'a Value, key: &str) -> Option<&'a Value> {
    let mut node = config;
    for part in key.split('.') {
        node = node.as_object()?.get(part)?;
    }
    Some(node)
}

const HIGHLIGHTS: &[&str] = &[
    "code", "diffs", "prose", "tools", "signals", "mute", "guides", "shape", "prompts",
];

const COLORS: &[&str] = &[
    "keyword", "string", "comment", "number", "function", "variable", "added", "removed", "path",
    "command", "ref", "quantity", "tool", "error", "success", "warning", "question", "guide", "prompt",
];

/// settings.look: what the pages draw with.
pub fn look(config: &Value) -> Value {
    let mut colors = Map::new();
    for name in COLORS {
        if let Some(Value::String(v)) = get(config, &format!("highlight.colors.{name}")) {
            let v = v.to_lowercase();
            let ok =
                v.len() == 7 && v.starts_with('#') && v[1..].bytes().all(|b| b.is_ascii_hexdigit());
            if ok {
                colors.insert((*name).into(), Value::String(v));
            }
        }
    }
    let mut hl = Map::new();
    for name in HIGHLIGHTS {
        // Every highlight defaults to on.
        let on = match get(config, &format!("highlight.{name}")) {
            Some(v) => truthy(Some(v)),
            None => true,
        };
        hl.insert((*name).into(), Value::Bool(on));
    }
    json!({
        "fonts": {
            "chat": get(config, "chat.font_size").cloned().unwrap_or(json!(14)),
            "matrix": get(config, "chat.matrix_font_size").cloned().unwrap_or(json!(13)),
        },
        "hl": hl,
        "colors": colors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_defaults() {
        let l = look(&json!({}));
        assert_eq!(l["fonts"]["chat"], 14);
        assert_eq!(l["hl"]["code"], true);
        let l = look(
            &json!({"chat": {"font_size": 15}, "highlight": {"mute": false, "colors": {"error": "#FF0000", "path": "red"}}}),
        );
        assert_eq!(l["fonts"]["chat"], 15);
        assert_eq!(l["hl"]["mute"], false);
        assert_eq!(l["colors"], json!({"error": "#ff0000"}));
    }
}
