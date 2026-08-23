#!/bin/bash
set -e

cd "$(git rev-parse --show-toplevel)"
args=(
    --exe scratch/hello/hello6.0.exe
    --out out/hello6
)
cargo run -p tc -- "${args[@]}"
echo cargo build --profile fast -p hello6
