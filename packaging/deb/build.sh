#!/usr/bin/env bash
# Builds brimlim_<version>_<arch>.deb from a release build.
#
# Hand-rolled with dpkg-deb rather than cargo-deb: the package carries three
# different kinds of thing (two binaries, a GNOME extension, a schema), and
# spelling the layout out is clearer than configuring a tool to guess it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${1:-$ROOT/dist}"
UUID="brimlim@the-wagner-dev.github.io"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
ARCH="$(dpkg --print-architecture)"
MAINTAINER="${MAINTAINER:-wagner-val <wagner-val@users.noreply.github.com>}"

cargo build --release --manifest-path "$ROOT/Cargo.toml"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
# mktemp gives 0700; the package root has to be world-readable.
chmod 755 "$STAGE"

install -Dm755 "$ROOT/target/release/brimlimd"    "$STAGE/usr/bin/brimlimd"
install -Dm755 "$ROOT/target/release/brimlim-gtk" "$STAGE/usr/bin/brimlim-gtk"

install -Dm644 "$ROOT/packaging/systemd/brimlimd.service" \
    "$STAGE/usr/lib/systemd/user/brimlimd.service"
install -Dm644 "$ROOT/packaging/dbus/org.brimlim.Daemon.service" \
    "$STAGE/usr/share/dbus-1/services/org.brimlim.Daemon.service"

# The extension goes in system-wide; its schema goes where every other
# schema lives, and postinst recompiles the cache.
EXT="$STAGE/usr/share/gnome-shell/extensions/$UUID"
mkdir -p "$EXT/lib"
install -Dm644 "$ROOT/gnome-extension/$UUID/metadata.json"  "$EXT/metadata.json"
install -Dm644 "$ROOT/gnome-extension/$UUID/extension.js"   "$EXT/extension.js"
install -Dm644 "$ROOT/gnome-extension/$UUID/prefs.js"       "$EXT/prefs.js"
install -Dm644 "$ROOT/gnome-extension/$UUID/stylesheet.css" "$EXT/stylesheet.css"
for file in "$ROOT/gnome-extension/$UUID/lib/"*.js; do
    install -Dm644 "$file" "$EXT/lib/$(basename "$file")"
done
install -Dm644 "$ROOT/gnome-extension/$UUID/schemas/org.gnome.shell.extensions.brimlim.gschema.xml" \
    "$STAGE/usr/share/glib-2.0/schemas/org.gnome.shell.extensions.brimlim.gschema.xml"

# The GTK frontend gets a launcher and an icon. It is hidden on GNOME,
# where it refuses to run: the Shell extension is the frontend there.
install -Dm644 "$ROOT/assets/brimlim.png" \
    "$STAGE/usr/share/icons/hicolor/256x256/apps/brimlim.png"
install -Dm644 /dev/stdin "$STAGE/usr/share/applications/brimlim-gtk.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=brimlim
GenericName=AI usage notch
Comment=Usage limits and agent activity for AI coding assistants
Exec=brimlim-gtk
Icon=brimlim
Categories=Utility;Monitor;
Terminal=false
NotShowIn=GNOME;
StartupNotify=false
EOF

install -Dm644 "$ROOT/README.md" "$STAGE/usr/share/doc/brimlim/README.md"
install -Dm644 "$ROOT/config.example.toml" "$STAGE/usr/share/doc/brimlim/config.example.toml"

# Policy files. Both are what a user — and lintian — expects to find under
# /usr/share/doc for any package that is not built from a debian/ directory.
install -Dm644 /dev/stdin "$STAGE/usr/share/doc/brimlim/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: brimlim
Source: https://github.com/the-wagner-dev/brimlim

Files: *
Copyright: 2026 brimlim contributors
License: MIT

License: MIT
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files (the "Software"), to deal
 in the Software without restriction, including without limitation the rights
 to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 copies of the Software, and to permit persons to whom the Software is
 furnished to do so, subject to the following conditions:
 .
 The above copyright notice and this permission notice shall be included in
 all copies or substantial portions of the Software.
 .
 THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
 FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
 IN THE SOFTWARE.
EOF

printf '%s\n' \
    "brimlim ($VERSION) unstable; urgency=medium" \
    "" \
    "  * Release $VERSION. See https://github.com/the-wagner-dev/brimlim/releases" \
    "" \
    " -- $MAINTAINER  $(date -R)" \
    | gzip -9n > "$STAGE/usr/share/doc/brimlim/changelog.Debian.gz"
chmod 644 "$STAGE/usr/share/doc/brimlim/changelog.Debian.gz"

mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: brimlim
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Homepage: https://github.com/the-wagner-dev/brimlim
Depends: libc6, libgcc-s1, libgtk-4-1, libgtk4-layer-shell0, libcairo2, libpango-1.0-0, libpangocairo-1.0-0, libglib2.0-0t64 | libglib2.0-0
Recommends: gnome-shell, libcanberra-gtk3-module
Maintainer: $MAINTAINER
Description: Usage-limit overlay for AI coding assistants
 brimlimd polls each assistant for how much of its limits you have burned
 and whether an agent is working right now, and publishes that on the session
 bus. Two frontends read it: a GNOME Shell extension, and a GTK4 layer-shell
 notch for Hyprland and KWin.
 .
 A provider that cannot be read degrades to a visible status. It never shows
 a percentage the source did not return.
EOF

cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "configure" ]; then
    if command -v glib-compile-schemas >/dev/null 2>&1; then
        glib-compile-schemas /usr/share/glib-2.0/schemas || true
    fi
    if command -v systemctl >/dev/null 2>&1; then
        systemctl --global enable brimlimd.service >/dev/null 2>&1 || true
    fi
fi
EOF

cat > "$STAGE/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "remove" ] && command -v systemctl >/dev/null 2>&1; then
    systemctl --global disable brimlimd.service >/dev/null 2>&1 || true
fi
EOF

cat > "$STAGE/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "remove" ] && command -v glib-compile-schemas >/dev/null 2>&1; then
    glib-compile-schemas /usr/share/glib-2.0/schemas || true
fi
EOF

chmod 755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/prerm" "$STAGE/DEBIAN/postrm"

mkdir -p "$OUT"
DEB="$OUT/brimlim_${VERSION}_${ARCH}.deb"
dpkg-deb --root-owner-group --build "$STAGE" "$DEB" >/dev/null
echo "$DEB"
