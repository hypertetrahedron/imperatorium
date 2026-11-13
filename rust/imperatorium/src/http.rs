//! The one HTTP call the hooks make: POST a JSON body to the local
//! dispatcher, with Python's urlopen semantics (the timeout bounds the
//! connect and each read; a 4xx/5xx is an error).

use std::time::Duration;

pub fn post_json(url: &str, body: &str, timeout: Duration) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .timeout_write(timeout)
        .build();
    let resp = agent
        .post(url)
        .set("Content-Type", "application/json")
        .send_string(body)
        .map_err(|e| e.to_string())?;
    resp.into_string().map_err(|e| e.to_string())
}

/// A timeout from an environment variable in seconds, else `default`.
pub fn env_timeout(name: &str, default: f64) -> Duration {
    let secs = std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(default);
    Duration::from_secs_f64(secs)
}
