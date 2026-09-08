#!/bin/bash
# Live input probe for the windowed macOS build.
#
# Runs the game with a real SDL window for a fixed time while sampling which
# application macOS considers frontmost and which windows the game owns, then
# summarizes the window messages the guest actually received (focus edges,
# key presses, clicks). Click the window and press arrow keys while it runs.
#
# This is the only way to verify physical input: THESEUS_INJECT_* events are
# synthesized after SDL and can never show whether macOS delivers keys.
#
# usage: out/mm2/probe-input.sh [seconds (default 40)] [extra env, e.g. SDL_MAC_BACKGROUND_APP=1]
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

seconds="${1:-40}"
shift || true
if ! [[ "$seconds" =~ ^[1-9][0-9]*$ ]]; then
    printf 'usage: %s [seconds] [VAR=value ...]\n' "$0" >&2
    exit 2
fi
if [[ ! -x target/fast/mm2 ]]; then
    printf 'missing target/fast/mm2; run: cargo build --profile fast -p mm2\n' >&2
    exit 1
fi

log="$(mktemp -t mm2-probe).log"
printf 'game log: %s\n' "$log"
printf 'running for %ds: click the game window, then press arrow keys / Enter / Esc\n' "$seconds"

env "$@" RUST_LOG='warn,winapi=info' THESEUS_TRACE=wm \
    THESEUS_MISSING_ADDRS=out/mm2/missing.txt \
    ./target/fast/mm2 game > "$log" 2>&1 &
pid=$!
# Keep the shell quiet about the job when the timer kills it.
disown "$pid" 2>/dev/null || true

sample() {
    if ! command -v osascript >/dev/null; then
        return
    fi
    local front windows werr
    front="$(osascript -e 'tell application "System Events" to get name of first process whose frontmost is true' 2>/dev/null)"
    # Window enumeration needs Accessibility permission for the calling app;
    # without it osascript fails with -1728, which is not "no windows".
    windows="$(osascript -e 'tell application "System Events" to tell process "mm2" to get {name, position, size} of every window' 2>&1)"
    werr=$?
    if (( werr != 0 )); then
        case "$windows" in
            *"assistive access"*|*"-1728"*) windows="no-accessibility-permission" ;;
            *"Can't get process"*) windows="no-window" ;;
            *) windows="error:$windows" ;;
        esac
    fi
    printf '  t=%3ds frontmost=%-12s windows=%s\n' "$1" "${front:-?}" "${windows:-none}"
}

elapsed=0
while (( elapsed < seconds )) && kill -0 "$pid" 2>/dev/null; do
    sleep 2
    elapsed=$((elapsed + 2))
    if (( elapsed % 4 == 0 )); then
        sample "$elapsed"
    fi
done
if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null
    sleep 1
    kill -9 "$pid" 2>/dev/null
    status=timeout
else
    wait "$pid"
    status="exit $?"
fi
printf 'game: %s\n' "$status"

# The `wm` trace prints each queued MSG as a multi-line Debug dump; fold the
# message id and host time back onto one line.
awk '
    /MSG \{/ { inmsg = 1; msg = ""; t = "" }
    inmsg && /message:/ { sub(/,/, "", $2); msg = $2 }
    inmsg && /time:/ { sub(/,/, "", $2); t = $2 }
    inmsg && /^\}/ {
        inmsg = 0
        if (msg != "") print t, msg
    }
' "$log" > "$log.msgs"

name() {
    case "$1" in
        0x6) echo WM_ACTIVATE ;; 0x7) echo WM_SETFOCUS ;; 0x8) echo WM_KILLFOCUS ;;
        0x1c) echo WM_ACTIVATEAPP ;; 0x100) echo WM_KEYDOWN ;; 0x101) echo WM_KEYUP ;;
        0x104) echo WM_SYSKEYDOWN ;; 0x105) echo WM_SYSKEYUP ;; 0x200) echo WM_MOUSEMOVE ;;
        0x201) echo WM_LBUTTONDOWN ;; 0x202) echo WM_LBUTTONUP ;; *) echo "$1" ;;
    esac
}

printf '\nmessages the guest received:\n'
if [[ ! -s "$log.msgs" ]]; then
    printf '  none (no window messages were queued)\n'
else
    awk '{ print $2 }' "$log.msgs" | sort | uniq -c | sort -rn | while read -r count id; do
        printf '  %5d %s\n' "$count" "$(name "$id")"
    done
    printf '\nfocus and key timeline (ms since start):\n'
    grep -E ' 0x(6|7|8|1c|100|101|104|105)$' "$log.msgs" | head -40 | while read -r t id; do
        printf '  %7d %s\n' "$((t))" "$(name "$id")"
    done
fi

if ! grep -q ' 0x10[0145]$' "$log.msgs"; then
    printf '\nNO KEY MESSAGES: macOS did not deliver keyboard input to the game window.\n'
    printf 'If frontmost never showed mm2, activation failed (see doc/mm2.md, SDL_MAC_BACKGROUND_APP).\n'
fi
