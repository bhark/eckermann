#!/bin/bash
# downloads the English source models at pinned revisions; punct_model.py converts the punctuation one
M=$(dirname "$(realpath "$0")")/models
T=https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1
F=https://huggingface.co/felflare/bert-restore-punctuation/resolve/954108a105ef1f89f08b71c25d6e33bb89cde724
curl -sfL -C - -o $M/turbo/ggml-large-v3-turbo-q8_0.bin $T/ggml-large-v3-turbo-q8_0.bin && echo turbo ok
for f in config.json model_args.json vocab.txt pytorch_model.bin; do
  curl -sfL -C - -o $M/felflare/hf/$f $F/$f && echo $f ok
done
sha256sum $M/turbo/ggml-large-v3-turbo-q8_0.bin $M/felflare/hf/pytorch_model.bin
echo finished
