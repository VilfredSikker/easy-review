# Release 0.6.2 target

Owner decision on 2026-11-02: v0.6.1 shipped from `main` as a patch release
(v0.6.0 plus two desktop fixes) without this epic, so the epic moves to 0.6.2.
This addendum overrides the [0.6.1 addendum](./ADDENDUM-release-0.6.1.md) and
earlier branch targets.

Use `release/v0.6.2`, created from the `release/v0.6.1` head `29d12bc4`. All
merged epic work is inherited. Remaining work branches merge this new base
before opening their PRs, and open epic PRs are retargeted to it.

All new browser rework PRs target `release/v0.6.2` in
`VilfredSikker/easy-review`. Only the release target changed. The parking proof
stays a draft pending macOS compilation and real-window observations.
