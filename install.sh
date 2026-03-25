#!/usr/bin/env sh
# rustyochestrator installer — detects your platform and downloads the right binary.
# Usage: curl -fsSL https://github.com/KodeSage/rustyochestrator/releases/latest/download/install.sh | sh

set -e

REPO="KodeSage/rustyochestrator"
BINARY="rustyochestrator"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# ── Detect OS and architecture ───────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux*)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
      arm64)   TARGET="aarch64-unknown-linux-gnu" ;;
      *)       echo "Unsupported architecture: $ARCH" && exit 1 ;;
    esac
    ARCHIVE_EXT="tar.gz"
    ;;
  Darwin*)
    case "$ARCH" in
      x86_64) TARGET="x86_64-apple-darwin" ;;
      arm64)  TARGET="aarch64-apple-darwin" ;;
      *)      echo "Unsupported architecture: $ARCH" && exit 1 ;;
    esac
    ARCHIVE_EXT="tar.gz"
    ;;
  *)
    echo "Unsupported OS: $OS"
    echo "On Windows, download from: https://github.com/$REPO/releases/latest"
    exit 1
    ;;
esac

# ── Resolve the latest release tag ───────────────────────────────────────────

LATEST_URL="https://api.github.com/repos/$REPO/releases/latest"
VERSION="$(curl -fsSL "$LATEST_URL" | grep '"tag_name"' | sed 's/.*"tag_name": *"\(.*\)".*/\1/')"

if [ -z "$VERSION" ]; then
  echo "Could not determine latest version. Check https://github.com/$REPO/releases"
  exit 1
fi

echo "Installing $BINARY $VERSION for $TARGET..."

# ── Download and install ──────────────────────────────────────────────────────

ARCHIVE="${BINARY}-${VERSION}-${TARGET}.${ARCHIVE_EXT}"
URL="https://github.com/$REPO/releases/download/$VERSION/$ARCHIVE"
CHECKSUM_URL="https://github.com/$REPO/releases/download/$VERSION/$ARCHIVE.sha256"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

curl -fsSL "$URL" -o "$TMP_DIR/$ARCHIVE"

# Verify checksum if sha256sum / shasum is available
if command -v sha256sum >/dev/null 2>&1; then
  curl -fsSL "$CHECKSUM_URL" -o "$TMP_DIR/$ARCHIVE.sha256"
  # checksum file contains "hash  filename"; adjust path to match temp dir
  sed "s|  .*|  $TMP_DIR/$ARCHIVE|" "$TMP_DIR/$ARCHIVE.sha256" | sha256sum --check --quiet
  echo "Checksum OK"
elif command -v shasum >/dev/null 2>&1; then
  curl -fsSL "$CHECKSUM_URL" -o "$TMP_DIR/$ARCHIVE.sha256"
  sed "s|  .*|  $TMP_DIR/$ARCHIVE|" "$TMP_DIR/$ARCHIVE.sha256" | shasum -a 256 --check --quiet
  echo "Checksum OK"
fi

tar -xzf "$TMP_DIR/$ARCHIVE" -C "$TMP_DIR"

# ── Place binary ──────────────────────────────────────────────────────────────

if [ -w "$INSTALL_DIR" ]; then
  mv "$TMP_DIR/$BINARY" "$INSTALL_DIR/$BINARY"
  chmod +x "$INSTALL_DIR/$BINARY"
else
  echo "Installing to $INSTALL_DIR (sudo required)..."
  sudo mv "$TMP_DIR/$BINARY" "$INSTALL_DIR/$BINARY"
  sudo chmod +x "$INSTALL_DIR/$BINARY"
fi

echo ""
echo "rustyochestrator $VERSION installed to $INSTALL_DIR/$BINARY"
echo "Run: rustyochestrator --help"
