# Midtown Madness 2 progress

This is the durable handoff log for the MM2 agent loop. Each iteration should make one focused change, run a targeted check, and append a short entry here.

## Baseline

- Local input: `game/Midtown2.exe` and `game/MIDTOWN2.ICD` (ignored, user-provided)
- Format: PE32 Intel 80386 GUI executables
- `Midtown2.exe` is a loader; `MIDTOWN2.ICD` contains the main code but is encrypted on disk
- Translation input: a private decrypted PE image supplied through `MM2_INPUT`
- Translation entry point: `out/mm2/translate.sh`
- Loader hurdle: capture/reconstruct the decrypted ICD image with usable PE/import metadata
- Compiler baseline: the launcher scan found 99,382 blocks and about 93.6% code-section coverage
- Compiler blocker: `Float80` handling in `tc/src/codegen/mod.rs::mem_size` and x87 load/store codegen is resolved by the latest iteration
- Other compiler findings: two statically known jump targets were not discovered

## Ordered backlog

- [ ] Capture/reconstruct a decrypted ICD PE image from a compatible Windows runtime.
- [ ] Add an input validation/reconstruction tool or documented converter for the decrypted image.
- [x] Define and test correct handling for x87 `Float80` memory operands.
- [ ] Regenerate the decrypted MM2 image and fix the next compiler/code-generation failure.
- [ ] Compile the generated `mm2` crate and resolve generated Rust errors without masking unsupported behavior.
- [ ] Inventory static imports and implement only the missing Win32/DirectX/host behavior needed to reach startup.
- [ ] Run the translated program from `game/` and feed missing dynamic addresses back through `missing.txt`.
- [ ] Establish the first stable runtime milestone, then add and test selected Rust function overrides.
- [ ] Document a repeatable macOS run/debug workflow after the first successful launch.

## Iteration log

- Initial assessment: renamed the local installation to `game/`, added the root ignore rule, created the `out/mm2` target scaffold, and confirmed the translator reaches static discovery before failing on `Float80` code generation.
- Loader assessment: `Midtown2.exe` is a small loader and `MIDTOWN2.ICD` is a PE32 image with scrambled on-disk code. The target now requires a private decrypted/reconstructed ICD image through `MM2_INPUT`; `tc` accepts `.icd` inputs for that workflow.
- Float80 iteration: `MM2_INPUT=game/Midtown2.exe out/mm2/translate.sh` initially reproduced the `Float80` panic at `tc/src/codegen/mod.rs:190`; after adding 10-byte x87 extended-memory conversion and load/store codegen, the same command completed with 99,382 blocks. `rustfmt --edition 2024 --check runtime/src/fpu.rs runtime/src/lib.rs tc/src/codegen/mod.rs tc/src/codegen/fpu.rs && cargo check -p runtime -p tc && cargo test -p runtime -p tc` passed (10 tests). `cargo build --profile fast -p mm2` reached generated-crate compilation but failed on missing Win32 stdcall exports and unresolved `imm32`/`dplayx` modules. Next blocker: inventory and implement the missing generated Win32/DirectX imports needed for the `mm2` crate to compile.
- GetSystemDefaultLangID iteration: `cargo fmt --all -- --check` failed because the unrelated `out/winpin/src/generated.rs` module is absent; `rustfmt --edition 2024 --check win32/winapi/src/kernel32/nls.rs && cargo check -p winapi && cargo test -p winapi` passed (1 test). Added `kernel32::GetSystemDefaultLangID`, returning the US-English `LANGID` expected by the generated startup code. `cargo build --profile fast -p mm2` then failed with 232 errors (down from 234), with `user32::EnumDisplaySettingsA_stdcall` the first remaining missing export. Next blocker: implement `user32::EnumDisplaySettingsA` with the existing display-mode model rather than stubbing the generated call.
