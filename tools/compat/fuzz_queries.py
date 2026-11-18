#!/usr/bin/env python3
"""Differential fuzz: random queries against the C driver and the Rust app on one database.

Usage: fuzz_queries.py <c_driver> <rust_app> <db> <n_batches> [seed]
Each batch runs 40 random queries (random sort, direction, flags and filter) in one process per
side and compares stdout. A mismatching batch is narrowed down to the single failing query.
"""
import hashlib
import random
import subprocess
import sys

WORDS = ["a", "e", "lib", "bin", "share", "py", "so", "doc", "LICENSE", "README", "x86", "64", "test",
         "usr", "include", "gz", "png", "conf", "man", "locale", "zh", "de", "ä", "ß", "İ", "ı", "é",
         "Σ", "ℹ", "K", "1", "2", "10", "0", "_", "-", ".", "..", "/", "/usr/lib", "usr/share", "ğ"]
WILD = ["*", "?", "*.so", "lib*", "*.py", "?.h", "*a*", "*.*"]
FIELDS = ["case:", "nocase:", "exact:", "path:", "nopath:", "regex:", "file:", "folder:", "files:",
          "folders:", "ext:", "size:", "depth:", "parents:", "childcount:", "childfilecount:",
          "childfoldercount:", "empty:", "dm:", "datemodified:", "parent:", "doc:", "pic:", "audio:",
          "video:", "app:", "archive:", "nofoldersonly:", "unknown:"]
NUMS = ["0", "1", "5", "100", "1k", "1kb", "10mb", "1g", ">5", "<3", ">=2", "<=1", "1..3", "..10", "5..",
        "2020", "2023-05", "2021-01-01..2022-01-01", "x", ""]
OPS = ["AND", "OR", "NOT", "!", "&&", "||", "(", ")", "\"", "\\"]
SORTS = ["name", "path", "size", "mtime", "ext", "type"]
FILTERS = ["-", "-", "-", "All", "Folders", "Files", "Documents", "Pictures", "Audio", "Archives"]


def token(r):
    k = r.random()
    if k < 0.35:
        return r.choice(WORDS)
    if k < 0.45:
        return r.choice(WILD)
    if k < 0.65:
        f = r.choice(FIELDS)
        if f in ("size:", "depth:", "parents:", "childcount:", "childfilecount:", "childfoldercount:", "dm:",
                 "datemodified:"):
            return f + r.choice(NUMS)
        if f == "ext:":
            return f + r.choice(["so", "py;h", "", "PNG;png", "gz", "c;h;cpp"])
        if f == "parent:":
            return f + r.choice(["/usr/lib", "/usr", "", "/usr/share/doc", "/USR"])
        if f == "regex:":
            return f + r.choice(["^lib", "\\.so$", "[0-9]+", "(", "a|b", "^.{2}$"])
        return f + r.choice(WORDS + [""])
    if k < 0.85:
        return r.choice(OPS)
    return "\"" + r.choice(WORDS) + " " + r.choice(WORDS) + "\""


def query(r):
    n = r.randint(1, 4)
    sep = r.choice([" ", " ", ""])
    return sep.join(token(r) for _ in range(n))


def run(cmd, args):
    p = subprocess.run([cmd] + args, capture_output=True, timeout=600)
    return p.returncode, p.stdout


def main():
    c, rs, db, batches = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
    seed = int(sys.argv[5]) if len(sys.argv) > 5 else 1
    r = random.Random(seed)
    total = 0
    bad = 0
    for b in range(batches):
        sort = r.choice(SORTS)
        d = r.choice(["asc", "desc"])
        flags = r.choice([18, 18, 18, 0, 1, 2, 4 | 18, 8, 16, 32, 64, 128 | 18, 1 | 8])
        flt = r.choice(FILTERS)
        resort = ["--resort", r.choice(SORTS)] if r.random() < 0.3 else []
        qs = [query(r) for _ in range(40)]
        base = ["query", db, sort, d, str(flags), flt] + resort
        total += len(qs)
        ca = run(c, base + qs)
        ra = run(rs, base + qs)
        if ca == ra:
            continue
        for q in qs:
            ca1, ra1 = run(c, base + [q]), run(rs, base + [q])
            if ca1 != ra1:
                bad += 1
                print("MISMATCH", base[2:], repr(q),
                      hashlib.md5(ca1[1]).hexdigest()[:8], hashlib.md5(ra1[1]).hexdigest()[:8], flush=True)
    print(f"{total} queries, {bad} mismatches")


if __name__ == "__main__":
    main()
