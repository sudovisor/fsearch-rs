//! Benchmark harness mirroring `rust/harness/fsdriver.c` (`scan` with FSDRIVER_TIME, and `bench`).
//!
//!   bench scan  <db_out> <root>...                    -> "scan_ms X save_ms Y" on stderr
//!   bench bench <db> <runs> <sort> <query>...         -> load / query / sort medians (ms)
use fsearch_core::db::{FLAG_DEFAULT, Prop};
use fsearch_core::filter::default_filters;
use fsearch_core::query::flags;
use fsearch_core::rules::{ExcludeManager, Include, add_include};
use fsearch_core::{Query, SortDir, Store, file};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Instant;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn sort_of(s: &str) -> Prop {
    match s {
        "path" => Prop::Path,
        "size" => Prop::Size,
        "mtime" => Prop::Mtime,
        "ext" => Prop::Extension,
        "type" => Prop::FileType,
        _ => Prop::Name,
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a[0].as_str() {
        "scan" => {
            let mut inc = Vec::new();
            for r in &a[2..] {
                add_include(&mut inc, Include::new(r, true, false, false, false, 0));
            }
            let t0 = Instant::now();
            let store = Store::scan(
                inc,
                ExcludeManager::default(),
                FLAG_DEFAULT,
                &AtomicBool::new(false),
                &AtomicU64::new(0),
            )
            .expect("scan");
            let t1 = Instant::now();
            file::save(&store, std::path::Path::new(&a[1])).expect("save");
            let t2 = Instant::now();
            println!(
                "folders {} files {}",
                store.num_folders(),
                store.num_files()
            );
            eprintln!(
                "scan_ms {:.3} save_ms {:.3}",
                (t1 - t0).as_secs_f64() * 1e3,
                (t2 - t1).as_secs_f64() * 1e3
            );
        }
        "bench" => {
            let db = std::path::Path::new(&a[1]);
            let runs: usize = a[2].parse().expect("runs");
            let sort = sort_of(&a[3]);
            let filters = default_filters();
            let mut t = Vec::new();
            for _ in 0..runs {
                let t0 = Instant::now();
                let s = file::load(db, None).expect("load");
                t.push(t0.elapsed().as_secs_f64() * 1e3);
                drop(s);
            }
            println!("load {:.3}", median(t));
            let store = file::load(db, None).expect("load");
            for q in &a[4..] {
                let mut t = Vec::new();
                let mut n = 0;
                for _ in 0..runs {
                    let query = Query::new(
                        q,
                        None,
                        &filters,
                        flags::AUTO_MATCH_CASE | flags::AUTO_SEARCH_IN_PATH,
                    );
                    let t0 = Instant::now();
                    let v = store.search(&query, sort, SortDir::Ascending, None);
                    t.push(t0.elapsed().as_secs_f64() * 1e3);
                    n = v.len();
                }
                println!("query {:.3} {} {}", median(t), n, q);
            }
            for s in ["path", "size", "mtime", "ext", "type"] {
                let mut t = Vec::new();
                for _ in 0..runs {
                    let query = Query::new("", None, &filters, 0);
                    let mut v = store.search(&query, Prop::Name, SortDir::Ascending, None);
                    let t0 = Instant::now();
                    store.sort_view(&mut v, sort_of(s), SortDir::Ascending, None);
                    t.push(t0.elapsed().as_secs_f64() * 1e3);
                }
                println!("sort {:.3} {}", median(t), s);
            }
        }
        _ => eprintln!("usage: bench scan|bench ..."),
    }
}
