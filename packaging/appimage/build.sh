#!/usr/bin/env bash
# Builds two AppImages: the daemon, and the layer-shell frontend.
#
# They are separate on purpose. brimlimd is a plain Rust binary that bundles
# to a couple of megabytes and is what a non-Debian user actually needs for a
# waybar module; brimlim-gtk has to carry GTK4 with it, which is a different
# kind of package and a much larger one.
#
# Needs: appimagetool, linuxdeploy and linuxdeploy-plugin-gtk.sh on PATH
# (or in $TOOLS). Set APPIMAGE_EXTRACT_AND_RUN=1 where FUSE is unavailable.
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

appimagetool "$DAEMON_DIR" "$OUT/brimlimd-x86_64.AppImage" >/dev/null 2>&1
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

linuxdeploy \
    --appdir "$GTK_DIR" \
    --executable "$ROOT/target/release/brimlim-gtk" \
    --desktop-file "$(dirname "$GTK_DIR")/brimlim-gtk.desktop" \
    --icon-file "$ROOT/assets/brimlim.png" \
    --plugin gtk \
    --output appimage >/dev/null 2>&1

mv ./Brimlim*.AppImage "$OUT/brimlim-gtk-x86_64.AppImage" 2>/dev/null \
    || mv ./brimlim*gtk*.AppImage "$OUT/brimlim-gtk-x86_64.AppImage"
rm -rf "$(dirname "$GTK_DIR")"
echo "$OUT/brimlim-gtk-x86_64.AppImage"
