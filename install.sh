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
    echo "  --help              Show this help"
    echo ""
    echo "Examples:"
    echo "  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | bash"
    echo "  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | bash -s -- --version v0.1.0"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --dir) INSTALL_DIR="$2"; shift 2 ;;
        --help) usage; exit 0 ;;
        *) echo "Unknown option: $1"; usage; exit 1 ;;
    esac
done

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
