//! Pushes and speech: which channels are on, the notify page's data, announcing a waiting prompt.

use super::*;

impl Shared {
    /// Control._announce: tell the notifier a session started waiting,
    /// named by its project, which reads better aloud than `gem-trip-4`.
    pub(super) fn announce(&self, key: &str, caption: &str) {
        let (Some(store), Some(me)) = (
            &self.store,
            self.me.get().and_then(std::sync::Weak::upgrade),
        ) else {
            return;
        };
        let rec = store.session(key).unwrap_or_default();
        let since = rec.get("attention_since").cloned().unwrap_or(Value::Null);
        let cwd = rec
            .get("cwd")
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str)
            .unwrap_or_default();
        let base = |p: &str| p.rsplit(['/', '\\']).next().unwrap_or("").to_string();
        let name = rec
            .get("name")
            .filter(|v| truthy(Some(v)))
            .map(crate::pyish::py_str)
            .or_else(|| Some(base(cwd.trim_end_matches(['/', '\\']))).filter(|b| !b.is_empty()));
        let project = Some(base(cwd.replace('\\', "/").trim_end_matches('/')))
            .filter(|b| !b.is_empty())
            .or(name);
        let owner: Arc<dyn crate::notify::Waiting> = me;
        self.notifier
            .attention(owner, key, project, Some(caption.to_string()), since);
    }

    /// Each notification channel on or off: config.json at start, with the
    /// switch kept in prefs (`/api/notify`, impctl) winning, as Control does.
    pub(super) fn notify_enabled(&self) -> Vec<(&'static str, Map<String, Value>, bool)> {
        let st = self.state();
        let prefs = st
            .get("prefs")
            .and_then(|p| p.get("notify"))
            .cloned()
            .unwrap_or(json!({}));
        settings::notify_channels(&self.config())
            .into_iter()
            .map(|(name, cfg)| {
                let on = match prefs.get(name) {
                    Some(v) => truthy(Some(v)),
                    None => truthy(cfg.get("enabled")),
                };
                (name, cfg, on)
            })
            .collect()
    }

    /// GET /api/notify: notify.status(). `last_error` and `sent` count
    /// what this process has pushed, and the shadow pushes nothing.
    pub(super) fn api_notify(&self) -> Value {
        let mut out = Map::new();
        for (name, cfg, on) in self.notify_enabled() {
            let configured = if name == "ntfy" {
                truthy(cfg.get("topic"))
            } else {
                true
            };
            out.insert(
                name.into(),
                json!({"enabled": on, "delay": cfg.get("delay").cloned().unwrap_or(Value::Null),
                       "configured": configured}),
            );
        }
        out.insert("last_error".into(), json!(self.notifier.last_error()));
        out.insert("sent".into(), json!(self.notifier.sent()));
        Value::Object(out)
    }
}

impl crate::notify::Waiting for Shared {
    fn channels(&self) -> crate::notify::Channels {
        self.notify_enabled()
    }

    /// Control._still_waiting: through the session list first, which is
    /// where a prompt answered at the terminal is noticed.
    fn still_waiting(&self, key: &str, since: &Value) -> bool {
        self.invalidate();
        let _ = self.sessions(2.0);
        let Some(store) = &self.store else {
            return false;
        };
        truthy(Some(since))
            && store
                .session(key)
                .and_then(|r| r.get("attention_since").cloned())
                .as_ref()
                == Some(since)
    }
}
