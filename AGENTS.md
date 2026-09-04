# Agent instructions

## Midtown Madness 2 target

The local Midtown Madness 2 installation belongs in `game/`. It is intentionally ignored by Git; never stage, copy, modify, or delete files under that directory. Each contributor must acquire their own compatible game installation.

The target scaffold is `out/mm2/`. The checked-in `game/MIDTOWN2.ICD` is an encrypted-at-rest PE image whose code is populated by the original loader at runtime; do not pass it directly to the translator and mistake scrambled bytes for game code. First produce a standalone decrypted PE image with its original load layout and usable import metadata, normally at `scratch/mm2/MIDTOWN2.decrypted.exe` or another path supplied through `MM2_INPUT`. Translation writes generated Rust, memory images, reports, and runtime feedback under `out/mm2/`; those artifacts are ignored by `out/mm2/.gitignore`.

The usual commands are:

```text
MM2_INPUT=scratch/mm2/MIDTOWN2.decrypted.exe out/mm2/translate.sh
cargo build --profile fast -p mm2
THESEUS_MISSING_ADDRS=out/mm2/missing.txt cargo run --profile fast -p mm2 -- game
```

The executable is a 32-bit PE and the generated program uses the shared `runtime`, `winapi`, and `host` crates. On macOS, the native host path is SDL-backed.

## Generated-code workflow

Work on one blocker at a time within a longer agent session. Reproduce the failure, make the smallest change in the shared compiler/runtime/API code or `out/mm2` target, and run the narrowest relevant check before moving on. Keep `doc/mm2-progress.md` current with the exact command, result, and next blocker before committing each milestone.

Static function overrides use Theseus's existing `tc --extern ADDRESS[=NAME]` mechanism. Add the matching Rust function under `out/mm2/src/externs.rs`; generated code refers to it as `crate::externs::NAME`. Overrides receive `&mut runtime::Context` and return `runtime::Cont`.

Do not commit generated output, game assets, missing-address logs, or reports. Each Ralph session should process several milestones, controlled by `RALPH_AGENT_PASSES` (default 8, maximum 50), and commit every milestone's tracked source, documentation, or configuration changes with `git -c commit.gpgSign=false commit --no-gpg-sign`. Failed checks belong in `doc/mm2-progress.md`, followed by a commit so the session can continue. The loop must not push. Manual agent work remains uncommitted unless the user explicitly asks for a commit.

Before submitting a change, run the relevant `cargo fmt --all`, `cargo check`, `cargo test`, or target build command and report any environment-specific limitation.
