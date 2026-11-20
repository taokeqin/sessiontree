#!/bin/sh
set -eu

REPO="taokeqin/sessiontree"
APP="Sessiontree"
VERSION=""
DIR=""

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

info() {
    printf '%s\n' "$*"
}

usage() {
    cat <<EOF
Install $APP from GitHub Releases.

Usage:
  install.sh [options]

Options:
  --version <tag>  Install a specific release, for example v0.1.0
  --dir <path>     Install under this directory
                   macOS: the folder holding Sessiontree.app (default ~/Applications)
                   Linux: the folder holding Sessiontree/ (default ~/.local/share)
  -h, --help       Show this help

Examples:
  curl -fsSL https://raw.githubusercontent.com/taokeqin/sessiontree/main/install.sh | sh
  curl -fsSL https://raw.githubusercontent.com/taokeqin/sessiontree/main/install.sh | sh -s -- --version v0.1.0
  curl -fsSL https://raw.githubusercontent.com/taokeqin/sessiontree/main/install.sh | sh -s -- --dir /Applications
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || die "--version needs a value"
            VERSION="$2"
            shift 2
            ;;
        --version=*)
            VERSION="${1#*=}"
            shift
            ;;
        --dir)
            [ "$#" -ge 2 ] || die "--dir needs a value"
            DIR="$2"
            shift 2
            ;;
        --dir=*)
            DIR="${1#*=}"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown option: $1 (try --help)"
            ;;
    esac
done

if [ -n "$VERSION" ]; then
    case "$VERSION" in
        v*) ;;
        *) VERSION="v$VERSION" ;;
    esac
fi

if [ -n "$DIR" ]; then
    case "$DIR" in
        /*) ;;
        *) DIR="$(pwd)/$DIR" ;;
    esac
fi

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin)
        ASSET="sessiontree-macos-universal.zip"
        ;;
    Linux)
        ASSET="sessiontree-linux-x86_64.tar.gz"
        case "$ARCH" in
            x86_64|amd64) ;;
            *) die "Linux packages are x86_64 only (found $ARCH)" ;;
        esac
        ;;
    *)
        die "unsupported operating system: $OS (on Windows use install.ps1)"
        ;;
esac

if [ -n "$VERSION" ]; then
    BASE="https://github.com/$REPO/releases/download/$VERSION"
else
    BASE="https://github.com/$REPO/releases/latest/download"
fi

command -v curl >/dev/null 2>&1 || die "curl is required"

TMP="$(mktemp -d "${TMPDIR:-/tmp}/sessiontree.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT INT TERM

info "Downloading $APP..."
curl -fL --retry 3 --silent --show-error "$BASE/$ASSET" -o "$TMP/$ASSET"
curl -fL --retry 3 --silent --show-error "$BASE/SHA256SUMS.txt" -o "$TMP/SHA256SUMS.txt"

EXPECTED="$(awk -v asset="$ASSET" '$2 == asset { print $1; exit }' "$TMP/SHA256SUMS.txt")"
[ -n "$EXPECTED" ] || die "SHA256SUMS.txt does not list $ASSET"

if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "$TMP/$ASSET" | awk '{ print $1 }')"
elif command -v shasum >/dev/null 2>&1; then
    ACTUAL="$(shasum -a 256 "$TMP/$ASSET" | awk '{ print $1 }')"
else
    die "sha256sum or shasum is required"
fi

[ "$ACTUAL" = "$EXPECTED" ] || die "checksum mismatch for $ASSET"

if [ "$OS" = "Darwin" ]; then
    DEST_DIR="${DIR:-$HOME/Applications}"
    mkdir -p "$DEST_DIR" || die "cannot create $DEST_DIR"
    [ -w "$DEST_DIR" ] || die "$DEST_DIR is not writable; try --dir ~/Applications or sudo"

    ditto -x -k "$TMP/$ASSET" "$TMP/unpacked"
    [ -d "$TMP/unpacked/$APP.app" ] || die "the release does not contain $APP.app"

    rm -rf "$DEST_DIR/$APP.app"
    mv "$TMP/unpacked/$APP.app" "$DEST_DIR/$APP.app"

    info "Installed $DEST_DIR/$APP.app"
    info "Open it with: open \"$DEST_DIR/$APP.app\""
else
    PARENT="${DIR:-$HOME/.local/share}"
    DEST="$PARENT/$APP"
    mkdir -p "$PARENT" || die "cannot create $PARENT"

    tar -xzf "$TMP/$ASSET" -C "$TMP"
    [ -d "$TMP/$APP" ] || die "the release does not contain $APP/"

    rm -rf "$DEST"
    mv "$TMP/$APP" "$DEST"

    BIN_DIR="$HOME/.local/bin"
    mkdir -p "$BIN_DIR"
    ln -sfn "$DEST/$APP" "$BIN_DIR/sessiontree"

    APPS_DIR="$HOME/.local/share/applications"
    mkdir -p "$APPS_DIR"
    sed "s|^Exec=.*|Exec=\"$DEST/$APP\"|" \
        "$DEST/com.hacken.sessiontree.desktop" >"$APPS_DIR/com.hacken.sessiontree.desktop"

    ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
    mkdir -p "$ICON_DIR"
    cp "$DEST/com.hacken.sessiontree.png" "$ICON_DIR/com.hacken.sessiontree.png"

    command -v update-desktop-database >/dev/null 2>&1 &&
        update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 &&
        gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true

    info "Installed $DEST"
    info "Run it with: sessiontree"
    case ":$PATH:" in
        *":$BIN_DIR:"*) ;;
        *) info "Add $BIN_DIR to PATH to run 'sessiontree' from a terminal." ;;
    esac
fi
