"""Zero-shot MASSIVE intent accuracy and calibration for decision models.

usage: massive_eval.py run {laya,thx} {da,en}
       massive_eval.py report
"""
import gzip
import json
import random
import statistics
import sys
import time
from pathlib import Path

HERE = Path(__file__).parent
RUNS = HERE / "runs"
QUESTION = "What does the user want the voice assistant to do?"
N_OPTIONS = 20


def load(lang):
    with gzip.open(HERE / f"data/massive-{lang}-test.json.gz", "rt") as f:
        return [json.loads(line) for line in f]


def options(row, intents):
    # seeded by utterance id, so every model and language sees the same 20 options in the same order
    rng = random.Random(int(row["id"]))
    opts = rng.sample([i for i in intents if i != row["label"]], N_OPTIONS - 1) + [row["label"]]
    rng.shuffle(opts)
    return {i: i.replace("_", " ") for i in opts}


def loader(model):
    if model == "laya":
        import laya
        agent = laya.load("convaiinnovations/laya-multilingual")
        return lambda text, q: agent.predict({"body": text}, {"intent": {"type": "choice", "instructions": QUESTION, "criteria": q}})
    import thx01
    agent = thx01.load("doofz/THX-01")
    return lambda text, q: agent.decide(text, {"intent": {"type": "choice", "question": QUESTION, "criteria": q}})


def run(model, lang):
    rows = load(lang)
    intents = sorted({r["label"] for r in load("en")})
    out = RUNS / f"{model}-{lang}.jsonl"
    done = {json.loads(line)["id"] for line in out.open()} if out.exists() else set()
    decide = loader(model)
    with out.open("a") as f:
        for row in rows:
            if row["id"] in done:
                continue
            t0 = time.perf_counter()
            ans = decide(row["text"], options(row, intents))["answers"]["intent"]
            ms = (time.perf_counter() - t0) * 1000
            probs = ans["probabilities"]
            f.write(json.dumps({"id": row["id"], "gold": row["label"], "pred": ans["choice"],
                                "p_pred": probs[ans["choice"]], "p_gold": probs[row["label"]], "ms": round(ms, 1)}) + "\n")
            f.flush()


def ece(rows, bins=10):
    total = 0.0
    for b in range(bins):
        lo, hi = b / bins, (b + 1) / bins
        sel = [r for r in rows if lo <= r["p_pred"] < hi or (b == bins - 1 and r["p_pred"] == 1.0)]
        if sel:
            acc = sum(r["pred"] == r["gold"] for r in sel) / len(sel)
            conf = sum(r["p_pred"] for r in sel) / len(sel)
            total += len(sel) / len(rows) * abs(acc - conf)
    return total


def report():
    print(f"{'run':10} {'n':>5} {'acc':>6} {'ece':>6} {'ms':>5}  acted/accuracy at p>=0.5, 0.7, 0.9")
    for path in sorted(RUNS.glob("*.jsonl")):
        rows = [json.loads(line) for line in path.open()]
        acc = sum(r["pred"] == r["gold"] for r in rows) / len(rows)
        ms = statistics.median_high(r["ms"] for r in rows)
        sel = []
        for t in (0.5, 0.7, 0.9):
            s = [r for r in rows if r["p_pred"] >= t]
            sel.append(f"{len(s) / len(rows):.0%}/{sum(r['pred'] == r['gold'] for r in s) / len(s):.0%}" if s else "0%/-")
        print(f"{path.stem:10} {len(rows):5} {acc:6.3f} {ece(rows):6.3f} {ms:5.0f}  {'  '.join(sel)}")


if __name__ == "__main__":
    if sys.argv[1] == "run":
        RUNS.mkdir(exist_ok=True)
        run(sys.argv[2], sys.argv[3])
    else:
        report()
