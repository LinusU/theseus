#!/bin/bash
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

iterations="${1:-1}"
agent_passes="${RALPH_AGENT_PASSES:-8}"
if ! [[ "$iterations" =~ ^[1-9][0-9]*$ ]] || (( iterations > 1000 )); then
    printf 'usage: %s [sessions 1-1000]\n' "$0" >&2
    exit 2
fi
if ! [[ "$agent_passes" =~ ^[1-9][0-9]*$ ]] || (( agent_passes > 50 )); then
    printf 'RALPH_AGENT_PASSES must be between 1 and 50\n' >&2
    exit 2
fi

if [[ -n "$(git status --porcelain)" ]]; then
    printf 'working tree must be clean before starting the MM2 loop\n' >&2
    exit 1
fi

permission_mode="${DEVIN_PERMISSION_MODE:-smart}"

commit_fallback() {
    git add -A
    if git diff --cached --quiet; then
        return 1
    fi
    git -c commit.gpgSign=false commit --no-gpg-sign -m "$(cat <<EOF
Ralph iteration ${iteration} progress.

Generated with [Devin](https://devin.ai)

Co-Authored-By: Devin <158243242+devin-ai-integration[bot]@users.noreply.github.com>
EOF
)"
}

for ((iteration = 1; iteration <= iterations; iteration++)); do
    printf 'MM2 Ralph session %d/%d (%d milestone passes)\n' "$iteration" "$iterations" "$agent_passes"
    before_commit="$(git rev-parse HEAD)"
    if RALPH_AGENT_PASSES="$agent_passes" \
        GIT_CONFIG_COUNT=1 \
        GIT_CONFIG_KEY_0=commit.gpgSign \
        GIT_CONFIG_VALUE_0=false \
        devin --print --permission-mode "$permission_mode" \
        --prompt-file "$root/.devin/ralph-mm2-prompt.md"
    then
        agent_status=0
    else
        agent_status=$?
    fi
    after_commit="$(git rev-parse HEAD)"

    if [[ "$before_commit" == "$after_commit" && -n "$(git status --porcelain)" ]]; then
        if ! commit_fallback; then
            printf 'the agent left changes that could not be committed; stopping for review\n' >&2
            exit 1
        fi
        after_commit="$(git rev-parse HEAD)"
    fi

    if [[ "$before_commit" == "$after_commit" ]]; then
        if (( agent_status != 0 )); then
            printf 'the agent exited with status %d without changes; stopping\n' "$agent_status" >&2
            exit "$agent_status"
        fi
        printf 'the agent made no changes; stopping\n'
        break
    fi

    if [[ -n "$(git status --porcelain)" ]]; then
        if ! commit_fallback; then
            printf 'the agent left changes after committing that could not be committed; stopping for review\n' >&2
            exit 1
        fi
        after_commit="$(git rev-parse HEAD)"
    fi

    if [[ -n "$(git status --porcelain)" ]]; then
        printf 'the agent left uncommitted changes after fallback commit; stopping for review\n' >&2
        exit 1
    fi

    commit_count="$(git rev-list --count "$before_commit..$after_commit")"
    if (( commit_count < 1 )); then
        printf 'the iteration produced no commit; stopping for review\n' >&2
        exit 1
    fi
    while IFS= read -r commit; do
        signature_status="$(git log -1 --format='%G?' "$commit")"
        if [[ "$signature_status" != "N" ]]; then
            printf 'the iteration produced a signed commit (%s); stopping for review\n' "$signature_status" >&2
            exit 1
        fi
    done < <(git rev-list "$before_commit..$after_commit")
    if ! git diff --check "$before_commit" "$after_commit"; then
        printf 'the iteration contains whitespace errors; stopping for review\n' >&2
        exit 1
    fi
    if (( agent_status != 0 )); then
        printf 'the agent exited with status %d; committed its changes and continuing\n' "$agent_status" >&2
    fi
done
