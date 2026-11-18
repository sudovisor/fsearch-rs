#!/usr/bin/env python3
"""Build a deterministic fixture tree for recording fsearch behavior.

Usage: make_fixture.py <dest_dir>
Every file gets a fixed size and mtime so recorded results are stable across runs and machines.
"""
import os
import sys

BASE_T = 1_700_000_000  # 2023-11-14

TREE = {
    "docs": {
        "report.pdf": 120_000,
        "Report Final.PDF": 5_000,
        "notes.txt": 12,
        "README": 300,
        "archive.tar.gz": 2_500_000,
        ".hidden_note.txt": 4,
        "trailing.": 1,
        ".bashrc": 77,
        "file2.txt": 2,
        "file10.txt": 10,
        "file02.txt": 3,
        "v1.9.txt": 19,
        "v1.10.txt": 110,
        "x0010.dat": 8,
        "x010.dat": 9,
        "sub dir": {
            "deep": {"deeper": {"bottom.c": 999, "Bottom.H": 100}},
            "space name.md": 50,
        },
        "empty_folder": {},
    },
    "media": {
        "photo.JPG": 3_000_000,
        "photo.jpg": 2_900_000,
        "song.mp3": 4_000_000,
        "movie.mkv": 700_000_000,
        "clip.webm": 1_234_567,
        "image.svg": 2_048,
        "Ärger.png": 4096,
        "ärger.png": 4097,
        "straße.txt": 33,
        "STRASSE.TXT": 34,
        "İstanbul.txt": 35,
        "istanbul.txt": 36,
        "ΣΊΣΥΦΟΣ.txt": 37,
        "σίσυφος.txt": 38,
        "café.txt": 39,                 # precomposed é
        "café.txt": 40,           # e + combining acute
        "日本語ファイル.txt": 41,
        "emoji_😀.txt": 42,
        "ﬁle_ligature.txt": 43,
        "Kelvin_K.txt": 44,
    },
    "code": {
        "main.rs": 1000,
        "lib.rs": 2000,
        "Makefile": 300,
        "build.sh": 450,
        "app.desktop": 220,
        "weird[1](2){3}.txt": 5,
        "star*name.txt": 6,
        "question?.txt": 7,
        "back\\slash.txt": 8,
        "dollar$caret^.txt": 9,
        "pipe|plus+.txt": 10,
        "colon:name.txt": 11,
        "quote\"name.txt": 12,
        "NOT": 13,
        "AND.txt": 14,
        "a-b_c.d": 15,
        "src": {
            "mod1": {"a.c": 1, "b.c": 2, "c.h": 3},
            "mod2": {"a.c": 4, "B.C": 5},
            "mod10": {"z.c": 6},
            "mod9": {"y.c": 7},
        },
    },
    ".config": {"settings.ini": 64, "app": {"state.json": 128}},
    "big": {f"item_{i:03d}.log": i * 37 for i in range(120)},
}

DIR_TIMES = {}


def build(path, node, depth, counter):
    os.makedirs(path, exist_ok=True)
    for name, val in sorted(node.items()):
        p = os.path.join(path, name)
        counter[0] += 1
        if isinstance(val, dict):
            build(p, val, depth + 1, counter)
        else:
            with open(p, "wb") as f:
                f.truncate(val)
            os.utime(p, (BASE_T, BASE_T - counter[0] * 86_400 * 3 - depth * 3600))
    DIR_TIMES[path] = BASE_T - counter[0] * 7_200


def main():
    dest = sys.argv[1]
    counter = [0]
    build(dest, TREE, 0, counter)
    # symlinks are indexed as files (lstat), never followed
    os.symlink("docs/report.pdf", os.path.join(dest, "link_to_report"))
    os.symlink("code", os.path.join(dest, "link_to_code"))
    for link in ("link_to_report", "link_to_code"):
        os.utime(os.path.join(dest, link), (BASE_T, BASE_T - 500), follow_symlinks=False)
    # directory mtimes last, deepest first
    for d in sorted(DIR_TIMES, key=len, reverse=True):
        os.utime(d, (BASE_T, DIR_TIMES[d]))


if __name__ == "__main__":
    main()
