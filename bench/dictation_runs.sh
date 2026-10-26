#!/bin/bash
# usage: dictation_runs.sh TAG SET...  (logs in bench/runs/dictation/TAG-SET.log)
B=$(dirname "$(realpath "$0")")
M=${MODELS:-$HOME/.local/share/dk.forn.eckermann/models}
E=$B/../src-tauri/target/release/examples/dictation_eval
v=$1; shift
for set in "$@"; do
  $E ${MODEL:-$M/ggml-edda-v0.2-q8_0.bin} $M/ggml-silero-v6.2.0.bin $M/punct-da $B/data/cases-$set.tsv 2>/dev/null > $B/runs/dictation/${DIR:-.}/$v-$set.log
done
echo done
