#!/bin/bash
# Unpack the GOG installer (an Inno Setup executable) in moto-racer-input/
# into scratch/moto/install/, the game's installed directory: game.EXE, the
# data/ directory, and the CD audio tracks as NN.wav.
#
# GOG ships the July 1998 "Polygons 3.22" patch build of the game. Its
# MotoRacer.exe is only GOG's launcher, which runs game.EXE.
set -e

cd "$(git rev-parse --show-toplevel)"
installer=$(ls moto-racer-input/setup_moto_racer_*.exe 2>/dev/null | head -1)
if [ -z "$installer" ]; then
    echo "no moto-racer-input/setup_moto_racer_*.exe; place the GOG installer there" >&2
    exit 1
fi
if ! command -v innoextract >/dev/null; then
    echo "innoextract not found; brew install innoextract" >&2
    exit 1
fi

tmp=scratch/moto/innoextract
rm -rf "$tmp" scratch/moto/install
mkdir -p "$tmp"
innoextract --quiet --output-dir "$tmp" "$installer"
mv "$tmp/app" scratch/moto/install
rm -rf "$tmp"
echo "extracted to scratch/moto/install"
