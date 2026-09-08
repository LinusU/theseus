---
name: mm2-ralph
description: Advance Midtown Madness 2 through multiple commit-sized milestones
argument-hint: "[focus]"
model: sonnet
triggers:
  - user
---

You are a long-running work session on the Theseus Midtown Madness 2 target. Work through several commit-sized milestones in this invocation, not just one blocker. If `RALPH_AGENT_PASSES` is available, use it as the pass budget; otherwise use 8 milestones.

Read `AGENTS.md`, `doc/mm2.md`, and `doc/mm2-progress.md` first; the user-requested priority backlog in the progress log outranks the audit backlog. Physical-input, focus, fullscreen, and window-size items are only verifiable through `out/mm2/probe-input.sh` on a machine with a display, never through `THESEUS_INJECT_*` or headless runs; without a display, record that and leave the item open. Treat `game/` as a user-owned, ignored game installation: inspect it only as needed, and never modify, stage, copy, or delete anything under it.

At the start of each milestone, reread `doc/mm2-progress.md`, choose the next smallest actionable blocker, reproduce it, make the smallest correct implementation change, and add or update a focused test when the code has test infrastructure. Run the narrowest relevant formatter, check, test, or target build. A failing build or test is diagnostic information, not a reason to stop. Record the command, result, error count or next missing symbol, and next blocker in `doc/mm2-progress.md`.

After every milestone, stage only its tracked source, documentation, or configuration changes and create an unsigned commit with `git -c commit.gpgSign=false commit --no-gpg-sign`, using a concise why-focused message with the standard Devin footer. Never include `game/`, generated output, reports, data images, or missing-address logs. Continue immediately with the next milestone after committing. Do not use `-S`, push, reset, or discard existing user changes.

Prefer fixing shared compiler, runtime, and Win32 behavior over target-specific workarounds. Function overrides belong in `out/mm2/src/externs.rs` and must be wired through an explicit `tc --extern ADDRESS=NAME` argument. If an external prerequisite is unavailable, document the exact blocker, commit that progress, and continue with an independent backlog item when possible. Stop only when the pass budget is exhausted, no actionable work remains, or a fatal tooling or permission failure prevents further progress.
