#!/usr/bin/env sh
# Install Manager of Tasks for the current user (or system-wide with PREFIX=/usr/local and sudo).
#
# From a source checkout it builds with cargo first. From an extracted release
# archive it installs the prebuilt binary that sits next to this script's folder.
#
#   ./scripts/install.sh              # installs to ~/.local
#   ./scripts/install.sh --uninstall  # removes it again
set -eu

PREFIX="${PREFIX:-$HOME/.local}"
cd "$(dirname "$0")/.."

BIN="$PREFIX/bin/manager-of-tasks"
DESKTOP="$PREFIX/share/applications/manager-of-tasks.desktop"
ICON="$PREFIX/share/icons/hicolor/scalable/apps/manager-of-tasks.svg"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN" "$DESKTOP" "$ICON"
    echo "Removed Manager of Tasks from $PREFIX"
    exit 0
fi

if [ -f Cargo.toml ]; then
    cargo build --release
    SRC=target/release/manager-of-tasks
elif [ -x manager-of-tasks ]; then
    SRC=manager-of-tasks
else
    echo "error: no Cargo.toml or prebuilt manager-of-tasks binary found in $(pwd)" >&2
    exit 1
fi

install -Dm755 "$SRC" "$BIN"
install -Dm644 assets/manager-of-tasks.desktop "$DESKTOP"
install -Dm644 assets/manager-of-tasks.svg "$ICON"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true

echo "Installed Manager of Tasks to $BIN"
case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "Note: $PREFIX/bin is not in your PATH. Launch it from your app menu or add it to PATH." ;;
esac
