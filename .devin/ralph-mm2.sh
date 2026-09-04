#!/bin/bash
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

iterations="${1:-1}"
if ! [[ "$iterations" =~ ^[1-9][0-9]*$ ]] || (( iterations > 20 )); then
    printf 'usage: %s [iterations 1-20]\n' "$0" >&2
    exit 2
fi

if [[ -n "$(git status --porcelain)" ]]; then
    printf 'working tree must be clean before starting the MM2 loop\n' >&2
    exit 1
fi

permission_mode="${DEVIN_PERMISSION_MODE:-smart}"
for ((iteration = 1; iteration <= iterations; iteration++)); do
    printf 'MM2 Ralph iteration %d/%d\n' "$iteration" "$iterations"
    before_commit="$(git rev-parse HEAD)"
    devin --print --permission-mode "$permission_mode" \
        --prompt-file "$root/.devin/ralph-mm2-prompt.md"
    after_commit="$(git rev-parse HEAD)"

    if [[ "$before_commit" == "$after_commit" ]]; then
        if [[ -z "$(git status --porcelain)" ]]; then
            printf 'the agent made no changes; stopping\n'
            break
        fi
        printf 'the agent left uncommitted changes; stopping for review\n' >&2
        exit 1
    fi

    commit_count="$(git rev-list --count "$before_commit..$after_commit")"
    if [[ "$commit_count" -ne 1 ]]; then
        printf 'expected one commit from the agent, found %s; stopping for review\n' "$commit_count" >&2
        exit 1
    fi
    if ! git diff --check "$before_commit" "$after_commit"; then
        printf 'the agent commit contains whitespace errors; stopping for review\n' >&2
        exit 1
    fi
    if [[ -n "$(git status --porcelain)" ]]; then
        printf 'the agent left uncommitted changes after committing; stopping for review\n' >&2
        exit 1
    fi
done
