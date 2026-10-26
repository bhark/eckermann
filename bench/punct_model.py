"""Converts Alvenir/bert-punct-restoration-da into the folder the app loads.

hf download Alvenir/bert-punct-restoration-da config.json pytorch_model.bin vocab.txt --local-dir HF
.venv/bin/python -I punct_model.py HF ~/.local/share/dk.forn.eckermann/models/punct-da
"""

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
for name in ["config.json", "vocab.txt"]:
    shutil.copy(src / name, dst / name)
print(sorted(p.name for p in dst.iterdir()))
