#!/bin/bash
set -e

cd "$(git rev-parse --show-toplevel)"
exe=scratch/moto/install/game.EXE
if [ ! -f "$exe" ]; then
    out/moto/extract.sh
fi
args=(
    --exe "$exe"
    --out out/moto
    --scan-immediates
    --scan-memory
    --entry-points-file out/moto/entry-points.txt
    # Self-modifying texture-mapping loops: the game writes real addresses and
    # step constants over 12345678h / 12h placeholders before running them.
    # Find them by grepping the generated code's disassembly for 12345678h.
    --patched-code 4a4e24..4a4ec0
    --patched-code 4a4f8f..4a5020
    --patched-code 4a5b0b..4a5c70
    # How many track sections the game lists as visible (see
    # widen_section_window in src/lib.rs): displacements patched at startup.
    --patched-code 40d1aa..40d1b1
    --patched-code 40d1cc..40d1d3
    --patched-code 40d274..40d27b
    --patched-code 40d29e..40d2a2
)
cargo run -p tc -- "${args[@]}"
echo cargo build --profile fast -p moto
