"""Local LLMs turning Danish spoken edit commands into grounded JSON actions.

usage: edit_eval.py run NAME   (NAME from MODELS)
       edit_eval.py report
"""
import json
import re
import statistics
import subprocess
import sys
import time
import urllib.request
from difflib import SequenceMatcher
from pathlib import Path

HERE = Path(__file__).parent
RUNS = HERE / "runs"
PORT = 8091
SERVER = HERE / "llama.cpp/build/bin/llama-server"
FORK_SERVER = HERE / "llama-fork/build/bin/llama-server"
GPU = ["-ngl", "99"]
# models over 8 GB VRAM: leave layer placement to llama.cpp's --fit
FIT = []
MODELS = {
    "gemma4-e2b": ("google/gemma-4-E2B-it-qat-q4_0-gguf", "gemma-4-E2B_q4_0-it.gguf", SERVER, GPU),
    "gemma4-e4b": ("google/gemma-4-E4B-it-qat-q4_0-gguf", "gemma-4-E4B_q4_0-it.gguf", SERVER, GPU),
    "qwen3.5-2b": ("unsloth/Qwen3.5-2B-GGUF", "Qwen3.5-2B-Q4_K_M.gguf", SERVER, GPU),
    "qwen3.5-4b": ("unsloth/Qwen3.5-4B-GGUF", "Qwen3.5-4B-Q4_K_M.gguf", SERVER, GPU),
    "qwen3.5-9b": ("unsloth/Qwen3.5-9B-GGUF", "Qwen3.5-9B-Q4_K_M.gguf", SERVER, GPU),
    "qwen3.5-9b-q8": ("unsloth/Qwen3.5-9B-GGUF", "Qwen3.5-9B-Q8_0.gguf", SERVER, FIT),
    "qwen3.5-9b-iq4xs": ("unsloth/Qwen3.5-9B-GGUF", "Qwen3.5-9B-IQ4_XS.gguf", SERVER, GPU),
    "qwen3.5-9b-q3kxl": ("unsloth/Qwen3.5-9B-GGUF", "Qwen3.5-9B-UD-Q3_K_XL.gguf", SERVER, GPU),
    "qwen3.5-9b-iq3xxs": ("unsloth/Qwen3.5-9B-GGUF", "Qwen3.5-9B-UD-IQ3_XXS.gguf", SERVER, GPU),
    "qwen3.5-9b-iq2m": ("unsloth/Qwen3.5-9B-GGUF", "Qwen3.5-9B-UD-IQ2_M.gguf", SERVER, GPU),
    "qwen3.6-35b-a3b-iq2xxs": ("unsloth/Qwen3.6-35B-A3B-GGUF", "Qwen3.6-35B-A3B-UD-IQ2_XXS.gguf", SERVER, FIT),
    # prefix lm: whole prompt must fit one micro-batch; trained context is 4096
    "mimir-1b": ("danish-foundation-models/DFM-Mimir-v1.5-GGUF", "dfm-mimir-v1.5-q4_k_m.gguf", FORK_SERVER,
                 GPU + ["-c", "4096", "-b", "4096", "-ub", "4096"]),
}
TARGET_OPS = {"format", "delete", "replace", "select", "move"}
STYLES = ["bold", "italic", "underline", "heading1", "heading2", "heading3", "body_text", "bullet_list", "numbered_list"]
SCHEMA = {
    "type": "object",
    "properties": {
        "op": {"enum": ["format", "delete", "replace", "select", "move", "new_line", "new_paragraph", "undo", "redo", "none"]},
        "style": {"enum": STYLES + [None]},
        "target": {"type": ["string", "null"]},
        "text": {"type": ["string", "null"]},
        "position": {"enum": ["before", "after", None]},
    },
    "required": ["op", "style", "target", "text", "position"],
}
SYSTEM = """You turn a spoken Danish editing command into one JSON action for a text editor.
The user dictates text with one key and gives editing commands with another key. The command is transcribed speech and may contain recognition errors, such as misspelled names or words split in two.
You get the document, with <markør/> at the cursor and <markering>...</markering> around selected text, and the command.

Fields:
- op: format | delete | replace | select | move | new_line | new_paragraph | undo | redo | none
- style (only for format): bold | italic | underline | heading1 | heading2 | heading3 | body_text | bullet_list | numbered_list
- target (for format, delete, replace, select, move): the exact text from the document that the command refers to. Copy it character for character from the document, even when the command spells it differently. Use <MARKERING> for the selected text and <ALT> for the whole document.
- text (only for replace): the new text.
- position (only for move): before or after the target.
Words like "sidste", "linjen" and "ordet" refer to the text at the cursor. "det" or "det her" means the selection when there is one.
Use op none when the speech is not an editing command, for example dictated text, a greeting, a question or filler words. Set unused fields to null."""
EX_DOC = "Indkøb\nVi skal huske mælk og brød til weekenden.\nPeter kommer på søndag klokken tre.<markør/>"
EX_SEL = "Indkøb\nVi skal huske <markering>mælk og brød</markering> til weekenden.\nPeter kommer på søndag klokken tre."
EXAMPLES = [
    (EX_DOC, "gør Peter fed", {"op": "format", "style": "bold", "target": "Peter"}),
    (EX_DOC, "ret søndag til lørdag", {"op": "replace", "target": "søndag", "text": "lørdag"}),
    (EX_DOC, "slet den sidste sætning", {"op": "delete", "target": "Peter kommer på søndag klokken tre."}),
    (EX_DOC, "gør petter kursiv", {"op": "format", "style": "italic", "target": "Peter"}),
    (EX_DOC, "gå til starten af dokumentet", {"op": "move", "target": "<ALT>", "position": "before"}),
    (EX_DOC, "gør første linje til en overskrift", {"op": "format", "style": "heading1", "target": "Indkøb"}),
    (EX_DOC, "fortryd", {"op": "undo"}),
    (EX_DOC, "og så gik vi en tur i skoven", {"op": "none"}),
    (EX_SEL, "slet det", {"op": "delete", "target": "<MARKERING>"}),
]
PUNCT = " \n\t.,;:!?"


def user_msg(doc, cmd):
    return f"Dokument:\n{doc}\n\nKommando: {cmd}"


def full(action):
    return json.dumps({k: action.get(k) for k in SCHEMA["properties"]}, ensure_ascii=False)


def messages(doc, cmd):
    msgs = [{"role": "system", "content": SYSTEM}]
    for ex_doc, ex_cmd, ex_out in EXAMPLES:
        msgs += [{"role": "user", "content": user_msg(ex_doc, ex_cmd)}, {"role": "assistant", "content": full(ex_out)}]
    return msgs + [{"role": "user", "content": user_msg(doc, cmd)}]


def render(doc, cursor, sel):
    if sel:
        return doc[:sel[0]] + "<markering>" + doc[sel[0]:sel[1]] + "</markering>" + doc[sel[1]:]
    return doc[:cursor] + "<markør/>" + doc[cursor:]


def trim(doc, span):
    s, e = span
    while s < e and doc[s] in PUNCT:
        s += 1
    while e > s and doc[e - 1] in PUNCT:
        e -= 1
    return s, e


def locate(quote, doc, sel, cursor):
    # exact, then case-insensitive, then best fuzzy word window; nearest to cursor wins ties
    if not quote:
        return None, None
    q = quote.strip()
    if q == "<ALT>":
        return (0, len(doc)), "special"
    if q == "<MARKERING>":
        return sel, "special"
    for hay, needle, how in ((doc, q, "exact"), (doc.lower(), q.lower(), "nocase")):
        hits = [m.start() for m in re.finditer(re.escape(needle), hay)]
        if hits:
            s = min(hits, key=lambda h: abs(h - cursor))
            return (s, s + len(q)), how
    words = [m.span() for m in re.finditer(r"\S+", doc)]
    n = len(q.split())
    best, span = 0.0, None
    for i in range(len(words)):
        for length in range(max(1, n - 2), n + 3):
            j = i + length - 1
            if j >= len(words):
                break
            s, e = words[i][0], words[j][1]
            r = SequenceMatcher(None, q.lower(), doc[s:e].lower()).ratio()
            if r > best:
                best, span = r, (s, e)
    return (span, "fuzzy") if best >= 0.75 else (None, None)


def gold_span(case, doc):
    t = case.get("target")
    if t == "<ALT>":
        return 0, len(doc)
    s = doc.find(t)
    assert s >= 0, (case["cmd"], t)
    return s, s + len(t)


def norm(text):
    return (text or "").strip(PUNCT).casefold()


def score(case, pred, doc, sel, cursor):
    """-> (outcome, detail) with outcome in right / wrong / held"""
    op = pred.get("op")
    if case["op"] == "none":
        return ("right", "") if op == "none" else ("wrong", f"acted: {op}")
    if op == "none":
        return "held", "said none"
    if op != case["op"]:
        return "wrong", f"op {op}"
    if op == "format" and pred.get("style") != case["style"]:
        return "wrong", f"style {pred.get('style')}"
    if op in TARGET_OPS:
        span, how = locate(pred.get("target"), doc, sel, cursor)
        if span is None:
            return "held", f"target not found: {pred.get('target')!r}"
        got, want = trim(doc, span), trim(doc, gold_span(case, doc))
        if op == "move":
            pos = pred.get("position")
            if pos not in ("before", "after"):
                return "wrong", f"position {pos}"
            at = got[1] if pos == "after" else got[0]
            if at != (want[1] if case["position"] == "after" else want[0]):
                return "wrong", f"moved to {at} via {pred.get('target')!r} {pos}"
        elif got != want:
            return "wrong", f"target {doc[got[0]:got[1]]!r} ({how})"
        if op == "replace" and norm(pred.get("text")) != norm(case["text"]):
            return "wrong", f"text {pred.get('text')!r}"
        return "right", how if how != "exact" else ""
    return "right", ""


def post(payload):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}/v1/chat/completions", json.dumps(payload).encode(),
                                 {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=300) as r:
        return json.load(r)


def start_server(name):
    from huggingface_hub import hf_hub_download
    repo, fname, binary, args = MODELS[name]
    path = hf_hub_download(repo, fname)
    log = (RUNS / f"server-{name}.log").open("w")
    proc = subprocess.Popen([str(binary), "-m", path, "-c", "8192", "-np", "1", "--jinja",
                             "--host", "127.0.0.1", "--port", str(PORT), *args], stdout=log, stderr=subprocess.STDOUT)
    for _ in range(300):
        try:
            if json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/health", timeout=2)).get("status") == "ok":
                return proc
        except Exception:
            if proc.poll() is not None:
                raise SystemExit(f"server for {name} exited, see runs/server-{name}.log")
        time.sleep(1)
    proc.kill()
    raise SystemExit(f"server for {name} not ready")


def place(data, case):
    """-> (doc, selection span or None, cursor) for a case"""
    doc = data["docs"][case["doc"]]
    sel = None
    if case.get("selection"):
        s = doc.find(case["selection"])
        sel = (s, s + len(case["selection"]))
    cursor = sel[1] if sel else doc.find(data["cursor_after"][case["doc"]]) + len(data["cursor_after"][case["doc"]])
    return doc, sel, cursor


def run(name):
    data = json.loads((HERE / "edit_cases.json").read_text())
    proc = start_server(name)
    recs = []
    try:
        for case in data["cases"]:
            doc, sel, cursor = place(data, case)
            t0 = time.perf_counter()
            resp = post({"messages": messages(render(doc, cursor, sel), case["cmd"]), "temperature": 0, "max_tokens": 300,
                         "response_format": {"type": "json_schema", "json_schema": {"name": "edit", "schema": SCHEMA}},
                         "chat_template_kwargs": {"enable_thinking": False}})
            ms = (time.perf_counter() - t0) * 1000
            raw = resp["choices"][0]["message"]["content"]
            try:
                pred = json.loads(raw)
            except json.JSONDecodeError:
                pred = {"op": "invalid"}
            outcome, detail = score(case, pred, doc, sel, cursor)
            recs.append({**case, "pred": pred, "outcome": outcome, "detail": detail, "ms": round(ms),
                         "prompt_tokens": resp["usage"]["prompt_tokens"]})
            (RUNS / f"edit-{name}.json").write_text(json.dumps(recs, ensure_ascii=False, indent=1))
    finally:
        proc.terminate()
        proc.wait()


def report():
    kinds = ["quote", "relative", "selection", "asr", "no_target", "none"]
    print(f"{'model':12} {'right':>5} {'wrong':>5} {'held':>4}  " + " ".join(f"{k:>9}" for k in kinds) + "   median ms")
    for name in MODELS:
        path = RUNS / f"edit-{name}.json"
        if not path.exists():
            continue
        recs = json.loads(path.read_text())
        c = [r["outcome"] for r in recs]
        per = []
        for k in kinds:
            sub = [r for r in recs if r["kind"] == k]
            per.append(f"{sum(r['outcome'] == 'right' for r in sub)}/{len(sub)}")
        ms = statistics.median(r["ms"] for r in recs[1:])
        print(f"{name:12} {c.count('right'):5} {c.count('wrong'):5} {c.count('held'):4}  " + " ".join(f"{p:>9}" for p in per) + f"   {ms:6.0f}")


def check():
    data = json.loads((HERE / "edit_cases.json").read_text())
    for case in data["cases"]:
        doc, sel, cursor = place(data, case)
        gold = {k: case.get(k) for k in SCHEMA["properties"]}
        assert score(case, gold, doc, sel, cursor)[0] == "right", case["cmd"]
    doc = data["docs"]["brev"]
    case = {"op": "format", "style": "underline", "target": "Kirsten"}
    assert score(case, {**case, "target": "Kirsten."}, doc, None, 0)[0] == "right"
    assert score(case, {**case, "target": "Kirstens"}, doc, None, 0)[0] == "right"
    assert score(case, {**case, "target": "Mette"}, doc, None, 0)[0] == "wrong"
    assert score(case, {**case, "target": "Ingen her"}, doc, None, 0)[0] == "held"
    assert score({"op": "none"}, {"op": "undo"}, doc, None, 0)[0] == "wrong"
    print(f"ok: {len(data['cases'])} gold cases score right")


if __name__ == "__main__":
    RUNS.mkdir(exist_ok=True)
    {"run": lambda: run(sys.argv[2]), "report": report, "check": check}[sys.argv[1]]()
