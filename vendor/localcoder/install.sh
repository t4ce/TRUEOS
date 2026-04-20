#!/bin/bash

set -e

REPO="iamwjun/localcoder"
BIN_NAME="localcoder"
INSTALL_DIR="/usr/local/bin"

# --- Detect OS and architecture ---
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64)  TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *)      echo "❌ Unsupported macOS architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
      *)       echo "❌ Unsupported Linux architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  *)
    echo "❌ Unsupported OS: $OS"
    exit 1
    ;;
esac

ARCHIVE="${BIN_NAME}-${TARGET}.tar.gz"

echo "  OS:     $OS"
echo "  Arch:   $ARCH"
echo "  Target: $TARGET"
echo ""

# --- Get latest release version ---
echo "🔍 Fetching latest release..."

if command -v curl &>/dev/null; then
  FETCH="curl -fsSL"
elif command -v wget &>/dev/null; then
  FETCH="wget -qO-"
else
  echo "❌ Neither curl nor wget found"
  exit 1
fi

LATEST_URL="https://api.github.com/repos/${REPO}/releases/latest"
VERSION=$($FETCH "$LATEST_URL" | grep '"tag_name"' | sed 's/.*"tag_name": *"\(.*\)".*/\1/')

if [ -z "$VERSION" ]; then
  echo "❌ Could not determine latest release version"
  exit 1
fi

echo "📦 Latest version: $VERSION"

# --- Download ---
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"
TMP_DIR=$(mktemp -d)
TMP_ARCHIVE="${TMP_DIR}/${ARCHIVE}"

echo "⬇️  Downloading ${ARCHIVE}..."
if command -v curl &>/dev/null; then
  curl -fSL --progress-bar "$DOWNLOAD_URL" -o "$TMP_ARCHIVE"
else
  wget -q --show-progress "$DOWNLOAD_URL" -O "$TMP_ARCHIVE"
fi

# --- Extract and install ---
echo "📂 Extracting..."
tar -xzf "$TMP_ARCHIVE" -C "$TMP_DIR"

echo "🔧 Installing to ${INSTALL_DIR}..."
if [ -w "$INSTALL_DIR" ]; then
  mv "${TMP_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
else
  sudo mv "${TMP_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
fi
chmod +x "${INSTALL_DIR}/${BIN_NAME}"

# --- Cleanup ---
rm -rf "$TMP_DIR"

echo ""
echo "✅ Installed: $(which $BIN_NAME)"
echo "   Version:   $VERSION"
echo ""
echo "Run: $BIN_NAME"
