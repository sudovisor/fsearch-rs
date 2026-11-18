//! Checks `Store::search_refine` against `Store::search` on typing sequences, and times typing.
//!
//!   refine_check <fsearch.db> <sequences>
use fsearch_core::db::Prop;
use fsearch_core::filter::default_filters;
use fsearch_core::query::flags;
use fsearch_core::{Query, SortDir, file};
use std::time::Instant;

const WORDS: &[&str] = &[
    "python3",
    "libreoffice",
    "share/doc",
    "Report",
    "ärger",
    "x86_64",
    "README",
    "lib python",
    "lib py ext",
    "test case",
    "usr/lib/gcc",
    "Makefile",
    "café",
    "ß",
    "config.h",
    "a b c",
];

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let store = file::load(std::path::Path::new(&a[1]), None).expect("load");
    let n: usize = a[2].parse().unwrap_or(200);
    let filters = default_filters();
    let fl = flags::AUTO_MATCH_CASE | flags::AUTO_SEARCH_IN_PATH;
    let sorts = [
        Prop::Name,
        Prop::Path,
        Prop::Size,
        Prop::Mtime,
        Prop::Extension,
        Prop::FileType,
    ];
    let (mut t_full, mut t_ref, mut checked, mut refined_ok) = (0f64, 0f64, 0usize, 0usize);
    let mut seed: u64 = 0x9e3779b97f4a7c15;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for i in 0..n {
        let word = WORDS[i % WORDS.len()];
        let sort = sorts[(rnd() % 6) as usize];
        let dir = if rnd() % 2 == 0 {
            SortDir::Ascending
        } else {
            SortDir::Descending
        };
        let filter = if rnd() % 4 == 0 {
            filters.get((rnd() % filters.len() as u64) as usize)
        } else {
            None
        };
        let mut prev: Option<(Query, fsearch_core::View)> = None;
        let chars: Vec<char> = word.chars().collect();
        for k in 1..=chars.len() {
            let text: String = chars[..k].iter().collect();
            let q = Query::new(&text, filter, &filters, fl);
            let t0 = Instant::now();
            let full = store.search(&q, sort, dir, None);
            t_full += t0.elapsed().as_secs_f64();
            let t1 = Instant::now();
            let refined = match &prev {
                Some((pq, pv)) => store.search_refine(pq, pv, &q, sort, dir, None),
                None => store.search(&q, sort, dir, None),
            };
            t_ref += t1.elapsed().as_secs_f64();
            checked += 1;
            if *full.folders != *refined.folders
                || *full.files != *refined.files
                || full.chain != refined.chain
            {
                panic!("MISMATCH for {text:?} sort {sort:?}");
            }
            refined_ok += 1;
            prev = Some((q, refined));
        }
    }
    println!(
        "{checked} keystrokes checked, {refined_ok} identical; full search {:.1} ms total, with refinement {:.1} ms total ({:.2}x)",
        t_full * 1e3,
        t_ref * 1e3,
        t_full / t_ref
    );
}
