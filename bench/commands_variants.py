"""Setup variants for THX-01 on Danish commands: yes/no gate and context in the state.

usage: commands_variants.py
"""
import csv
import json
import statistics
import time
from pathlib import Path

import thx01

from commands_eval import ACTIONS, QUESTIONS, TARGETS

HERE = Path(__file__).parent
NO_TARGET = {"undo", "redo", "new_line", "new_paragraph", "replace", "none"}
GATE_Q = "Er dette en instruks om at redigere tekst?"
CONTEXT = ("Følgende er transskriberet tale fra kommandotasten i et dikteringsværktøj. "
           "Brugeren kan have trykket på tasten ved en fejl og bare talt.\nTale: ")
VARIANTS = {"A baseline": (False, False), "B gate": (True, False), "C context": (False, True), "D gate+context": (True, True)}
GRID = [i / 20 for i in range(19)]


def ask(agent, text, gate, context):
    qa, qt, _, _ = QUESTIONS["da"]
    actions = {k: v for k, v in ACTIONS["da"].items() if not (gate and k == "none")}
    qs = {"action": {"type": "choice", "question": qa, "criteria": actions},
          "target": {"type": "choice", "question": qt, "criteria": TARGETS["da"]}}
    if gate:
        qs["command"] = {"type": "noul", "question": GATE_Q}
    t0 = time.perf_counter()
    ans = agent.decide(CONTEXT + text if context else text, qs)["answers"]
    ms = (time.perf_counter() - t0) * 1000
    a, t = ans["action"], ans["target"]
    return {"pred_action": a["choice"], "p_action": a["probabilities"][a["choice"]],
            "pred_target": t["choice"], "p_target": t["probabilities"][t["choice"]],
            "p_command": ans["command"]["noul"] if gate else None, "ms": ms}


def correct(r):
    return r["pred_action"] == r["action"] and (r["action"] in NO_TARGET or r["pred_target"] == r["target"])


def outcomes(recs, g, t):
    right = wrong = held = 0
    for r in recs:
        conf = r["p_action"] if r["pred_action"] in NO_TARGET else min(r["p_action"], r["p_target"])
        acts = r["pred_action"] != "none" and conf >= t and (r["p_command"] is None or r["p_command"] >= g)
        if not acts:
            right += r["action"] == "none"
            held += r["action"] != "none"
        elif correct(r):
            right += 1
        else:
            wrong += 1
    return right, wrong, held


def best(recs, max_wrong):
    gates = GRID if recs[0]["p_command"] is not None else [0.0]
    return max((outcomes(recs, g, t) + (g, t) for g in gates for t in GRID if outcomes(recs, g, t)[1] <= max_wrong),
               key=lambda o: o[0])


if __name__ == "__main__":
    with open(HERE / "commands-da.tsv", newline="") as f:
        rows = [{k: v or "" for k, v in r.items()} for r in csv.DictReader(f, delimiter="\t")]
    agent = thx01.load("doofz/THX-01")
    results = {}
    for name, (gate, context) in VARIANTS.items():
        recs = [{**row, **ask(agent, row["text"], gate, context)} for row in rows]
        results[name] = recs
        cmds = [r for r in recs if r["action"] != "none"]
        acc = sum(map(correct, cmds)) / len(cmds)
        ms = statistics.median_high(r["ms"] for r in recs)
        print(f"\n== {name}: commands fully right {acc:.1%} (n={len(cmds)}), median {ms:.0f} ms")
        for max_wrong in (0, 1, 3):
            right, wrong, held, g, t = best(recs, max_wrong)
            print(f"  best with <= {max_wrong} wrong: right {right:3}  wrong {wrong}  held {held:3}  (gate {g:.2f}, threshold {t:.2f})")
        if gate:
            non = sorted(r["p_command"] for r in recs if r["action"] == "none")
            com = sorted(r["p_command"] for r in cmds)
            print(f"  p(command) non-commands: max {non[-1]:.2f}, median {statistics.median_high(non):.2f}; "
                  f"commands: min {com[0]:.2f}, median {statistics.median_high(com):.2f}, above non-command max: {sum(p > non[-1] for p in com)}/{len(com)}")
        for r in recs:
            if r["action"] == "none" and outcomes([r], 0.5, 0.0)[1]:
                print(f"  non-command acted on at gate 0.5: {r['text']!r} -> {r['pred_action']} {r['p_action']:.2f}"
                      + (f" (p_command {r['p_command']:.2f})" if gate else ""))
    (HERE / "runs/commands-variants.json").write_text(json.dumps(results, ensure_ascii=False, indent=1))
