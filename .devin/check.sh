#!/bin/bash
# Pre-commit check for the MM2 loop: format, lint, test, and build only
# what the working tree changed, then report one summary line.
#
# usage: .devin/check.sh            # checks changed files against HEAD
#        .devin/check.sh --all      # checks every crate the target uses
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

all=false
if [[ "${1:-}" == "--all" ]]; then
    all=true
fi

# Changed paths: modified, staged, and untracked (but not ignored) files.
# (macOS ships bash 3.2: no mapfile, no associative arrays.)
changed=()
while IFS= read -r f; do
    [[ -n "$f" ]] && changed+=("$f")
done < <(git status --porcelain --untracked-files=all | cut -c4- | sed 's/.* -> //')

fail() {
    printf 'check.sh: FAILED at %s\n' "$1"
    exit 1
}

# 1. Formatting, per file: `cargo fmt --all` cannot run in this workspace
# because out/winpin/src/generated.rs is absent.
rs_files=()
for f in "${changed[@]:-}"; do
    [[ "$f" == *.rs && -f "$f" && "$f" != */generated* ]] && rs_files+=("$f")
done
if (( ${#rs_files[@]} > 0 )); then
    rustfmt --edition 2024 --check "${rs_files[@]}" || fail "rustfmt (run: rustfmt --edition 2024 ${rs_files[*]})"
fi

# 2. Which crates to lint and test. mm2 is built but not linted or tested:
# its generated code is large and not ours.
crate_for() {
    case "$1" in
        dos/*) echo dos ;; exe/*) echo exe ;; host/*) echo host ;;
        logger/*) echo logger ;; runtime/*) echo runtime ;; tc/*) echo tc ;;
        win32/winapi/*) echo winapi ;; win32/derive/*) echo win32-derive ;;
    esac
}
crates=()
build_mm2=false
translate=false
for f in "${changed[@]:-}"; do
    case "$f" in
        out/mm2/*) build_mm2=true ;;
        tc/*) translate=true; build_mm2=true ;;
        runtime/*|host/*|win32/*|dos/*|exe/*) build_mm2=true ;;
    esac
    c="$(crate_for "$f")"
    if [[ -n "$c" ]]; then
        case " ${crates[*]:-} " in
            *" $c "*) ;;
            *) crates+=("$c") ;;
        esac
    fi
done
if $all; then
    crates=(runtime tc winapi host dos exe logger)
    build_mm2=true
fi

pkg_args=()
for c in "${crates[@]:-}"; do [[ -n "$c" ]] && pkg_args+=(-p "$c"); done

# 3. Lint and test the touched crates.
if (( ${#crates[@]} > 0 )); then
    cargo clippy "${pkg_args[@]}" --all-targets -- -D warnings || fail "clippy (${crates[*]})"
    # Several winapi tests use global RefCell state; running them in parallel
    # leads to "already borrowed" panics on multi-core hosts.
    RUST_TEST_THREADS=1 THESEUS_HEADLESS=1 cargo test "${pkg_args[@]}" || fail "cargo test (${crates[*]})"
fi

# 4. Regenerate when the translator changed, then build the target.
if $translate; then
    out/mm2/translate.sh || fail "out/mm2/translate.sh"
fi
if $build_mm2; then
    cargo build --profile fast -p mm2 || fail "cargo build --profile fast -p mm2"
fi

# 5. Whitespace errors stop the outer loop.
git diff --check || fail "git diff --check"
git diff --cached --check || fail "git diff --cached --check"

summary="fmt ${#rs_files[@]} files"
if (( ${#crates[@]} > 0 )); then summary+=", clippy+test ${crates[*]}"; fi
if $translate; then summary+=", translate"; fi
if $build_mm2; then summary+=", build mm2"; fi
printf 'check.sh: OK (%s, whitespace)\n' "$summary"
