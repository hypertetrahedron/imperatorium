//! The account's monthly spend limit, read from Anthropic itself.
//!
//! Claude Code puts `rate_limits.spend_limit` in the statusline only behind a
//! Claude apps gateway. A usage-based Team or Enterprise login (an admin
//! sets a monthly budget) gets nothing there, yet the Claude desktop app and
//! Claude Code's own `/usage` show "$441.53 of $1000". Both read
//! `GET /api/oauth/usage` with the login's OAuth token; this reads the same
//! answer and hands it to `Usage` as the `spend_limit` window, so the meter,
//! the "left" row and the budget alerts need nothing more.
//!
//! The endpoint is not documented, so every field is read defensively and a
//! shape that does not fit shows nothing rather than a wrong number. The
//! token is only ever read, never refreshed: Claude Code owns it, and a
//! refresh token that rotates would log it out. An expired token waits for
//! Claude Code to write a new one.

use std::time::Duration;

use serde_json::{json, Map, Value};

const URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA: &str = "oauth-2025-04-20";
/// How often to ask, and the longest wait after failures.
pub const EVERY: f64 = 5.0 * 60.0;
pub const BACKOFF_MAX: f64 = 30.0 * 60.0;
/// A token this close to expiring is left alone until Claude Code renews it.
const EXPIRY_SLACK: f64 = 30.0;

/// The endpoint, overridable for tests.
fn url() -> String {
    std::env::var("IMPERATORIUM_ACCOUNT_SPEND_URL").unwrap_or_else(|_| URL.into())
}

/// `IMPERATORIUM_ACCOUNT_SPEND=off` turns the poll off.
pub fn enabled() -> bool {
    cfg!(feature = "tls")
        && !matches!(
            std::env::var("IMPERATORIUM_ACCOUNT_SPEND").as_deref(),
            Ok("off" | "0" | "false")
        )
}

/// Only a Team or Enterprise login has an admin's budget to show; a Pro or
/// Max login has its windows in the statusline.
pub fn wanted() -> bool {
    enabled() && crate::usage::account_plan().is_some()
}

/// Claude Code's OAuth access token, when the login is on disk and not about
/// to expire. (macOS keeps it in the keychain instead; there it is absent.)
pub fn token(now: f64) -> Option<String> {
    let text = std::fs::read_to_string(crate::paths::claude_home().join(".credentials.json")).ok()?;
    token_from(&serde_json::from_str::<Value>(&text).ok()?, now)
}

fn token_from(creds: &Value, now: f64) -> Option<String> {
    let oauth = creds.get("claudeAiOauth")?;
    let access = oauth.get("accessToken")?.as_str().filter(|t| !t.is_empty())?;
    if let Some(expires_ms) = oauth.get("expiresAt").and_then(Value::as_f64) {
        if expires_ms / 1000.0 <= now + EXPIRY_SLACK {
            return None;
        }
    }
    Some(access.into())
}

pub enum Failure {
    /// The token was refused: wait for Claude Code to renew it.
    Auth,
    /// Anything else (network, a 5xx, a body that is not JSON).
    Other(String),
}

pub fn fetch(token: &str, timeout: Duration) -> Result<Value, Failure> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .timeout_write(timeout)
        .build();
    let got = agent
        .get(&url())
        .set("Authorization", &format!("Bearer {token}"))
        .set("anthropic-beta", BETA)
        .set("Accept", "application/json")
        .call();
    match got {
        Ok(r) => {
            let body = r.into_string().map_err(|e| Failure::Other(e.to_string()))?;
            serde_json::from_str(&body).map_err(|e| Failure::Other(format!("not JSON: {e}")))
        }
        Err(ureq::Error::Status(401 | 403, _)) => Err(Failure::Auth),
        Err(ureq::Error::Status(code, _)) => Err(Failure::Other(format!("HTTP {code}"))),
        Err(e) => Err(Failure::Other(e.to_string())),
    }
}

/// Epoch seconds of the first moment of the month after the one `now` is in
/// (UTC, which is when Anthropic's monthly budget turns over).
pub fn next_month_start(now: f64) -> f64 {
    let (y, m, _) = crate::paths::civil((now / 86400.0).floor() as i64);
    let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (crate::paths::days_from_civil(y, m, 1) * 86400) as f64
}

fn minor(v: Option<&Value>, exponent: Option<&Value>) -> Option<f64> {
    let n = v?.as_f64()?;
    let e = exponent.and_then(Value::as_i64).unwrap_or(2).clamp(0, 6);
    Some(n / 10f64.powi(e as i32))
}

/// The `spend_limit` window (as `usage::reading` keeps it) from a usage
/// answer, or None when the account has no spend limit to show (disabled,
/// unlimited, not dollars, or a shape this does not know).
pub fn spend_limit(body: &Value, now: f64) -> Option<Map<String, Value>> {
    // `spend` switched off says so for both; the fallback is for a shape
    // without `spend`, or one this does not read.
    if body.pointer("/spend/enabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let (used, limit) = from_spend(body).or_else(|| from_extra_usage(body))?;
    if !(limit > 0.0) || !used.is_finite() || used < 0.0 {
        return None;
    }
    let mut w = Map::new();
    w.insert("used".into(), json!(used / limit * 100.0));
    w.insert("resets_at".into(), json!(next_month_start(now)));
    w.insert("used_usd".into(), json!(used));
    w.insert("limit_usd".into(), json!(limit));
    w.insert("period".into(), json!("monthly"));
    let mut out = Map::new();
    out.insert("spend_limit".into(), Value::Object(w));
    Some(out)
}

/// `spend`: {enabled, used: {amount_minor, exponent, currency}, limit: {..}}.
fn from_spend(body: &Value) -> Option<(f64, f64)> {
    let s = body.get("spend")?;
    if s.get("enabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let (u, l) = (s.get("used")?, s.get("limit")?);
    for m in [u, l] {
        if m.get("currency").and_then(Value::as_str).is_some_and(|c| c != "USD") {
            return None;
        }
    }
    Some((
        minor(u.get("amount_minor"), u.get("exponent"))?,
        minor(l.get("amount_minor"), l.get("exponent"))?,
    ))
}

/// `extra_usage`: {is_enabled, monthly_limit, used_credits, decimal_places,
/// currency}, in minor units: the same numbers, kept as a fallback.
fn from_extra_usage(body: &Value) -> Option<(f64, f64)> {
    let e = body.get("extra_usage")?;
    if e.get("is_enabled").and_then(Value::as_bool) != Some(true)
        || e.get("currency").and_then(Value::as_str).is_some_and(|c| c != "USD")
    {
        return None;
    }
    let dp = e.get("decimal_places");
    Some((minor(e.get("used_credits"), dp)?, minor(e.get("monthly_limit"), dp)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-18 00:00 UTC and the first moment of December.
    const NOW: f64 = 1_794_960_000.0;
    const DEC_1: f64 = 1_796_083_200.0;

    fn real_answer() -> Value {
        json!({
            "five_hour": null, "seven_day": null,
            "extra_usage": {"is_enabled": true, "monthly_limit": 100000, "used_credits": 44189.0,
                            "utilization": 44.189, "currency": "USD", "decimal_places": 2},
            "spend": {"used": {"amount_minor": 44189, "currency": "USD", "exponent": 2},
                      "limit": {"amount_minor": 100000, "currency": "USD", "exponent": 2},
                      "percent": 44, "enabled": true}
        })
    }

    fn only(w: Map<String, Value>) -> Map<String, Value> {
        w["spend_limit"].as_object().unwrap().clone()
    }

    #[test]
    fn the_answer_for_a_usage_based_enterprise_login_becomes_a_spend_window() {
        let w = only(spend_limit(&real_answer(), NOW).unwrap());
        assert_eq!(w["used_usd"], 441.89);
        assert_eq!(w["limit_usd"], 1000.0);
        assert!((w["used"].as_f64().unwrap() - 44.189).abs() < 1e-9);
        assert_eq!(w["period"], "monthly");
        assert_eq!(w["resets_at"], DEC_1);
    }

    #[test]
    fn extra_usage_alone_gives_the_same_numbers() {
        let mut b = real_answer();
        b.as_object_mut().unwrap().remove("spend");
        let w = only(spend_limit(&b, NOW).unwrap());
        assert_eq!((w["used_usd"].clone(), w["limit_usd"].clone()), (json!(441.89), json!(1000.0)));
    }

    #[test]
    fn no_limit_to_show_means_no_window() {
        let mut off = real_answer();
        off["spend"]["enabled"] = json!(false);
        assert!(spend_limit(&off, NOW).is_none());
        let mut unlimited = real_answer();
        unlimited["spend"]["limit"] = Value::Null;
        unlimited.as_object_mut().unwrap().remove("extra_usage");
        assert!(spend_limit(&unlimited, NOW).is_none());
        let mut zero = real_answer();
        zero["spend"]["limit"]["amount_minor"] = json!(0);
        zero["extra_usage"]["monthly_limit"] = json!(0);
        assert!(spend_limit(&zero, NOW).is_none());
        let mut euros = real_answer();
        euros["spend"]["used"]["currency"] = json!("EUR");
        euros["extra_usage"]["currency"] = json!("EUR");
        assert!(spend_limit(&euros, NOW).is_none());
        assert!(spend_limit(&json!({"five_hour": {"utilization": 3}}), NOW).is_none());
        assert!(spend_limit(&json!("nonsense"), NOW).is_none());
    }

    #[test]
    fn a_zero_exponent_is_whole_dollars() {
        let b = json!({"spend": {"used": {"amount_minor": 50, "exponent": 0, "currency": "USD"},
                                  "limit": {"amount_minor": 200, "exponent": 0, "currency": "USD"}}});
        let w = only(spend_limit(&b, NOW).unwrap());
        assert_eq!((w["used_usd"].clone(), w["used"].clone()), (json!(50.0), json!(25.0)));
    }

    #[test]
    fn the_month_turns_over_at_midnight_utc_and_across_the_year() {
        assert_eq!(next_month_start(NOW), DEC_1);
        assert_eq!(next_month_start(DEC_1 - 1.0), DEC_1);
        assert_eq!(next_month_start(DEC_1), DEC_1 + 31.0 * 86400.0);
        // 2026-12-31 23:59:59 -> 2027-01-01
        assert_eq!(next_month_start(1_798_761_599.0), 1_798_761_600.0);
    }

    #[test]
    fn a_token_is_used_only_while_it_has_time_left() {
        let creds = |exp_ms: f64| json!({"claudeAiOauth": {"accessToken": "tok", "expiresAt": exp_ms}});
        assert_eq!(token_from(&creds((NOW + 3600.0) * 1000.0), NOW).as_deref(), Some("tok"));
        assert!(token_from(&creds((NOW + 5.0) * 1000.0), NOW).is_none());
        assert!(token_from(&creds((NOW - 5.0) * 1000.0), NOW).is_none());
        assert!(token_from(&json!({"claudeAiOauth": {"accessToken": ""}}), NOW).is_none());
        assert!(token_from(&json!({}), NOW).is_none());
        // No expiry recorded: the server decides.
        assert_eq!(token_from(&json!({"claudeAiOauth": {"accessToken": "t"}}), NOW).as_deref(), Some("t"));
    }

    /// One canned answer from a local server; the request line and headers back.
    fn serve_once(status: u16, body: &str) -> (String, std::thread::JoinHandle<(String, Vec<(String, String)>)>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = format!("http://{}/api/oauth/usage", server.server_addr().to_ip().unwrap());
        let body = body.to_string();
        let h = std::thread::spawn(move || {
            let req = server.recv().unwrap();
            let seen = (
                format!("{} {}", req.method(), req.url()),
                req.headers().iter().map(|h| (h.field.to_string().to_lowercase(), h.value.to_string())).collect(),
            );
            let _ = req.respond(tiny_http::Response::from_string(body).with_status_code(status));
            seen
        });
        (addr, h)
    }

    // The URL comes from the environment, which the tests share: one test
    // sets it for all three fetch cases.
    #[test]
    fn fetch_sends_the_token_and_reads_the_answer() {
        let t = Duration::from_secs(5);
        let (addr, h) = serve_once(200, &real_answer().to_string());
        std::env::set_var("IMPERATORIUM_ACCOUNT_SPEND_URL", &addr);
        let got = fetch("secret-token", t).ok().expect("200 parses");
        assert_eq!(got["spend"]["percent"], 44);
        let (line, headers) = h.join().unwrap();
        assert_eq!(line, "GET /api/oauth/usage");
        let has = |k: &str, v: &str| headers.iter().any(|(hk, hv)| hk == k && hv == v);
        assert!(has("authorization", "Bearer secret-token"));
        assert!(has("anthropic-beta", BETA));

        let (addr, h) = serve_once(401, "{}");
        std::env::set_var("IMPERATORIUM_ACCOUNT_SPEND_URL", &addr);
        assert!(matches!(fetch("t", t), Err(Failure::Auth)));
        h.join().unwrap();

        let (addr, h) = serve_once(200, "<html>");
        std::env::set_var("IMPERATORIUM_ACCOUNT_SPEND_URL", &addr);
        assert!(matches!(fetch("t", t), Err(Failure::Other(m)) if m.starts_with("not JSON")));
        h.join().unwrap();
        std::env::remove_var("IMPERATORIUM_ACCOUNT_SPEND_URL");
    }
}
