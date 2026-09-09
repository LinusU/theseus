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
    --scan-immediates
    --scan-memory
    --entry-points-file out/poptb/entry-points.txt
    --jump-table 56b368..56c364
)
cargo run -p tc -- "${args[@]}"
echo "cargo build --profile fast -p poptb"
