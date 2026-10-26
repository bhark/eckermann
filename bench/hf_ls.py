import sys

from huggingface_hub import HfApi

for repo in sys.argv[1:]:
    print("==", repo)
    for f in HfApi().list_repo_tree(repo, recursive=True):
        p = f.path
        if p.endswith(".gguf") and any(q in p.lower() for q in ("q4_k_m", "q8_0", "q4_0")):
            print(f"   {p} {f.size / 1e9:.2f} GB")
