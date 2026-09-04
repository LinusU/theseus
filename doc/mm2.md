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

The project skill `/mm2-ralph` advances one focused blocker and records the result in `doc/mm2-progress.md`. For unattended bounded iterations, start from a clean worktree and run:

```text
.devin/ralph-mm2.sh 5
```

The loop commits one successful iteration at a time, but never pushes. It stops after at most 20 iterations, when an agent makes no changes, or when an iteration reports a failure. A failed or blocked iteration is left uncommitted for review. The default permission mode is `smart`; set `DEVIN_PERMISSION_MODE` explicitly if a different local policy is appropriate.

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
