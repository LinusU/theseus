#!/bin/bash
set -e

cd "$(git rev-parse --show-toplevel)"
bin=target/fast/poptb
if [ ! -x "$bin" ]; then
    echo "missing $bin; run: out/poptb/translate.sh && cargo build --profile fast -p poptb" >&2
    exit 1
fi
bin="$(pwd)/$bin"
cd scratch/poptb/install
exec "$bin" "$@"
