# Findings belong to the AI that produced them

A finding is evidence about the code: a claim the agent made, anchored to a
point in a diff. So a person reads findings and acts on them, and does not edit
them. Editing one in place would destroy the record of what the review actually
claimed, and a report that no longer says what was produced cannot be trusted to
be the AI's output. If you disagree with a finding, remove it; don't correct its
text.

## Considered Options

**Promotion as a copy, leaving the finding in place** — a `promoted_to` stamp on
the finding, a `finding-promotions.json` index, and a "Promoted to #N" badge.
Built, and then never wired: `promoted_to` is only ever constructed as `None` and
the promotions index is only ever read from. What ships instead is a move. A
reader who finds that scaffolding should not mistake it for the live behaviour.

## Consequences

- **Promotion destroys the source.** Promoting a finding into a comment creates
  the comment and then deletes the finding from its sidecar, along with threads
  linked to it (`remove_finding_from_sidecars`, `delete_threads_linked_to_finding`).
  There is no undo and the promote dialog does not warn. That is a data-loss
  decision made silently, and the dead scaffolding above is the evidence a
  non-destructive variant was considered and dropped.
- The finding lifecycle writes to AI-owned sidecars in three places, which looks
  like a violation of the rule at a glance. Validation replies
  (`finding_responses.rs`) attach a person's answer; resolve/remove
  (`finding_cleanup.rs`) drops a finding from whichever of `review.json`,
  `professor.json` or `experts/*.json` owns it; promotion is the third and the
  destructive one. All three react to a finding or remove it — none rewrites its
  text. A maintainer grepping for writes into `review.json` should not read them
  as a licence to add in-place editing.
