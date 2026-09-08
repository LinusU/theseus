# Agent instructions

## Midtown Madness 2 target

The local Midtown Madness 2 installation belongs in `game/`. It is intentionally ignored by Git; never stage, copy, modify, or delete files under that directory. Each contributor must acquire their own compatible game installation.

The target scaffold is `out/mm2/`. In this installation, `game/Midtown2.exe` is the cracked/unpacked PE that Theseus can translate directly, so it is the primary input for the target. `game/MIDTOWN2.ICD` is the protected original image with scrambled on-disk code; do not pass the raw ICD to the translator. Set `MM2_INPUT` only when using another usable PE image, such as a separately reconstructed ICD image. Translation writes generated Rust, memory images, reports, and runtime feedback under `out/mm2/`; those artifacts are ignored by `out/mm2/.gitignore`.

The usual commands are:

```text
out/mm2/translate.sh
cargo build --profile fast -p mm2
THESEUS_MISSING_ADDRS=out/mm2/missing.txt cargo run --profile fast -p mm2 -- game
```

The executable is a 32-bit PE and the generated program uses the shared `runtime`, `winapi`, and `host` crates. On macOS, the native host path is SDL-backed.

## Generated-code workflow

Work on one blocker at a time within a longer agent session. Reproduce the failure, make the smallest change in the shared compiler/runtime/API code or `out/mm2` target, and run the narrowest relevant check before moving on. Keep `doc/mm2-progress.md` current with the exact command, result, and next blocker before committing each milestone.

Static function overrides use Theseus's existing `tc --extern ADDRESS[=NAME]` mechanism. Add the matching Rust function under `out/mm2/src/externs.rs`; generated code refers to it as `crate::externs::NAME`. Overrides receive `&mut runtime::Context` and return `runtime::Cont`.

Do not commit generated output, game assets, missing-address logs, or reports. Each Ralph session should process several milestones, controlled by `RALPH_AGENT_PASSES` (default 8, maximum 50), and commit every milestone's tracked source, documentation, or configuration changes with `git -c commit.gpgSign=false commit --no-gpg-sign`. Failed checks belong in `doc/mm2-progress.md`, followed by a commit so the session can continue. The loop must not push. Manual agent work remains uncommitted unless the user explicitly asks for a commit.

Before submitting a change, run the relevant `cargo fmt --all`, `cargo check`, `cargo test`, or target build command and report any environment-specific limitation.

## Physical input and window activation cannot be verified headlessly

Every `THESEUS_INJECT_VKEY`, `THESEUS_INJECT_HOLD`, and `THESEUS_INJECT_CLICK` event is synthesized inside `Host::poll`, after SDL. They exercise `user32`, DirectInput, and the game, but they can never show whether macOS delivers a real key press or click to the SDL window, whether the process is the active application, or which window has keyboard focus. Unit tests that hand-build `SDL_Event` values prove the scancode table, not delivery. A headless smoke plus injected input is therefore not evidence for any claim about live input, focus, activation, fullscreen, or resizing.

Facts established on 2026-09-08 by running the windowed build on a macOS desktop:

- SDL 3.4 defaults `SDL_HINT_MAC_BACKGROUND_APP` to `1` on macOS 14 and later, so a non-bundled process is not activated at launch; `SDL_RaiseWindow` does not compensate. The host sets the hint to `0` before `SDL_Init`. An inactive app receives no key events at all.
- `SDL_HINT_MOUSE_FOCUS_CLICKTHROUGH` makes clicks reach an inactive window. It hides activation failures rather than fixing them: if clicks work but keys do not, check activation first.
- Focus edges are delivered as the Win32 activation sequence on both sides (`WM_ACTIVATEAPP`, `WM_ACTIVATE`, `WM_SETFOCUS`/`WM_KILLFOCUS`).
- The game creates a first 640x480 popup window, binds a DirectDraw object to it, destroys the window, then creates the real one. `IDirectDraw7::Release` is a stub, so that DirectDraw object keeps the first `user32::Window` (and its SDL window) alive.

Rules for input, focus, and display work:

- Reproduce and verify with `out/mm2/probe-input.sh` on a machine with a display. It runs the windowed build for a fixed time, samples the frontmost application and the game's windows, and summarizes the focus and key messages the guest received. A claim that a physical-input item is fixed must quote that script's output showing `frontmost=mm2` and `WM_KEYDOWN` entries.
- If no display is available, say so in `doc/mm2-progress.md` and leave the item open. Do not mark it verified from injected input, and do not close it by adding a hint that bypasses the symptom.
- macOS has no `timeout` command by default; the probe script has its own timer. Prefer it over ad-hoc `timeout` invocations for windowed runs.
