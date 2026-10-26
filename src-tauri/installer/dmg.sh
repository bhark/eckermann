#!/bin/sh
# builds the dmg with dmgbuild in a throwaway venv; from src-tauri: installer/dmg.sh <Eckermann.app> <out.dmg> [dark|light]
set -eu
venv=$(mktemp -d)
trap 'rm -rf "$venv"' EXIT
python3 -m venv "$venv"
"$venv/bin/pip" install -q --require-hashes --only-binary :all: -r installer/dmg-requirements.txt
"$venv/bin/dmgbuild" -s installer/dmg.py -D app="$1" -D look="${3:-dark}" Eckermann "$2"
