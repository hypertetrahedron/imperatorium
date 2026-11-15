//! Every real-time wait the dispatcher keeps, in one place, with one test
//! hook: `IMPERATORIUM_TEST_TIME_SCALE`.
//!
//! Tests must not wait out production intervals (a 15 s queue sweep, a 60 s
//! idle timer, a 3 minute update-check delay, ...). Setting the variable to a
//! positive number multiplies every wait listed in `Times` by it -
//! `IMPERATORIUM_TEST_TIME_SCALE=0.05` makes the sweep fire every 0.75 s and
//! the idle check 3 s after a Stop. The variable is read once, at first use;
//! unset, empty, zero, negative or not a number all mean 1.0, and at 1.0
//! every value is exactly the production one (`defaults_are_unchanged` pins
//! them).
//!
//! What it scales: the queue sweep, IDLE_AFTER after a Stop, the notifier's
//! per-channel `delay`, a parked permission/question's hold (the waits in
//! policy.json arrive as the hook's `wait`; the clamp to 600 s is applied
//! first, then the scale), the window streams' intervals, the update
//! checker's first-check delay and its daily repeat, the PR tracker's polls
//! and refresh, and the remote fleet's poll interval, backoffs and channel
//! retry.
//!
//! What it does not: timeouts that bound real work (an SSH call, an HTTP
//! request, the voice worker's reply timeout), the voice reaper and
//! `idle_unload` (a setting), and anything a caller passes in explicitly.

use std::sync::OnceLock;

/// The variable's name, for the docs and the tests that set it.
pub const ENV: &str = "IMPERATORIUM_TEST_TIME_SCALE";

/// The production waits, in seconds, times a scale.
#[derive(Debug, Clone, PartialEq)]
pub struct Times {
    /// server::serve: how often the queue is swept for sessions that went idle.
    pub queue_sweep: f64,
    /// dispatcher.IDLE_AFTER: how long after Stop the idle check looks.
    pub idle_after: f64,
    /// The stream looks at the session list this often; also the age a
    /// shared snapshot may have.
    pub stream_every: f64,
    /// ... never more often than this, ...
    pub stream_min_gap: f64,
    /// ... and sends a keepalive after this much quiet.
    pub stream_keepalive: f64,
    /// The matrix stream's look.
    pub matrix_every: f64,
    /// update::Checker: the first check after start, then every ...
    pub update_first_check: f64,
    pub update_every: f64,
    /// prs::Tracker: a pass every ...; an entry is fresh for ...
    pub pr_pass: f64,
    pub pr_refresh: f64,
    /// remote: the poll interval, the first backoff of a failing host
    /// poll, the first backoff of a failing relay, the longest backoff, and
    /// how long a failed channel is left alone.
    pub remote_poll_every: f64,
    pub remote_backoff_start: f64,
    pub remote_relay_backoff_start: f64,
    pub remote_backoff_max: f64,
    pub remote_channel_retry: f64,
}

impl Times {
    pub fn at(scale: f64) -> Times {
        let s = |v: f64| v * scale;
        Times {
            queue_sweep: s(15.0),
            idle_after: s(60.0),
            stream_every: s(5.0),
            stream_min_gap: s(2.0),
            stream_keepalive: s(15.0),
            matrix_every: s(1.5),
            update_first_check: s(180.0),
            update_every: s(24.0 * 3600.0),
            pr_pass: s(5.0),
            pr_refresh: s(120.0),
            remote_poll_every: s(20.0),
            remote_backoff_start: s(60.0),
            remote_relay_backoff_start: s(5.0),
            remote_backoff_max: s(15.0 * 60.0),
            remote_channel_retry: s(30.0),
        }
    }
}

/// The scale a value of the variable means.
pub fn parse_scale(v: Option<&str>) -> f64 {
    v.and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|f| f.is_finite() && *f > 0.0)
        .unwrap_or(1.0)
}

/// The scale in force: read once.
pub fn scale() -> f64 {
    static S: OnceLock<f64> = OnceLock::new();
    *S.get_or_init(|| parse_scale(std::env::var(ENV).ok().as_deref()))
}

/// The waits in force: read once.
pub fn t() -> &'static Times {
    static T: OnceLock<Times> = OnceLock::new();
    T.get_or_init(|| Times::at(scale()))
}

/// A wait the caller already has in seconds (a notify delay, a park hold),
/// scaled.
pub fn secs(base: f64) -> f64 {
    base * scale()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_unchanged() {
        let d = Times::at(1.0);
        assert_eq!(
            d,
            Times {
                queue_sweep: 15.0,
                idle_after: 60.0,
                stream_every: 5.0,
                stream_min_gap: 2.0,
                stream_keepalive: 15.0,
                matrix_every: 1.5,
                update_first_check: 180.0,
                update_every: 86_400.0,
                pr_pass: 5.0,
                pr_refresh: 120.0,
                remote_poll_every: 20.0,
                remote_backoff_start: 60.0,
                remote_relay_backoff_start: 5.0,
                remote_backoff_max: 900.0,
                remote_channel_retry: 30.0,
            }
        );
        assert_eq!(secs(7.5) / scale(), 7.5);
    }

    #[test]
    fn scale_parsing() {
        assert_eq!(parse_scale(None), 1.0);
        for bad in ["", " ", "0", "-1", "abc", "nan", "inf"] {
            assert_eq!(parse_scale(Some(bad)), 1.0, "{bad:?}");
        }
        assert_eq!(parse_scale(Some("0.05")), 0.05);
        assert_eq!(parse_scale(Some(" 2 ")), 2.0);
        assert_eq!(Times::at(0.1).queue_sweep, 1.5);
    }
}
