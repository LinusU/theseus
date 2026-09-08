# Midtown Madness 2

This target uses Theseus to translate the 32-bit Windows executable from a local Midtown Madness 2 installation into Rust that runs through Theseus's native macOS host.

## Local inputs

Acquire a compatible copy of the game independently and place it at:

```text
game/
```

The directory is ignored by Git. This installation includes a cracked/unpacked `game/Midtown2.exe`, which is the usable PE input for Theseus. `game/MIDTOWN2.ICD` is the protected original image with scrambled code on disk; it is retained as part of the installation but is not needed by the current translation path. The data files remain in `game/` so the generated program can run with the game directory as its working directory. Set `MM2_INPUT` only when intentionally translating another usable PE image.

## Pipeline

```text
game/Midtown2.exe (cracked/unpacked PE)
        |
        v
out/mm2/translate.sh
        |
        v
theseus compiler (tc)
        |
        +--> out/mm2/src/generated.rs and generated/parts
        +--> out/mm2/data/*.raw
        +--> out/mm2/report.html
        v
cargo build --profile fast -p mm2
        |
        v
runtime + winapi + host/SDL on macOS
```

Generated files under `out/mm2/` are ignored. The target scaffold, translation script, override module, and configuration remain tracked.

## Current assessment

The two files are PE32 Intel 80386 GUI binaries. In this cracked installation, `Midtown2.exe` is the usable code-bearing PE that Theseus can translate directly. `MIDTOWN2.ICD` has the matching image base and imports but its `.text` bytes are scrambled on disk, so the raw ICD is not a valid translation input. The cracked executable effectively bypasses the original protected loader path for this port.

The initial translation of `Midtown2.exe` discovered 99,382 blocks covering about 93.6% of its code section. It first stopped on an x87 `Float80` code-generation gap, which the agents fixed. The generated snapshot then built and reached the main menu, lobby paths, London, and multiple race modes on macOS. This was not a decrypted-ICD extraction; it was translation of the cracked executable already present in `game/`.

The current port therefore has one primary path:

1. Translate the cracked `game/Midtown2.exe` with `out/mm2/translate.sh`.
2. Build and run the generated native target through Theseus's runtime, Win32 implementation, and SDL host.
3. Feed any dynamically missing addresses back through `missing.txt`, then add focused Rust overrides once a suitable target function is identified.

Reconstructing a decrypted ICD image remains an optional compatibility/research path for comparing against the protected release. It is not required to regenerate or run the current cracked-installation target. If an alternate usable PE is later supplied, set `MM2_INPUT` to that file. The checked-in workflow does not redistribute game binaries or protection keys.

## Agent loop

The project skill `/mm2-ralph` runs several focused milestones in one agent session and records each result in `doc/mm2-progress.md` (a short status board plus recent log; older history is in `doc/mm2-progress-archive.md`). `.devin/check.sh` is the pre-commit check: it formats changed Rust files, runs clippy and tests for the touched crates, rebuilds the target, and checks whitespace. Each spawned session uses `RALPH_AGENT_PASSES` (default 8, maximum 50) and commits after every milestone, including milestones whose builds or tests fail. For unattended bounded sessions, start from a clean worktree and run:

```text
RALPH_AGENT_PASSES=8 .devin/ralph-mm2.sh 1000
```

The outer loop can launch up to 1,000 such sessions. It also creates a generic fallback commit for uncommitted changes left by an agent, including changes from a failed build or test. It never pushes. The default permission mode is `smart`; set `DEVIN_PERMISSION_MODE` explicitly if a different local policy is appropriate.

## Function overrides

The translator already supports replacing a block at a known address:

```text
--extern 005xxxxx=my_override
```

The generated block table then calls `crate::externs::my_override`. An override has the form:

```rust
use runtime::{Cont, Context};

pub fn my_override(ctx: &mut Context) -> Cont {
    todo!()
}
```

Use the runtime context helpers to read/write emulated memory and registers, and return a continuation consistent with the original calling convention. Keep overrides small and add a regression or runtime reproduction whenever possible.

## Useful commands

```text
out/mm2/translate.sh
cargo build --profile fast -p mm2
THESEUS_MISSING_ADDRS=out/mm2/missing.txt cargo run --profile fast -p mm2 -- game
```

## macOS run/debug workflow

The translated program currently launches, navigates the menus, loads the
London level, and runs multiple race types with the software Direct3D
rasterizer. The cracked `game/Midtown2.exe` is the reproducible translation
input, while the generated snapshot under `out/mm2/` remains ignored. Rerun
`translate.sh` after acquiring the installation; use `MM2_INPUT` only for an
alternate usable PE image.

Headless runs drive the game with synthesized input and frame dumps:

```text
RUST_LOG=warn THESEUS_HEADLESS=1 \
THESEUS_FRAME_DUMP=/tmp/mm2.ppm \
THESEUS_INJECT_AT_MS=5000 \
THESEUS_INJECT_VKEY=0x0d,0x0d,0x28,0x28,0x28,0x28,0x0d \
THESEUS_INJECT_CLICK="540,450;530,440" \
THESEUS_INJECT_CLICK_MS=15000 THESEUS_INJECT_CLICK_GAP=2000 \
THESEUS_INJECT_HOLD=0x26@45000 \
timeout 150 ./target/fast/mm2 game
```

The sequence above still reaches a London race, but the observed focus
model is a button panel, not a list: the main screen shows `SELECT DRIVER`
on the left (CREATE NEW DRIVER / DELETE THIS DRIVER / DRIVER'S STATS /
RACE RECORDS) and a right column of CRASH COURSE, RACES, MULTIPLAYER,
QUICK RACE. Down-arrow taps move focus down that column and Enter
activates it: `0x0d` picks CRASH COURSE, `0x28,0x0d` RACES,
`0x28,0x28,0x0d` MULTIPLAYER, and `0x28,0x28,0x28,0x0d` QUICK RACE. The
click sequence then goes through `SELECT VEHICLE` and `GO DRIVE`, and the
hold is the accelerator (Up arrow) from 45s on. Without injection the game
idles on the main menu.

The SDL window is resizable: the guest keeps its logical client size while
present letterboxes the frame into the window (or fullscreen) preserving
aspect, and mouse coordinates map back to guest space. `THESEUS_FULLSCREEN=1`
starts in native fullscreen and Alt+Enter toggles it at runtime; the chord is
consumed by the host and never reaches the game.

The host asks macOS to activate the process at launch by setting SDL's
`SDL_HINT_MAC_BACKGROUND_APP` to `0` before `SDL_Init`. SDL 3.4 defaults that
hint to `1` on macOS 14 and later, which leaves a non-bundled binary behind the
terminal it was started from: an inactive app receives no keyboard events at
all, while clicks still reach the window through the click-through hint. Focus
changes are delivered as the Win32 activation sequence (`WM_ACTIVATEAPP`,
`WM_ACTIVATE`, `WM_SETFOCUS`/`WM_KILLFOCUS`) on both edges.

Physical input can only be checked with a real window. `out/mm2/probe-input.sh
[seconds]` runs the windowed build, samples which application is frontmost and
which windows the game owns, and summarizes the focus and key messages the
guest received; click the window and press keys while it runs. Injected input
(`THESEUS_INJECT_*`) enters after SDL and cannot show whether macOS delivers
keys at all.

Debug environment knobs, all optional:

- `THESEUS_HEADLESS=1` — run without an SDL window.
- `THESEUS_FULLSCREEN=1` — start the SDL window in native fullscreen.
- `THESEUS_FRAME_DUMP=<path>` — write the presented frame as a PPM on every
  flip; `THESEUS_FRAME_DUMP_EVERY=<n>` writes `path.NNNNN.ppm` every n frames
  instead.
- `THESEUS_FLIP_DUMP=<path>` / `THESEUS_FLIP_DUMP_AT=<n>` — dump the raw
  back-buffer pixels at the nth flip.
- `THESEUS_SRC_DUMP=<path>` / `THESEUS_SRC_DUMP_AT=<n>` — dump the nth
  640x480 blit source.
- `THESEUS_TEX_DUMP=<dir>` — write each bound 16bpp texture once as a PPM.
- `THESEUS_NO_ZTEST=1` — disable the z-test for depth debugging.
- `THESEUS_LINE_DEBUG=1` — log every rasterized line segment's endpoints and
  the render-state gates (z-test, alpha test, blend) for line-primitive
  debugging.
- `THESEUS_INJECT_VKEY` / `THESEUS_INJECT_AT_MS` — comma-separated hex VK
  codes tapped 300ms apart starting at the given millisecond. The harness
  maps the usual menu keys plus letters A–Z and digits 0–9 to PC set-1
  scancodes, so scripted text entry works (letters arrive lowercase, no
  shift state).
- `THESEUS_INJECT_CLICK` / `THESEUS_INJECT_CLICK_MS` /
  `THESEUS_INJECT_CLICK_GAP` — `;`-separated `x,y` left-clicks (move, down,
  up 50ms apart) with a configurable gap. An `x,y@ms` suffix overrides the
  `CLICK_MS + i*GAP` schedule with an absolute time for that click.
- `THESEUS_INJECT_HOLD` — `;`-separated `vkey@down_ms[+up_ms]` entries that
  hold keys for gameplay input.
- `THESEUS_MISSING_ADDRS=<path>` — append dynamically reached but
  untranslated addresses for the next `--entry-points-file` pass.
- `THESEUS_PROBE=x,y[,w,h]` — log every rasterized draw that writes the
  given render-target pixel (or rectangle) plus the draw's render state,
  texture, and vertex data; each distinct bound texture is dumped once to
  `/tmp/probe_tex_<addr>.ppm`.
- `THESEUS_NO_CLEAR_SKIP=1` — disable the optimization that skips a
  mid-scene `Clear` once geometry has already landed on the render target.
- `THESEUS_DSOUND_WAV=<path>` — write the DirectSound mixer's 44.1 kHz
  stereo output as a WAV file for offline inspection.
- `THESEUS_AUD_CACHE=<dir>` — redirect guest file access under `aud\` (or
  `aud22\`) to a writable host directory, for titles that generate
  on-demand audio caches (`.22k`/`.dls`) without touching the read-only
  game install.
- `THESEUS_TRACE=<rules>` — `log::info!` a `<ret> Name(arg=…)` line for
  each winapi export whose defining source path contains a comma-separated
  key (`kernel32/file` selects every export in `kernel32/file.rs`); prefix
  a key with `-` to disable or `+` to re-enable it, and the last matching
  rule wins. The records land at `info`, so pair it with a `RUST_LOG`
  spec that includes `info` for the `winapi` target.
- `RUST_LOG` — env-filter-style logging: a bare level (`warn`, `debug`) or
  comma-separated `target=level` directives (`warn,winapi=debug`); the
  draw-call and rasterizer diagnostics live at `debug`, honest problems at
  `warn`/`error`.

Known non-blocking noise: missing `.CHK` checksum caches (including the
game's own `MM2AUD.CHKHK` name-mangling bug, faithfully reproduced), absent
`aud\aud22`/`aud\dmusic` content, `nodeGetBitmap` art misses, LOD warnings,
the `london` room-count version warning, `datParser::Read` warnings for
unrecognized race/city tokens like `Approach` and `Ocean`, `DMusicObject`
scan failures for missing segment directories, and benign `SelectObject`
diagnostics for game-internal handles.

## External prerequisites that cannot be filled from this repo

Several categories of content are intentionally owned by the original game
installation and are not present in the checked-in `game/` tree. They cause
warnings in the run log but are outside the scope of the shared port code:

- A complete `aud\aud22\*` 22kHz sound bank for UI, vehicle, ambient,
  creature, and surface audio. The `mm2aud.ar` archive ships the `.22k.wav`
  source files, but the game expects pre-compiled `.22k` banks in
  `aud\aud22\`; this installation has an empty `aud\aud22\` directory, so
  `CreateBankManager` cannot build the banks and the game logs
  `Could not create ... .22k for agesound` for every referenced sample.
- DirectMusic content under the paths the game scans (`.dmusic`/`.wav`
  segments referenced by the `DMusicObject::ScanDirectory` path).
- `.CHK` checksum caches next to the `.AR` archives, which the loader looks
  for but does not require.
- Medium and low LOD meshes for many vehicle and prop models; only the
  highest LOD is present in the local installation.
- Description bitmaps for several UI nodes (`ulock_amvpcab`, `vpcab_desc`,
  `ama_rank_desc`), which the game requests through `nodeGetBitmap()`. The
  current `mm2tex.ar` does ship `texture/*_DESC.tga` for some vehicles
  (`VPBUG`, `VPBULLET`, `VPBUS`, `VPCADDIE`, `VPCOP`, `VPFORD`, `VPMUSTANG99`,
  `VPPANOZGT`, `VPPANOZG`, `VPPANOZ`, `VPSEMI`) but not for others, so the
  SELECT VEHICLE description panel works for e.g. the Mustang but cannot render
  labels that depend on the missing `*_DESC` assets.
- `datParser` race/city configuration tokens that the game's own parser does not
  recognize, such as `Approach` and `Ocean`. These tokens are logged and skipped
  by the parser; they may come from a data format or patch revision that differs
  from the executable's parser table.

The missing `nodeGetBitmap` assets are the direct cause of the translucent
overlay panels not showing their text labels, and the absent audio/DirectMusic
content is the source of most of the runtime warnings. The port handles the
missing files gracefully and the race loop is otherwise stable.

The current target can be fully regenerated from the cracked
`game/Midtown2.exe`. A decrypted `MIDTOWN2.ICD` PE image with valid imports is
only needed for an optional comparison against the protected release, not for
the working macOS port.
