//! Ports of test_convcache.py through the dispatcher: a transcript is parsed
//! once while its size and mtime stand still, again when they move; a limit
//! applies before turns merge; the least recently used conversations are
//! dropped; one that cannot be read is an error, not an empty chat.
//!
//! The cache cannot be asked how often it parsed. What it answers can: a
//! transcript rewritten with the same size and the same mtime is, to a cache
//! that goes by those two, the transcript it already holds.
#![cfg(windows)]
mod common;
use common::core_support::*;
use common::*;

fn sid_query(session: &str) -> String {
    format!("session={session}")
}

#[test]
fn an_unchanged_transcript_is_not_parsed_again() {
    let d = Dispatcher::start(Options::new());
    let s = sid(1);
    let path = write_transcript(&d, &s, &[("user", "hi"), ("assistant", "yo")]);
    let first = conversation(&d, &sid_query(&s));
    assert_eq!(shown(&first), ["you: hi", "claude: yo"]);
    let sig = first["sig"].as_str().unwrap().to_string();
    // The same size and mtime with other words in it: still the old answer,
    // however often it is asked, and a poll that knows the signature is told
    // nothing changed.
    rewrite_unseen(&path, &transcript_text(&s, &[("user", "ho"), ("assistant", "yo")]));
    for _ in 0..5 {
        let again = conversation(&d, &sid_query(&s));
        assert_eq!(shown(&again), ["you: hi", "claude: yo"]);
        assert_eq!(again["sig"], sig);
        let limited = conversation(&d, &format!("{}&limit=1", sid_query(&s)));
        assert_eq!(shown(&limited), ["claude: yo"]);
    }
    let same = conversation(&d, &format!("{}&known={sig}", sid_query(&s)));
    assert_eq!(same["unchanged"], true);
}

#[test]
fn an_appended_transcript_is_parsed_again() {
    let d = Dispatcher::start(Options::new());
    let s = sid(2);
    let path = write_transcript(&d, &s, &[("user", "hi")]);
    let first = conversation(&d, &sid_query(&s));
    assert_eq!(shown(&first), ["you: hi"]);
    std::fs::write(&path, transcript_text(&s, &[("user", "hi"), ("assistant", "yo")])).unwrap();
    let second = conversation(&d, &sid_query(&s));
    assert_eq!(shown(&second), ["you: hi", "claude: yo"]);
    assert_ne!(first["sig"], second["sig"]);
}

#[test]
fn the_limit_applies_before_turns_merge() {
    // Two claude messages are one turn; a limit of 1 must still be one
    // message, exactly as a read with limit=1 would give.
    let d = Dispatcher::start(Options::new());
    let s = sid(3);
    write_transcript(&d, &s, &[("user", "q"), ("assistant", "one"), ("assistant", "two")]);
    let whole = conversation(&d, &sid_query(&s));
    assert_eq!(shown(&whole), ["you: q", "claude: one | two"]);
    let one = conversation(&d, &format!("{}&limit=1", sid_query(&s)));
    assert_eq!(shown(&one), ["claude: two"]);
    // and the whole conversation is unchanged by having been asked for a piece
    assert_eq!(shown(&conversation(&d, &sid_query(&s))), shown(&whole));
}

#[test]
fn merging_does_not_change_the_cached_messages() {
    let d = Dispatcher::start(Options::new());
    let s = sid(4);
    write_transcript(&d, &s, &[("assistant", "one"), ("assistant", "two")]);
    let two = conversation(&d, &format!("{}&limit=2", sid_query(&s)));
    assert_eq!(two["items"][0]["blocks"].as_array().unwrap().len(), 2);
    let one = conversation(&d, &format!("{}&limit=1", sid_query(&s)));
    assert_eq!(one["items"][0]["blocks"].as_array().unwrap().len(), 1);
    let again = conversation(&d, &format!("{}&limit=2", sid_query(&s)));
    assert_eq!(again["items"][0]["blocks"].as_array().unwrap().len(), 2);
}

#[test]
fn without_a_transcript_file_nothing_is_cached() {
    // No file to watch: each ask reads through, so a transcript that turns
    // up (or changes) is shown at once.
    let d = Dispatcher::start(Options::new());
    let s = sid(5);
    assert!(shown(&conversation(&d, &sid_query(&s))).is_empty());
    let path = write_transcript(&d, &s, &[("user", "hi")]);
    assert_eq!(shown(&conversation(&d, &sid_query(&s))), ["you: hi"]);
    // (read through, so a rewrite that keeps size and mtime is seen too if
    // the cache never took it in; if it did, this is the cache's own test)
    std::fs::write(&path, transcript_text(&s, &[("user", "hi"), ("assistant", "yo")])).unwrap();
    assert_eq!(shown(&conversation(&d, &sid_query(&s))), ["you: hi", "claude: yo"]);
}

#[test]
fn least_recently_used_sessions_are_dropped() {
    // The binary keeps 32 conversations (CONVS_KEPT in server/conv.rs).
    let d = Dispatcher::start(Options::new());
    let sids: Vec<String> = (10..43).map(sid).collect(); // 33 of them
    let paths: Vec<_> = sids
        .iter()
        .map(|s| write_transcript(&d, s, &[("user", "aa")]))
        .collect();
    let read = |i: usize| shown(&conversation(&d, &sid_query(&sids[i])));
    for i in 0..32 {
        assert_eq!(read(i), ["you: aa"]);
    }
    read(0); // the oldest, used again: now the newest
    assert_eq!(read(32), ["you: aa"]); // the 33rd: pushes out the least recently used, #1
    for (s, p) in sids.iter().zip(&paths) {
        rewrite_unseen(p, &transcript_text(s, &[("user", "bb")]));
    }
    // Still held: shown as they were. Dropped: read again, so as they are.
    for i in [0usize, 2, 17, 31, 32] {
        assert_eq!(read(i), ["you: aa"], "session {i} should still be cached");
    }
    assert_eq!(read(1), ["you: bb"], "session 1 should have been dropped");
}

#[test]
fn an_unreadable_transcript_is_an_error_not_an_empty_chat() {
    let d = Dispatcher::start(Options::new());
    let s = sid(6);
    let path = write_transcript(&d, &s, &[("user", "hi")]);
    std::fs::write(&path, [b'{', 0xff, 0xfe, b'\n']).unwrap(); // not UTF-8
    let body = conversation(&d, &sid_query(&s));
    assert!(shown(&body).is_empty(), "{body}");
    let error = body["error"].as_str().unwrap_or("");
    assert!(error.starts_with("RuntimeError") && error.contains("not UTF-8"), "{body}");
}
