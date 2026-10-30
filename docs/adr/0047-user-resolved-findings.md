# A person can resolve a finding without removing it

ADR 0009 left a person one way to act on a finding they are done with: remove it. That loses the record of what the review claimed, so a fixed finding and a wrong one look the same afterwards — both gone. The review redesign keeps a resolved finding visible, dimmed and struck through, and counts it apart from the open ones.

So a person can now set a finding's `resolved` flag, and clear it again (`resolve_finding` / `unresolve_finding`, through `set_finding_resolved` in `finding_cleanup.rs`). Only the status fields move — `resolved`, `resolved_at`, and on reopen the validate pass's `resolved_note`. The title, description, severity and evidence stay as the AI wrote them, so ADR 0009's rule still holds: a person reacts to a finding and never rewrites its claim. Dismiss stays a removal, for a finding the person thinks is wrong.

## Considered Options

**Map Resolve to dismiss.** No new write into an AI sidecar, but the button would delete the finding, which is the outcome the design is trying to avoid, and the "resolved" count would always read zero for anything a person did.

**A person-owned overlay** — a separate `finding-resolutions.json` keyed by finding id, merged at load. It keeps `review.json` untouched by people, but the validate pass already writes `resolved` into the finding itself, so two places would hold the same fact and need a precedence rule. The flag already existed with the meaning we want, so the person writes it too.

## Consequences

- `resolved` no longer means "the validate pass found it fixed". It means "no longer needs attention", set by either the validate pass or a person, and the latest write wins: a validate run can reopen a finding the person resolved when the issue is still in the code, and a person can reopen one validate resolved. An empty `resolved_note` with a set `resolved_at` is the only sign a person, not validate, did it.
- A resolution lives in the finding, so it lasts as long as the finding does. A new review regenerates `review.json` and drops it, exactly as it brings back a dismissed finding.
- Reopening clears `resolved_note`. The note explained a resolution that no longer stands, and leaving it would show a reason on an open finding.
