#!/usr/bin/env sh
# Build and install Manager of Tasks for the current user (or system-wide with PREFIX=/usr/local and sudo).
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

cargo build --release
install -Dm755 target/release/manager-of-tasks "$BIN"
install -Dm644 assets/manager-of-tasks.desktop "$DESKTOP"
install -Dm644 assets/manager-of-tasks.svg "$ICON"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true

echo "Installed Manager of Tasks to $BIN"
case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "Note: $PREFIX/bin is not in your PATH." ;;
esac
