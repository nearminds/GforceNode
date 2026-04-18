#!/bin/sh
# Gforce Node Agent installer (macOS / Linux).
#
# One-shot usage (auto-register + install service):
#   curl -sSL https://gforce.nearminds.org/install.sh | TOKEN=<token> sh
#
# Manual usage (download only):
#   curl -sSL https://gforce.nearminds.org/install.sh | sh
#   # then
#   gforce-node register --token <token> --server gforce.nearminds.org
#   gforce-node install
#
# Options (env vars):
#   GFORCE_INSTALL_DIR   where to install binaries (default: /usr/local/bin)
#   GFORCE_VERSION       specific release tag (default: latest)
#   GFORCE_SERVER        server hostname (default: gforce.nearminds.org)
#   TOKEN                one-time enrollment token — if set, we auto-run
#                        `gforce-node register` + `gforce-node install`.
#   GFORCE_NO_SERVICE    set to 1 to skip `gforce-node install` even when
#                        TOKEN is provided.

set -e

INSTALL_DIR="${GFORCE_INSTALL_DIR:-/usr/local/bin}"
REPO="nearminds/GforceNode"
VERSION="${GFORCE_VERSION:-latest}"
SERVER="${GFORCE_SERVER:-gforce.nearminds.org}"

# Detect OS and architecture
detect_platform() {
    OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
    ARCH="$(uname -m)"

    case "$OS" in
        linux)  OS="linux" ;;
        darwin) OS="darwin" ;;
        *)      echo "Unsupported OS: $OS" >&2; exit 1 ;;
    esac

    case "$ARCH" in
        x86_64|amd64)   ARCH="x86_64" ;;
        aarch64|arm64)  ARCH="aarch64" ;;
        *)              echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
    esac

    echo "${OS}-${ARCH}"
}

get_download_url() {
    PLATFORM="$1"
    if [ "$VERSION" = "latest" ]; then
        URL="https://github.com/${REPO}/releases/latest/download/gforce-node-${PLATFORM}.tar.gz"
    else
        URL="https://github.com/${REPO}/releases/download/${VERSION}/gforce-node-${PLATFORM}.tar.gz"
    fi
    echo "$URL"
}

# Run with sudo only when the target dir is not writable.
maybe_sudo() {
    if [ -w "$INSTALL_DIR" ]; then
        "$@"
    else
        sudo "$@"
    fi
}

main() {
    echo "Gforce Node Agent installer"
    echo "==========================="
    echo ""

    PLATFORM="$(detect_platform)"
    echo "Platform: $PLATFORM"

    URL="$(get_download_url "$PLATFORM")"
    echo "Download: $URL"
    echo ""

    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT

    echo "Downloading..."
    if command -v curl >/dev/null 2>&1; then
        curl -sSL "$URL" -o "$TMP_DIR/gforce-node.tar.gz"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$URL" -O "$TMP_DIR/gforce-node.tar.gz"
    else
        echo "Error: curl or wget is required" >&2
        exit 1
    fi

    echo "Extracting..."
    tar -xzf "$TMP_DIR/gforce-node.tar.gz" -C "$TMP_DIR"

    echo "Installing to $INSTALL_DIR..."
    maybe_sudo cp "$TMP_DIR/gforce-node" "$INSTALL_DIR/"
    maybe_sudo cp "$TMP_DIR/gforce-node-daemon" "$INSTALL_DIR/"
    maybe_sudo chmod +x "$INSTALL_DIR/gforce-node" "$INSTALL_DIR/gforce-node-daemon"

    mkdir -p "$HOME/.gforce-node"

    echo ""
    echo "Binaries installed."

    # Auto-register + install-service path: triggered by TOKEN env var.
    if [ -n "${TOKEN:-}" ]; then
        echo ""
        echo "Registering this machine with Gforce (server: $SERVER)..."
        "$INSTALL_DIR/gforce-node" register --token "$TOKEN" --server "$SERVER"

        if [ "${GFORCE_NO_SERVICE:-}" != "1" ]; then
            echo ""
            echo "Installing system service..."
            if [ -w "/etc/systemd/system" ] || [ -w "/Library/LaunchDaemons" ]; then
                "$INSTALL_DIR/gforce-node" install
            else
                sudo "$INSTALL_DIR/gforce-node" install
            fi
        else
            echo "Skipping service install (GFORCE_NO_SERVICE=1)."
        fi

        echo ""
        echo "Done. Check status:"
        echo "  gforce-node status"
    else
        echo ""
        echo "Next steps:"
        echo "  1. Register:  gforce-node register --token <TOKEN> --server $SERVER"
        echo "  2. Install:   gforce-node install"
        echo ""
        echo "Tip: set TOKEN=<token> before running this script to do both"
        echo "     in one command."
    fi
}

main "$@"
