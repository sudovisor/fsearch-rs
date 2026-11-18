# Contributing

Bug reports and pull requests are welcome.

## Before opening a pull request

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --release
```

`cargo test` runs the unit tests, the query tables taken from the original's test suite, headless UI tests
of the window (egui_kittest) and the recorded app cases (needs `python3` to generate their fixture tree).

## Behavior matches the original

Search results, sort orders, `fsearch.db` and `fsearch.conf` are meant to stay identical to the C version of
FSearch. A change to any of them needs a reason in the pull request. `tools/compat` has the tools to check
it against the original: a headless driver for the C code, the case recorder, a differential query fuzzer and
the benchmark. Design decisions taken during the port are in `docs/porting/DECISIONS.md`.

## Code

- No `unwrap` on data from disk or the user; a damaged file or odd input is an error, not a crash.
- `unsafe` only where needed, with a `// SAFETY:` comment.
- New looks go in `src/gui/theme.rs` (a `Look` variant and its `Palette`).
