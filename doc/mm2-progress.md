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
3. Missing UI text: DONE 2026-09-12. `DrawTextA` now rasterizes the
   built-in bitmap font into the DC's bitmap (shared core with
   `TextOutA`); a headless frame shows the driver-info panel text
   ("RANKING: AMATEUR", "LAST RACE: LONDON'S CALLING", ...).
4. In-race graphical glitches: PARTIAL. `FOGENABLE` gating fixed the
   washed-out screen. A 2026-09-08 frame survey of the scripted London
   crash-course race found no non-content defect: the dominant artifact
   (a large grey rectangle once the car leaves the course) is a correctly
   alpha-blended translucent modal veil — probe: untextured TRIANGLEFAN,
   RHW verts (128,48)-(512,432), diffuse 0x80101f5d, SRCALPHA/INVSRCALPHA —
   whose panel contents are the known missing `nodeGetBitmap` assets; the
   bare horizon is the missing-LOD gap. Remaining: a live capture on a
   display while a human drives, to catch anything scripted input misses.
5. Sound: DONE 2026-09-09. The banks were always in `mm2aud.ar`; the game
   never opened DirectSound because enumeration listed only the NULL-GUID
   primary driver. UI clicks now produce audio (mixer WAV peaks at the
   click times). Only `UIreplay.22k` is genuinely absent. Music is a
   separate lead: `aud/dmusic` content ships, but no DirectMusic object is
   created in the first 25 s; trace the in-race path.
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

Before calling anything "missing content", grep the archive directories
(`head -c 4000000 game/mm2aud.ar | strings | grep -i <name>`); the retail
install is complete, and every such claim so far has been a Theseus gap.

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
- 2026-09-08 texture-stage survey: What: whether the flat pale polygon is a
  missing second texture stage (the rasterizer samples stage 0 only).
  Change: `SetTexture` warns once per bound stage>0 (`d3d7.rs`). Repro:
  70s London CRUISE roam -> no warning; MM2 binds stage 0 only, so the
  polygon is a correctly-bound surface, not a multitexture gap. Check:
  `check.sh: OK (fmt 1 files, clippy+test winapi, build mm2, whitespace)`.
  Next: external content or a fresh defect report.
- 2026-09-09 GetGDISurface correctness: What: `IDirectDraw7::GetGDISurface` ignored
  its `this` pointer and returned the first window-backed surface in the global
  table, which could hand the game a stale or wrong primary. Repro: new unit test
  `get_gdi_surface_returns_not_found_without_a_matching_primary` plus the code path
  in `ddraw7.rs`. Change: `GetGDISurface` now looks up the current DirectDraw
  object's window and requires a matching `PRIMARYSURFACE` (`ddraw7.rs`); test
  added (`ddraw.rs`). Check: `check.sh: OK (fmt 2 files, clippy+test winapi,
  build mm2, whitespace)`. Next: scripted race still stalls at `Just before
  GameLoop`; diagnose the menu/GO DRIVE path or run a longer live capture.
- 2026-09-09 vkey absolute-time suffixes: What: `THESEUS_INJECT_VKEY` only
  supported a single 300ms-spaced phase, so a scripted run could not press a
  key after a long loading screen. Repro: new unit tests
  `vkey_schedule_supports_absolute_time_suffixes` and
  `vkey_schedule_is_300ms_apart_by_default` in `host/src/sdl.rs`. Change:
  `Host::parse_vkey_schedule` parses optional `vkey@<ms>` absolute-time
  suffixes per key; `THESEUS_INJECT_AT_MS` now defaults to 0 (`host/src/sdl.rs`).
  Check: `check.sh: OK (fmt 1 files, clippy+test host, build mm2, whitespace)`.
  Next: use `0x0d@260000` and `0x26@270000` in a 300s headless race to press
  start and throttle after GameLoop; then survey frames.
- 2026-09-09 vkey @ms race smoke and docs: What: `doc/mm2.md` did not describe
  the new `vkey@ms` syntax, and a long race was needed to confirm the car still
  drives with the vkey parser changes. Repro:
  `THESEUS_INJECT_AT_MS=5000 THESEUS_INJECT_VKEY=0x0d,0x0d,0x28,0x28,0x28,0x28,0x0d
  THESEUS_INJECT_CLICK="540,450;530,440" THESEUS_INJECT_CLICK_MS=15000
  THESEUS_INJECT_CLICK_GAP=2000 THESEUS_INJECT_HOLD=0x26@45000
  THESEUS_FRAME_DUMP_EVERY=500` for 240s. Result: reached GameLoop, the car left
  the start line and drove off the course (HUD timer 00:25:00 -> 00:04:16); the
  only visible artifacts are the same alpha veil and missing LOD/water already
  attributed to absent content. Change: documented `@ms` vkey suffix and default
  `THESEUS_INJECT_AT_MS` in `doc/mm2.md`. Check: `check.sh: OK (fmt 0 files,
  whitespace)`. Next: external content or a fresh runtime/DirectDraw audit.
- 2026-09-09 surface QueryInterface AddRef: What: `IDirectDrawSurface7::QueryInterface`
  returned the surface pointer without AddRefing it, so a caller that released the
  new pointer could drop the object too early. Repro: new unit test
  `surface_query_interface_addrefs` in `win32/winapi/src/ddraw/ddraw.rs` (refs
  stayed 1 before the fix). Change: `ddraw7.rs` now increments `surface.refs` for
  matching IIDs and exports `IID_IDIRECTDRAWSURFACE7` for tests; added the test.
  Check: `check.sh: OK (fmt 2 files, clippy+test winapi, build mm2, whitespace)`.
  Next: other COM ref-count leaks in ddraw/d3d7 (`GetDDInterface`, `IDirect3D7`
  and `IDirect3DDevice7` AddRef/Release).
- 2026-09-09 GetDDInterface AddRef: What: `IDirectDrawSurface7::GetDDInterface` returned
  the DirectDraw pointer without AddRefing it. Repro: new unit test
  `get_dd_interface_addrefs` in `win32/winapi/src/ddraw/ddraw.rs` (refs stayed 1
  before the fix). Change: `ddraw7.rs` now AddRefs the DirectDraw object and
  rejects a null output pointer; added the test. Check: `check.sh: OK (fmt 2
  files, clippy+test winapi, build mm2, whitespace)`. Next: `IDirect3D7` and
  `IDirect3DDevice7` AddRef/Release stub leaks.
- 2026-09-09 IDirect3D7 ref counting: What: `IDirect3D7::AddRef` and `Release`
  returned stub constants (`1` and `0`) and never freed the object, so a
  `QueryInterface`/`Release` pair from `IDirectDraw7::QueryInterface` would
  leak. Repro: new unit test `d3d7_object_lifetime_addref_release` in
  `win32/winapi/src/ddraw/d3d7.rs` (refs stayed 1 and the heap block leaked
  before the fix). Change: `d3d7.rs` now tracks `d3d7_objects` ref counts,
  AddRefs on `QueryInterface` and `AddRef`, and frees the heap block on the
  last `Release`; updated `.devin/check.sh` to run `RUST_TEST_THREADS=1` so the
  new test does not race the existing global-RefCell tests. Check:
  `check.sh: OK (fmt 1 files, clippy+test winapi, build mm2, whitespace)`.
  Next: `IDirect3DDevice7` and `IDirect3DVertexBuffer7` AddRef/Release.
- 2026-09-09 IDirect3DDevice7 ref counting: What: `IDirect3DDevice7::AddRef`
  and `Release` returned stub constants and never freed the object. Repro: new
  unit test `d3d7_device_lifetime_addref_release` in
  `win32/winapi/src/ddraw/d3d7.rs`. Change: added `refs` to `Device`, set it to
  1 in `CreateDevice`, AddRef/Release through `d3d_state().devices`, and free
  the heap block on the last `Release`; `QueryInterface` AddRefs for `IUnknown`
  or `IID_IDIRECT3DDEVICE7`. Check: `check.sh: OK (fmt 1 files, clippy+test
  winapi, build mm2, whitespace)`. Next: `IDirect3DVertexBuffer7` AddRef/Release.
- 2026-09-09 IDirect3DVertexBuffer7 ref counting: What:
  `IDirect3DVertexBuffer7::AddRef` and `Release` returned stub constants and
  never freed the object or its vertex data. Repro: new unit test
  `d3d7_vertex_buffer_lifetime_addref_release` in
  `win32/winapi/src/ddraw/d3d7.rs`. Change: added `refs` to `VertexBuffer`, set
  it to 1 in `CreateVertexBuffer`, AddRef/Release through
  `d3d_state().vertex_buffers`, and free the vtable and data blocks on the last
  `Release`; `QueryInterface` AddRefs for `IUnknown` or
  `IID_IDIRECT3DVERTEXBUFFER7`. Check: `check.sh: OK (fmt 1 files, clippy+test
  winapi, build mm2, whitespace)`. Next: a 90s headless race smoke reached
  `Just before GameLoop` and `GameLoop` with no `out/mm2/missing.txt`; no d3d7
  COM regressions. The remaining blockers are external content
  (`.CHK`, `aud\\aud22\\*.22k`, DirectMusic, UI description bitmaps, LODs).
- 2026-09-09 DirectDraw cross-version QueryInterface: What: the v1
  `IDirectDraw::QueryInterface` returned E_NOINTERFACE for every IID — the
  standard `DirectDrawCreate` + `QueryInterface(IID_IDirectDraw7)` upgrade
  path (the `chillin` target calls v1 `DirectDrawCreate`) could never reach
  the v7 interface — and no ddraw QI matched the canonical `IID_IUnknown`
  ({00000000-0000-0000-C000-000000000046}). Repro: new test
  `ddraw1_query_interface_upgrades_to_ddraw7` -> E_NOINTERFACE before the
  fix. Change: `DirectDraw` gains `aliases` (extra interface pointers sharing
  the object's ref count), `get_ddraw`/`Release` accept any of them
  (`ddraw.rs`, `mod.rs`, `ddraw7.rs`); v1 QI answers IUnknown+IID_IDirectDraw
  with `this` and IID_IDirectDraw7 with a new v7-vtable pointer, and
  `IDirectDraw7::QI` answers IID_IDirectDraw symmetrically (`ddraw1.rs`,
  `ddraw7.rs`); 2 new tests. Check: `check.sh: OK (fmt 4 files, clippy+test
  winapi, build mm2, whitespace)`. Next: same treatment for surfaces
  (`IDirectDrawSurface` v1 QI rejects everything) and palette ref counting.
- 2026-09-09 surface cross-version QueryInterface: What: the v1
  `IDirectDrawSurface::QueryInterface` returned E_NOINTERFACE for every IID,
  and `IDirectDrawSurface7::QueryInterface` did not answer the v1 surface IID,
  so a surface could never be queried across interface versions. Repro: new
  test `surface_query_interface_crosses_versions` -> E_NOINTERFACE before the
  fix. Change: new shared helpers `surface_alias` (hands out a same-object
  interface pointer of the other version, registered as a second map entry)
  and `release_surface` (last release drops every interface-pointer entry via
  `Rc::ptr_eq`) in `ddraw.rs`; both surface QIs answer canonical+null
  IUnknown, their own IID, and the other version's IID (`ddraw1.rs`,
  `ddraw7.rs`). Check: `check.sh: OK (fmt 3 files, clippy+test winapi, build
  mm2, whitespace)`. Next: `IDirectDrawPalette` has no ref counting at all —
  `AddRef` returns a constant 1 and `Release` frees unconditionally.
- 2026-09-09 palette ref counting: What: `IDirectDrawPalette` ignored COM
  lifetimes — `AddRef` returned a constant 1, `Release` freed the object
  unconditionally, `QueryInterface` refused even its own IID, `GetPalette`
  did not AddRef, and `SetPalette` did not hold a reference so an app
  releasing its own pointer killed a palette a surface still used. Repro:
  new test `palette_lifetime_addref_release_and_attachment`. Change:
  `Palette` gains `refs`; shared `set_palette`/`release_palette_ref` helpers
  in `ddraw.rs` hold/drop the attachment reference (dropped on replace and
  on surface death in `release_surface`); palette QI answers IUnknown and
  IID_IDirectDrawPalette; `GetPalette` AddRefs (`ddraw1.rs`, `ddraw7.rs`).
  Check: `check.sh: OK (fmt 3 files, clippy+test winapi, build mm2,
  whitespace)`. Next: survey a headless run for remaining warn-level stubs.
- 2026-09-09 EnumSurfaces dedupes interface aliases: What: `EnumSurfaces`
  walked `state().surf` keys, and after the cross-version QI change one
  object can hold several keys, so it would be enumerated once per interface
  pointer. Repro: new test `live_surfaces_dedupes_interface_aliases` -> 3
  entries for 2 objects. Change: shared `live_surfaces()` snapshots one entry
  per object (`Rc::ptr_eq` dedup) and both `EnumSurfaces` report the
  canonical `surface.addr` while still skipping objects a callback released
  mid-walk (`ddraw.rs`, `ddraw1.rs`, `ddraw7.rs`). Check: `check.sh: OK (fmt
  3 files, clippy+test winapi, build mm2, whitespace)`. Next: 30s headless
  smoke is clean (GameLoop, no missing.txt); remaining backlog is external
  content or needs live input.
- 2026-09-09 DirectSound COM lifetimes: What: `IDirectSound::QueryInterface`
  returned DSERR_INVALIDPARAM for every IID, `AddRef`/`Release` returned
  constants, the object was never registered anywhere, and
  `IDirectSoundBuffer::QueryInterface` refused even its own IID. Repro: new
  tests `dsound_object_lifetime_addref_release`,
  `buffer_query_interface_answers_its_iid`. Change: `dsound::State` gains an
  `objects` ref map registered by `DirectSoundCreate`; both QIs answer
  canonical+null IUnknown and their own IID (`IID_IDIRECTSOUND`,
  `IID_IDIRECTSOUNDBUFFER`) with `this` +AddRef and E_NOINTERFACE otherwise;
  `IDirectSound` AddRef/Release count real refs and the last Release frees
  the interface block (`dsound.rs`). Check: `check.sh: OK (fmt 1 files,
  clippy+test winapi, build mm2, whitespace)`. Next: audit
  `IDirectInput`/`IDirectInputDevice` QI shape, then a live probe run.
- 2026-09-09 IDirectInput object ref counting: What: `IDirectInput`'s
  `AddRef`/`Release` returned constants and the object was never registered,
  so `QueryInterface` could not AddRef it either; the device side already
  counted real refs. Repro: new test
  `dinput_object_lifetime_addref_release`. Change: `dinput::State` gains an
  `objects` ref map registered by `DirectInputCreateA`; QI AddRefs on
  success, AddRef/Release count real refs, and the last Release frees the
  interface block (`dinput.rs`). Check: `check.sh: OK (fmt 1 files,
  clippy+test winapi, build mm2, whitespace)`. Next: live
  `out/mm2/probe-input.sh` run — a display is available in this session.
- 2026-09-09 DirectDrawEnumerate callback args + W variants: What:
  `DirectDrawEnumerateA` invoked `LPDDENUMCALLBACK` with only 3 args —
  `(desc, name, context)` where the signature is `(GUID*, desc, name,
  context)` — so the callback read its arguments shifted by one; and
  `DirectDrawEnumerateW`/`DirectDrawEnumerateExW` returned OK without ever
  calling the callback, so a W-variant enumeration saw zero adapters. Repro:
  code inspection against the `call32_x86` arg order and
  `LPDDENUMCALLBACK[A/W]` typedefs. Change: the A callback now gets the NULL
  GUID leading arg; W variants share `alloc_wstring` (UTF-16 heap strings)
  and `alloc_zeroed_guid` helpers and invoke their callbacks with the
  primary-display description/name (ddraw.rs); 2 new tests for the helpers.
  Check: `check.sh: OK (fmt 1 files, clippy+test winapi, build mm2,
  whitespace)`. Next: `out/mm2/probe-input.sh 20` ran live — `frontmost=mm2`
  for the whole run and WM_ACTIVATEAPP/WM_ACTIVATE/WM_SETFOCUS were
  delivered at t≈3.5s, so window creation and macOS activation work on this
  display; window enumeration needs Accessibility permission, and no keys
  were physically pressed so the key path stays verified-but-not-reprobed.
- 2026-09-09 IDirect3D7::EnumDevices callback — original was correct,
  reverted: What: an earlier commit today "fixed" the callback to the
  6-arg pre-DX7 `LPD3DENUMDEVICESCALLBACK` shape — wrong: DX7's
  `LPD3DENUMDEVICESCALLBACK7` is `(desc, name, LPD3DDEVICEDESC7, context)`
  (4 args, GUID lives in `D3DDEVICEDESC7::deviceGUID`). Repro: a headless
  run halted at `return_from_x86 invoked unexpectedly` right after the
  Detect line; the game's callback at `0x4ac3c0` is `ret 10h` (4 args) and
  reads param2 as the devicedesc — `dwDevCaps & 0x10000` (HW T&L) plus the
  `deviceGUID` compare at +0xC4, which `device_desc7` fills. Change:
  restored the 4-arg call and noted the signature in a comment (d3d7.rs);
  headless run reaches GameLoop again. Check: `check.sh: OK (fmt 1 files,
  clippy+test winapi, build mm2, whitespace)`; smoke run reached `Heap
  MemUsed (Just before GameLoop): 3.1M`. Lesson: verify callback arg counts
  against the guest's `ret N` before "fixing" them. Next: continue COM
  audit.
- 2026-09-09 dplayx COM ref counting: What: `IDirectPlayLobby3A` and the
  `IDirectPlay4A` (`CLSID_DirectPlay`) objects returned constant
  AddRef/Release (1/0), QueryInterface never AddRefed, and the 4-byte
  interface blocks were never freed. Repro: new tests
  `lobby_object_lifetime_addref_release` and
  `directplay_object_lifetime_addref_release`. Change: shared `OBJECTS`
  `this`->refs map registered by both `create` paths, QI AddRefs on
  success, real AddRef/Release that drop the map entry and free the heap
  block at refs=0 (`dplayx.rs`). Check: `check.sh: OK (fmt 1 files,
  clippy+test winapi, build mm2, whitespace)`. Next: survey a headless run
  for remaining warn-level stubs; dmusic objects are stubs but their
  content is missing anyway.
- 2026-09-09 dmusic COM ref counting: What: every DirectMusic stub object
  (performance, directmusic, port, music_object, persist_stream, segment,
  loader, composer) had constant AddRef/Release (1/0), QueryInterface never
  AddRefed, and `new_object` blocks were never freed — the game holds a
  global IDirectMusicPerformance and calls it every frame. Repro: new tests
  `stub_objects_count_real_references` and `custom_query_interfaces_addref_this`.
  Change: `new_object` registers each object in the shared
  `dplayx::OBJECTS` map (helpers now `pub(crate)`), the shared
  `query_interface!` and the three hand-rolled QIs AddRef `this` on
  success, and the per-module `stub!(AddRef/Release)` pairs became two
  shared `AddRef_stub`/`Release_stub` fns that free the heap block at
  refs=0 (`dmusic.rs`, `dplayx.rs`). Check: `check.sh: OK (fmt 2 files,
  clippy+test winapi, build mm2, whitespace)`; 30s headless smoke reached
  `Just before GameLoop` with only the known `.CHK`/aud22/nodeGetBitmap
  warnings and no `missing.txt`. Next: ole32/registry or a fresh defect
  report.
- 2026-09-09 IDirectInputDevice last-Release leak: What: the device's
  `release` dropped the `devices` map entry at refcount 0 but never freed
  the 4-byte interface block `CreateDevice` allocates on the process heap.
  Repro: new test `device_release_frees_the_interface_block`
  (`dinput.rs`). Change: `release` takes `ctx` and frees the block via
  `process_heap` at refs=0; refcount decrement is now saturating. Check:
  `check.sh: OK (fmt 1 files, clippy+test winapi, build mm2, whitespace)`.
  Next: remaining E_NOTIMPL sites are honest no-hardware answers
  (RunControlPanel, Escape, force feedback); survey another area or take a
  fresh defect report.
- 2026-09-09 DirectSoundEnumerateA callback: What: `DirectSoundEnumerateA`
  (dsound ordinal 2) returned `DS_OK` without invoking the callback, so the
  game's device list stayed empty. Repro: a `winapi=info` trace of a
  scripted London BLITZ run shows `ordinal2(lpCallback=5a4f90)` called at
  boot and race init; reading the generated callback at `0x5a4f90` shows it
  grows a GUID-keyed device list, tolerates a NULL lpGuid (compares a
  static GUID), and `lstrcpy`s the description into its own record, so
  heap strings need only outlive the call. Change: invoke the callback
  once via `call32_x86` with (NULL, "Primary Sound Driver", "dsound.dll",
  context), reject null-page callbacks, free the arg strings on return
  (`dsound.rs`); new test `direct_sound_enumerate_calls_the_callback`.
  Check: `check.sh: OK (fmt 1 files, clippy+test winapi, build mm2,
  whitespace)`; 40s headless smoke reached the lobby with the callback
  exercised, no panic, no `missing.txt`. Next: survey the same trace for
  other never-answered enumerations or take a fresh defect report.
- 2026-09-09 D3D7 getter AddRefs: What: `IDirect3DDevice7::GetDirect3D`,
  `GetRenderTarget`, and `GetTexture` returned interface pointers without
  AddRef, breaking the COM contract that `GetDDInterface`/`GetPalette`
  already document — a caller balancing Get/Release would free the object
  or surface early. Repro: audit found the missing AddRefs; the MM2 trace
  shows none of the three are called today, so this is contract
  correctness, not an observed failure. Change: each getter AddRefs the
  returned object's entry in `d3d7_objects`/`state().surf` before writing
  the out pointer (`d3d7.rs`); new test
  `device_getters_addref_returned_interfaces` covers all three. Check:
  `check.sh: OK (fmt 1 files, clippy+test winapi, build mm2, whitespace)`.
  Next: continue the getter/setter contract sweep (SetTexture/SetRenderTarget
  ownership) or take a fresh defect report.
- 2026-09-09 D3D7 setter ownership + surface block free: What: three leaks
  in the COM surface lifetime — `SetTexture`/`SetRenderTarget`/`CreateDevice`
  stored bound surfaces without AddRef, device `Release` never dropped the
  held texture/render-target/Direct3D references, and `release_surface`
  dropped the `state().surf` alias entries without freeing their
  heap-allocated interface blocks (each cross-version QI allocates one).
  Repro: audit of the ref paths against the `release_surface` implementation;
  extended `surface_query_interface_crosses_versions` now proves all three
  alias blocks are freed. Change: `release_surface` collects and frees every
  alias block via `Rc::ptr_eq`; `SetTexture`/`SetRenderTarget`/`CreateDevice`
  AddRef the binding and release the replaced one; device `Release` at 0
  releases held textures, the render target, and `IDirect3D7::Release`es the
  parent object (`d3d7.rs`, `ddraw.rs`). Extended
  `device_getters_addref_returned_interfaces` covers bind/get/rebind/free.
  Check: `check.sh: OK (fmt 2 files, clippy+test winapi, build mm2,
  whitespace)`; 45s headless smoke reached the lobby with only known
  missing-content warnings and no `missing.txt`. Next: the remaining
  SetTexture hot path is contract-correct now; take a fresh defect report
  or survey `dplayx`/`dmusic` for unimplemented methods the game hits.
- 2026-09-09 message paint loop: What: long headless race smokes reach
  `Just before GameLoop: 12.2M` and then the message trace showed a tight
  `PeekMessageA` loop with no `Translate`/`Dispatch`. Repro: `RUST_LOG=warn
  THESEUS_HEADLESS=1 ... 240s headless race (doc/mm2.md line 212 schedule)`
  -> repeated `PeekMessageA` at the end. Change: `paint_msg` is only
  synthesized for a visible dirty window, `pop_filtered` retires the update
  region with the removed message, and `peek`/`pop` check due timers before
  paint; added `paint_message_retires_on_pop_not_peek` and
  `timers_take_priority_over_synthetic_paint` tests
  (`win32/winapi/src/user32/message.rs`). Check: `check.sh: OK (fmt 1
  files, clippy+test winapi, build mm2, whitespace)`. Next: the run still
  stops at `Just before GameLoop: 12.2M`, now with `PeekMessageA` returning
  nothing rather than looping; diagnose whether it is waiting for a fresh
  input message, a timer, or a different event.
- 2026-09-09 race input poll diagnosis: What: the `Just before GameLoop: 12.2M`
  tail is the normal race input loop, not a stall; the game renders frames and
  the car leaves the start line when the `VK_UP` throttle hold is active. Repro:
  `RUST_LOG=warn THESEUS_HEADLESS=1 THESEUS_MISSING_ADDRS=out/mm2/missing.txt
  THESEUS_FRAME_DUMP=/tmp/mm2frames/run.ppm THESEUS_FRAME_DUMP_EVERY=500
  THESEUS_INJECT_AT_MS=5000 THESEUS_INJECT_VKEY=0x0d,0x0d,0x28,0x28,0x28,0x28,0x0d
  THESEUS_INJECT_CLICK="540,450;530,440" THESEUS_INJECT_CLICK_MS=15000
  THESEUS_INJECT_CLICK_GAP=2000 THESEUS_INJECT_HOLD=0x26@30000+90000 ./target/fast/mm2 game`
  for 120s -> frames progress and the final frame shows the race (timer 00:59:48,
  car on grass, minimap). Change: `doc/mm2-progress.md`; `doc/mm2.md` notes that
  `0x26@45000` is tied to `THESEUS_FRAME_DUMP` slowing and `0x26@30000` is safer
  for fast runs. Check: `check.sh: OK (fmt 0 files, whitespace)`. Next: remaining
  backlog is external content (audio, UI bitmaps, LOD) or live display capture.
- 2026-09-09 DirectInput GetDeviceState/GetDeviceData buffering: What:
  `GetDeviceState` ignored `cbData` and wrote the full `data_size`, and
  `GetDeviceData` ignored `cbObjectData` and always wrote the full
  `DIDEVICEOBJECTDATA`, both overrunning the caller's buffer. Repro: unit tests
  `get_device_state_honors_cbdata_and_data_size` and
  `get_device_data_honors_cbObjectData_and_buffer` set a key, call with small
  and full element sizes, and verify no bytes past the requested length are
  touched. Change: `GetDeviceState` clamps `len` to `min(data_size, cbData)`;
  `GetDeviceData` writes at most `cbObjectData` bytes per element and stops at
  `rgdod + capacity * cbObjectData`; `Host::poll` uses `main_thread.try_get()`
  so unit tests on non-main threads do not panic (`dinput.rs`,
  `host/src/sdl.rs`). Check: `check.sh: OK (fmt 1 files,
  clippy+test winapi, build mm2, whitespace)`; 90s headless race still renders
  the London Blitz scene. Next: remaining backlog is external content or a
  fresh display capture.
- 2026-09-09 sound root cause: What: the "missing .22k banks" were in
  `mm2aud.ar` all along; the game seeks to the sample's data offset, then
  gives up because no DirectSound object exists. `DirectSoundEnumerateA`
  listed only the NULL-GUID primary and the game never called
  `DirectSoundCreate`. Repro: `THESEUS_TRACE=winapi` showed `ordinal2` then
  no dsound calls. Change: enumerate a named device with a fixed GUID and
  accept that GUID (or the null GUID) in `DirectSoundCreate`
  (`win32/winapi/src/dsound.rs`); two dsound tests de-raced (shared handle,
  shared process heap). Check: `.devin/check.sh` OK; live run: 3x
  `DirectSoundCreate`, 42 buffers, warnings 42 -> 1, mixer WAV non-silent at
  the click times. Next: #3 (`DrawTextA` glyphs) or music trace.
- 2026-09-12 DrawTextA rasterizes glyphs: What: `DrawTextA` only measured,
  so the panel text the game draws into a DirectDraw surface DC (9 calls at
  startup, `SetBkMode(TRANSPARENT)` + yellow `SetTextColor`, formats
  0x824/0x800) never produced pixels. Repro: `THESEUS_HEADLESS=1
  THESEUS_FRAME_DUMP=/tmp/mm2frames/f.ppm THESEUS_FRAME_DUMP_EVERY=500` 40s
  -> driver-info panel empty. Change: extracted `TextOutA`'s glyph drawing
  into shared `gdi32::draw_text` taking an optional clip rect; `DrawTextA`
  now rasterizes each line honoring DT_CENTER/RIGHT/VCENTER/BOTTOM
  (vertical only with DT_SINGLELINE) and clips to lprc unless DT_NOCLIP
  (`gdi32/dc.rs`, `user32/misc.rs`); `fill_pixels` takes RECT bounds; new
  test `draw_text_rasterizes_glyphs_into_the_dc_bitmap`. Check: `check.sh:
  OK (fmt 2 files, clippy+test winapi, build mm2, whitespace)`; same dump
  now shows "RANKING: AMATEUR / LAST RACE: LONDON'S CALLING / LAST VEHICLE:
  LONDON CAB / CONTROLLER: KEYBOARD" in the panel. Next: #5 music —
  `aud/dmusic` content ships but no DirectMusic object is created in the
  first 25s; trace the in-race path.
- 2026-09-12 DirectMusic Style/Band classes registered: What: in-race music
  stalled before `PlaySegment`; the load loop creates `CLSID_DirectMusicStyle`
  (d2ac288a) and `CLSID_DirectMusicBand` (79ba9e00) for `IID_IDirectMusicObject`
  and both returned `REGDB_E_CLASSNOTREG`. The `6B0650&4` music-enable bit is
  set during boot (0x57 once settings register), so the gate was the classes,
  not the flag. Repro: `THESEUS_HEADLESS=1 THESEUS_INJECT_* RUST_LOG=warn,
  winapi::dmusic=debug,winapi::ole32=debug ./target/fast/mm2 game` -> segments
  stream-load then `style GetMotif`, `band CreateSegment`, `Download`, and
  `PlaySegment seg=0x11d6b58`. Change: register the DirectMusic content classes
  (Style/Band/ChordMap + the `d2ac28xx` track/segment-state family) as generic
  objects reporting their own class via `GetDescriptor` and answering
  `IPersistStream` + the matching class interface (`win32/winapi/src/dmusic.rs`,
  `ole32.rs`); tests `content_objects_report_their_class`,
  `unregistered_class_is_rejected`. Check: `check.sh: OK (fmt 2 files,
  clippy+test winapi, build mm2, whitespace)`; 105s headless race reaches
  PlaySegment, no `missing.txt`. Next: segments still empty (Load no-ops) and
  no synth path, so no audible music; decide whether to parse `.sgt` tracks.
