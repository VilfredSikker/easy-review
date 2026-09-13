# Findings belong to the AI that produced them

A finding is evidence about the code: a claim the agent made, anchored to a
point in a diff. So a person reads findings and acts on them, and does not edit
them. Editing one in place would destroy the record of what the review actually
claimed, and a report that no longer says what was produced cannot be trusted to
be the AI's output. Promotion of a finding into a comment is the one-way, explicit
way out: it creates a new artifact rather than mutating the reviewed one. If you
disagree with a finding, remove it; don't correct its text.

## Consequences

The finding lifecycle still writes to AI-owned sidecars, which looks like a
violation of the above at a glance. Validation replies (`finding_responses.rs`)
attach a person's answer to a finding, and resolve/remove (`finding_cleanup.rs`)
deletes a finding from whichever of `review.json`, `professor.json`, or
`experts/*.json` owns it. Both record a reaction to the finding — or drop it —
without rewriting the finding's own text. A future maintainer grepping for
writes into `review.json` should not read those two paths as a licence to add
in-place editing.
