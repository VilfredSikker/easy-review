#!/usr/bin/env bash
# Signed + notarized macOS desktop release (.app + .dmg).
#
# Uses your Developer ID Application identity and Apple notarization credentials.
# Unlike tauri-build.sh, this never ad-hoc re-signs (which would wipe Developer ID).
#
# Credentials — export in the shell, or put in repo-root `.env.signing` (gitignored):
#
#   APPLE_SIGNING_IDENTITY="Developer ID Application: Name (TEAMID)"  # optional; auto-detected
#   APPLE_ID="you@example.com"
#   APPLE_PASSWORD="xxxx-xxxx-xxxx-xxxx"   # app-specific password
#   APPLE_TEAM_ID="XXXXXXXXXX"
#
# Or App Store Connect API key auth instead of Apple ID:
#   APPLE_API_KEY / APPLE_API_ISSUER / APPLE_API_KEY_PATH
#
# Usage:
#   ./scripts/tauri-sign-release.sh
#   cargo desktop-sign-release
#   just sign-release-desktop
#
# Env knobs:
#   ER_SKIP_DMG=1          skip DMG (default: build DMG)
#   ER_SKIP_INSTALL=0      also copy .app to /Applications (default: skip)
#   ER_SKIP_OPEN_DMG=0     open the DMG in Finder when done (default: skip)
#   ER_SKIP_NOTARIZE=1     sign only (no notarization) — Gatekeeper will still warn
#   ER_SIGNING_ENV=path    load credentials from a different file
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/preflight-desktop.sh"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: signed release builds are macOS-only" >&2
  exit 1
fi

SIGNING_ENV="${ER_SIGNING_ENV:-$ROOT/.env.signing}"
if [[ -f "$SIGNING_ENV" ]]; then
  echo "Loading credentials from $SIGNING_ENV" >&2
  set -a
  # shellcheck disable=SC1090
  source "$SIGNING_ENV"
  set +a
fi

export CARGO_TARGET_DIR="$ROOT/target/desktop"
CONF="$ROOT/crates/er-desktop/tauri.conf.json"
BUNDLE_ROOT="$CARGO_TARGET_DIR/release/bundle"
MACOS_BUNDLE_DIR="$BUNDLE_ROOT/macos"
DMG_DIR="$BUNDLE_ROOT/dmg"
APP_PATH="$MACOS_BUNDLE_DIR/Easy Review.app"

: "${ER_SKIP_DMG:=0}"
: "${ER_SKIP_INSTALL:=1}"
: "${ER_SKIP_OPEN_DMG:=1}"
: "${ER_SKIP_NOTARIZE:=0}"

resolve_signing_identity() {
  if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
    printf '%s\n' "$APPLE_SIGNING_IDENTITY"
    return 0
  fi
  local matches match count
  matches="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p')"
  count="$(printf '%s\n' "$matches" | sed '/^$/d' | wc -l | tr -d ' ')"
  if [[ "$count" -eq 0 ]]; then
    cat >&2 <<'EOF'
error: no Developer ID Application identity found in the keychain.

Create one in Apple Developer → Certificates → Developer ID Application,
install the .cer, then re-run. Or set APPLE_SIGNING_IDENTITY explicitly.
EOF
    return 1
  fi
  if [[ "$count" -gt 1 ]]; then
    echo "error: multiple Developer ID Application identities found; set APPLE_SIGNING_IDENTITY:" >&2
    printf '%s\n' "$matches" >&2
    return 1
  fi
  match="$(printf '%s\n' "$matches" | head -1)"
  printf '%s\n' "$match"
}

resolve_team_id() {
  if [[ -n "${APPLE_TEAM_ID:-}" ]]; then
    printf '%s\n' "$APPLE_TEAM_ID"
    return 0
  fi
  # Identity form: Developer ID Application: Name (TEAMID)
  if [[ "${APPLE_SIGNING_IDENTITY}" =~ \(([A-Z0-9]+)\)\s*$ ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
    return 0
  fi
  return 1
}

require_notarization_creds() {
  if [[ "${ER_SKIP_NOTARIZE}" == "1" ]]; then
    return 0
  fi
  if [[ -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_ISSUER:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
    if [[ ! -f "${APPLE_API_KEY_PATH}" ]]; then
      echo "error: APPLE_API_KEY_PATH does not exist: ${APPLE_API_KEY_PATH}" >&2
      return 1
    fi
    return 0
  fi
  if [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" ]]; then
    if [[ -z "${APPLE_TEAM_ID:-}" ]]; then
      if team="$(resolve_team_id)"; then
        export APPLE_TEAM_ID="$team"
      else
        echo "error: APPLE_TEAM_ID is required (or include (TEAMID) in APPLE_SIGNING_IDENTITY)" >&2
        return 1
      fi
    fi
    return 0
  fi
  cat >&2 <<'EOF'
error: notarization credentials missing.

Set either:
  APPLE_ID + APPLE_PASSWORD (app-specific) + APPLE_TEAM_ID
or:
  APPLE_API_KEY + APPLE_API_ISSUER + APPLE_API_KEY_PATH

Put them in .env.signing (see .env.signing.example), or export them.
To sign without notarizing: ER_SKIP_NOTARIZE=1
EOF
  return 1
}

notarytool_submit() {
  local path="$1"
  if [[ -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_ISSUER:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
    xcrun notarytool submit "$path" \
      --key "$APPLE_API_KEY_PATH" \
      --key-id "$APPLE_API_KEY" \
      --issuer "$APPLE_API_ISSUER" \
      --wait
  else
    xcrun notarytool submit "$path" \
      --apple-id "$APPLE_ID" \
      --password "$APPLE_PASSWORD" \
      --team-id "$APPLE_TEAM_ID" \
      --wait
  fi
}

resign_and_notarize_app() {
  local app="$1"
  echo "Re-signing $app …" >&2
  # Clear extended attrs that can break Gatekeeper after local copies.
  xattr -cr "$app" 2>/dev/null || true
  codesign --force --deep --options runtime --timestamp \
    --sign "$APPLE_SIGNING_IDENTITY" "$app"
  codesign --verify --deep --strict --verbose=2 "$app"

  if [[ "${ER_SKIP_NOTARIZE}" == "1" ]]; then
    echo "ER_SKIP_NOTARIZE=1 — skipping app notarization" >&2
    return 0
  fi
  echo "Notarizing app (zipping for upload) …" >&2
  local zip
  zip="$(mktemp "${TMPDIR:-/tmp}/er-app-notarize.XXXXXX.zip")"
  ditto -c -k --keepParent "$app" "$zip"
  notarytool_submit "$zip"
  rm -f "$zip"
  xcrun stapler staple "$app"
  spctl -a -vv "$app"
}

patch_macos_bundle_plist() {
  local plist="$APP_PATH/Contents/Info.plist"
  if [[ ! -f "$plist" ]]; then
    return 1
  fi
  if /usr/libexec/PlistBuddy -c "Print :NSRequiresCarbon" "$plist" >/dev/null 2>&1; then
    /usr/libexec/PlistBuddy -c "Delete :NSRequiresCarbon" "$plist"
    echo "Removed legacy NSRequiresCarbon from Info.plist (will re-sign)" >&2
    return 0
  fi
  return 1
}

clean_stale_dmg_temps() {
  find "$MACOS_BUNDLE_DIR" -maxdepth 1 -name 'rw.*.dmg' -delete 2>/dev/null || true
  find "$DMG_DIR" -maxdepth 1 -name 'rw.*.dmg' -delete 2>/dev/null || true
}

detach_easy_review_volume() {
  local mount="/Volumes/Easy Review"
  if [[ -d "$mount" ]]; then
    echo "Ejecting previously mounted Easy Review volume..." >&2
    hdiutil detach "$mount" -quiet 2>/dev/null || hdiutil detach "$mount" -force 2>/dev/null || true
  fi
}

open_easy_review_dmg() {
  local dmg_path="$1"
  detach_easy_review_volume
  open "$dmg_path"
  for _ in 1 2 3 4 5; do
    if [[ -d "/Volumes/Easy Review" ]]; then
      open "/Volumes/Easy Review"
      return 0
    fi
    sleep 0.5
  done
  echo "DMG created but mount did not appear — try: open \"$dmg_path\"" >&2
  return 1
}

bundle_dmg_hdiutil() {
  local version arch_tag dmg_path staging
  if [[ ! -d "$APP_PATH" ]]; then
    echo "error: missing app bundle at $APP_PATH" >&2
    return 1
  fi
  version="$(sed -n 's/.*"version": "\([^"]*\)".*/\1/p' "$CONF" | head -1)"
  case "$(uname -m)" in
    arm64) arch_tag="aarch64" ;;
    x86_64) arch_tag="x64" ;;
    *) arch_tag="$(uname -m)" ;;
  esac
  dmg_path="$DMG_DIR/Easy Review_${version}_${arch_tag}.dmg"
  mkdir -p "$DMG_DIR"
  clean_stale_dmg_temps
  detach_easy_review_volume

  staging="$(mktemp -d "${TMPDIR:-/tmp}/er-dmg-stage.XXXXXX")"
  # Preserve the Developer ID signature — do not codesign ad-hoc here.
  ditto "$APP_PATH" "$staging/Easy Review.app"
  ln -s /Applications "$staging/Applications"

  echo "Creating DMG with hdiutil …" >&2
  hdiutil create -volname "Easy Review" -srcfolder "$staging" -ov -format UDZO "$dmg_path"
  rm -rf "$staging"

  if ! hdiutil verify "$dmg_path" >/dev/null 2>&1; then
    echo "DMG verify failed: $dmg_path" >&2
    return 1
  fi

  if [[ "${ER_SKIP_NOTARIZE}" != "1" ]]; then
    echo "Notarizing DMG …" >&2
    notarytool_submit "$dmg_path"
    xcrun stapler staple "$dmg_path"
  fi

  echo "DMG ready: $dmg_path" >&2
  if [[ "${ER_SKIP_OPEN_DMG}" != "1" ]]; then
    open_easy_review_dmg "$dmg_path"
  fi
}

install_to_applications() {
  local dest="/Applications/Easy Review.app"
  if [[ "${ER_SKIP_INSTALL}" == "1" ]]; then
    echo "ER_SKIP_INSTALL=1 — not copying to /Applications" >&2
    return 0
  fi
  echo "Installing to $dest …" >&2
  ditto "$APP_PATH" "$dest"
  # Clear quarantine only — keep Developer ID signature intact.
  xattr -cr "$dest" 2>/dev/null || true
}

# ── main ──────────────────────────────────────────────────────────────────────

APPLE_SIGNING_IDENTITY="$(resolve_signing_identity)"
export APPLE_SIGNING_IDENTITY
echo "Signing identity: $APPLE_SIGNING_IDENTITY" >&2

if team="$(resolve_team_id)"; then
  export APPLE_TEAM_ID="${APPLE_TEAM_ID:-$team}"
fi

require_notarization_creds

preflight_desktop "$ROOT"
"$ROOT/scripts/cargo-gc.sh" --quiet

cd "$ROOT/crates/er-desktop"
# ACL manifests live in gen/ (gitignored). Wipe so build.rs always re-embeds
# permission changes from permissions/*.toml into the binary.
rm -rf gen/schemas

echo "Building signed .app (Tauri) …" >&2
# App only — Tauri's create-dmg path is flaky on macOS; we pack the DMG below.
# With APPLE_* notarization env set, Tauri signs + notarizes the .app.
cargo tauri build -c "$CONF" --bundles app "$@"

if [[ ! -d "$APP_PATH" ]]; then
  echo "error: build finished but app missing at $APP_PATH" >&2
  exit 1
fi

# Plist edit invalidates the signature — re-sign (+ re-notarize) when needed.
if patch_macos_bundle_plist; then
  resign_and_notarize_app "$APP_PATH"
else
  echo "Verifying existing signature …" >&2
  codesign --verify --deep --strict --verbose=2 "$APP_PATH"
  if [[ "${ER_SKIP_NOTARIZE}" != "1" ]]; then
    # Tauri should have notarized; staple if Apple's ticket is available.
    xcrun stapler staple "$APP_PATH" 2>/dev/null \
      || echo "note: stapler could not staple yet (notarization may still be processing)" >&2
    spctl -a -vv "$APP_PATH" || true
  fi
fi

install_to_applications

if [[ "${ER_SKIP_DMG}" == "1" ]]; then
  echo "ER_SKIP_DMG=1 — not creating DMG" >&2
else
  bundle_dmg_hdiutil
fi

echo "Signed release complete." >&2
echo "  app: $APP_PATH" >&2
if [[ "${ER_SKIP_DMG}" != "1" ]]; then
  echo "  dmg: $DMG_DIR/" >&2
fi
