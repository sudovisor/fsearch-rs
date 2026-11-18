//! Turkish table of `src/tests/test_query.c` (`test_turkic_case_mapping`).
//!
//! In the C test these rows have five fields, so `true`/`false` lands in `flags` (1 = MATCH_CASE)
//! and `result` is zero-initialized: every row expects no match. The case folding mode is chosen
//! from the locale once per process, so the check runs in a child process with `LANG=tr_TR.UTF-8`.

use fsearch_core::db::{Db, Entry, Id, NONE};
use fsearch_core::filter::default_filters;
use fsearch_core::query::{MatchData, Query};

const TR_CASES: &[(&str, &str, u32)] = &[
    ("i", "ı", 0),
    ("i", "I", 0),
    ("ı", "i", 0),
    ("ı", "İ", 0),
    ("İ", "ı", 0),
    ("İ", "I", 0),
    ("I", "i", 0),
    ("I", "İ", 0),
    ("ı", "I", 1),
    ("i", "İ", 1),
];

fn matches(needle: &str, haystack: &str, flags: u32) -> bool {
    let mut db = Db::default();
    let (off, len) = db.push_name(haystack.as_bytes());
    db.files.push(Entry {
        name_off: off,
        name_len: len,
        parent: NONE,
        ..Default::default()
    });
    let q = Query::new(needle, None, &default_filters(), flags);
    let mut md = MatchData::new(&db);
    md.set(Id::File(0));
    q.matches(&mut md)
}

#[test]
#[ignore = "run through `turkic_case_mapping` with a Turkish locale"]
fn turkic_inner() {
    for &(n, h, f) in TR_CASES {
        assert!(
            !matches(n, h, f),
            "[{n}] should NOT match [{h}] (flags {f})"
        );
        assert!(
            !matches(h, n, f),
            "[{h}] should NOT match [{n}] (flags {f})"
        );
    }
}

#[test]
fn turkic_case_mapping() {
    let exe = std::env::current_exe().expect("test binary path");
    let out = std::process::Command::new(exe)
        .args(["--ignored", "--exact", "turkic_inner"])
        .env("LANG", "tr_TR.UTF-8")
        .env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .output()
        .expect("run turkic_inner");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}
