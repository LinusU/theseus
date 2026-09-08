# Midtown Madness 2 progress

Handoff log for the MM2 agent loop. Read this whole file at the start of
every milestone; it is kept short on purpose. The complete history through
2026-09-08 (about 1,200 entries) lives in `doc/mm2-progress-archive.md`:
grep it for a symbol, file, or API name to learn whether something was
already tried, and never read it end to end.

## Status board

State: `game/Midtown2.exe` translates (99,384 blocks, 93.8% code coverage),
builds with `cargo build --profile fast -p mm2`, and reaches the menus,
lobby, London and San Francisco races headless and windowed. Current runs
produce no `out/mm2/missing.txt`.

User-requested priority backlog, in order. Open items outrank every audit:

1. Single-click input: DONE. Root cause was macOS never activating the
   process plus a leaked first window; user-confirmed 2026-09-08.
2. Arrow-key driving: DONE, same fix, user-confirmed 2026-09-08.
3. Missing UI text: BLOCKED, external content. `nodeGetBitmap` assets
   `vpcab_desc`, `ulock_amvpcab`, `ama_rank_desc` are absent from
   `mm2tex.ar` (see `doc/mm2.md`). Reopen only if a screen loses text for
   an asset that does ship.
4. In-race graphical glitches: PARTIAL. `FOGENABLE` gating fixed the
   washed-out screen; remaining artifacts were attributed to missing LOD
   meshes, but no one has captured race frames on a display since. Next:
   capture representative frames windowed, isolate the first renderer
   defect that is not a content gap.
5. Sound: BLOCKED, external content. `aud\aud22\*.22k` banks are missing;
   the DirectSound path is verified end to end with `THESEUS_DSOUND_WAV`.
6. Window resolutions and fullscreen: IMPLEMENTED, NOT LIVE-VERIFIED.
   Letterboxed present, resizable window, `THESEUS_FULLSCREEN=1`, Alt+Enter.
   Verify with `out/mm2/probe-input.sh` on a display: resize, toggle
   fullscreen, confirm clicks still hit the right controls.
7. Draw distance and LOD: SURVEYED. Zero unhandled render states; the
   difference from the original is missing LOD content.

Shared-code audits (pointer validation, arithmetic, QueryInterface,
`Ptr::write` sweeps, `runtime::ops`, `tc` gather/codegen, `dos`) were
completed on 2026-09-07. Do not repeat them. Revisit a file only when a
reproduced failure points at it.

Suspects worth a look when the backlog is otherwise blocked:

- `IDirectDraw7::Release` (`win32/winapi/src/ddraw/ddraw7.rs`) is a stub, so
  the first DirectDraw object and its surfaces are never freed.
- `IDirectDraw7::SetCooperativeLevel` with a null hwnd still binds the
  current window instead of unbinding.

## Rules for a milestone

- A milestone changes code or a test, or records a reproduced failure with
  the exact command. "Session baseline", "close-out", and "verification
  sweep" entries are not milestones and must not be committed on their own.
- Run `.devin/check.sh` before every commit and paste its last line into
  the entry. It formats, lints, tests, and builds only what changed.
- Claims about physical input, focus, fullscreen, or window size need
  `out/mm2/probe-input.sh` output from a machine with a display (see
  `AGENTS.md`). Without a display, leave the item open and say so.
- Keep an entry to the template below, about six lines. Long entries are
  what made the archive unreadable.
- Never touch `game/`, generated output, `missing.txt`, or reports.

## Entry template

```text
- YYYY-MM-DD <title>: What: <one sentence>. Repro: `<command>` -> <result>.
  Change: <files>. Check: <last line of .devin/check.sh>. Next: <blocker>.
```

## Environment notes

- macOS has no `timeout`; use `out/mm2/probe-input.sh N` for windowed runs
  or `( cmd & pid=$!; sleep N; kill $pid )` for headless ones.
- `cargo fmt --all` fails because `out/winpin/src/generated.rs` is absent;
  format with `rustfmt --edition 2024 <files>` (`check.sh` does this).
- Workspace-wide test and clippy runs must exclude `winpin`,
  `win32-extract`, and `sbaitso-sbtalker`.
- Headless smoke: `RUST_LOG=warn THESEUS_HEADLESS=1
  THESEUS_MISSING_ADDRS=out/mm2/missing.txt ./target/fast/mm2 game` for
  25 s; success is `test profile loaded` in the log and no `missing.txt`.
- Scripted London Blitz race (150 s): the injection line in `doc/mm2.md`
  under "macOS run/debug workflow".
- Live input and focus: `out/mm2/probe-input.sh 40`, then click and press
  arrow keys; success is `frontmost=mm2` at every sample and `WM_KEYDOWN`
  in the summary.

## Log (newest last; move entries older than about 20 into the archive)

- 2026-09-08 macOS activation: What: the game window was never the active
  app, so macOS delivered no keys; SDL 3.4 defaults
  `SDL_HINT_MAC_BACKGROUND_APP` to 1 on macOS 14+. Repro: frontmost
  process sampled while running windowed from Terminal -> `Terminal` x4.
  Change: `host/src/sdl.rs` sets the hint to 0 before `SDL_Init`;
  `WINDOW_FOCUS_GAINED` now delivered; both focus edges post
  `WM_ACTIVATEAPP`/`WM_ACTIVATE`/`WM_SETFOCUS` or `WM_KILLFOCUS`
  (`user32/message.rs`). Check: host 10, winapi 184 tests, clippy clean,
  mm2 builds; frontmost -> `mm2` x4. Next: hidden first window.
- 2026-09-08 leaked first window: What: the game creates a 640x480 popup,
  binds DirectDraw to it, destroys it, creates the real window; the stub
  `Release` kept the first SDL window alive next to the second. Change:
  `DestroyWindow` calls new `host::Window::close` (hides the SDL window).
  Check: tests and build pass; `probe-input.sh 24` lists one window,
  `frontmost=mm2` x6. Next: user re-test.
- 2026-09-08 user confirmation: keyboard input works and the window keeps
  focus after the two changes above. Backlog #1 and #2 closed. Added
  `out/mm2/probe-input.sh`, `.devin/check.sh`, and this restructured log.
  Next: backlog #4 (capture race frames windowed) or #6 (live-verify
  resize and fullscreen with the probe).
