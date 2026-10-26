"""Danish WER and speed of whisper.cpp models on FLEURS da_dk test.

usage: stt_eval.py run NAME MODEL.bin [--greedy] [--vad] [-n 200]
       stt_eval.py report
"""
import argparse
import csv
import json
import re
import subprocess
import time
from pathlib import Path

HERE = Path(__file__).parent
DATA = HERE / "data/fleurs-da"
OUT = HERE / "runs/stt"
CLI = HERE / "whisper.cpp/build/bin/whisper-cli"
VAD = HERE / "models/vad/ggml-silero-v6.2.0.bin"


def clips(n):
    with open(DATA / "test.tsv", newline="") as f:
        rows = list(csv.reader(f, delimiter="\t", quoting=csv.QUOTE_NONE))
    return [(r[1], r[2]) for r in rows[:n]]


def norm(text):
    text = text.lower().replace("-", " ")
    return re.sub(r"[^\w\s]", " ", text).split()


def edits(ref, hyp):
    prev = list(range(len(hyp) + 1))
    for i, r in enumerate(ref, 1):
        cur = [i]
        for j, h in enumerate(hyp, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (r != h)))
        prev = cur
    return prev[-1]


def run(name, model, greedy, vad, n):
    # symlink clips into a per-run dir so whisper-cli's json outputs don't collide between runs
    d = OUT / name
    d.mkdir(parents=True, exist_ok=True)
    items = clips(n)
    files = []
    for fname, _ in items:
        link = d / fname
        if not link.exists():
            link.symlink_to(DATA / "test" / fname)
        files.append(str(link))
    args = [str(CLI), "-m", str(model), "-l", "da", "-oj", "-np", "-nt"]
    if greedy:
        args += ["-bs", "1", "-bo", "1"]
    if vad:
        args += ["--vad", "-vm", str(VAD)]
    t0 = time.perf_counter()
    proc = subprocess.run(args + [x for f in files for x in ("-f", f)], capture_output=True, text=True)
    wall = time.perf_counter() - t0
    if proc.returncode:
        raise SystemExit(proc.stderr[-2000:])
    ref_words = errs = 0
    rows = []
    for fname, ref in items:
        segs = json.loads((d / f"{fname}.json").read_text())["transcription"]
        hyp = " ".join(s["text"].strip() for s in segs)
        r, h = norm(ref), norm(hyp)
        e = edits(r, h)
        ref_words += len(r)
        errs += e
        rows.append({"file": fname, "ref": ref, "hyp": hyp, "edits": e, "words": len(r)})
    audio_s = sum(float(subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", f],
                                       capture_output=True, text=True).stdout) for f in files)
    summary = {"name": name, "model": str(model), "greedy": greedy, "vad": vad, "n": len(items), "wer": errs / ref_words,
               "wall_s": wall, "audio_s": audio_s}
    (OUT / f"{name}.json").write_text(json.dumps({"summary": summary, "rows": rows}, ensure_ascii=False, indent=1))
    print(json.dumps(summary))


def report():
    print(f"{'run':24} {'n':>4} {'WER':>6} {'wall s':>7} {'x realtime':>10}")
    for p in sorted(OUT.glob("*.json")):
        s = json.loads(p.read_text())["summary"]
        print(f"{s['name']:24} {s['n']:4} {s['wer']:6.1%} {s['wall_s']:7.1f} {s['audio_s'] / s['wall_s']:10.1f}")


if __name__ == "__main__":
    assert edits("a b c".split(), "a x c d".split()) == 2
    assert norm("Bevarelses- og Adgangsgebyrer, 1999!") == ["bevarelses", "og", "adgangsgebyrer", "1999"]
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd")
    ap.add_argument("name", nargs="?")
    ap.add_argument("model", nargs="?")
    ap.add_argument("--greedy", action="store_true")
    ap.add_argument("--vad", action="store_true")
    ap.add_argument("-n", type=int, default=200)
    a = ap.parse_args()
    run(a.name, a.model, a.greedy, a.vad, a.n) if a.cmd == "run" else report()
