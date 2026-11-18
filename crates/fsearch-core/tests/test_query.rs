//! Port of the original query test suite (`src/tests/test_query.c`), main and German tables.
//! The Turkish table needs a `tr_TR` process locale and is covered by `turkic_case_mapping` in a
//! separate process (see `tests/turkic.rs`).

use fsearch_core::db::{Db, Entry, Id, NONE};
use fsearch_core::filter::default_filters;
use fsearch_core::query::{MatchData, Query, flags};

fn entry_for(haystack: &str, is_dir: bool, size: i64) -> (Db, Id) {
    let mut db = Db::default();
    let mut parent = NONE;
    let name = if let Some(rest) = haystack.strip_prefix('/') {
        let (off, len) = db.push_name(b"");
        db.folders.push(Entry {
            name_off: off,
            name_len: len,
            parent: NONE,
            ..Default::default()
        });
        parent = 0;
        if rest.is_empty() {
            // g_strsplit("", "/") is empty: the entry is the root folder itself
            return (db, Id::Folder(0));
        }
        let names: Vec<&str> = rest.split('/').collect();
        for n in &names[..names.len() - 1] {
            let (off, len) = db.push_name(n.as_bytes());
            db.folders.push(Entry {
                name_off: off,
                name_len: len,
                parent,
                ..Default::default()
            });
            parent = (db.folders.len() - 1) as u32;
        }
        names[names.len() - 1]
    } else {
        haystack
    };
    let (off, len) = db.push_name(name.as_bytes());
    let e = Entry {
        name_off: off,
        name_len: len,
        parent,
        size,
        ..Default::default()
    };
    if is_dir {
        db.folders.push(e);
        let i = (db.folders.len() - 1) as u32;
        (db, Id::Folder(i))
    } else {
        db.files.push(e);
        (db, Id::File(0))
    }
}

fn check(cases: &[(&str, &str, bool, i64, u32, bool)]) {
    let filters = default_filters();
    let mut failed = Vec::new();
    for &(needle, haystack, is_dir, size, fl, expect) in cases {
        let q = Query::new(needle, None, &filters, fl);
        let (db, id) = entry_for(haystack, is_dir, size);
        let mut md = MatchData::new(&db);
        md.set(id);
        if q.matches(&mut md) != expect {
            failed.push(format!(
                "[{needle}] should{} match [name:{haystack}, size:{size}]",
                if expect { "" } else { " NOT" }
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn main_tests() {
    check(&[
        ("i j l", "I J K", false, 0, 0, false),
        ("i", "j", false, 0, 0, false),
        ("i", "ı", false, 0, 0, false),
        ("abc", "ab_c", false, 0, 0, false),
        ("é", "e", false, 0, 0, false),
        ("ó", "o", false, 0, 0, false),
        ("å", "a", false, 0, 0, false),
        ("ı", "i", false, 0, 0, false),
        ("ı", "I", false, 0, 0, false),
        ("i", "ı", false, 0, 0, false),
        ("i", "İ", false, 0, 0, false),
        ("I", "ı", false, 0, 0, false),
        ("İ", "i", false, 0, 0, false),
        ("?", "aa", false, 0, 0, false),
        ("*.txt", "testtxt", false, 0, 0, false),
        ("^a", "ba", false, 0, flags::REGEX, false),
        ("a", "A", false, 0, flags::MATCH_CASE, false),
        ("A", "a", false, 0, flags::AUTO_MATCH_CASE, false),
        ("é", "É", false, 0, 0, true),
        ("ó", "Ó", false, 0, 0, true),
        ("å", "Å", false, 0, 0, true),
        ("É", "é", false, 0, 0, true),
        ("Ó", "Ó", false, 0, 0, true),
        ("Å", "å", false, 0, 0, true),
        ("ﬀ", "affe", false, 0, 0, true),
        ("i", "I J K", false, 0, 0, true),
        ("j i", "I J K", false, 0, 0, true),
        ("i j", "İIäój", false, 0, 0, true),
        ("abc", "abcdef", false, 0, 0, true),
        ("ab cd", "abcdef", false, 0, 0, true),
        ("?", "ı", false, 0, 0, true),
        ("*c*f", "abcdef", false, 0, 0, true),
        ("ab*ef", "abcdef", false, 0, 0, true),
        ("abc?ef", "abcdef", false, 0, 0, true),
        ("^b", "ba", false, 0, flags::REGEX, true),
        ("^B", "ba", false, 0, flags::REGEX, true),
        ("a", "a", false, 0, flags::MATCH_CASE, true),
        ("A", "A", false, 0, flags::AUTO_MATCH_CASE, true),
        ("a && (b || c)", "ab", false, 0, 0, true),
        ("a && (b || c)", "ac", false, 0, 0, true),
        ("a && (b || c)", "ad", false, 0, 0, false),
        ("a && (b || c)", "bc", false, 0, 0, false),
        ("a && (b || c || d || e)", "ae", false, 0, 0, true),
        ("a && (b || (c && d))", "bc", false, 0, 0, false),
        ("a && (b || (c && d))", "ac", false, 0, 0, false),
        ("a && (b || (c && d))", "bcd", false, 0, 0, false),
        ("a && (b || (c && d))", "acd", false, 0, 0, true),
        ("a && (b || (c && d))", "ab", false, 0, 0, true),
        ("!a", "b", false, 0, 0, true),
        ("!b", "b", false, 0, 0, false),
        ("!!b", "b", false, 0, 0, true),
        ("a && !(b || c)", "abc", false, 0, 0, false),
        ("a && !(b || !c)", "ac", false, 0, 0, true),
        ("a && !(b || !c)", "ac", false, 0, 0, true),
        ("a (b || c)", "ac", false, 0, 0, true),
        ("a (b || c)", "ab", false, 0, 0, true),
        ("a (b || c)", "a", false, 0, 0, false),
        ("a (b || c)", "b", false, 0, 0, false),
        ("a (b || c)", "c", false, 0, 0, false),
        ("a (b || c)", "bc", false, 0, 0, false),
        ("a b (c || d)", "abc", false, 0, 0, true),
        ("a b (c || d)", "abd", false, 0, 0, true),
        ("a b (c || d)", "ab", false, 0, 0, false),
        ("a b (c || d)", "cd", false, 0, 0, false),
        ("a b (c || d)", "acd", false, 0, 0, false),
        ("a b c (d || e)", "abcd", false, 0, 0, true),
        ("a b c (d || e)", "bcde", false, 0, 0, false),
        ("a || b case:(c)", "a", false, 0, 0, true),
        ("a || b case:(c)", "bc", false, 0, 0, true),
        ("a || b case:(c)", "b", false, 0, 0, false),
        ("a || b case:(c)", "c", false, 0, 0, false),
        ("a || b case:(c)", "z", false, 0, 0, false),
        ("a b path:(c || d)", "abc", false, 0, 0, true),
        ("a b path:(c || d)", "ab", false, 0, 0, false),
        ("a b path:(c || d)", "acd", false, 0, 0, false),
        ("a nocase:(b || c) d", "abd", false, 0, 0, true),
        ("a nocase:(b || c) d", "ad", false, 0, 0, false),
        ("a !b", "ac", false, 0, 0, true),
        ("a !b", "ab", false, 0, 0, false),
        ("a !b", "cd", false, 0, 0, false),
        ("a b !c", "abc", false, 0, 0, false),
        ("a b !c", "abd", false, 0, 0, true),
        ("a b c !d", "abcd", false, 0, 0, false),
        ("a b c !d", "abce", false, 0, 0, true),
        ("a !b || c)", "ad", false, 0, 0, false),
        ("a !b || c)", "c", false, 0, 0, false),
        ("a !b || c)", "ac", false, 0, 0, false),
        ("a !b || c)", "ab", false, 0, 0, false),
        ("a !b || c)", "b", false, 0, 0, false),
        ("size:1", "test", false, 1, 0, true),
        ("size:300..", "test", false, 1000, 0, true),
        ("size:300..", "test", false, 200, 0, false),
        ("size:>300", "test", false, 301, 0, true),
        ("size:>300", "test", false, 300, 0, false),
        ("size:>=300", "test", false, 300, 0, true),
        ("size:>300 size:<400", "test", false, 350, 0, true),
        ("size:>300 size:<400", "test", false, 250, 0, false),
        ("size:>300 size:<400", "test", false, 450, 0, false),
        ("size:>1MB", "test", false, 1000001, 0, true),
        ("size:>1MB", "test", false, 1000000, 0, false),
        ("size:abc", "test", false, 1000000, 0, false),
        ("size:abc test", "test", false, 1000000, 0, false),
        ("size:abc abc", "test", false, 1000000, 0, false),
        ("size:1kb..2kb", "test", false, 1000, 0, true),
        ("regex:suffix$", "suffix prefix", false, 0, 0, false),
        ("regex:suffix$", "prefix suffix", false, 0, 0, true),
        ("exact:ABC", "aBc", false, 0, 0, true),
        ("exact:ABC", "aBcd", false, 0, 0, false),
        ("case:exact:ABC", "aBc", false, 0, 0, false),
        ("exact:Ȁ", "Ȁ", false, 0, 0, true),
        ("exact:ȁ", "Ȁ", false, 0, 0, true),
        ("exact:Ȁ", "ȁ", false, 0, 0, true),
        ("case:exact:ȁ", "Ȁ", false, 0, 0, false),
        ("case:exact:Ȁ", "ȁ", false, 0, 0, false),
        ("case:exact:Ȁ", "Ȁ", false, 0, 0, true),
        ("exact:Ȁ", "Ȁb", false, 0, 0, false),
        ("case:(A (b || c)) d", "AbD", false, 0, 0, true),
        ("D case:(A (b || c))", "Acd", false, 0, 0, true),
        ("case:(A (b || c)) d", "ab", false, 0, 0, false),
        ("case:(A (b || c)) d", "AC", false, 0, 0, false),
        ("!case:(A || B) c", "ac", false, 0, 0, true),
        ("!case:(A || B) c", "bc", false, 0, 0, true),
        ("!case:(A || B) c", "abc", false, 0, 0, true),
        ("!case:(A || B) c", "Ac", false, 0, 0, false),
        ("!case:(A || B) c", "Bc", false, 0, 0, false),
        ("!case:(A || B) c", "ABc", false, 0, 0, false),
        ("!case:(A || B) c", "abd", false, 0, 0, false),
        ("ext:pdf;jpg", "test.pdf", false, 0, 0, true),
        ("ext:pdf;jpg", "test.jpg", false, 0, 0, true),
        ("ext:pdf;jpg", "test.c", false, 0, 0, false),
        ("ext:", "test.c", false, 0, 0, false),
        ("ext:", "test", false, 0, 0, true),
        ("case:(TE || AB) cd", "TEcd", false, 0, 0, true),
        ("case:(TE || AB) cd", "ABcd", false, 0, 0, true),
        ("case:(TE || AB) cd", "AB", false, 0, 0, false),
        ("case:(TE || AB) cd", "TE", false, 0, 0, false),
        ("case:(TE || AB) cd", "ABTE", false, 0, 0, false),
        ("case:(TE || AB) cd", "cd", false, 0, 0, false),
        ("nocase:a", "A", false, 0, flags::MATCH_CASE, true),
        ("depth:0", "/", false, 0, 0, true),
        ("depth:2", "/1/2/3", false, 0, 0, false),
        ("depth:3", "/1/2/3", false, 0, 0, true),
        ("path:d", "/a/b/c", false, 0, 0, false),
        ("path:a", "/a/b/c", false, 0, 0, true),
        ("path:b", "/a/b/c", false, 0, 0, true),
        ("path:c", "/a/b/c", false, 0, 0, true),
        ("path:/", "/a/b/c", false, 0, 0, true),
        ("path:/a/b/c", "/a/b/c", false, 0, 0, true),
        ("path:(a && b && c && d)", "/a/b/c", false, 0, 0, false),
        ("path:(a && b && c)", "/a/b/c", false, 0, 0, true),
        ("parent:/b/a", "/a/b/c", false, 0, 0, false),
        ("parent:/a/b", "/a/b/c", false, 0, 0, true),
        ("test || (pic: video:)", "test.jpg", false, 0, 0, true),
        ("test || (pic: video:)", "test.mp4", false, 0, 0, true),
        ("test || (pic: video:)", "test.mp4", false, 0, 0, true),
        ("test || (pic: video:)", "test.doc", false, 0, 0, true),
        ("test || (pic: video:)", "test.doc", false, 0, 0, true),
        ("(", "test", false, 0, flags::REGEX, false),
        ("folder:", "", false, 0, 0, false),
    ]);
}

#[test]
fn german_case_mapping() {
    check(&[
        ("a", "ä", false, 0, 0, false),
        ("A", "ä", false, 0, 0, false),
        ("a", "Ä", false, 0, 0, false),
        ("A", "Ä", false, 0, 0, false),
        ("o", "ö", false, 0, 0, false),
        ("O", "ö", false, 0, 0, false),
        ("o", "Ö", false, 0, 0, false),
        ("O", "Ö", false, 0, 0, false),
        ("u", "ü", false, 0, 0, false),
        ("U", "ü", false, 0, 0, false),
        ("u", "Ü", false, 0, 0, false),
        ("U", "Ü", false, 0, 0, false),
        ("ä", "ä", false, 0, 0, true),
        ("ö", "ö", false, 0, 0, true),
        ("ü", "ü", false, 0, 0, true),
        ("Ä", "ä", false, 0, 0, true),
        ("Ö", "ö", false, 0, 0, true),
        ("Ü", "ü", false, 0, 0, true),
        ("ß", "ẞ", false, 0, 0, true),
    ]);
}
