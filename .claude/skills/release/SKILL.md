---
name: release
description: Cut an easy-review release — reconcile the version across every manifest, build and sign the desktop DMG, merge the release branch to main, tag the merge commit, and verify what CI published. Use when asked to build, sign, or release a version.
---

Cut a release of `er`, `er-mcp` and the macOS desktop app. The mechanics live in
`docs/DEVELOPMENT.md`; this is the order that works and the places it bites.

## 0. Confirm the version

Manifests move together, or the tag builds a release that lies about its own
version. Bump them in one commit, and verify at the commit you are about to tag,
not in the working tree. Discover the set rather than trusting a list:

```bash
git grep -n -E '"[0-9]+\.[0-9]+\.[0-9]+"' -- '*.toml' '*.json'
git show <commit>:Cargo.toml | grep -m1 '^version'
```

A branch name is not the version. A branch cut as `release/v0.4.19` shipped
`0.5.0` when the work earned a minor bump — confirm which one is intended
before anything is published.

Anything left at an older number is usually fine: prose about an older release,
a test fixture, or a dependency version. Match the `major.minor` of the release
being cut to spot the real stragglers.

## 1. Verify before publishing

```bash
just check          # workspace type-check
just test           # engine + TUI
just docs-check
just comments-check
```

Two `er-tui` tests write under `/tmp` and fail inside the sandbox with
`Operation not permitted`. They pass unsandboxed; that is the baseline, not a
regression. Run the full `just ci` yourself when the release carries more than
version and doc changes.

## 2. Build and sign the desktop release

```bash
just sign           # Developer ID + notarized .app and .dmg
```

This reaches Apple's timestamp server and notary service, so in the sandbox it
dies at codesign with `A timestamp was expected but was not found`. Run it
outside the sandbox. The compile is cached, so a retry costs the re-bundle and
sign only.

Verify without `codesign` if that command is denied:

```bash
xcrun stapler validate "target/desktop/release/bundle/dmg/Easy Review_<version>_aarch64.dmg"
plutil -p "target/desktop/release/bundle/macos/Easy Review.app/Contents/Info.plist" | grep CFBundleShortVersionString
```

## 3. Land the release branch

Resolve the release branch rather than hardcoding it (highest `origin/release/v*`).
`main` may have moved since the branch was cut, because bug fixes go straight to
`main` — merge `origin/main` in and resolve the conflicts before opening the PR.
`git fetch` fails silently in the sandbox, so fetch with it off or `origin/*`
goes stale and the PR reports a conflict you cannot see.

The branch carries a release PR into `main`, squashed. That PR needs one
approving review, which a solo maintainer cannot give themselves; the ruleset
grants admins a pull-request bypass for exactly this:

```bash
gh pr merge <N> --squash --admin
```

## 4. Tag the merge commit

Tag `main`'s new head, which is the squash commit, rather than the release
branch head. The tag is what triggers `.github/workflows/release.yml`.

```bash
git tag v<version> <merge-commit>
git push origin v<version>
```

## 5. Finish what CI cannot

CI builds `er` and `er-mcp` for three targets, creates the release, and
publishes npm. Its DMG job is gated on `APPLE_CERTIFICATE`, which this repo does
not set, so CI publishes no DMG and the signed one is yours to upload:

```bash
gh release upload v<version> "target/desktop/release/bundle/dmg/Easy Review_<version>_aarch64.dmg"
```

Pushing the tag is a real npm publish. The workflow checks every package's
version against the tag first, so a half-bumped manifest fails the release
rather than publishing something mislabelled.

## 6. Verify the published artifact

```bash
gh release view v<version> --json assets --jq '.assets[].name'
gh release list --limit 2                       # expect Latest on the new tag
```

Download what actually shipped and run it — the binary agreeing with the
manifests is the proof that the release is coherent:

```bash
gh release download v<version> --pattern "er-aarch64-apple-darwin.tar.gz" --dir "$TMPDIR/rel"
tar -xzf "$TMPDIR/rel/er-aarch64-apple-darwin.tar.gz" -C "$TMPDIR/rel"
"$TMPDIR/rel/er" --version
```

`npm` on this machine fails with EPERM from a root-owned `~/.npm` cache. Read
the registry over HTTPS (`https://registry.npmjs.org/easy-review-mcp`) instead
of fighting it.
