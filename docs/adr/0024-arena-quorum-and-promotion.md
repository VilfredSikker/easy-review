# Arena findings need a quorum and promotion is explicit

The arena is for changes one pass is not trusted enough to act on, so its reviewers each work the same diff independently and the later rounds re-poll only the reviewers that survived the first, letting each vote on the others' findings; an arbiter rules in the final round. A run with fewer than two surviving reviewers aborts rather than reporting agreement it does not have — a finding two reviewers reached independently is the signal worth acting on.

Findings then stay in the arena run's own sidecar, and the review is untouched until the user accepts one explicitly, which stamps `accepted_at` and merges it into `review.json`. `auto_accept_threshold` can mark a high-confidence pending finding as kept, but that is the arena grading itself and still writes nothing to the review, so a run cannot silently rewrite the artifacts it was meant to inform.

## Consequences

- Runs are safe to inspect cold and to abandon. Results land in the arena store, and `accepted_at` guards each finding, so accepting again only adds what is new.
- Quorum clamps to one for a single-reviewer run, and a one-round run has no cross-check or arbiter; those findings reach the same accept button carrying no agreement signal, so nothing downstream should present them as though they did.
