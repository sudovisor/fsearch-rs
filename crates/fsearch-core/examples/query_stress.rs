//! Builds and runs pathological queries in a worker thread with a GUI-like stack (8 MiB) and
//! reports time per query. A stack overflow aborts the process (that is the bug to find).
use fsearch_core::db::{Db, Entry, Id, NONE};
use fsearch_core::filter::default_filters;
use fsearch_core::query::{MatchData, Query};
use std::time::Instant;

fn main() {
    let filters = default_filters();
    let cases: Vec<(&str, String)> = vec![
        ("200k words (implicit AND)", "a ".repeat(200_000)),
        ("100k OR", "a OR ".repeat(100_000)),
        ("200k NOT", "NOT ".repeat(200_000) + "a"),
        ("200k open brackets", "(".repeat(200_000)),
        (
            "100k nested brackets",
            "(".repeat(100_000) + "a" + &")".repeat(100_000),
        ),
        ("30k nested case:(", "case:(".repeat(30_000) + "a"),
        ("20k nested macros", "pic:".repeat(20_000)),
        ("1 MB word", "x".repeat(1_000_000)),
        ("1 MB quoted", format!("\"{}", "y ".repeat(500_000))),
        ("50k size: ranges", "size:1..2 ".repeat(50_000)),
        ("50k dm:", "dm:2022 ".repeat(50_000)),
    ];
    let mut db = Db::default();
    let (o, l) = db.push_name(b"abc");
    db.files.push(Entry {
        name_off: o,
        name_len: l,
        parent: NONE,
        ..Default::default()
    });
    for (name, q) in cases {
        let t = Instant::now();
        let q2 = q.clone();
        let filters2 = filters.clone();
        let db2 = db.clone();
        let r = std::thread::Builder::new()
            .stack_size(8 << 20)
            .spawn(move || {
                let query = Query::new(&q2, None, &filters2, 18);
                let mut md = MatchData::new(&db2);
                md.set(Id::File(0));
                let m = query.matches(&mut md);
                drop(query);
                m
            })
            .expect("thread")
            .join();
        println!(
            "{name:28} {:>8.1} ms  {}",
            t.elapsed().as_secs_f64() * 1e3,
            if r.is_ok() { "ok" } else { "PANIC" }
        );
    }
}
