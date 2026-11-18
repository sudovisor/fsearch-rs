#!/usr/bin/env python3
"""Time savings of the Rust port against the C original (ADR-007).

Usage: benchmark.py <work_dir> <out_json> [roots...]   (default roots: /usr and $HOME)

Measures, warm page cache, C and Rust runs alternated, medians:
  1. index build: scan + save of the roots          (C: fsdriver scan, Rust: bench scan)
  2. database load, 16 queries, 5 re-sorts           (both on the same fsearch.db written by C)
  3. GUI startup: launch -> first results for `-s "lib python3"` (C GTK app vs Rust egui app)
"""
import datetime
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import time

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
C_DRIVER = os.path.join(REPO, "rust/harness/fsdriver")
R_BENCH = os.path.join(REPO, "rust/app/target/release/examples/bench")
C_GUI = os.path.join(REPO, "build-orig/src/fsearch")
R_GUI = os.path.join(REPO, "rust/app/target/release/fsearch")

QUERIES = ["a", "lib", "lib python3", "*.so", "regex:^lib.*\\.so$", "ext:png;jpg;svg", "size:>1mb",
           "dm:2024", "ärger", "Report", "usr/share/doc", "folder:", "NOT a", "pic:", "\"test\"", "*a*b*"]
SCAN_RUNS = 5
RUNS = 15
GUI_RUNS = 7


def run(cmd, env=None):
    p = subprocess.run(cmd, capture_output=True, text=True, env=env)
    if p.returncode != 0:
        raise SystemExit(f"{cmd[0]} failed: {p.stderr[-2000:]}")
    return p


def scan_once(kind, db, roots):
    if kind == "c":
        p = run([C_DRIVER, "scan", db] + roots, env=dict(os.environ, FSDRIVER_TIME="1"))
    else:
        p = run([R_BENCH, "scan", db] + roots)
    m = re.search(r"scan_ms ([\d.]+) save_ms ([\d.]+)", p.stderr)
    counts = p.stdout.strip()
    return float(m.group(1)), float(m.group(2)), counts


def parse_bench(out):
    res = {"query": {}, "sort": {}}
    for line in out.splitlines():
        parts = line.split(" ", 3)
        if parts[0] == "load":
            res["load"] = float(parts[1])
        elif parts[0] == "query":
            res["query"][parts[3]] = {"ms": float(parts[1]), "results": int(parts[2])}
        elif parts[0] == "sort":
            res["sort"][parts[2]] = float(parts[1])
    return res


def gui_once(kind, sandbox, db_src):
    data = os.path.join(sandbox, "data", "fsearch")
    os.makedirs(data, exist_ok=True)
    shutil.copy(db_src, os.path.join(data, "fsearch.db"))
    env = dict(os.environ, XDG_CONFIG_HOME=os.path.join(sandbox, "config"), XDG_DATA_HOME=os.path.join(sandbox, "data"),
               XDG_STATE_HOME=os.path.join(sandbox, "state"))
    if kind == "c":
        env["G_MESSAGES_DEBUG"] = "fsearch-database-index-store"
        t0 = time.time()
        p = subprocess.Popen([C_GUI, "-s", "lib python3"], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True)
        stamp = None
        for line in p.stdout:
            if 'search "lib python3"' in line:
                m = re.search(r"(\d\d):(\d\d):(\d\d)\.(\d\d\d):", line)
                h, mi, s, ms = map(int, m.groups())
                day = datetime.datetime.fromtimestamp(t0)
                stamp = day.replace(hour=h, minute=mi, second=s, microsecond=ms * 1000).timestamp()
                break
        p.kill()
        p.wait()
        return (stamp - t0) * 1000.0
    env["FSEARCH_STARTUP_TIMING"] = "1"
    t0 = time.time()
    p = subprocess.run([R_GUI, "-s", "lib python3"], env=env, capture_output=True, text=True, timeout=120)
    m = re.search(r"first-results ([\d.]+)", p.stderr)
    return (float(m.group(1)) - t0) * 1000.0


def main():
    work, out_json = sys.argv[1], sys.argv[2]
    roots = sys.argv[3:] or ["/usr", os.path.expanduser("~")]
    os.makedirs(work, exist_ok=True)
    cdb, rdb = os.path.join(work, "bench_c.db"), os.path.join(work, "bench_r.db")
    result = {"roots": roots, "machine": {"cpus": os.cpu_count()}}

    print("scan warm-up…", flush=True)
    scan_once("c", cdb, roots)
    scan_once("r", rdb, roots)
    sc, sr = [], []
    for i in range(SCAN_RUNS):
        sc.append(scan_once("c", cdb, roots))
        sr.append(scan_once("r", rdb, roots))
        print(f"  scan run {i + 1}: C {sc[-1][0]:.0f}+{sc[-1][1]:.0f} ms, Rust {sr[-1][0]:.0f}+{sr[-1][1]:.0f} ms", flush=True)
    result["entries"] = sc[-1][2]
    result["scan"] = {"c": statistics.median(x[0] for x in sc), "rust": statistics.median(x[0] for x in sr)}
    result["save"] = {"c": statistics.median(x[1] for x in sc), "rust": statistics.median(x[1] for x in sr)}

    print("load/query/sort…", flush=True)
    c = parse_bench(run([C_DRIVER, "bench", cdb, str(RUNS), "name"] + QUERIES).stdout)
    r = parse_bench(run([R_BENCH, "bench", cdb, str(RUNS), "name"] + QUERIES).stdout)
    for q in QUERIES:
        if c["query"][q]["results"] != r["query"][q]["results"]:
            raise SystemExit(f"result count differs for {q!r}: C {c['query'][q]} Rust {r['query'][q]}")
    result["load"] = {"c": c["load"], "rust": r["load"]}
    result["query"] = {q: {"c": c["query"][q]["ms"], "rust": r["query"][q]["ms"], "results": c["query"][q]["results"]}
                       for q in QUERIES}
    result["sort"] = {s: {"c": c["sort"][s], "rust": r["sort"][s]} for s in c["sort"]}

    print("GUI startup…", flush=True)
    sandbox = os.path.join(work, "gui")
    os.makedirs(os.path.join(sandbox, "config", "fsearch"), exist_ok=True)
    with open(os.path.join(sandbox, "config", "fsearch", "fsearch.conf"), "w") as f:
        f.write("[Database]\n")
        for i, root in enumerate(roots, 1):
            f.write(f"folder_{i}_active=true\nfolder_{i}_path={root}\nfolder_{i}_one_file_system=false\n"
                    f"folder_{i}_monitor=false\nfolder_{i}_scan_after_launch=false\nfolder_{i}_rescan_after=0\n")
        f.write("exclude_hidden_files_and_folders=false\n")
    # a database whose include list matches the config, so both apps load instead of rescanning
    run([C_DRIVER, "scan", cdb] + roots)
    gui_once("c", sandbox, cdb)
    gui_once("r", sandbox, cdb)
    gc, gr = [], []
    for i in range(GUI_RUNS):
        gc.append(gui_once("c", sandbox, cdb))
        gr.append(gui_once("r", sandbox, cdb))
        print(f"  startup run {i + 1}: C {gc[-1]:.0f} ms, Rust {gr[-1]:.0f} ms", flush=True)
    result["startup"] = {"c": statistics.median(gc), "rust": statistics.median(gr)}

    with open(out_json, "w") as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
    print(json.dumps(result, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
