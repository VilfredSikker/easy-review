#!/usr/bin/env bash
# Release desktop bundle (.app install by default; DMG/open are opt-in).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/preflight-desktop.sh"
preflight_desktop "$ROOT"
export CARGO_TARGET_DIR="$ROOT/target/desktop"
CONF="$ROOT/crates/er-desktop/tauri.conf.json"
BUNDLE_ROOT="$CARGO_TARGET_DIR/release/bundle"
MACOS_BUNDLE_DIR="$BUNDLE_ROOT/macos"
DMG_DIR="$BUNDLE_ROOT/dmg"

: "${ER_SKIP_DMG:=1}"
: "${ER_SKIP_OPEN_APP:=1}"

"$ROOT/scripts/cargo-gc.sh" --quiet
cd "$ROOT/crates/er-desktop"

# ACL manifests live in gen/ (gitignored). Wipe so build.rs always re-embeds
# permission changes from permissions/*.toml into the binary.
rm -rf gen/schemas

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
  # Ensure Finder shows the install window (open on an already-mounted dmg is a no-op).
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
  local app_path="$MACOS_BUNDLE_DIR/Easy Review.app"
  local version arch_tag dmg_path staging
  if [[ "$(uname -s)" != "Darwin" ]] || [[ ! -d "$app_path" ]]; then
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

  # Stage app + Applications alias (same drag-to-install layout as create-dmg).
  staging="$(mktemp -d "${TMPDIR:-/tmp}/er-dmg-stage.XXXXXX")"
  ditto "$app_path" "$staging/Easy Review.app"
  ln -s /Applications "$staging/Applications"

  # Ad-hoc sign the staged app. A fully unsigned + quarantined app gets Gatekeeper's
  # "is damaged and can't be opened" dialog with no override; ad-hoc signing upgrades
  # that to the standard "can't verify developer" prompt (right-click → Open works).
  xattr -cr "$staging/Easy Review.app" 2>/dev/null || true
  codesign --force --deep --sign - "$staging/Easy Review.app" 2>/dev/null \
    || echo "codesign skipped — DMG app may show Gatekeeper's damaged dialog" >&2

  echo "Creating DMG with hdiutil (skipping create-dmg/bundle_dmg.sh)..." >&2
  hdiutil create -volname "Easy Review" -srcfolder "$staging" -ov -format UDZO "$dmg_path"
  rm -rf "$staging"

  if ! hdiutil verify "$dmg_path" >/dev/null 2>&1; then
    echo "DMG verify failed: $dmg_path" >&2
    return 1
  fi

  echo "DMG ready: $dmg_path" >&2
  if [[ "${ER_SKIP_OPEN_DMG:-}" != "1" ]]; then
    open_easy_review_dmg "$dmg_path"
  fi
}

# Build the .app only — Tauri's bundle_dmg.sh (create-dmg) often fails on macOS with
# `hdiutil convert: Resource temporarily unavailable` during the compress step.
cargo tauri build -c "$CONF" --bundles app "$@"

if [[ "$(uname -s)" != "Darwin" ]]; then
  exit 0
fi

patch_macos_bundle_plist() {
  local plist="$MACOS_BUNDLE_DIR/Easy Review.app/Contents/Info.plist"
  if [[ ! -f "$plist" ]]; then
    return 0
  fi
  # Tauri currently emits this legacy key in generated app bundles. On modern
  # macOS it can abort during AppKit registration before logs start.
  if /usr/libexec/PlistBuddy -c "Print :LSRequiresCarbon" "$plist" >/dev/null 2>&1; then
    /usr/libexec/PlistBuddy -c "Delete :LSRequiresCarbon" "$plist"
  fi
}

patch_macos_bundle_plist

install_to_applications() {
  local src="$MACOS_BUNDLE_DIR/Easy Review.app"
  local dest="/Applications/Easy Review.app"
  if [[ ! -d "$src" ]]; then
    echo "No .app at $src — skip install" >&2
    return 0
  fi
  if [[ "${ER_SKIP_INSTALL:-}" == "1" ]]; then
    echo "ER_SKIP_INSTALL=1 — not copying to /Applications" >&2
    return 0
  fi
  echo "Installing to $dest ..." >&2
  ditto "$src" "$dest"
  # Local unsigned builds: Finder double-click often does nothing until quarantine
  # is cleared and the bundle is ad-hoc signed.
  xattr -cr "$dest" 2>/dev/null || true
  codesign --force --deep --sign - "$dest" 2>/dev/null \
    || echo "codesign skipped (install may need Right-click → Open once)" >&2
  if [[ "${ER_SKIP_OPEN_APP:-}" != "1" ]]; then
    open -a "Easy Review"
  fi
}

install_to_applications
if [[ "${ER_SKIP_DMG:-}" == "1" ]]; then
  echo "ER_SKIP_DMG=1 — not creating DMG" >&2
else
  bundle_dmg_hdiutil
fi
