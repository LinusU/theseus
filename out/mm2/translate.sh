#!/bin/bash
set -e

cd "$(git rev-parse --show-toplevel)"
input="${MM2_INPUT:-$PWD/game/Midtown2.exe}"
if [[ ! -f "$input" ]]; then
    printf 'missing MM2 PE input: %s\n' "$input" >&2
    printf 'acquire the cracked installation or set MM2_INPUT to another usable PE image\n' >&2
    exit 1
fi
args=(
    --exe "$input"
    --out out/mm2
    --scan-memory --scan-immediates --scan-prologues
)
if [[ -f out/mm2/missing.txt ]]; then
    args+=(--entry-points-file out/mm2/missing.txt)
fi
cargo run -p tc -- "${args[@]}"
printf '%s\n' 'cargo build --profile fast -p mm2' 'THESEUS_MISSING_ADDRS=out/mm2/missing.txt cargo run --profile fast -p mm2 -- game'
