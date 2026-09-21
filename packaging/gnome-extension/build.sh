#!/usr/bin/env bash
# Builds the zip that extensions.gnome.org accepts.
#
# `gnome-extensions pack` is the canonical tool: it enforces the layout the
# review site expects (metadata.json at the root, no wrapping directory) and
# fails loudly on a schema it cannot compile, which is exactly the class of
# mistake that otherwise comes back as a rejection days later.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${1:-$ROOT/dist}"
UUID="brimlim@the-wagner-dev.github.io"
SRC="$ROOT/gnome-extension/$UUID"

mkdir -p "$OUT"

# The version the site sorts releases by is an integer; version-name is the
# human one. Keep them in step with the workspace version at pack time rather
# than hand-editing metadata.json for every release.
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp -r "$SRC" "$STAGE/$UUID"

python3 - "$STAGE/$UUID/metadata.json" "$VERSION" <<'PY'
import json, sys
path, version = sys.argv[1], sys.argv[2]
with open(path) as fh:
    meta = json.load(fh)
meta["version-name"] = version
with open(path, "w") as fh:
    json.dump(meta, fh, indent=2)
    fh.write("\n")
PY

ZIP="$OUT/$UUID.shell-extension.zip"
rm -f "$ZIP"

if command -v gnome-extensions >/dev/null 2>&1; then
    gnome-extensions pack "$STAGE/$UUID" \
        --extra-source=lib \
        --schema="schemas/org.gnome.shell.extensions.brimlim.gschema.xml" \
        --out-dir="$OUT" >/dev/null
else
    # `gnome-extensions` lives in the gnome-shell package, which is far too
    # much to install on a build machine that will never run a Shell. The
    # layout it produces is a flat zip, so produce that directly.
    (cd "$STAGE/$UUID" && zip -qr "$ZIP" \
        metadata.json extension.js prefs.js stylesheet.css lib schemas \
        -x '*/gschemas.compiled')
fi

echo "$ZIP"
