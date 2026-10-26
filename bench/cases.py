"""Writes the case files for src-tauri/examples/dictation_eval.rs into bench/data.

python3 -I cases.py             NST and FLEURS sets
python3 -I cases.py own 1,2,0,3 own recordings: script line per dictation in time order, 0 skips
python3 -I cases.py own-en 1,2,3 the same for English (own-en-script.txt)
"""

import re
import subprocess
import sys
import wave
from array import array
from math import log10
from pathlib import Path

DATA = Path(__file__).parent / "data"


def write(name, rows):
    (DATA / f"cases-{name}.tsv").write_text("".join(f"{ref}\t{audio}\n" for ref, audio in rows))
    print(name, len(rows))


def pairs(rows, sep="+"):
    """sep + joins two clips into one phrase, | makes them two phrases of one dictation."""
    half = len(rows) // 2
    return [(f"{a[0]} {b[0]}", f"{a[1]}{sep}{b[1]}") for a, b in zip(rows[:half], rows[half:])]


def nst():
    rows = []
    for line in (DATA / "nst-da/test.tsv").read_text().splitlines():
        wav, text = line.split("\t")
        # the readers said each tag aloud; the text should hold only the sign
        rows.append((re.sub(r"\\\w+", "", text), f"nst-da/test/{wav}"))
    return rows


def fleurs(n=120, language="da"):
    rows = []
    for line in (DATA / f"fleurs-{language}/test.tsv").read_text().splitlines()[:n]:
        cols = line.split("\t")
        rows.append((cols[2], f"fleurs-{language}/test/{cols[1]}"))
    return rows


def mid(language, n=400):
    """Clips cut in two phrases at their longest pause inside the speech, as a pause mid-sentence cuts them."""
    out = DATA / f"fleurs-{language}-mid"
    out.mkdir(exist_ok=True)
    rows = []
    frame = 320  # 20 ms
    for text, clip in fleurs(n, language):
        raw = subprocess.run(
            ["ffmpeg", "-v", "error", "-i", DATA / clip, "-f", "s16le", "-ac", "1", "-ar", "16000", "-"],
            capture_output=True,
            check=True,
        ).stdout
        pcm = array("h", raw)
        db = [10 * log10(sum(x * x for x in pcm[i : i + frame]) / frame + 1) for i in range(0, len(pcm) - frame, frame)]
        # levels differ a lot between FLEURS readers, so quiet is relative to the clip's loud frames
        quiet = sorted(db)[len(db) * 95 // 100] - 35
        loud = [i for i, d in enumerate(db) if d > quiet]
        runs, start = [], None
        for i in range(loud[0], loud[-1] + 1):
            if db[i] <= quiet and start is None:
                start = i
            elif db[i] > quiet and start is not None:
                runs.append((start, i))
                start = None
        span = loud[-1] - loud[0]
        inner = [r for r in runs if r[1] - r[0] >= 10 and 0.2 < (r[0] - loud[0]) / span < 0.8]
        if not inner:
            continue
        a, b = max(inner, key=lambda r: r[1] - r[0])
        cut = (a + b) // 2 * frame
        stem = Path(clip).stem
        for half, samples in (("a", pcm[:cut]), ("b", pcm[cut:])):
            with wave.open(str(out / f"{stem}-{half}.wav"), "wb") as w:
                w.setnchannels(1)
                w.setsampwidth(2)
                w.setframerate(16000)
                w.writeframes(samples.tobytes())
        rows.append((text, f"fleurs-{language}-mid/{stem}-a.wav|fleurs-{language}-mid/{stem}-b.wav"))
    return rows


def own(lines, name="own"):
    expected = {}
    for line in (Path(__file__).parent / f"{name}-expected.tsv").read_text().splitlines():
        num, text = line.split("\t")
        expected[int(num)] = text
    dictations = sorted(p.stem for p in (DATA / name).glob("*.txt"))
    if len(dictations) != len(lines):
        sys.exit(f"{len(dictations)} dictations, {len(lines)} lines given")
    rows = []
    for stem, num in zip(dictations, lines):
        if num == 0:
            continue
        count = len((DATA / name / f"{stem}.txt").read_text().splitlines())
        rows.append((expected[num], "|".join(f"{name}/{stem}-{i}.wav" for i in range(count))))
    return rows


if __name__ == "__main__":
    if sys.argv[1:2] in (["own"], ["own-en"]):
        write(sys.argv[1], own([int(x) for x in sys.argv[2].split(",")], sys.argv[1]))
    else:
        write("nst", nst())
        write("nst-pairs", pairs(nst()))
        write("fleurs", fleurs()[:100])
        write("fleurs-pairs", pairs(fleurs()))
        write("fleurs-en", fleurs(language="en")[:100])
        write("fleurs-en-pairs", pairs(fleurs(language="en")))
        write("fleurs-en-split", pairs(fleurs(language="en"), sep="|"))
        write("fleurs-en-mid", mid("en")[:100])
