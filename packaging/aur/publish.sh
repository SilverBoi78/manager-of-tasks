#!/usr/bin/env bash
# Push the AUR packages in this folder to the AUR.
# Requires an AUR account with your SSH key added (see README.md here).
#
#   ./packaging/aur/publish.sh            # publish both packages
#   ./packaging/aur/publish.sh manager-of-tasks-bin
set -euo pipefail

cd "$(dirname "$0")"
PACKAGES=("${@:-manager-of-tasks manager-of-tasks-bin}")
WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT

for pkg in ${PACKAGES[@]}; do
    [ -f "$pkg/PKGBUILD" ] || { echo "error: no $pkg/PKGBUILD" >&2; exit 1; }
    (cd "$pkg" && makepkg --printsrcinfo > .SRCINFO)
    version="$(sed -n 's/^\tpkgver = //p' "$pkg/.SRCINFO")-$(sed -n 's/^\tpkgrel = //p' "$pkg/.SRCINFO")"

    echo "==> $pkg $version"
    git clone -q "ssh://aur@aur.archlinux.org/$pkg.git" "$WORK/$pkg"
    cp "$pkg/PKGBUILD" "$pkg/.SRCINFO" "$WORK/$pkg/"
    cd "$WORK/$pkg"
    git add PKGBUILD .SRCINFO
    if git diff --cached --quiet; then
        echo "    already up to date"
    else
        git commit -q -m "Update to $version"
        git push -q origin HEAD:master
        echo "    published: https://aur.archlinux.org/packages/$pkg"
    fi
    cd - > /dev/null
done
