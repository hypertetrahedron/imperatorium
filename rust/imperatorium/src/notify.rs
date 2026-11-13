//! Telling you a session wants you when you are not looking at the window,
//! from `imperatorium/notify.py`: a push through an ntfy server, and
//! Windows' speech synthesiser. Both are off until config.json turns them on.
//!
//! Each channel waits its `delay` and then checks that the session is
//! *still* waiting before it fires, so answering at the desk within that
//! window buzzes nothing.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use crate::pyish::{py_float, py_str, strip, truthy};

/// notify.headline: "gem-trip: needs permission".
pub fn headline(name: Option<&str>, reason: Option<&str>) -> String {
    format!(
        "{}: {}",
        name.filter(|n| !n.is_empty()).unwrap_or("a session"),
        reason
            .filter(|r| !r.is_empty())
            .unwrap_or("waiting for you")
    )
}

/// notify.spoken: hyphens read badly aloud.
pub fn spoken(name: Option<&str>, reason: Option<&str>) -> String {
    let name = name.filter(|n| !n.is_empty()).unwrap_or("a session");
    format!(
        "{} {}",
        name.replace(['-', '_'], " "),
        reason
            .filter(|r| !r.is_empty())
            .unwrap_or("is waiting for you")
    )
}

/// notify.speak_command: PowerShell that says `text` with System.Speech.
pub fn speak_command(text: &str, rate: i64) -> Vec<String> {
    let safe = text.replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Speech; \
         $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $s.Rate = {rate}; $s.Speak('{safe}')"
    );
    [
        "powershell",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &script,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// One push: (url, headers, body).
pub type Push = (String, Vec<(String, String)>, Vec<u8>);

/// notify.ntfy_request: one push. Pure.
pub fn ntfy_request(
    cfg: &Map<String, Value>,
    title: &str,
    message: &str,
    tags: &[&str],
) -> Result<Push, String> {
    let text = |k: &str| cfg.get(k).filter(|v| truthy(Some(v))).map(py_str);
    let topic = text("topic").unwrap_or_default();
    let topic = strip(&topic).trim_matches('/');
    if topic.is_empty() {
        return Err("ValueError: ntfy topic is not set".into());
    }
    let server = text("server").unwrap_or_else(|| "https://ntfy.sh".into());
    let url = format!("{}/{}", server.trim_end_matches('/'), topic);
    let mut headers = vec![
        ("Title".to_string(), title.to_string()),
        (
            "Priority".to_string(),
            text("priority").unwrap_or_else(|| "default".into()),
        ),
    ];
    if !tags.is_empty() {
        headers.push(("Tags".into(), tags.join(",")));
    }
    if let Some(t) = text("token") {
        headers.push(("Authorization".into(), format!("Bearer {t}")));
    }
    Ok((url, headers, message.as_bytes().to_vec()))
}

/// Channel settings with whether each is on right now.
pub type Channels = Vec<(&'static str, Map<String, Value>, bool)>;

fn channel<'a>(
    channels: &'a Channels,
    name: &str,
) -> Option<&'a (&'static str, Map<String, Value>, bool)> {
    channels.iter().find(|(n, _, _)| *n == name)
}

/// What a firing needs to know at the moment it fires: whether the channel
/// is still on, and whether the session is still waiting on that raise.
pub trait Waiting: Send + Sync {
    fn channels(&self) -> Channels;
    fn still_waiting(&self, key: &str, since: &Value) -> bool;
}

#[derive(Default)]
pub struct Notifier {
    scheduled: Mutex<HashSet<(String, String, String)>>,
    last_error: Mutex<Option<String>>,
    sent: AtomicU64,
}

impl Notifier {
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().clone()
    }

    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::SeqCst)
    }

    /// Notifier.attention: a session just started waiting; schedule each
    /// enabled channel, once per (channel, session, raise).
    pub fn attention(
        self: &Arc<Self>,
        owner: Arc<dyn Waiting>,
        key: &str,
        name: Option<String>,
        reason: Option<String>,
        since: Value,
    ) {
        let channels = owner.channels();
        for ch in ["ntfy", "speak"] {
            let Some((_, cfg, true)) = channel(&channels, ch) else {
                continue;
            };
            let mark = (ch.to_string(), key.to_string(), py_str(&since));
            if !self.scheduled.lock().unwrap().insert(mark.clone()) {
                continue;
            }
            let delay = cfg
                .get("delay")
                .filter(|v| truthy(Some(v)))
                .and_then(py_float)
                .unwrap_or(0.0);
            let (me, owner, key) = (Arc::clone(self), Arc::clone(&owner), key.to_string());
            let (name, reason, since) = (name.clone(), reason.clone(), since.clone());
            std::thread::spawn(move || {
                if delay > 0.0 && delay.is_finite() {
                    std::thread::sleep(std::time::Duration::from_secs_f64(delay));
                }
                let now = owner.channels();
                let on = channel(&now, ch).is_some_and(|c| c.2);
                if on && owner.still_waiting(&key, &since) {
                    if ch == "ntfy" {
                        let text = headline(name.as_deref(), reason.as_deref());
                        me.push(
                            &channel(&now, "ntfy").unwrap().1,
                            &text,
                            "Imperatorium",
                            &["warning"],
                        );
                    } else {
                        me.say(
                            &channel(&now, "speak").unwrap().1,
                            &spoken(name.as_deref(), reason.as_deref()),
                        );
                    }
                }
                me.scheduled.lock().unwrap().remove(&mark);
            });
        }
    }

    /// Notifier.budget: a usage threshold was crossed - at once, since no
    /// one answers these. The line, which the event records too.
    pub fn budget(&self, channels: &Channels, crossing: &Value) -> String {
        let window = crossing["window"].as_str().unwrap_or("");
        let label = match window {
            "five_hour" => "5-hour",
            "seven_day" => "weekly",
            "spend_limit" => "spend",
            other => other,
        };
        let used = crossing["used"].as_f64().unwrap_or(0.0);
        let mut text = format!("{label} limit at {}%", crate::pyish::py_round(used));
        if let Some(r) = crossing
            .get("resets_at")
            .and_then(Value::as_f64)
            .filter(|r| *r != 0.0)
        {
            text.push_str(&format!(", resets {}", crate::paths::local_hm(r)));
        }
        if let Some((_, cfg, true)) = channel(channels, "ntfy") {
            self.push(
                cfg,
                &text,
                "Imperatorium: usage",
                &["chart_with_upwards_trend"],
            );
        }
        if let Some((_, cfg, true)) = channel(channels, "speak") {
            self.say(cfg, &text.replace('%', " percent"));
        }
        text
    }

    fn push(&self, cfg: &Map<String, Value>, message: &str, title: &str, tags: &[&str]) {
        let sent = ntfy_request(cfg, title, message, tags).and_then(|(url, headers, body)| {
            let mut req = ureq::post(&url).timeout(std::time::Duration::from_secs(10));
            for (k, v) in &headers {
                req = req.set(k, v);
            }
            req.send_bytes(&body).map(|_| ()).map_err(|e| e.to_string())
        });
        match sent {
            Ok(()) => {
                self.sent.fetch_add(1, Ordering::SeqCst);
                *self.last_error.lock().unwrap() = None;
            }
            Err(e) => *self.last_error.lock().unwrap() = Some(format!("ntfy: {e}")),
        }
    }

    fn say(&self, cfg: &Map<String, Value>, text: &str) {
        if !cfg!(windows) {
            return;
        }
        let rate = cfg
            .get("rate")
            .filter(|v| truthy(Some(v)))
            .and_then(py_float)
            .map(|r| r.trunc() as i64)
            .unwrap_or(0);
        let argv = speak_command(text, rate);
        let mut cmd = std::process::Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        if let Err(e) = cmd.spawn() {
            *self.last_error.lock().unwrap() = Some(format!("speak: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lines_match_python() {
        assert_eq!(
            headline(Some("gem-trip"), Some("needs permission")),
            "gem-trip: needs permission"
        );
        assert_eq!(headline(None, None), "a session: waiting for you");
        assert_eq!(
            spoken(Some("gem-trip_2"), None),
            "gem trip 2 is waiting for you"
        );
        // python: notify.speak_command("it's 5", 2)[-1]
        assert_eq!(
            speak_command("it's 5", 2)[4],
            "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
             $s.Rate = 2; $s.Speak('it''s 5')"
        );
    }

    #[test]
    fn ntfy_request_matches_python() {
        let cfg = json!({"server": "https://n.example/", "topic": " /t1/ ", "priority": null, "token": "k"});
        let (url, headers, body) =
            ntfy_request(cfg.as_object().unwrap(), "T", "m\u{e9}", &["a", "b"]).unwrap();
        assert_eq!(url, "https://n.example/t1");
        assert_eq!(
            headers,
            vec![
                ("Title".into(), "T".into()),
                ("Priority".into(), "default".into()),
                ("Tags".into(), "a,b".into()),
                ("Authorization".into(), "Bearer k".into())
            ]
        );
        assert_eq!(body, "m\u{e9}".as_bytes());
        let none = json!({"topic": ""});
        assert!(ntfy_request(none.as_object().unwrap(), "T", "m", &[]).is_err());
    }
}
