# ai/ — review sidecars

Data model, loader, and persistence for the review sidecars. This module reads
what an agent produced; it does not spawn one. Spawns live in
`app/card_ai_spawn.rs` and `app/state/comments.rs`, and the prompt text they pass
is compiled into `ai/prompts.rs` — nothing reads a prompt or skill file at
runtime. `docs/adr/0023-self-contained-agent-prompts.md`.

Sidecars live in the tab's per-view bucket (`TabState::er_dir()`): the branch
bucket or `prs/pr-<N>/`. `docs/adr/0004-per-view-artifact-scoping.md`.

## Who writes which file

**`er` writes these:** `questions.json` (things you want answered, including
Hub-written probes), `notes.json` (instructions to hand an agent),
`github-comments.json` (the shared GitHub threads, two-way synced). The first
two are private. The questions and notes AI actions are the exception — each
rewrites its store in the bucket, leaving a `.prev.json` beside it. Probe pass
is host-written from stdout: the agent cannot write `questions.json`, and the
host drops extras past the cap.

**AI-owned, read-only to `er` as a whole:** `review.json`, `order.json`,
`summary.md`, `triage.json`, `professor.json`, `experts/*.json`, `tour.json`.

**`checklist.json` is split by field.** Its *items* are AI-owned like the rest;
its `checked` flags are the reviewer's own progress, written by the toggle in
either front end (`App::toggle_checklist_item_at`). Nothing else in the file is
theirs to write, and the agent that generates it never sets `checked`.

**Agent-emitted, host-written:** `diagrams/*.json`, and probe Questions in
`questions.json`. The agent runs read-only and prints JSON on stdout; the host
parses and writes the file.
Never hand that agent a write path — its prompt carries untrusted diff content,
so `Write` there is a prompt-injection primitive. Read-only is enforced per CLI
family (Claude allowlist, OpenCode permission env, Codex `--sandbox read-only`,
Cursor `--mode plan`) and the `--add-dir` grant is withheld from the two that
have no allowlist. Anything an agent must persist durably goes through the same
parse-validate-write path.
`docs/adr/0022-host-written-diagram-sidecars.md`.

Privacy is a property of which file the data is in, so no push path can reach a
question or a note — not even one added later. Do not collapse the three comment
stores behind a `type` discriminator.
`docs/adr/0007-three-comment-stores.md`.

## The finding lifecycle is the one AI-owned write

`finding_responses.rs` (validation replies) and `finding_cleanup.rs`
(resolve/remove) write back to whichever of `review.json`, `professor.json`, or
`experts/*.json` owns the finding. Both attach a reaction or drop the finding;
neither rewrites the finding's own text — a report that no longer says what the
review claimed cannot be trusted as the AI's output. Disagree with a finding by
removing it. Those two paths are not licence to add in-place editing.
`docs/adr/0009-findings-are-ai-owned.md`.

## Staleness is two conditions, plus a third for tours

`docs/adr/0010-staleness-is-two-conditions.md`.

- A sidecar is stale when the SHA-256 of the diff it was generated against no
  longer matches the current diff. Remedy: regenerate.
- A comment is stale when its anchor line is gone from the current diff. An
  anchor whose line merely moved is relocated to the new position and is *not*
  stale. Recomputed on load; its persisted counterpart is `anchor_status`
  (`original` / `relocated` / `lost`), not a boolean.
- A tour is stale per context and kept out of `is_stale`: its diff hash is its
  identity, so regeneration is its only fix. A tour from the other bucket is
  reused when that hash matches the active diff, so nothing may assume the
  active bucket holds the tour being shown.
  `docs/adr/0025-tour-per-view-with-reuse.md`

Every JSON sidecar carries that hash, and the spawn validation refuses one whose
stored hash does not match. `summary.md` carries none.

## Traps

- The load is branch-guarded, and the guard empties everything. A sidecar
  declaring a real `head_branch` that disagrees with the bucket's branch returns
  a default `AiState` for the whole directory — `review.json` and, with it,
  `summary.md`, `order.json`, the lot. An empty AI panel over a populated bucket
  usually means this, not a missing file. Placeholders (`unknown`, `<head branch
  if known>`, `pr/<N>`) do not count as a declaration and must not disqualify.
- A reload replaces `AiState` wholesale, so anything that has to outlive one is
  carried across by hand in `finish_ai_reload` — today `stale_files` and
  `tour_stale`. A new field is reset on every reload unless it joins that list,
  and a second adoption path that skips `finish_ai_reload` loses them all.
