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
    # The level of detail riders' bikes are drawn at by distance (see
    # full_detail_bikes in src/lib.rs): immediates patched at startup.
    --patched-code 45f1f3..45f1fd
    --patched-code 45f216..45f220
    --patched-code 45f274..45f27e
    # Every use of the transformed-vertex array, which is moved to a larger
    # one (see grow_vertex_array in src/lib.rs).
    --patched-code 497ce2..497cec
    --patched-code 498005..49800c
    --patched-code 498032..498039
    --patched-code 498410..498417
    --patched-code 49843d..498444
    --patched-code 498813..49881a
    --patched-code 49883e..498845
    --patched-code 498ac9..498ad0
    --patched-code 498af6..498afd
    --patched-code 498ec4..498ecb
    --patched-code 498ef1..498ef8
    --patched-code 49916c..499173
    --patched-code 499199..4991a0
    --patched-code 49b2d7..49b2de
    --patched-code 49b4ca..49b4d1
    --patched-code 49c7f2..49c7f8
    --patched-code 49c800..49c806
    --patched-code 49cc43..49cc49
    --patched-code 49cc51..49cc57
)
cargo run -p tc -- "${args[@]}"
echo cargo build --profile fast -p moto
