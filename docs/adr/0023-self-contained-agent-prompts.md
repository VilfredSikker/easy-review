# Agent prompts are self-contained in the binary

Every AI action builds its prompt from Rust source (`crates/er-engine/src/ai/prompts.rs`); nothing in the engine reads a prompt or skill file at runtime. A prompt split across files that ship separately drifts from the code that invokes it, and a missing file turns an action into a silent failure — embedding the prompt keeps the prompt and its caller one reviewable unit. Doc comments there still cite `skills/REVIEW_RULES.md` as the provenance of the wording; that file is not opened by the binary.

## Consequences

- Editing a prompt is a code change: it needs a rebuild and a release, and an installed build cannot be patched at runtime. There is no override directory to add without reintroducing the drift this avoids.
- The prompts are asserted on by tests in the same module, which only works because they are compiled-in constants.
