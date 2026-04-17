#!/bin/sh
# GForce Node Agent installer — detects OS/arch and downloads the correct binary.
#
# Usage:
#   curl -sSL https://gforce.nearminds.org/install.sh | sh
#
# Options (via env vars):
#   GFORCE_INSTALL_DIR   — where to install (default: /usr/local/bin)
#   GFORCE_VERSION       — specific version (default: latest)

set -e

INSTALL_DIR="${GFORCE_INSTALL_DIR:-/usr/local/bin}"
REPO="nearminds/gforce-node"
VERSION="${GFORCE_VERSION:-latest}"

# Detect OS and architecture
detect_platform() {
    OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
    ARCH="$(uname -m)"

    case "$OS" in
        linux)  OS="linux" ;;
        darwin) OS="darwin" ;;
        *)      echo "Unsupported OS: $OS"; exit 1 ;;
    esac

    case "$ARCH" in
        x86_64|amd64)   ARCH="x86_64" ;;
        aarch64|arm64)  ARCH="aarch64" ;;
        *)              echo "Unsupported architecture: $ARCH"; exit 1 ;;
    esac

    echo "${OS}-${ARCH}"
}

# Get the download URL for the latest release
get_download_url() {
    PLATFORM="$1"

    if [ "$VERSION" = "latest" ]; then
        URL="https://github.com/${REPO}/releases/latest/download/gforce-node-${PLATFORM}.tar.gz"
    else
        URL="https://github.com/${REPO}/releases/download/${VERSION}/gforce-node-${PLATFORM}.tar.gz"
    fi

    echo "$URL"
}

main() {
    echo "GForce Node Agent Installer"
    echo "==========================="
    echo ""

    PLATFORM="$(detect_platform)"
    echo "Platform: $PLATFORM"

    URL="$(get_download_url "$PLATFORM")"
    echo "Download: $URL"
    echo ""

    # Create temp directory
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT

    # Download and extract
    echo "Downloading..."
    if command -v curl >/dev/null 2>&1; then
        curl -sSL "$URL" -o "$TMP_DIR/gforce-node.tar.gz"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$URL" -O "$TMP_DIR/gforce-node.tar.gz"
    else
        echo "Error: curl or wget is required"
        exit 1
    fi

    echo "Extracting..."
    tar -xzf "$TMP_DIR/gforce-node.tar.gz" -C "$TMP_DIR"

    # Install binaries
    echo "Installing to $INSTALL_DIR..."
    if [ -w "$INSTALL_DIR" ]; then
        cp "$TMP_DIR/gforce-node" "$INSTALL_DIR/"
        cp "$TMP_DIR/gforce-node-daemon" "$INSTALL_DIR/"
    else
        echo "Need sudo to install to $INSTALL_DIR"
        sudo cp "$TMP_DIR/gforce-node" "$INSTALL_DIR/"
        sudo cp "$TMP_DIR/gforce-node-daemon" "$INSTALL_DIR/"
    fi

    chmod +x "$INSTALL_DIR/gforce-node"
    chmod +x "$INSTALL_DIR/gforce-node-daemon"

    # Create config directory
    mkdir -p "$HOME/.gforce-node"

    echo ""
    echo "Installation complete!"
    echo ""
    echo "Next steps:"
    echo "  1. Register: gforce-node register --token <TOKEN> --server gforce.nearminds.org"
    echo "  2. Install:  gforce-node install"
    echo ""
}

main "$@"
