# Compatibility tools

These compare the Rust port with the C original. They need a build of the original
([cboxdoerfer/fsearch](https://github.com/cboxdoerfer/fsearch)) next to this repository.

| File | Purpose |
| --- | --- |
| `fsdriver.c` | Headless driver for the original's core library: `scan`, `query`, `dump`, `bench` |
| `make_fixture.py` | Builds the fixture tree with fixed sizes and times that the recorded cases use |
| `make_cases.py` | Writes the case inputs that were recorded from the original into `tests/app-cases` |
| `fuzz_queries.py` | Random queries against both programs on one database; prints any difference |
| `benchmark.py` | Times index build, load, queries, sorts and GUI startup for both programs |
| `write_benchmark_md.py` | Turns the benchmark JSON into `docs/BENCHMARK.md` |

## Build the original and the driver

```bash
git clone https://github.com/cboxdoerfer/fsearch ../fsearch-c && cd ../fsearch-c
meson setup build-orig --buildtype=release && ninja -C build-orig
gcc -O2 -o fsdriver ../fsearch-rs/tools/compat/fsdriver.c -Isrc -Ibuild-orig -Ibuild-orig/src \
  build-orig/src/libfsearch_core.a $(pkg-config --cflags --libs gio-unix-2.0 pango libpcre2-8 icu-uc) -lm
```

The Rust binary answers the same driver commands (`fsearch query <db> …`, `fsearch dump <db>`), so both can
be run on the same database.

## Fuzz

```bash
python3 tools/compat/fuzz_queries.py ../fsearch-c/fsdriver target/release/fsearch <fsearch.db> 50
```

## Benchmark

```bash
python3 tools/compat/benchmark.py /tmp/fsbench bench.json
python3 tools/compat/write_benchmark_md.py bench.json docs/BENCHMARK.md
```

## Recorded cases

`tests/app-cases` holds the original's output for 400 inputs (queries in every sort order and flag
combination, filters, scans with exclude rules, damaged databases). The fixture location is written as
`@HARNESS@`; `tests/app_cases.rs` generates the tree and replays the cases against the built binary.
