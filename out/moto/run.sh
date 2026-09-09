#!/bin/bash
# Run the translated game from its install directory, so relative data\ paths
# resolve. Extra arguments are passed to the game; game.EXE lists its flags in
# its strings (-NoD3D, -NoSound, -NoCpuDetect, -windowed, -FrameRateMax60, ...).
set -e

cd "$(git rev-parse --show-toplevel)"
bin=target/fast/moto
if [ ! -x "$bin" ]; then
    echo "missing $bin; run: cargo build --profile fast -p moto" >&2
    exit 1
fi
bin="$(pwd)/$bin"
# Registry entries the installer would have created.
export THESEUS_REGISTRY="$(pwd)/out/moto/moto.reg"

cd scratch/moto/install
# The game checks that its CD is in the drive by volume label.
export THESEUS_CD_LABEL=MOTO_RACER
exec "$bin" "$@"
