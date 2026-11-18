# FSearch: C original vs Rust port — time savings

Machine: 16 logical CPUs, Ubuntu 26.04, warm page cache. Dataset: `/usr` + `$HOME`,
108,179 folders and 768,006 files (876,185 entries). C: the original built with meson
`--buildtype=release` (`-O2`). Rust: release profile (LTO, one codegen unit), generic x86-64. Scan: median of 5
alternating runs; load, queries and sorts: median of 15 runs; GUI startup: median of 7 alternating launches.
Raw numbers: `benchmark-results.json`; rerun with `python3 tools/compat/benchmark.py <work_dir> <out.json>`.

## Headline

| Operation | C (ms) | Rust (ms) | Speedup | Time saved |
| --- | ---: | ---: | ---: | ---: |
| Index build: scan | 3,882.3 | 486.3 | 7.98× | 3,396.0 ms (87%) |
| Index build: save `fsearch.db` | 271.0 | 125.3 | 2.16× | 145.7 ms (54%) |
| **Index build total (`--update-database`)** | 4,153.3 | 611.6 | 6.79× | 3,541.7 ms (85%) |
| Database load | 156.3 | 71.2 | 2.20× | 85.1 ms (54%) |
| **Startup → first results (GUI)** | 346.8 | 192.9 | 1.80× | 153.9 ms (44%) |
| All 16 queries below, summed | 94.3 | 38.9 | 2.42× | 55.4 ms (59%) |
| All 5 re-sorts below, summed | 1,405.3 | 3.02 | > 100× | 1,402.3 ms (100%) |

Search as you type is faster again on top of the per-query numbers: each keystroke that narrows the
query filters the previous results instead of the whole index, 3.56× faster per keystroke on this
index with identical results (`refine_check` example, 1,160 keystrokes).

## Queries (default flags: auto match case, auto search in path; sorted by name)

| Query | Results | C (ms) | Rust (ms) | Speedup |
| --- | ---: | ---: | ---: | ---: |
| `a` | 476,596 | 5.01 | 2.22 | 2.26× |
| `lib` | 63,187 | 3.84 | 2.41 | 1.59× |
| `lib python3` | 41 | 4.62 | 2.81 | 1.64× |
| `*.so` | 10,494 | 8.72 | 3.19 | 2.74× |
| `regex:^lib.*\.so$` | 4,329 | 4.81 | 4.12 | 1.17× |
| `ext:png;jpg;svg` | 50,569 | 2.71 | 1.29 | 2.10× |
| `size:>1mb` | 19,680 | 2.21 | 0.79 | 2.79× |
| `dm:2024` | 13,165 | 2.35 | 0.74 | 3.17× |
| `ärger` | 20 | 13.6 | 1.29 | 10.60× |
| `Report` | 90 | 2.32 | 1.61 | 1.44× |
| `usr/share/doc` | 18,848 | 17.4 | 4.94 | 3.53× |
| `folder:` | 108,179 | 1.95 | 0.44 | 4.40× |
| `NOT a` | 399,589 | 4.96 | 2.09 | 2.38× |
| `pic:` | 209,346 | 8.57 | 3.29 | 2.61× |
| `"test"` | 20,435 | 5.92 | 2.54 | 2.33× |
| `*a*b*` | 101,026 | 5.31 | 5.15 | 1.03× |

Regex queries run the same PCRE2 JIT in both builds; run-to-run noise is about ±15% for both.

## Re-sorting all results

| Sort by | C (ms) | Rust (ms) | Speedup |
| --- | ---: | ---: | ---: |
| path | 2.44 | < 0.01 | > 100× |
| size | 2.38 | < 0.01 | > 100× |
| mtime | 2.38 | < 0.01 | > 100× |
| ext | 2.29 | < 0.01 | > 100× |
| type | 1,395.9 | 3.02 | > 100× |

Sorting by a column with a prebuilt index shares that index instead of copying it; sorting by Type
is a stable sort by type rank of the already-ordered list (same result as the original's comparator chain).

## What it means in practice

- **Updating the index** (startup scan, `fsearch -u`, rescans of watched folders): 0.61 s instead of
  4.15 s, **3.5 s saved per update (6.8× faster)**.
- **Launching** with an existing index shows results 153.9 ms sooner (1.8×).
- **Sorting by Type**: 3.02 ms instead of 1.40 s.
- **Typing a query**: every one of the 16 queries is at least as fast as in C; the sum is 2.4× faster, and narrowing
  keystrokes another 3.56× on top.

Illustrative day (an assumption, not a measurement): 1 index update, 10 launches, 300 searches as you
type, 5 sorts by Type, 20 other re-sorts → C ≈ 16.4 s of waiting, Rust ≈ 2.8 s.

## Why it is faster

- The scanner walks sibling folders in parallel (the original walks one folder at a time).
- Path order is a preorder rank of the folder tree, so every sort chain compares integers, in parallel.
- `fsearch.db` is read and written in one buffer; checks happen before anything is allocated.
- Unicode case-insensitive search skips ICU for ASCII names; folder paths are cached for path searches.
- Extension lists use binary search; `*`-only wildcards use direct prefix, suffix and substring checks;
  regex matching calls PCRE2's JIT with per-thread match data, as the original does.
