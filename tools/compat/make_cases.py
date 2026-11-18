#!/usr/bin/env python3
"""Write the JSONL inputs for `rust-anything app record` (one case per line).

The command under test is a headless driver (`fsdriver` for the C original, `fsearch-rs --driver`
for the port). All cases run from the repository root.
"""
import json
import os
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TREE = os.path.join(ROOT, "rust/harness/fixture/tree")
DB = os.path.join(ROOT, "rust/harness/fixture/fixture.db")
OUT = os.path.join(ROOT, "rust/harness/out")

UI = 18  # AUTO_MATCH_CASE | AUTO_SEARCH_IN_PATH: the GUI default
MATCH_CASE, REGEX, PATH, FILES, FOLDERS, EXACT = 1, 4, 8, 32, 64, 128

QUERIES = [
    "", "   ", "file", "FILE", "File", "ile", "fi", "ärger", "ÄRGER", "Ärger", "arger",
    "strasse", "straße", "STRASSE", "STRAẞE", "istanbul", "İstanbul", "ISTANBUL", "ı",
    "σίσυφος", "ΣΊΣΥΦΟΣ", "σισυφος", "café", "cafe", "café", "é", "日本", "😀", "ﬁle", "file_lig",
    "kelvin_k", "Kelvin_K", "k.txt",
    "file*", "*.txt", "?.c", "*", "file?.txt", "*file*", "item_00?.log", "*.C", "**", "?",
    "docs/", "sub dir/", "src/mod", "/deep", "mod1/a", "/", "tree/docs/file",
    "a b", "report notes", "report OR notes", "report || notes", "report | notes", "report AND pdf",
    "report && pdf", "NOT txt", "!txt", "txt !file", "!!txt", "NOT NOT txt", "NOT NOT NOT txt",
    "(report OR notes) pdf", "((a)", "a)", "( OR c )", "AND", "OR", "NOT", "!", "a OR", "OR a",
    "a AND OR b", "NOT (a OR b)", "(a (b c))", "()", ")(", "(((", "a NOT b",
    "\"sub dir\"", "space\\ name", "\"a b", "\\\\", "\\:",
    "case:Report", "case:report", "nocase:REPORT", "exact:main.rs", "exact:MAIN.RS", "exact:main",
    "file:", "folder:", "files:item", "folders:mod", "nofileonly:x", "path:docs", "path:docs/file",
    "nopath:docs", "regex:^file\\d+", "regex:(", "regex:\\.(c|h)$", "noregex:.*", "case:(file OR FILE)",
    "case:", "regex:", "exact:", "file:(a OR b)",
    "ext:txt", "ext:TXT", "ext:txt;md;c", "ext:", "ext:gz", "ext:C", "ext:c", "ext:h;H", "ext:;txt",
    "ext:pdf report",
    "size:>1mb", "size:<10", "size:10..100", "size:1kb..4kb", "size:>=4096", "size:<=3", "size:=37",
    "size:..100", "size:1gb..", "size:abc", "size:", "size:1k", "size:1KB", "size:2mb", "size:0",
    "size:>", "size:10..", "size:..", "size:-5", "size:1tb",
    "depth:2", "parents:3", "depth:>3", "depth:0", "depth:1..3", "depth:x",
    "childcount:0", "empty:", "childfilecount:>3", "childfoldercount:2", "childcount:>100",
    "childfoldercount:", "childcount:1..10",
    f"parent:{TREE}/docs", f"parent:{TREE}/DOCS", "parent:", "parent:docs", f"case:parent:{TREE}/docs",
    "app:", "archive:", "audio:", "doc:", "pic:", "video:", "doc:report", "pic: OR video:",
    "dm:2022", "dm:2022-05", "dm:2022-05-01..2022-06-01", "dm:<2022-04-01", "dm:>=2023", "dm:22",
    "dm:2022-05-01 12", "datemodified:2022", "dm:1960", "dm:2022-13", "dm:abc", "dm:", "dm:..2022",
    "unknownfield:x", "a:b:c", ":::", ":", "x:", "ext:txt:md", "=", "<", ">=5",
]

FLAG_CASES = [
    (MATCH_CASE, ["File", "file", "ÄRGER", "ärger", "Report", "ext:TXT"]),
    (0, ["File", "Report", "ÄRGER", "docs/", "İstanbul"]),
    (REGEX | UI, ["^item_0[0-4]", "\\.TXT$", "(?i)report", "[", "ärger|Ärger", "^.{3}$", "docs/.*\\.txt", "ı"]),
    (REGEX | MATCH_CASE, ["^[A-Z]", "ärger"]),
    (REGEX | PATH, ["docs/.*file", "^/"]),
    (EXACT | UI, ["notes.txt", "NOTES.TXT", "notes", "Ärger.png", "ärger.png"]),
    (EXACT | MATCH_CASE, ["notes.txt", "NOTES.TXT"]),
    (PATH, ["docs", "tree", "deep/deeper", "ärger"]),
    (FILES, ["", "mod", "doc"]),
    (FOLDERS, ["", "mod", "doc"]),
    (FILES | FOLDERS, [""]),
]

SORTS = ["name", "path", "size", "mtime", "ext", "type"]


def case(name, args):
    return {"name": name, "args": args}


def main():
    cases = []
    for i, q in enumerate(QUERIES):
        cases.append(case(f"q{i:03d}", ["query", DB, "name", "asc", str(UI), "-", q]))
    for f, qs in FLAG_CASES:
        for i, q in enumerate(qs):
            cases.append(case(f"flags{f}_{i}", ["query", DB, "name", "asc", str(f), "-", q]))
    for s in SORTS:
        for d in ("asc", "desc"):
            for i, q in enumerate(["", "txt", "item", "mod"]):
                cases.append(case(f"sort_{s}_{d}_{i}", ["query", DB, s, d, str(UI), "-", q]))
    for s in SORTS:
        for d in ("asc", "desc"):
            for i, q in enumerate(["", "c", "ärger"]):
                cases.append(case(f"resort_{s}_{d}_{i}", ["query", DB, "name", d, str(UI), "-", "--resort", s, q]))
    for s in ("path", "size"):
        for t in SORTS:
            cases.append(case(f"resort2_{s}_{t}", ["query", DB, s, "asc", str(UI), "-", "--resort", t, "e"]))
    for flt in ("All", "Folders", "Files", "Applications", "Archives", "Audio", "Documents", "Pictures", "Videos"):
        for i, q in enumerate(["", "a", "report", "ext:txt", "folder:"]):
            cases.append(case(f"filter_{flt}_{i}", ["query", DB, "name", "asc", str(UI), flt, q]))
    # several queries in one process: state must not leak between searches
    cases.append(case("multi", ["query", DB, "size", "desc", str(UI), "-", "a", "", "regex:x", "b", "ext:c"]))

    scan_variants = {
        "plain": [],
        "hidden": ["--exclude-hidden"],
        "fixed_full": ["--exclude", f"fixed:full_path:both:{TREE}/docs"],
        "fixed_full_files": ["--exclude", f"fixed:full_path:files:{TREE}/docs"],
        "fixed_base": ["--exclude", "fixed:basename:both:src"],
        "wild_base": ["--exclude", "wildcard:basename:files:*.log"],
        "wild_full": ["--exclude", f"wildcard:full_path:folders:{TREE}/*/s*"],
        "wild_q": ["--exclude", "wildcard:basename:both:mod?"],
        "regex_base": ["--exclude", "regex:basename:both:^item_0[0-5]"],
        "regex_full": ["--exclude", "regex:full_path:both:media/.*\\.(png|jpg)$"],
        "regex_bad": ["--exclude", "regex:basename:both:(["],
        "multi": ["--exclude-hidden", "--exclude", "wildcard:basename:files:*.c",
                  "--exclude", "fixed:basename:folders:big"],
    }
    for name, opts in scan_variants.items():
        cases.append(case(f"scan_{name}", ["scandump"] + opts + [TREE]))
        cases.append(case(f"scanmd5_{name}", ["scanmd5", f"{OUT}/{name}.db"] + opts + [TREE]))
    two = [f"{TREE}/docs", f"{TREE}/code", f"{TREE}/media"]
    cases.append(case("scan_three_roots", ["scandump"] + two))
    cases.append(case("scanmd5_three_roots", ["scanmd5", f"{OUT}/three.db"] + two))
    cases.append(case("scan_overlap", ["scandump", TREE, f"{TREE}/docs"]))
    cases.append(case("scan_missing_root", ["scandump", f"{TREE}/does-not-exist"]))
    cases.append(case("dump", ["dump", DB]))
    cases.append(case("resave", ["resave", DB, f"{OUT}/resaved.db"]))
    cases.append(case("load_not_a_db", ["dump", os.path.join(ROOT, "rust/harness/make_fixture.py")]))
    cases.append(case("load_missing", ["dump", os.path.join(ROOT, "rust/harness/fixture/nope.db")]))

    with open(sys.argv[1], "w") as f:
        for c in cases:
            f.write(json.dumps(c, ensure_ascii=False) + "\n")
    print(len(cases), "cases")


if __name__ == "__main__":
    main()
