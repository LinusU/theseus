#!/bin/bash
set -e

cd "$(git rev-parse --show-toplevel)"
input=${POPTB_INSTALLER:-poptb-input/setup_populous_the_beginning_1.02_d3dfix_\(76526\).exe}
out=scratch/poptb/install

if [ ! -f "$input" ]; then
    echo "missing $input; place the GOG installer in poptb-input/" >&2
    exit 1
fi
if ! command -v innoextract >/dev/null 2>&1; then
    echo "missing innoextract; install it with: brew install innoextract" >&2
    exit 1
fi

mkdir -p "$out"
innoextract --gog --lowercase --output-dir "$out" "$input"
echo "extracted to $out"
