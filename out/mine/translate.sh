#!/bin/bash
set -e

cd "$(git rev-parse --show-toplevel)"
args=(
    --exe ~/win/rs/deploy/archive/win2k/winmine.exe
    --out out/mine
    # wndproc
    --entry-point 100180a

    # todo: gets confused by ascii text
    #--scan-immediates
)
cargo run -p tc -- "${args[@]}"
