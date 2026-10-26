"""THX-01 on Danish editing commands: action + target in one call, then excerpts for replace.

usage: commands_eval.py
"""
import csv
import json
import statistics
import time
from pathlib import Path

import thx01

HERE = Path(__file__).parent

ACTIONS = {
    "en": {
        "bold": "make text bold", "italic": "make text italic", "underline": "underline text",
        "heading1": "heading level 1 (main heading)", "heading2": "heading level 2 (subheading)",
        "heading3": "heading level 3", "body_text": "normal body text, remove formatting",
        "bullet_list": "bulleted list", "numbered_list": "numbered list", "delete": "delete text",
        "undo": "undo the last change", "redo": "redo, undo the undo", "new_line": "insert a new line",
        "new_paragraph": "start a new paragraph", "select": "select text", "go_start": "move cursor to the start",
        "go_end": "move cursor to the end", "replace": "replace one word or phrase with another",
        "none": "not an editing command",
    },
    "da": {
        "bold": "fed skrift", "italic": "kursiv", "underline": "understreget",
        "heading1": "overskrift niveau 1 (hovedoverskrift)", "heading2": "overskrift niveau 2 (underoverskrift)",
        "heading3": "overskrift niveau 3", "body_text": "almindelig brødtekst, fjern formatering",
        "bullet_list": "punktopstilling", "numbered_list": "nummereret liste", "delete": "slet tekst",
        "undo": "fortryd sidste ændring", "redo": "annuller fortryd", "new_line": "ny linje",
        "new_paragraph": "nyt afsnit", "select": "marker tekst", "go_start": "flyt markøren til starten",
        "go_end": "flyt markøren til slutningen", "replace": "erstat et ord eller udtryk med et andet",
        "none": "ikke en redigeringskommando",
    },
}
TARGETS = {
    "en": {
        "default": "no specific part named, or 'it' / 'this' / the selection", "word": "a word",
        "sentence": "a sentence", "line": "a line", "paragraph": "a paragraph", "document": "the whole document",
        "last_dictation": "what the user just dictated",
    },
    "da": {
        "default": "ingen bestemt del nævnt, eller 'det' / 'det her' / det markerede", "word": "et ord",
        "sentence": "en sætning", "line": "en linje", "paragraph": "et afsnit", "document": "hele dokumentet",
        "last_dictation": "det brugeren lige har dikteret",
    },
}
QUESTIONS = {
    "en": ("Which editing action does the user ask for?", "Which part of the text should the action apply to?",
           "Which text should be replaced?", "What should it be replaced with?"),
    "da": ("Hvilken redigeringshandling beder brugeren om?", "Hvilken del af teksten skal handlingen bruges på?",
           "Hvilken tekst skal erstattes?", "Hvad skal den erstattes med?"),
}
THRESHOLDS = (0.0, 0.5, 0.7, 0.9)


def evaluate(agent, rows, lang):
    qa, qt, qo, qn = QUESTIONS[lang]
    out = []
    for row in rows:
        t0 = time.perf_counter()
        ans = agent.decide(row["text"], {
            "action": {"type": "choice", "question": qa, "criteria": ACTIONS[lang]},
            "target": {"type": "choice", "question": qt, "criteria": TARGETS[lang]},
        })["answers"]
        ms = (time.perf_counter() - t0) * 1000
        a, t = ans["action"], ans["target"]
        rec = {**row, "pred_action": a["choice"], "p_action": a["probabilities"][a["choice"]],
               "pred_target": t["choice"], "p_target": t["probabilities"][t["choice"]], "ms": round(ms, 1)}
        if row["action"] == "replace":
            ex = agent.decide(row["text"], {"old": {"type": "excerpt", "question": qo},
                                            "new": {"type": "excerpt", "question": qn}})["answers"]
            rec["pred_old"], rec["pred_new"] = ex["old"]["text"], ex["new"]["text"]
        out.append(rec)
    return out


def outcome(r, threshold):
    # act only when both answers clear the threshold; "none" means do nothing
    acts = r["pred_action"] != "none" and min(r["p_action"], r["p_target"]) >= threshold
    if not acts:
        return "held" if r["action"] != "none" else "right"
    return "right" if (r["pred_action"], r["pred_target"]) == (r["action"], r["target"]) else "wrong"


def report(lang, recs):
    n = len(recs)
    acc_a = sum(r["pred_action"] == r["action"] for r in recs) / n
    acc_t = sum(r["pred_target"] == r["target"] for r in recs) / n
    ms = statistics.median_high(r["ms"] for r in recs)
    print(f"\n== descriptions in {lang}: action {acc_a:.1%}, target {acc_t:.1%}, median {ms:.0f} ms, n={n}")
    for th in THRESHOLDS:
        c = [outcome(r, th) for r in recs]
        print(f"  threshold {th:.1f}: right {c.count('right'):3}  wrong action taken {c.count('wrong'):3}  held back {c.count('held'):3}")
    print("  errors (gold -> predicted):")
    for r in recs:
        if (r["pred_action"], r["pred_target"]) != (r["action"], r["target"]):
            print(f"    {r['text']!r}: {r['action']}/{r['target']} -> "
                  f"{r['pred_action']} {r['p_action']:.2f} / {r['pred_target']} {r['p_target']:.2f}")
    reps = [r for r in recs if r["action"] == "replace"]
    hits = sum((r["pred_old"], r["pred_new"]) == (r["old"], r["new"]) for r in reps)
    print(f"  replace excerpts exact: {hits}/{len(reps)}", [(r["pred_old"], r["pred_new"]) for r in reps[:3]])


if __name__ == "__main__":
    with open(HERE / "commands-da.tsv", newline="") as f:
        rows = [{k: v or "" for k, v in r.items()} for r in csv.DictReader(f, delimiter="\t")]
    agent = thx01.load("doofz/THX-01")
    (HERE / "runs").mkdir(exist_ok=True)
    for lang in ("en", "da"):
        recs = evaluate(agent, rows, lang)
        (HERE / f"runs/commands-{lang}.json").write_text(json.dumps(recs, ensure_ascii=False, indent=1))
        report(lang, recs)
