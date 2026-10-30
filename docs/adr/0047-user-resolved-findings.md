# A person can resolve a finding without removing it

ADR 0009 left a person one way to act on a finding they are done with: remove it. That loses the record of what the review claimed, so a fixed finding and a wrong one look the same afterwards — both gone. The review redesign keeps a resolved finding visible, dimmed and struck through, and counts it apart from the open ones.

So a person can now set a finding's `resolved` flag, and clear it again (`resolve_finding` / `unresolve_finding`, through `set_finding_resolved` in `finding_cleanup.rs`). Only the status fields move — `resolved` and `resolved_at`. The title, description, severity and evidence stay as the AI wrote them, so ADR 0009's rule still holds: a person reacts to a finding and never rewrites its claim. Dismiss stays a removal, for a finding the person thinks is wrong.

## Considered Options

**Map Resolve to dismiss.** No new write into an AI sidecar, but the button would delete the finding, which is the outcome the design is trying to avoid, and the "resolved" count would always read zero for anything a person did.

**A person-owned overlay** — a separate `finding-resolutions.json` keyed by finding id, merged at load. It keeps `review.json` untouched by people, but every finding already carries a `resolved` / `resolved_at` pair that the front ends read and that agents are told to leave at their defaults, so nothing else writes it. An overlay would add a second store and a load-time merge to hold a fact the finding already has a field for, and a finding id that drifts across a regeneration would leave the overlay pointing at nothing. The person writes the existing flag instead.

## Consequences

- A person is the only writer of `resolved` today. Agents generating a review leave it false, and the validate pass does not resolve findings: it removes the ones it judges `RESOLVED_OR_INVALID` from `review.json`.
- A validate run can therefore delete a finding the person resolved — the fix they made is exactly what makes validate judge the concern gone — and with it the dimmed record this ADR exists to keep. We accept that for now: the finding is gone because it no longer applies, which is the outcome either way. Telling the validate prompt in `ai/prompts.rs` to leave `resolved: true` findings untouched is a follow-up.
- A resolution lives in the finding, so it lasts as long as the finding does. A new review regenerates `review.json` and drops it, exactly as it brings back a dismissed finding.
- Reopening also clears `resolved_note`. Nothing in the engine writes that field, but the schema carries it and the TUI shows it on a resolved finding, so a legacy or hand-written sidecar may hold one; leaving it would show a reason on an open finding.
