"""Converts a BERT punctuation model (Alvenir/bert-punct-restoration-da,
felflare/bert-restore-punctuation) into the folder the app loads.

hf download Alvenir/bert-punct-restoration-da config.json model_args.json pytorch_model.bin vocab.txt --local-dir HF
.venv/bin/python -I punct_model.py HF ~/.local/share/dk.forn.eckermann/models/punct-da
"""

import json
import shutil
import sys
from pathlib import Path

import torch
from safetensors.torch import save_file

src, dst = Path(sys.argv[1]), Path(sys.argv[2])
dst.mkdir(parents=True, exist_ok=True)
state = torch.load(src / "pytorch_model.bin", map_location="cpu", weights_only=True)
# f16 halves the download; the app widens it to f32 on load
save_file({k: v.half().contiguous() for k, v in state.items() if v.is_floating_point()}, dst / "model.safetensors")
shutil.copy(src / "vocab.txt", dst / "vocab.txt")
config = json.loads((src / "config.json").read_text())
# simpletransformers keeps the real labels in model_args.json; the config may hold only LABEL_n
labels = json.loads((src / "model_args.json").read_text())["labels_list"]
config["id2label"] = {str(i): label for i, label in enumerate(labels)}
config["label2id"] = {label: i for i, label in enumerate(labels)}
(dst / "config.json").write_text(json.dumps(config, indent=2, ensure_ascii=False) + "\n")
print(sorted(p.name for p in dst.iterdir()))
