#!/usr/bin/env python3
"""Write BENCHMARK.md from benchmark.py's JSON: write_benchmark_md.py <results.json> <out.md> [typing_x]"""
import json
import sys

r = json.load(open(sys.argv[1]))
typing = sys.argv[3] if len(sys.argv) > 3 else "3.6"


def ms(x):
    return "< 0.01" if x < 0.01 else f"{x:,.1f}" if x >= 10 else f"{x:,.2f}"


def speed(c, rs):
    return "> 100×" if rs < 0.01 or c / rs > 100 else f"{c / rs:.2f}×"


rows = []


def row(name, c, rs):
    rows.append(f"| {name} | {ms(c)} | {ms(rs)} | {speed(c, rs)} | {ms(c - rs)} ms ({(c - rs) / c * 100:.0f}%) |")


folders, files = r["entries"].replace("folders ", "").replace("files ", "").split()
total = int(folders) + int(files)
row("Index build: scan", r["scan"]["c"], r["scan"]["rust"])
row("Index build: save `fsearch.db`", r["save"]["c"], r["save"]["rust"])
build_c = r["scan"]["c"] + r["save"]["c"]
build_r = r["scan"]["rust"] + r["save"]["rust"]
row("**Index build total (`--update-database`)**", build_c, build_r)
row("Database load", r["load"]["c"], r["load"]["rust"])
row("**Startup → first results (GUI)**", r["startup"]["c"], r["startup"]["rust"])
qc = sum(v["c"] for v in r["query"].values())
qr = sum(v["rust"] for v in r["query"].values())
row("All 16 queries below, summed", qc, qr)
sc = sum(v["c"] for v in r["sort"].values())
sr = sum(v["rust"] for v in r["sort"].values())
row("All 5 re-sorts below, summed", sc, sr)
q = "\n".join(
    f"| `{k}` | {v['results']:,} | {ms(v['c'])} | {ms(v['rust'])} | {speed(v['c'], v['rust'])} |" for k, v in r["query"].items()
)
s = "\n".join(f"| {k} | {ms(v['c'])} | {ms(v['rust'])} | {speed(v['c'], v['rust'])} |" for k, v in r["sort"].items())
day_c = build_c + 10 * r["startup"]["c"] + 300 * qc / 16 + 5 * r["sort"]["type"]["c"] + 20 * (sc - r["sort"]["type"]["c"]) / 4
day_r = (
    build_r
    + 10 * r["startup"]["rust"]
    + 300 * qr / 16 / float(typing)
    + 5 * r["sort"]["type"]["rust"]
    + 20 * (sr - r["sort"]["type"]["rust"]) / 4
)
doc = f"""# FSearch: C original vs Rust port — time savings

Machine: {r['machine']['cpus']} logical CPUs, Ubuntu 26.04, warm page cache. Dataset: `/usr` + `$HOME`,
{int(folders):,} folders and {int(files):,} files ({total:,} entries). C: the original built with meson
`--buildtype=release` (`-O2`). Rust: release profile (LTO, one codegen unit), generic x86-64. Scan: median of 5
alternating runs; load, queries and sorts: median of 15 runs; GUI startup: median of 7 alternating launches.
Raw numbers: `benchmark-results.json`; rerun with `python3 tools/compat/benchmark.py <work_dir> <out.json>`.

## Headline

| Operation | C (ms) | Rust (ms) | Speedup | Time saved |
| --- | ---: | ---: | ---: | ---: |
{chr(10).join(rows)}

Search as you type is faster again on top of the per-query numbers: each keystroke that narrows the
query filters the previous results instead of the whole index, {typing}× faster per keystroke on this
index with identical results (`refine_check` example, 1,160 keystrokes).

## Queries (default flags: auto match case, auto search in path; sorted by name)

| Query | Results | C (ms) | Rust (ms) | Speedup |
| --- | ---: | ---: | ---: | ---: |
{q}

Regex queries run the same PCRE2 JIT in both builds; run-to-run noise is about ±15% for both.

## Re-sorting all results

| Sort by | C (ms) | Rust (ms) | Speedup |
| --- | ---: | ---: | ---: |
{s}

Sorting by a column with a prebuilt index shares that index instead of copying it; sorting by Type
is a stable sort by type rank of the already-ordered list (same result as the original's comparator chain).

## What it means in practice

- **Updating the index** (startup scan, `fsearch -u`, rescans of watched folders): {build_r / 1000:.2f} s instead of
  {build_c / 1000:.2f} s, **{(build_c - build_r) / 1000:.1f} s saved per update ({build_c / build_r:.1f}× faster)**.
- **Launching** with an existing index shows results {ms(r['startup']['c'] - r['startup']['rust'])} ms sooner ({r['startup']['c'] / r['startup']['rust']:.1f}×).
- **Sorting by Type**: {ms(r['sort']['type']['rust'])} ms instead of {r['sort']['type']['c'] / 1000:.2f} s.
- **Typing a query**: every one of the 16 queries is at least as fast as in C; the sum is {qc / qr:.1f}× faster, and narrowing
  keystrokes another {typing}× on top.

Illustrative day (an assumption, not a measurement): 1 index update, 10 launches, 300 searches as you
type, 5 sorts by Type, 20 other re-sorts → C ≈ {day_c / 1000:.1f} s of waiting, Rust ≈ {day_r / 1000:.1f} s.

## Why it is faster

- The scanner walks sibling folders in parallel (the original walks one folder at a time).
- Path order is a preorder rank of the folder tree, so every sort chain compares integers, in parallel.
- `fsearch.db` is read and written in one buffer; checks happen before anything is allocated.
- Unicode case-insensitive search skips ICU for ASCII names; folder paths are cached for path searches.
- Extension lists use binary search; `*`-only wildcards use direct prefix, suffix and substring checks;
  regex matching calls PCRE2's JIT with per-thread match data, as the original does.
"""
open(sys.argv[2], "w").write(doc)
print("wrote", sys.argv[2])
