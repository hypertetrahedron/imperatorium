//! The dispatcher's parsed-conversation cache (convcache): polls that arrive
//! together share one parse, and chat.conversation_cache_mb bounds what is
//! kept. Counted through /api/health's `conversation_parses`.
#![cfg(windows)]
mod common;
use common::dispatch::*;
use serde_json::json;
use std::sync::{Arc, Barrier};

const OTHER: &str = "22222222-2222-2222-2222-222222222222";

fn parses(rig: &Rig) -> u64 {
    rig.get("/api/health").1["conversation_parses"].as_u64().expect("conversation_parses")
}

fn conversation(rig: &Rig, sid: &str) {
    let (status, body) = rig.get(&format!("/api/conversation?session={sid}"));
    assert_eq!(status, 200, "{body}");
}

/// A transcript big enough that parsing it takes a while.
fn long_chat(rig: &Rig, sid: &str, turns: usize) {
    let text = "a line of the reply, repeated to give the parse something to do. ".repeat(40);
    let pairs: Vec<(&str, &str)> = (0..turns)
        .flat_map(|_| [("user", "go on"), ("assistant", text.as_str())])
        .collect();
    rig.transcript(sid, &chat_lines(sid, &pairs, 100));
}

#[test]
fn polls_that_arrive_together_share_one_parse() {
    let rig = Rig::start();
    long_chat(&rig, SID, 1500);
    let before = parses(&rig);
    let n = 6;
    let gate = Arc::new(Barrier::new(n));
    let url = rig.url(&format!("/api/conversation?session={SID}"));
    let polls: Vec<_> = (0..n)
        .map(|_| {
            let (gate, url) = (Arc::clone(&gate), url.clone());
            std::thread::spawn(move || {
                gate.wait();
                ureq::get(&url).call().map(|r| r.status()).unwrap_or(0)
            })
        })
        .collect();
    for p in polls {
        assert_eq!(p.join().unwrap(), 200);
    }
    assert_eq!(parses(&rig) - before, 1, "{n} simultaneous polls parsed more than once");
    // And the next poll of the unchanged transcript parses nothing.
    conversation(&rig, SID);
    assert_eq!(parses(&rig) - before, 1);
}

#[test]
fn by_default_two_small_conversations_both_stay() {
    let rig = Rig::start();
    long_chat(&rig, SID, 3);
    long_chat(&rig, OTHER, 3);
    let before = parses(&rig);
    conversation(&rig, SID);
    conversation(&rig, OTHER);
    conversation(&rig, SID);
    assert_eq!(parses(&rig) - before, 2);
}

#[test]
fn a_zero_budget_keeps_only_the_newest() {
    let rig = Rig::with(json!({"chat": {"conversation_cache_mb": 0}}), |_| {});
    long_chat(&rig, SID, 3);
    long_chat(&rig, OTHER, 3);
    let before = parses(&rig);
    conversation(&rig, SID);
    conversation(&rig, SID); // the newest is kept, however big
    assert_eq!(parses(&rig) - before, 1);
    conversation(&rig, OTHER); // pushes SID out
    conversation(&rig, SID);
    assert_eq!(parses(&rig) - before, 3);
}

#[test]
fn the_budget_applies_without_a_restart() {
    let rig = Rig::start();
    long_chat(&rig, SID, 3);
    long_chat(&rig, OTHER, 3);
    let (status, body) = rig.post("/api/settings", &json!({"changes": {"chat.conversation_cache_mb": 0}}));
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true), "{body}");
    assert!(!body["restart"].as_array().is_some_and(|r| !r.is_empty()), "{body}");
    let before = parses(&rig);
    conversation(&rig, SID);
    conversation(&rig, OTHER);
    conversation(&rig, SID);
    assert_eq!(parses(&rig) - before, 3);
}
