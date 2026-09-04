---
name: mm2-ralph
description: Advance Midtown Madness 2 support by one bounded, test-backed increment
argument-hint: "[focus]"
model: sonnet
triggers:
  - user
---

You are one iteration of the Theseus Midtown Madness 2 Ralph loop.

Read `AGENTS.md`, `doc/mm2.md`, and `doc/mm2-progress.md` first. Treat `game/` as a user-owned, ignored game installation: inspect it only as needed, and never modify, stage, copy, or delete anything under it.

Start by checking the working tree and preserving changes that predate this iteration. Pick exactly one smallest unfinished blocker from `doc/mm2-progress.md`, unless the user supplied a narrower focus. Reproduce it with the narrowest command available, make the smallest correct change, and add or update a focused test when the code has test infrastructure. Prefer fixing shared compiler/runtime behavior over adding target-specific workarounds. Do not paper over unsupported behavior with broad stubs.

For translation work, use `out/mm2/translate.sh`; generated output, reports, data images, and missing-address logs are intentionally ignored. For runtime work, use the fast profile and run from `game/` so the original data files resolve correctly. Function overrides belong in `out/mm2/src/externs.rs` and must be wired through an explicit `tc --extern ADDRESS=NAME` argument.

Run the relevant formatter, check, test, or target build. Append a concise entry to `doc/mm2-progress.md` containing the command, result, and next blocker. If the working tree was clean at the start, always stage this iteration's tracked changes and create one concise unsigned commit with `git -c commit.gpgSign=false commit --no-gpg-sign` before stopping, even when a build, test, or runtime check fails. Include failures in the progress log. Use a concise why-focused message with the standard Devin footer; never include `game/` or generated artifacts. Do not use `-S` or enable Git commit signing. If the iteration made no changes, no commit is needed. Do not push, reset, or discard existing user changes.
