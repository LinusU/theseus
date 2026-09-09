#!/bin/bash
set -e

cd "$(git rev-parse --show-toplevel)"
exe=scratch/poptb/install/poptb.exe
if [ ! -f "$exe" ]; then
    out/poptb/extract.sh
fi

args=(
    --exe "$exe"
    --out out/poptb
)
cargo run -p tc -- "${args[@]}"
echo "cargo build --profile fast -p poptb"
