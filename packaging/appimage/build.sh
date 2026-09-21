#!/usr/bin/env bash
# Builds two AppImages: the daemon, and the layer-shell frontend.
#
# They are separate on purpose. brimlimd is a plain Rust binary that bundles
# to a couple of megabytes and is what a non-Debian user actually needs for a
# waybar module; brimlim-gtk has to carry GTK4 with it, which is a different
# kind of package and a much larger one.
#
# Needs: appimagetool, linuxdeploy and linuxdeploy-plugin-gtk.sh on PATH
# (or in $TOOLS) — packaging/appimage/fetch-tools.sh puts them there. Set
# APPIMAGE_EXTRACT_AND_RUN=1 where FUSE is unavailable.
#
# It also needs the GTK stack's *optional* pieces present on the build
# machine, because the plugin bundles what it finds rather than what it can
# link: librsvg2-common and libgdk-pixbuf2.0-bin, for the pixbuf loaders and
# the tool that indexes them. A desktop has them; a container does not, and
# the plugin's failure names a path rather than a package.
#
# The tools' own output is left on stdout on purpose. linuxdeploy fails for
# a dozen environmental reasons, every one of which it explains, and none of
# which can be guessed from an exit code alone.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${1:-$ROOT/dist}"
TOOLS="${TOOLS:-$ROOT/.tools}"
export APPIMAGE_EXTRACT_AND_RUN="${APPIMAGE_EXTRACT_AND_RUN:-1}"
export PATH="$TOOLS:$PATH"

cargo build --release --manifest-path "$ROOT/Cargo.toml"
mkdir -p "$OUT"

# --- brimlimd -------------------------------------------------------------

DAEMON_DIR="$(mktemp -d)/brimlimd.AppDir"
trap 'rm -rf "$(dirname "$DAEMON_DIR")"' EXIT
mkdir -p "$DAEMON_DIR/usr/bin"

install -Dm755 "$ROOT/target/release/brimlimd" "$DAEMON_DIR/usr/bin/brimlimd"
install -Dm644 "$ROOT/assets/brimlim.png" "$DAEMON_DIR/brimlim.png"
install -Dm644 "$ROOT/assets/brimlim.png" \
    "$DAEMON_DIR/usr/share/icons/hicolor/256x256/apps/brimlim.png"

cat > "$DAEMON_DIR/brimlimd.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Brimlim daemon
Comment=Usage limits and agent activity for AI coding assistants
Exec=brimlimd
Icon=brimlim
Categories=Utility;
Terminal=true
NoDisplay=true
EOF

cat > "$DAEMON_DIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/brimlimd" "$@"
EOF
chmod 755 "$DAEMON_DIR/AppRun"

appimagetool "$DAEMON_DIR" "$OUT/brimlimd-x86_64.AppImage"
echo "$OUT/brimlimd-x86_64.AppImage"

# --- brimlim-gtk ----------------------------------------------------------

GTK_DIR="$(mktemp -d)/AppDir"
mkdir -p "$GTK_DIR"

cat > "$(dirname "$GTK_DIR")/brimlim-gtk.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Brimlim
Comment=Usage limits and agent activity for AI coding assistants
Exec=brimlim-gtk
Icon=brimlim
Categories=Utility;
Terminal=false
EOF

# linuxdeploy-plugin-gtk copies $libdir/gtk-4.0 unconditionally, and GTK 4
# itself ships nothing there: immodules, print backends and media backends
# all come from optional packages. On a minimal system — a CI container, say
# — the directory simply does not exist and the plugin aborts with
# `cp: cannot stat 'gtk-4.0'`. The notch needs none of those modules: it has
# no text entry, nothing to print and nothing to play. So an empty directory
# is the entire fix.
GTK4_MODULES="$(pkg-config --variable=libdir gtk4)/gtk-4.0"
if [ ! -d "$GTK4_MODULES" ] && ! mkdir -p "$GTK4_MODULES" 2>/dev/null; then
    echo "linuxdeploy-plugin-gtk requires $GTK4_MODULES to exist." >&2
    echo "Create it (empty is fine) or install a package shipping GTK4 modules." >&2
    exit 1
fi

linuxdeploy \
    --appdir "$GTK_DIR" \
    --executable "$ROOT/target/release/brimlim-gtk" \
    --desktop-file "$(dirname "$GTK_DIR")/brimlim-gtk.desktop" \
    --icon-file "$ROOT/assets/brimlim.png" \
    --plugin gtk \
    --output appimage

mv ./Brimlim*.AppImage "$OUT/brimlim-gtk-x86_64.AppImage" 2>/dev/null \
    || mv ./brimlim*gtk*.AppImage "$OUT/brimlim-gtk-x86_64.AppImage"
rm -rf "$(dirname "$GTK_DIR")"
echo "$OUT/brimlim-gtk-x86_64.AppImage"
