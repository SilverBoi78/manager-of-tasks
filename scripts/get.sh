#!/usr/bin/env sh
# One-line installer for Manager of Tasks:
#
#   curl -fsSL https://raw.githubusercontent.com/SilverBoi78/manager-of-tasks/main/scripts/get.sh | sh
#
# Downloads the latest prebuilt release, verifies its SHA-256 checksum and installs
# it to ~/.local (override with PREFIX=/some/prefix). To remove it again:
#
#   curl -fsSL https://raw.githubusercontent.com/SilverBoi78/manager-of-tasks/main/scripts/get.sh | sh -s -- --uninstall
#
# Set VERSION=v0.1.0 to install a specific release instead of the latest.
set -eu

REPO="SilverBoi78/manager-of-tasks"
PREFIX="${PREFIX:-$HOME/.local}"
VERSION="${VERSION:-latest}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$PREFIX/bin/manager-of-tasks" \
        "$PREFIX/share/applications/manager-of-tasks.desktop" \
        "$PREFIX/share/icons/hicolor/scalable/apps/manager-of-tasks.svg"
    say "Removed Manager of Tasks from $PREFIX"
    exit 0
fi

[ "$(uname -s)" = "Linux" ] || die "Manager of Tasks only runs on Linux."
case "$(uname -m)" in
    x86_64 | amd64) ARCH=x86_64 ;;
    *) die "No prebuilt binary for $(uname -m) yet. Build from source instead: https://github.com/$REPO#build-from-source" ;;
esac

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
else
    die "curl or wget is required."
fi
command -v tar >/dev/null 2>&1 || die "tar is required."
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required."

ASSET="manager-of-tasks-$ARCH-linux.tar.gz"
if [ "$VERSION" = "latest" ]; then
    BASE="https://github.com/$REPO/releases/latest/download"
else
    BASE="https://github.com/$REPO/releases/download/$VERSION"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

say "Downloading $ASSET ($VERSION)..."
fetch "$BASE/$ASSET" "$TMP/$ASSET" || die "download failed: $BASE/$ASSET"
fetch "$BASE/$ASSET.sha256" "$TMP/$ASSET.sha256" || die "checksum download failed"

say "Verifying checksum..."
(cd "$TMP" && sha256sum -c "$ASSET.sha256" >/dev/null) || die "checksum mismatch; the download may be corrupted."

tar -xzf "$TMP/$ASSET" -C "$TMP"
PREFIX="$PREFIX" sh "$TMP/manager-of-tasks/scripts/install.sh"
