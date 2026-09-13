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

# Upscaled textures, if a pack has been made (see "Texture packs" in README.md).
if [ -z "${THESEUS_TEXTURE_PACK+set}" ] && [ -d scratch/moto/textures/pack ]; then
    export THESEUS_TEXTURE_PACK="$(pwd)/scratch/moto/textures/pack"
fi

cd scratch/moto/install
# The game checks that its CD is in the drive by volume label.
export THESEUS_CD_LABEL=MOTO_RACER
# A 1280x960 window for the game's 640x480 (THESEUS_WINDOW_SIZE or
# THESEUS_FULLSCREEN for others); Direct3D renders at the window's resolution.
export THESEUS_WINDOW_SCALE=${THESEUS_WINDOW_SCALE:-2}
# Without arguments: Direct3D, and no frame rate cap (the game's default is
# 30; its simulation runs on elapsed time, and presenting waits for the
# display's refresh, so frames come at up to e.g. 120 Hz).
if [ $# -eq 0 ]; then
    set -- -D3D -NoCpuDetect -FrameRateMax0
fi
exec "$bin" "$@"
