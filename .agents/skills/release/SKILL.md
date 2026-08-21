---
name: release
description: Bump monorepo versions, write RELEASE_NOTES.md, tag, and push a GitHub release for easy-review (er TUI, er-mcp, desktop DMG, npm). Use when the user asks to release, bump version, prepare a release, or publish a new version.
disable-model-invocation: true
---

# Release easy-review

Ship a new `v0.4.x` from `main`. CI ([`.github/workflows/release.yml`](../../.github/workflows/release.yml)) builds TUI + MCP tarballs, a macOS arm64 desktop `.dmg`, creates the GitHub release, and optionally publishes npm packages.

## Preconditions

- [ ] Intended changes are on `main` (bug fixes only per branching rules; feature work lands on release branches first).
- [ ] User asked to release (do not tag unprompted).

## Workflow

Copy and track:

```
Release progress:
- [ ] 1. Pick version + gather changelog
- [ ] 2. Write RELEASE_NOTES.md
- [ ] 3. Bump version files + refresh Cargo.lock
- [ ] 4. Verify pins
- [ ] 5. Commit
- [ ] 6. Tag + push
- [ ] 7. Monitor CI
```

### 1. Pick version

```bash
# Current workspace version
sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1

# Latest tag
git tag -l 'v*' | sort -V | tail -1

# Commits since last tag
git log --oneline v0.4.N..HEAD
```

Default: **patch bump** (`0.4.12` → `0.4.13`). Ask before minor/major.

### 2. Write `RELEASE_NOTES.md`

Insert a new section **at the top** (above the previous release). Match the house style in existing entries.

```markdown
# Easy Review v0.4.N

## In plain terms

- **What changed.** …
- **TL;DR.** …

## Highlights

- **Short label.** One sentence per user-visible item.

## What's Changed

### Features
- …

### Fixes
- …

### CI / Docs / Chore
- …

**Full Changelog**: https://github.com/VilfredSikker/easy-review/compare/v0.4.(N-1)...v0.4.N
```

Use PR numbers where they exist. Keep prose short.

### 3. Bump version files

**Files that must all match** (CI enforces this for npm):

| File | Field |
|------|-------|
| `Cargo.toml` | `[workspace.package]` `version` |
| `Cargo.lock` | `er-engine`, `er-tui`, `er-mcp`, `er-desktop` crate versions |
| `crates/er-desktop/tauri.conf.json` | `version` |
| `npm/er-mcp/package.json` | `version` + `optionalDependencies` pins |
| `npm/platforms/{darwin-arm64,darwin-x64,linux-x64}/package.json` | `version` |
| `npm/skills/package.json` + `package-lock.json` | `version` |

Run the bump script (preferred):

```bash
./scripts/bump-version.sh 0.4.N
```

Refresh `Cargo.lock`:

```bash
./scripts/er-tui.sh check -p er-engine -p er-tui -p er-mcp
CARGO_TARGET_DIR=target/desktop cargo check -p er-desktop
```

### 4. Verify

```bash
./scripts/verify-release-versions.sh 0.4.N
```

Must print `ok: all version pins match …` before commit.

### 5. Commit

Message pattern:

```
chore: bump version to v0.4.N
```

Stage: `Cargo.toml`, `Cargo.lock`, `RELEASE_NOTES.md`, `crates/er-desktop/tauri.conf.json`, all `npm/**/package.json` and `npm/skills/package-lock.json`.

Only commit when the user asked for a release commit.

### 6. Tag + push

```bash
git tag v0.4.N
git push origin main
git push origin v0.4.N
```

Tag push triggers the release workflow.

### 7. Monitor CI

```bash
gh run list --workflow=release.yml --limit 3
gh run watch   # optional
```

When complete:

- Release: https://github.com/VilfredSikker/easy-review/releases/tag/v0.4.N
- Artifacts: `er-*.tar.gz`, `er-mcp-*.tar.gz`, macOS `.dmg`

**npm publish** runs only when repo variable `PUBLISH_NPM=true`. It publishes `easy-review-mcp`, platform packages, and `@easy-review/skills`. Version must not already exist on npm (republishing the same version fails).

## Scripts

| Script | Purpose |
|--------|---------|
| [`scripts/bump-version.sh`](../../scripts/bump-version.sh) | Replace version across all release files |
| [`scripts/verify-release-versions.sh`](../../scripts/verify-release-versions.sh) | Pre-flight check (matches CI npm verify step) |

## Pitfalls

- **Skills npm drift.** `@easy-review/skills` must match the tag even if it was published out-of-band at another version. Align on release.
- **Cargo.lock stale.** `verify-release-versions.sh` checks workspace crate versions in the lockfile. Run `cargo check` after bump.
- **Protected `main`.** Direct push may need admin bypass; if push fails, open a PR with the bump commit and tag after merge.
- **Partial v0.4.x failures.** Older releases sometimes built binaries but failed npm publish. Check which jobs failed before assuming users got npm packages.

## Reference

- Maintainer notes: [`docs/DEVELOPMENT.md`](../../docs/DEVELOPMENT.md) — "Releasing the TUI (`er`)"
- Changelog archive: [`RELEASE_NOTES.md`](../../RELEASE_NOTES.md)
