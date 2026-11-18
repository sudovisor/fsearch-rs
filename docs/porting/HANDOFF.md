# Handoff

Skill to load next: `rust-anything` (whole-app port). Start with [MAP.md](MAP.md).

## Where the port stands (2026-11-18)
- Every unit of the plan is done; `rust/app` was lifted to a pure Rust app (`rust-anything app lift`).
- `app replay`: 400/400, fallback rate 0 (`app-cases/replay-report.md`); the same cases run in
  `cargo test` (`app/tests/app_cases.rs`).
- Gate: `fsearch-string-utils-rs` passes; `app` and `app/fsearch-core` pass fmt, clippy `-D warnings`
  and tests (ADR-012 explains why the core is proved at app level).
- Extra proof: 17,000 random queries against the C driver with 0 mismatches
  (`harness/fuzz_queries.py`), 178 rows of the original's query tests plus its Turkish/German,
  size and time tables (`app/fsearch-core/tests`), MIME descriptions equal to GIO for 308,745 real
  file names, `fsearch.db` byte-identical to the C writer.
- AppImage: `packaging/build-appimage.sh` (ADR-008, ADR-016).
- Speed: [BENCHMARK.md](BENCHMARK.md).

## Rebuilding the reference
The C original builds with meson in `../build-orig` (needs `meson ninja-build libgtk-3-dev
libglib2.0-dev libicu-dev libpcre2-dev`; `msgfmt` and `itstool` were replaced by no-op shims for
the benchmark build). The driver:

```sh
gcc -O2 -o rust/harness/fsdriver rust/harness/fsdriver.c -Isrc -Ibuild-orig -Ibuild-orig/src \
  build-orig/src/libfsearch_core.a $(pkg-config --cflags --libs gio-unix-2.0 pango libpcre2-8 icu-uc) -lm
```

The fixture tree for the cases is regenerated with `python3 rust/harness/make_fixture.py
$PWD/rust/harness/fixture/tree` (fixed sizes and mtimes; recorded paths are absolute).

## Open
- Known differences: ADR-015. Cutover steps that need the user: `cutover.sh`.
