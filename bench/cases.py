"""Writes the case files for src-tauri/examples/dictation_eval.rs into bench/data.

python3 -I cases.py             NST and FLEURS sets
python3 -I cases.py own 1,2,0,3 own recordings: script line per dictation in time order, 0 skips
"""

import re
import sys
from pathlib import Path

DATA = Path(__file__).parent / "data"


def write(name, rows):
    (DATA / f"cases-{name}.tsv").write_text("".join(f"{ref}\t{audio}\n" for ref, audio in rows))
    print(name, len(rows))


def pairs(rows):
    half = len(rows) // 2
    return [(f"{a[0]} {b[0]}", f"{a[1]}+{b[1]}") for a, b in zip(rows[:half], rows[half:])]


def nst():
    rows = []
    for line in (DATA / "nst-da/test.tsv").read_text().splitlines():
        wav, text = line.split("\t")
        # the readers said each tag aloud; the text should hold only the sign
        rows.append((re.sub(r"\\\w+", "", text), f"nst-da/test/{wav}"))
    return rows


def fleurs(n=120):
    rows = []
    for line in (DATA / "fleurs-da/test.tsv").read_text().splitlines()[:n]:
        cols = line.split("\t")
        rows.append((cols[2], f"fleurs-da/test/{cols[1]}"))
    return rows


def own(lines):
    expected = {}
    for line in (Path(__file__).parent / "own-expected.tsv").read_text().splitlines():
        num, text = line.split("\t")
        expected[int(num)] = text
    dictations = sorted(p.stem for p in (DATA / "own").glob("*.txt"))
    if len(dictations) != len(lines):
        sys.exit(f"{len(dictations)} dictations, {len(lines)} lines given")
    rows = []
    for stem, num in zip(dictations, lines):
        if num == 0:
            continue
        count = len((DATA / "own" / f"{stem}.txt").read_text().splitlines())
        rows.append((expected[num], "|".join(f"own/{stem}-{i}.wav" for i in range(count))))
    return rows


if __name__ == "__main__":
    if sys.argv[1:2] == ["own"]:
        write("own", own([int(x) for x in sys.argv[2].split(",")]))
    else:
        write("nst", nst())
        write("nst-pairs", pairs(nst()))
        write("fleurs", fleurs()[:100])
        write("fleurs-pairs", pairs(fleurs()))
