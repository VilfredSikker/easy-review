# A pending review lives on GitHub, not in the comment sidecar

"Start a review" collects line comments that post together with one Comment / Approve / Request changes decision. Those comments are held in the reviewer's GitHub pending review (a review in state PENDING, visible only to its author), not in `github-comments.json`. The composer appends to it through `gh`, the Review popover reads it back, removes from it, and submits it with the decision (ADR 0026). "Add single comment" posts one comment straight to the PR, outside any review.

GitHub allows one pending review per reviewer per PR, and github.com shows it as the reviewer's draft. Holding the draft anywhere else would make two drafts of one review: a comment started in the browser would be missing from the popover, and submitting a separate review from here while a GitHub draft exists is refused.

## Considered Options

**Unpushed local comments as the draft.** `submit_github_review` already bundles unsynced comments from `github-comments.json` into one review, so this needed no new GitHub calls. It was rejected because the draft would be invisible to github.com and to a second device, and would collide with a pending review started there.

## Consequences

- Every change to the draft is a network round trip that can be refused, so the composer keeps its text until GitHub accepts the comment, rather than painting it optimistically.
- No sidecar holds a pending comment, so the diff draws one only when a comment pull returns it from GitHub, and tags it "pending review" by matching its `gh-<id>` thread id against the draft. The popover lists the draft either way.
- The older unpushed-local-comment path (Comments panel push, `submit_github_review`) is unchanged and still bundles its own comments.
