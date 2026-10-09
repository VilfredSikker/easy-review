# In-app updates: signed updater for the desktop, checksummed self-replace for the TUI

Both front ends notice a newer GitHub release and install it themselves. They use different mechanisms because they ship differently. The desktop is a notarized `.app` that a running process cannot safely overwrite piece by piece. The TUI is one static binary that a rename can swap in a single step.

The desktop uses `tauri-plugin-updater`. The release carries `latest.json` and a `.app.tar.gz` signed with a minisign key. The plugin verifies the archive against the public key compiled into the app, replaces the bundle and relaunches. `scripts/tauri-sign-release.sh` builds that archive itself, after re-signing and stapling. Tauri's `createUpdaterArtifacts` stays off: the bundler archives the `.app` before the script's plist fix and staple, so the update would ship a bundle that differs from the one Apple notarized.

The TUI's `er update` downloads `er-<triple>.tar.gz`, checks it against the release's `SHA256SUMS`, and renames it over the running executable from a temp file in the same directory. It refuses a binary under `~/.cargo/bin` or a `target/` dir, because replacing a source build with a release binary would silently discard the user's build. The TUI cannot be clicked (no mouse capture), so the in-app part is a status-bar hint that names the command.

Each front end does its own fetching. ADR 0026 keeps `er-engine` free of an HTTP client, and release assets are anonymous public downloads that a user without `gh auth` should still get. The engine holds only the decisions (version order, asset naming, checksum match, the atomic swap) in `release_update.rs`.

## Considered Options

- **Desktop: mount the DMG and copy the `.app` over.** No new secret, but the only integrity check is Gatekeeper's, and a copy interrupted halfway leaves a broken bundle in `/Applications`. Rejected.
- **TUI: sign with the same minisign key.** Stronger than a checksum served from the same release, but it adds a verifier to the TUI for an attacker who would already need write access to the GitHub release. A checksum catches truncation and CDN corruption, which are the realistic failures. It can be upgraded later without changing the flow.

## Consequences

- The updater private key cannot be rotated without a manual reinstall. Builds trust only the public key they shipped with, so a lost key strands every desktop on its current version.
- Desktop builds from before this change have no updater. Their pill still opens the release page, and their first update is a manual download.
- A release whose desktop job fails publishes no `latest.json`. Clicking the pill then shows "Update failed — open release", and a second click opens the release page.
- The release check uses GitHub's anonymous API (60 requests/hour per IP). The TUI caches the answer for six hours in managed storage, and the desktop caches it in memory for one hour.
