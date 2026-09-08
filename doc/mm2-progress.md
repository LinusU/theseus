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
   washed-out screen. A 2026-09-08 frame survey of the scripted London
   crash-course race found no non-content defect: the dominant artifact
   (a large grey rectangle once the car leaves the course) is a correctly
   alpha-blended translucent modal veil — probe: untextured TRIANGLEFAN,
   RHW verts (128,48)-(512,432), diffuse 0x80101f5d, SRCALPHA/INVSRCALPHA —
   whose panel contents are the known missing `nodeGetBitmap` assets; the
   bare horizon is the missing-LOD gap. Remaining: a live capture on a
   display while a human drives, to catch anything scripted input misses.
5. Sound: BLOCKED, external content. `aud\aud22\*.22k` banks are missing;
   the DirectSound path is verified end to end with `THESEUS_DSOUND_WAV`.
6. Window resolutions and fullscreen: DONE, live-verified 2026-09-08.
   Resize to 1024x707, Alt+Enter into 1440x900 and back, and
   `THESEUS_FULLSCREEN=1` at launch all hold; a real click at the window
   centre maps to guest (320, 240) in every state.
7. Draw distance and LOD: SURVEYED. Zero unhandled render states; the
   difference from the original is missing LOD content.

Shared-code audits (pointer validation, arithmetic, QueryInterface,
`Ptr::write` sweeps, `runtime::ops`, `tc` gather/codegen, `dos`) were
completed on 2026-09-07. Do not repeat them. Revisit a file only when a
reproduced failure points at it.

Suspects worth a look when the backlog is otherwise blocked:

- DONE 2026-09-08: `IDirectDraw7::Release` and
  `SetCooperativeLevel(null)` — the object is refcounted and a null hwnd
  unbinds the window.

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
- Driving the window without a human (needs Accessibility permission for
  the app running the commands): keys via `osascript -e 'tell application
  "System Events" to key code 125'`, real clicks via `out/mm2/probe-click.swift`
  (see its header), resize via `set size of window 1 to {1024, 768}` in the
  same System Events `tell process "mm2"` form. Read the mapped guest
  coordinates back from the `wm` trace's `WM_LBUTTONDOWN` lParam.

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
- 2026-09-08 item 6 live verification: What: `THESEUS_FULLSCREEN=1` was
  lost at the window swap (the first window's fullscreen exit is
  asynchronous and swallowed the second window's request); resize, Alt+Enter
  and click mapping had never been checked live. Repro: `probe-input.sh 16
  THESEUS_FULLSCREEN=1` -> real window 640x512. Change: `host::Window::close`
  leaves fullscreen and `SDL_SyncWindow`s before hiding. Check: `.devin/check.sh`
  OK; fullscreen launch 1440x900 x3 samples; resize/fullscreen/windowed clicks
  map to (320,240); arrows deliver `WM_KEYDOWN` 0x28/0x26. Next: backlog #4.
- 2026-09-08 item 4 frame survey: What: captured the scripted London
  crash-course race headless (identical pixels to windowed — software
  rasterizer) and probed the dominant artifact. Repro: `THESEUS_HEADLESS=1
  THESEUS_FRAME_DUMP=/tmp/mm2frames/f.ppm THESEUS_FRAME_DUMP_EVERY=150`
  + menu clicks + `THESEUS_INJECT_HOLD="0x26@45000+115000"` -> race runs,
  car leaves course, a grey rectangle veils the view, timer freezes at the
  lesson end. `THESEUS_PROBE=320,200` -> a single untextured TRIANGLEFAN
  (128,48)-(512,432), diffuse 0x80101f5d, alpha-blend SRCALPHA/INVSRCALPHA:
  the translucent modal veil renders correctly; its panel contents are the
  missing `nodeGetBitmap` assets. Change: this doc. Check: check.sh OK.
  Next: `SetCooperativeLevel(null)` unbind + `IDirectDraw7::Release` stub.
- 2026-09-08 ddraw object lifetime: What: `SetCooperativeLevel(NULL,
  DDSCL_NORMAL)` bound the current window instead of unbinding, and
  `IDirectDraw7::Release` was a stub so the first object never died. Repro:
  `THESEUS_TRACE=ddraw7` boot -> `SetCooperativeLevel(6b63c8, hwnd=null,
  flags=8)` then `Release(6b63c8)` before the second object is created.
  Change: `DirectDraw` gains `refs`; null hwnd unbinds; AddRef/Release on
  IDirectDraw + IDirectDraw7 are real and the last release drops the object
  (`ddraw.rs`, `ddraw1.rs`, `ddraw7.rs`); QI on the same interface AddRefs.
  Check: `check.sh: OK (fmt 3 files, clippy+test winapi, build mm2,
  whitespace)`; 25s headless smoke -> GameLoop, no missing.txt. Next:
  remaining backlog items are external content; survey audit leftovers.
- 2026-09-08 probe window enumeration: What: `probe-input.sh` printed
  `windows=none` when osascript lacked Accessibility permission, which
  reads as "the game has no window". Repro: `probe-input.sh 20` ->
  `frontmost=mm2` x5 (activation and both window focus edges fine after the
  ddraw Release change) but `windows=none`; direct osascript -> error -1728
  "not allowed assistive access". Change: `sample()` captures stderr and
  prints `no-accessibility-permission` / `no-window` instead of `none`
  (`out/mm2/probe-input.sh`). Check: `check.sh: OK (fmt 0 files, build mm2,
  whitespace)`; `probe-input.sh 8` -> `windows=no-accessibility-permission`.
  Next: SF race path is unobservable headless (LOCATION value text is a
  missing bitmap); remaining items are external content or need a human.
- 2026-09-08 rainbow noise sheets = GetSurfaceDesc: What: a London CRUISE
  roam (`RACES -> CRUISE -> SELECT VEHICLE -> GO DRIVE`, hold throttle)
  showed large translucent rainbow-noise quads around the car. Probe: the
  draws are TRIANGLEFAN particle billboards sampling one narrow region of a
  256x256 A4R4G4B4 atlas; the dumped atlas was coherent 4444 in its bottom
  half but its top half decoded correctly only as RGB565 — mixed-format
  surface data. Cause: the game calls `GetSurfaceDesc` right after
  `CreateSurface`, then `Lock`s and writes the texture; our
  `IDirectDrawSurface7::GetSurfaceDesc` reported only WIDTH|HEIGHT, so with
  no PIXELFORMAT returned the game packed its texels as 565. Change:
  `GetSurfaceDesc` now reports CAPS, PIXELFORMAT, PITCH/LPSURFACE when the
  surface has storage, MIPMAPCOUNT, BACKBUFFERCOUNT, and color keys
  (`ddraw7.rs`); `Surface::to_rgba`/`write_rgba` (the GetDC/ReleaseDC
  path) convert through the declared channel masks instead of hardcoding
  565 (`ddraw.rs`); `THESEUS_TEX_DUMP` also writes `_a.ppm` (alpha) and
  `_565.ppm` (raw 565 view) (`d3d7.rs`). Check: `check.sh: OK (fmt 3 files,
  clippy+test winapi, build mm2, whitespace)`; 2 new unit tests; the atlas
  now dumps fully coherent 4444 (foliage sprites + digit cells) and roam
  frames show soft translucent light sprites instead of noise sheets.
  Next: the giant flat pale polygon is a distant/fogged surface sampling a
  small UV window of a valid texture — likely authentic; remaining items
  are external content or need a human.
