#!/usr/bin/env bash
# Downloads the three AppImage build tools into .tools/, which is where
# packaging/appimage/build.sh looks for them. Kept separate from the build so
# that CI and a laptop fetch the same binaries from the same places, and so a
# rebuild does not need the network.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOLS="${TOOLS:-$ROOT/.tools}"
mkdir -p "$TOOLS"

fetch() {
    local url="$1" dest="$2"
    [ -x "$dest" ] && { echo "have $(basename "$dest")"; return; }
    curl -fsSL --retry 3 -o "$dest" "$url"
    chmod +x "$dest"
    echo "fetched $(basename "$dest")"
}

fetch https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage \
    "$TOOLS/appimagetool"
fetch https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage \
    "$TOOLS/linuxdeploy"
fetch https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh \
    "$TOOLS/linuxdeploy-plugin-gtk.sh"
