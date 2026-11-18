//! Mutation fuzzer for `file::load`: corrupts bytes of a real database (keeping the header
//! checksum valid half the time), then loads, searches, sorts and saves each result. Any panic,
//! hang or abort is a bug.
//!
//!   fuzz_db <fsearch.db> <iterations>
use fsearch_core::db::Prop;
use fsearch_core::{Query, SortDir, file};
use md5::{Digest, Md5};
use std::panic;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let orig = std::fs::read(&a[1]).expect("db");
    let iters: usize = a[2].parse().unwrap_or(1000);
    let tmp = std::env::temp_dir().join(format!("fuzz-{}.db", std::process::id()));
    let mut seed: u64 = 0x2545F4914F6CDD1D;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    // header end = offset of the checksum: find it by re-hashing prefixes (cheap: one pass)
    let mut ck_off = 0;
    for end in 32..orig.len().min(1 << 20) {
        if end + 16 <= orig.len() && Md5::digest(&orig[..end]).as_slice() == &orig[end..end + 16] {
            ck_off = end;
            break;
        }
    }
    let (mut ok, mut rejected) = (0, 0);
    for i in 0..iters {
        let mut d = orig.clone();
        let n = 1 + (rnd() % 8) as usize;
        for _ in 0..n {
            let pos = (rnd() as usize) % d.len();
            match rnd() % 4 {
                0 => d[pos] = rnd() as u8,
                1 => d[pos] ^= 1 << (rnd() % 8),
                2 => {
                    let v = [0u8, 0xff, 0x7f, 0x80][(rnd() % 4) as usize];
                    for k in 0..4.min(d.len() - pos) {
                        d[pos + k] = v;
                    }
                }
                _ => {
                    let cut = pos.max(1);
                    d.truncate(cut);
                }
            }
        }
        if ck_off > 0 && d.len() >= ck_off + 16 && rnd() % 2 == 0 {
            let sum = Md5::digest(&d[..ck_off]);
            d[ck_off..ck_off + 16].copy_from_slice(&sum);
        }
        std::fs::write(&tmp, &d).expect("write");
        let res = panic::catch_unwind(|| {
            let Ok(store) = file::load(&tmp, None) else {
                return false;
            };
            let q = Query::new("a", None, &[], 0);
            for s in [Prop::Name, Prop::Path, Prop::Size, Prop::FileType] {
                let mut v = store.search(&q, s, SortDir::Descending, None);
                store.sort_view(&mut v, Prop::FileType, SortDir::Ascending, None);
                for k in 0..v.len().min(50) {
                    if let Some(id) = v.get(k) {
                        let _ = store.db.full_path(id);
                    }
                }
            }
            let qp = Query::new(
                "usr/",
                None,
                &[],
                fsearch_core::query::flags::SEARCH_IN_PATH,
            );
            let _ = store.search(&qp, Prop::Name, SortDir::Ascending, None);
            let out = std::env::temp_dir().join(format!("fuzz-out-{}.db", std::process::id()));
            let _ = file::save(&store, &out);
            true
        });
        match res {
            Ok(true) => ok += 1,
            Ok(false) => rejected += 1,
            Err(_) => {
                let keep = format!("fuzz-crash-{i}.db");
                std::fs::write(&keep, &d).ok();
                eprintln!("PANIC at iteration {i}, input saved to {keep}");
                std::process::exit(1);
            }
        }
    }
    println!("{iters} mutated databases: {ok} loaded and exercised, {rejected} rejected, 0 panics");
}
