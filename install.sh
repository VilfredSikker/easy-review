#!/usr/bin/env bash
set -euo pipefail

REPO="VilfredSikker/easy-review"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION=""

usage() {
    echo "Install er (easy-review) — terminal git diff reviewer"
    echo ""
    echo "Usage: install.sh [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --version VERSION   Install a specific version (e.g., v0.1.0)"
    echo "  --dir DIR           Install directory (default: ~/.local/bin)"
    echo "  --uninstall         Remove er binary, config, and review data"
    echo "  --yes               Skip uninstall confirmation"
    echo "  --dry-run           List uninstall paths without deleting"
    echo "  --keep-data         Keep managed review storage"
    echo "  --keep-config       Keep ~/.config/er"
    echo "  --keep-apps         Keep the er binary and desktop app"
    echo "  --help              Show this help"
    echo ""
    echo "Examples:"
    echo "  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | bash"
    echo "  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | bash -s -- --version v0.1.0"
    echo "  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | bash -s -- --uninstall"
}

UNINSTALL=0
YES=0
DRY_RUN=0
KEEP_DATA=0
KEEP_CONFIG=0
KEEP_APPS=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --dir) INSTALL_DIR="$2"; shift 2 ;;
        --uninstall) UNINSTALL=1; shift ;;
        --yes|-y) YES=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        --keep-data) KEEP_DATA=1; shift ;;
        --keep-config) KEEP_CONFIG=1; shift ;;
        --keep-apps) KEEP_APPS=1; shift ;;
        --help) usage; exit 0 ;;
        *) echo "Unknown option: $1"; usage; exit 1 ;;
    esac
done

if [[ "$UNINSTALL" -eq 1 ]]; then
    if command -v er >/dev/null 2>&1; then
        args=(uninstall)
        [[ "$YES" -eq 1 ]] && args+=(--yes)
        [[ "$DRY_RUN" -eq 1 ]] && args+=(--dry-run)
        [[ "$KEEP_DATA" -eq 1 ]] && args+=(--keep-data)
        [[ "$KEEP_CONFIG" -eq 1 ]] && args+=(--keep-config)
        [[ "$KEEP_APPS" -eq 1 ]] && args+=(--keep-apps)
        exec er "${args[@]}"
    fi

    echo "er is not on PATH — removing common locations manually…"
    echo "(best-effort path list; prefer installing er and re-running for the canonical plan)"
    if [[ "$DRY_RUN" -eq 1 ]]; then
        echo "(dry-run) would remove:"
        [[ "$KEEP_APPS" -eq 0 ]] && echo "  ${INSTALL_DIR}/er" && echo "  $HOME/.local/bin/er" && echo "  $HOME/.cargo/bin/er"
        [[ "$KEEP_CONFIG" -eq 0 ]] && echo "  $HOME/.config/er"
        if [[ "$KEEP_DATA" -eq 0 ]]; then
            if [[ "$(uname -s)" == "Darwin" ]]; then
                echo "  $HOME/Library/Application Support/easy-review"
                echo "  $HOME/Library/Application Support/com.reshape.easy-review"
                echo "  $HOME/Library/Application Support/Easy Review"
                echo "  $HOME/Library/Caches/com.reshape.easy-review"
                echo "  $HOME/Library/Caches/Easy Review"
            fi
            echo "  $HOME/.local/share/easy-review"
            echo "  $HOME/.local/share/com.reshape.easy-review"
            echo "  $HOME/.cache/er"
        fi
        if [[ "$KEEP_APPS" -eq 0 ]]; then
            echo "  /Applications/Easy Review.app"
            echo "  $HOME/Applications/Easy Review.app"
        fi
        exit 0
    fi

    if [[ "$YES" -ne 1 ]]; then
        printf "Type uninstall to confirm: "
        read -r confirm
        if [[ "$confirm" != "uninstall" ]]; then
            echo "Cancelled."
            exit 0
        fi
    fi

    if [[ "$KEEP_APPS" -eq 0 ]]; then
        rm -f "${INSTALL_DIR}/er" "$HOME/.local/bin/er" "$HOME/.cargo/bin/er" 2>/dev/null || true
    fi
    if [[ "$KEEP_CONFIG" -eq 0 ]]; then
        rm -rf "$HOME/.config/er" 2>/dev/null || true
    fi
    if [[ "$KEEP_DATA" -eq 0 ]]; then
        if [[ "$(uname -s)" == "Darwin" ]]; then
            rm -rf "$HOME/Library/Application Support/er" \
                   "$HOME/Library/Application Support/easy-review" \
                   "$HOME/Library/Application Support/com.reshape.easy-review" \
                   "$HOME/Library/Application Support/Easy Review" \
                   "$HOME/Library/Caches/com.reshape.easy-review" \
                   "$HOME/Library/Caches/Easy Review" 2>/dev/null || true
        fi
        rm -rf "$HOME/.local/share/easy-review" \
               "$HOME/.local/share/com.reshape.easy-review" \
               "$HOME/.cache/er" 2>/dev/null || true
    fi
    if [[ "$KEEP_APPS" -eq 0 ]]; then
        rm -rf "/Applications/Easy Review.app" \
               "$HOME/Applications/Easy Review.app" 2>/dev/null || true
    fi
    echo "Done."
    exit 0
fi

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin) OS_TARGET="apple-darwin" ;;
    Linux)  OS_TARGET="unknown-linux-gnu" ;;
    *)      echo "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64)   ARCH_TARGET="x86_64" ;;
    arm64|aarch64)  ARCH_TARGET="aarch64" ;;
    *)              echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

TARGET="${ARCH_TARGET}-${OS_TARGET}"

# Get latest version if not specified
if [[ -z "$VERSION" ]]; then
    VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')
    if [[ -z "$VERSION" ]]; then
        echo "Failed to fetch latest version. Try: install.sh --version v0.1.0"
        exit 1
    fi
fi

TARBALL="er-${TARGET}.tar.gz"
URL="https://github.com/$REPO/releases/download/$VERSION/$TARBALL"

echo "Installing er $VERSION ($TARGET)..."

# Create install directory
mkdir -p "$INSTALL_DIR"

# Download and extract
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

if ! curl -fsSL "$URL" -o "$TMPDIR/$TARBALL"; then
    echo "Download failed. Check that $VERSION exists for $TARGET."
    echo "Available releases: https://github.com/$REPO/releases"
    exit 1
fi

tar -xzf "$TMPDIR/$TARBALL" -C "$TMPDIR"
mv "$TMPDIR/er" "$INSTALL_DIR/er"
chmod +x "$INSTALL_DIR/er"

# Verify
if "$INSTALL_DIR/er" --version >/dev/null 2>&1; then
    echo "Installed er $VERSION to $INSTALL_DIR/er"
else
    echo "Warning: er was installed but could not run. Check $INSTALL_DIR/er"
fi

# PATH hint
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo ""
        echo "Add to your PATH if not already there:"
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
        ;;
esac
