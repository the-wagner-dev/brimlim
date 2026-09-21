# Contributing

## Getting set up

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libgtk4-layer-shell-dev \
                 libglib2.0-dev-bin gjs gir1.2-gtk-4.0
cargo build
```

Everything CI checks, you can run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all
gjs -m tools/test.js
```

## The one rule

**Never show a percentage that a provider did not produce.** If a reading is
missing, old, or unreadable, the state carries that as a status and the
frontends draw it as one. A patch that fills a gap with an estimate, an
average, or a token count will not be merged, however well it reads on screen.
[docs/design.md](docs/design.md) explains what each failure looks like.

## The drawing code exists twice

`gnome-extension/…/lib/paint.js` (GJS/Cairo) and
`crates/brimlim-gtk/src/paint.rs` (Rust/Cairo) draw the same pixels for the
two frontends. Change one and you must change the other, then regenerate the
table that keeps them honest:

```bash
gjs -m tools/dump-reference.js > crates/brimlim-gtk/fixtures/reference.json
cargo test -p brimlim-gtk
```

CI regenerates that file and fails on any difference, so a one-sided change
cannot land.

## Working on the GNOME extension

```bash
tools/nested-shell.sh
```

loads the working tree into a throwaway GNOME Shell on a private bus and
reports whether it enabled, survived the daemon dying, tore down clean, and
what geometry the Shell actually allocated it. Two things that will cost you
an hour otherwise are written up in
[docs/design.md](docs/design.md#working-on-the-extension): GNOME 50 caches the
extension module for the life of the Shell process, and `dbus-run-session`
does not isolate `gsettings` unless `XDG_CONFIG_HOME` is exported *before* it.

## Building the packages

```bash
./packaging/deb/build.sh
./packaging/gnome-extension/build.sh
./packaging/appimage/fetch-tools.sh && ./packaging/appimage/build.sh
```

The AppImage build needs `librsvg2-common` and `libgdk-pixbuf2.0-bin` as
well: `linuxdeploy-plugin-gtk` bundles the GTK pieces it finds on the build
machine rather than the ones the binary links against, and it reports what is
missing as a path rather than as a package.

## Adding a provider

Implement `UsageProvider` in `crates/brimlimd/src/providers/`, and add a
recorded fixture next to the existing ones in `crates/brimlimd/fixtures/` —
every provider is tested against a real captured payload rather than a mock.
Set `fidelity` honestly: `official` only if the vendor produced the number.

## Pull requests

Small and focused, with a note on what you ran. If it touches how something
looks, a before/after from `tools/render-preview.js` or the Rust `render`
example saves a lot of back and forth.

By contributing you agree that your work is licensed under the MIT License.
