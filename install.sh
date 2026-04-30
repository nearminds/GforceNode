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
#   GFORCE_SKIP_VERIFY   set to 1 to skip SHA256 verification (do not use in
#                        production — the verify step exists to catch the
#                        "GitHub returned a 404 HTML page that tar will then
#                        try to extract" failure mode).
#   TOKEN                one-time enrollment token — if set, we auto-run
#                        `gforce-node register` + `gforce-node install`.
#   GFORCE_NO_SERVICE    set to 1 to skip `gforce-node install` even when
#                        TOKEN is provided.

set -e

INSTALL_DIR="${GFORCE_INSTALL_DIR:-/usr/local/bin}"
REPO="nearminds/GforceNode"
VERSION="${GFORCE_VERSION:-latest}"
SERVER="${GFORCE_SERVER:-gforce.nearminds.org}"

# Anything smaller than this is almost certainly a GitHub error page
# rather than an archive. The smallest legitimate release tarball we ship
# is around 4 MB; 1 KB is a generous floor that will never trip on a
# real binary but always trips on "Not Found" (9 bytes), HTML error
# pages (~few hundred bytes), and rate-limit responses.
MIN_ARCHIVE_BYTES=1024

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

base_url() {
    if [ "$VERSION" = "latest" ]; then
        echo "https://github.com/${REPO}/releases/latest/download"
    else
        echo "https://github.com/${REPO}/releases/download/${VERSION}"
    fi
}

# Download a URL to a file using whichever fetcher is available. Echos
# the HTTP status code on stdout so the caller can sanity-check it.
fetch() {
    URL="$1"
    OUT="$2"
    if command -v curl >/dev/null 2>&1; then
        # -f makes curl exit non-zero on 4xx/5xx so we don't silently
        # save the error body. We still capture the code for the caller.
        curl -fsSL -o "$OUT" -w "%{http_code}" "$URL" 2>/dev/null
    elif command -v wget >/dev/null 2>&1; then
        # wget doesn't have an equivalent of curl's -w, so we infer status
        # from exit code: 0 on success, anything else on failure.
        if wget -q "$URL" -O "$OUT"; then
            echo "200"
        else
            echo "000"
        fi
    else
        echo "Error: curl or wget is required" >&2
        exit 1
    fi
}

# `sha256sum` (GNU) on Linux, `shasum -a 256` on macOS. Both accept the
# same input format ("<hash>  <filename>"), so we just need a thin shim.
sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        echo "Error: sha256sum or shasum is required for verification. Set GFORCE_SKIP_VERIFY=1 to skip (not recommended)." >&2
        exit 1
    fi
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

    BASE="$(base_url)"
    ARCHIVE_URL="${BASE}/gforce-node-${PLATFORM}.tar.gz"
    SUMS_URL="${BASE}/sha256sums.txt"
    echo "Download: $ARCHIVE_URL"
    echo ""

    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT

    echo "Downloading..."
    code="$(fetch "$ARCHIVE_URL" "$TMP_DIR/gforce-node.tar.gz")"
    if [ "$code" != "200" ]; then
        echo "Error: archive download returned HTTP $code from $ARCHIVE_URL" >&2
        echo "Hint: check that a release exists at https://github.com/${REPO}/releases" >&2
        exit 1
    fi

    # Sanity-check the size before handing the file to tar. This catches
    # the "GitHub returned a tiny error page" class of failures before
    # tar produces an opaque error like "Unrecognized archive format".
    size="$(wc -c < "$TMP_DIR/gforce-node.tar.gz" | tr -d ' ')"
    if [ "$size" -lt "$MIN_ARCHIVE_BYTES" ]; then
        echo "Error: downloaded file is only $size bytes — that's not a real archive." >&2
        echo "First 200 bytes of the response (likely a GitHub error message):" >&2
        head -c 200 "$TMP_DIR/gforce-node.tar.gz" >&2
        echo "" >&2
        exit 1
    fi

    # SHA256 verification — protects against MITM, partial downloads, and
    # the scenario where the tag exists but its assets were uploaded in
    # the wrong order or replaced. Skipping is supported but loud.
    if [ "${GFORCE_SKIP_VERIFY:-}" = "1" ]; then
        echo "Skipping SHA256 verification (GFORCE_SKIP_VERIFY=1)."
    else
        echo "Verifying SHA256..."
        code="$(fetch "$SUMS_URL" "$TMP_DIR/sha256sums.txt")"
        if [ "$code" != "200" ]; then
            echo "Error: sha256sums.txt returned HTTP $code from $SUMS_URL" >&2
            echo "Hint: this release predates checksum support. Re-cut it, or set GFORCE_SKIP_VERIFY=1." >&2
            exit 1
        fi
        actual="$(sha256_file "$TMP_DIR/gforce-node.tar.gz")"
        # Find the line matching our archive name, take the first field.
        expected="$(grep "gforce-node-${PLATFORM}.tar.gz" "$TMP_DIR/sha256sums.txt" | awk '{print $1}' | head -n1)"
        if [ -z "$expected" ]; then
            echo "Error: no checksum line for gforce-node-${PLATFORM}.tar.gz in sha256sums.txt" >&2
            exit 1
        fi
        if [ "$actual" != "$expected" ]; then
            echo "Error: SHA256 mismatch." >&2
            echo "  expected: $expected" >&2
            echo "  actual:   $actual" >&2
            exit 1
        fi
        echo "Checksum OK ($actual)."
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
