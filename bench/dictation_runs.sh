#!/bin/bash
# usage: [DLANG=en] [PUNCT=none] [EVAL=binary] dictation_runs.sh TAG SET...  (logs in bench/runs/dictation/TAG-SET.log)
B=$(dirname "$(realpath "$0")")
M=${MODELS:-$HOME/.local/share/dk.forn.eckermann/models}
E=${EVAL:-$B/../src-tauri/target/release/examples/dictation_eval}
L=${DLANG:-da}
case $L in
  da) S=ggml-edda-v0.2-q8_0.bin ;;
  en) S=ggml-large-v3-turbo-q8_0.bin ;;
esac
v=$1; shift
for set in "$@"; do
  $E $L ${MODEL:-$M/$S} $M/ggml-silero-v6.2.0.bin ${PUNCT:-$M/punct-$L} $B/data/cases-$set.tsv 2>/dev/null > $B/runs/dictation/${DIR:-.}/$v-$set.log
done
echo done
