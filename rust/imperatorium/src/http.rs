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

/// One request to the dispatcher API (the CLI's `urlopen`): the status and
/// body of any answer, an error status included; `Err` only when nothing
/// answered (refused, timed out). `body` is sent as JSON, which makes it a POST.
pub fn call(url: &str, body: Option<&str>, timeout: Duration) -> Result<(u16, String), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .timeout_write(timeout)
        .build();
    let got = match body {
        Some(b) => agent.post(url).set("Content-Type", "application/json").send_string(b),
        None => agent.get(url).set("Content-Type", "application/json").call(),
    };
    match got {
        Ok(r) => {
            let code = r.status();
            Ok((code, r.into_string().map_err(|e| e.to_string())?))
        }
        Err(ureq::Error::Status(code, r)) => Ok((code, r.into_string().unwrap_or_default())),
        Err(e) => Err(e.to_string()),
    }
}
