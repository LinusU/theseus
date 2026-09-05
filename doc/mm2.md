# Midtown Madness 2

This target uses Theseus to translate the 32-bit Windows executable from a local Midtown Madness 2 installation into Rust that runs through Theseus's native macOS host.

## Local inputs

Acquire a compatible copy of the game independently and place it at:

```text
game/
```

The directory is ignored by Git. `game/Midtown2.exe` is the small loader. The executable containing the game code is `game/MIDTOWN2.ICD`, but its code sections are encrypted on disk and are populated only after the original loader starts it. The translator must receive a decrypted PE image, not the raw ICD. Keep that private working artifact under `scratch/mm2/` or set `MM2_INPUT` to another ignored path. The data files remain in `game/` so the generated program can run with the game directory as its working directory.

## Pipeline

```text
game/Midtown2.exe + game/MIDTOWN2.ICD
        |
        v
runtime unpack/capture on a compatible Windows environment
        |
        v
scratch/mm2/MIDTOWN2.decrypted.exe
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

The two files are PE32 Intel 80386 GUI binaries. `Midtown2.exe` is a small loader; `MIDTOWN2.ICD` has the matching image base and imports but its `.text` bytes are scrambled on disk. A direct disassembly of the ICD starts with invalid-looking instructions, so translating the raw file is not a viable path. This is the same general shape as the documented SafeDisc loader pattern: the original loader creates the real process and decrypts the ICD image in memory.

The first translation attempt against `Midtown2.exe` was useful only as a compiler smoke test: it discovered 99,382 blocks covering about 93.6% of that file's code section, then stopped because `tc/src/codegen/mod.rs` does not handle the `Float80` memory size used by an x87 instruction. It also reported two statically known jumps whose target blocks were not discovered. Those findings remain useful compiler work, but the MM2 target must first obtain a decrypted ICD image.

The staged port has two separate phases:

1. Capture/reconstruct a decrypted ICD PE image from the original runtime-loaded process. Preserve the PE headers, loaded code/data sections, image base, entry point, and enough import metadata for `tc`; a raw process-memory dump is not automatically a valid input file.
2. Translate and run that image through Theseus. Add correct 80-bit x87 load/store representation, fix subsequent code-generation and static-analysis gaps, implement the Win32/DirectX/host behavior needed to reach startup, feed dynamic missing addresses back through `missing.txt`, and finally add focused Rust overrides.

The runtime capture phase is intentionally outside the macOS translator for now. It can be performed in a compatible Windows environment using the user's own installation, after which the resulting private image is supplied with `MM2_INPUT`. The checked-in workflow does not include or redistribute game binaries or a decryption key.

## Agent loop

The project skill `/mm2-ralph` runs several focused milestones in one agent session and records each result in `doc/mm2-progress.md`. Each spawned session uses `RALPH_AGENT_PASSES` (default 8, maximum 50) and commits after every milestone, including milestones whose builds or tests fail. For unattended bounded sessions, start from a clean worktree and run:

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
MM2_INPUT=scratch/mm2/MIDTOWN2.decrypted.exe out/mm2/translate.sh
cargo build --profile fast -p mm2
THESEUS_MISSING_ADDRS=out/mm2/missing.txt cargo run --profile fast -p mm2 -- game
```

## macOS run/debug workflow

The translated program currently launches, navigates the menus, loads the
London level, and runs a Blitz race with the software Direct3D rasterizer.
The decrypted PE input is not committed, so the checked-in workflow relies on
the local generated snapshot under `out/mm2/`; rerun `translate.sh` only when
a decrypted image is available through `MM2_INPUT`.

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

This sequence reaches `SELECT DRIVER` (Enter, Enter), switches the city list
to London (four Down presses), starts a Blitz race (Enter), clicks through
`SELECT VEHICLE` and `GO DRIVE`, then holds the accelerator (Up arrow) from
45s on. Without injection the game idles on the main menu.

Debug environment knobs, all optional:

- `THESEUS_HEADLESS=1` — run without an SDL window.
- `THESEUS_FRAME_DUMP=<path>` — write the presented frame as a PPM on every
  flip; `THESEUS_FRAME_DUMP_EVERY=<n>` writes `path.NNNNN.ppm` every n frames
  instead.
- `THESEUS_FLIP_DUMP=<path>` / `THESEUS_FLIP_DUMP_AT=<n>` — dump the raw
  back-buffer pixels at the nth flip.
- `THESEUS_SRC_DUMP=<path>` / `THESEUS_SRC_DUMP_AT=<n>` — dump the nth
  640x480 blit source.
- `THESEUS_TEX_DUMP=<dir>` — write each bound 16bpp texture once as a PPM.
- `THESEUS_NO_ZTEST=1` — disable the z-test for depth debugging.
- `THESEUS_INJECT_VKEY` / `THESEUS_INJECT_AT_MS` — comma-separated hex VK
  codes tapped 300ms apart starting at the given millisecond.
- `THESEUS_INJECT_CLICK` / `THESEUS_INJECT_CLICK_MS` /
  `THESEUS_INJECT_CLICK_GAP` — `;`-separated `x,y` left-clicks (move, down,
  up 50ms apart) with a configurable gap.
- `THESEUS_INJECT_HOLD` — `;`-separated `vkey@down_ms[+up_ms]` entries that
  hold keys for gameplay input.
- `THESEUS_MISSING_ADDRS=<path>` — append dynamically reached but
  untranslated addresses for the next `--entry-points-file` pass.
- `RUST_LOG` — standard env-filter logging; the draw-call and rasterizer
  diagnostics live at `debug`, honest problems at `warn`/`error`.

Known non-blocking noise: missing `.CHK` checksum caches (including the
game's own `MM2AUD.CHKHK` name-mangling bug, faithfully reproduced), absent
`aud\aud22`/`aud\dmusic` content, `nodeGetBitmap` art misses, LOD warnings,
the `london` room-count version warning, and benign `SelectObject` diagnostics
for game-internal handles.
