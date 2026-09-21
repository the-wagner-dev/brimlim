# brimlim — design notes

The long version: what each piece is, the decisions behind it, and the
platform traps that shaped the code. [The README](../README.md) is the short
version, and is where install instructions live.

Target: Ubuntu 26.04 LTS (GNOME 50, Wayland-only). Secondary: Hyprland / KWin
/ sway.

## Architecture

Three components, no shared process and no webview anywhere:

| # | Component | Status |
|---|-----------|--------|
| 1 | `brimlimd` — Rust daemon, no GUI at all | **built** |
| 2 | GNOME Shell extension (GJS, GNOME 50) — the primary frontend | **built** |
| 3 | GTK4 + gtk4-layer-shell (Rust) — Hyprland/KWin frontend | **built**, not yet run on a layer-shell compositor |
| 4 | Packaging (.deb, AppImage) | **built** |

The drawing logic is written twice, in GJS/Cairo and Rust/Cairo. That is
deliberate: the Cairo API is the same in both, so it is a transliteration
rather than a second implementation.

## The daemon

```bash
cargo build --release
./target/release/brimlimd            # daemon: serves the session bus
./target/release/brimlimd --json     # one snapshot on stdout, then exit
```

`--json` asks a running daemon first and only polls directly if nobody is
serving the name — the daemon has warm caches and CPU history, so its answer
is better than anything a short-lived process can produce. Force the direct
path with `--no-daemon`.

### D-Bus (session bus)

| | |
|---|---|
| Name | `org.brimlim.Daemon` |
| Path | `/org/brimlim/Daemon` |
| `GetState() -> s` | the whole state as JSON |
| `Refresh(provider_id: s)` | poll for real now; empty id means all |
| `StateChanged(s)` | emitted only when the content actually changed |

State crosses the bus as a JSON string rather than a D-Bus struct, so adding a
field never breaks a running extension.

### State

```json
{
  "schema": 1,
  "generated_at": "2026-09-20T17:30:04Z",
  "providers": [{
    "id": "claude",
    "label": "Claude",
    "headline_percent": 0.62,
    "windows": [
      {"name": "Session", "percent": 0.62, "resets_at": "2026-09-20T18:00:00Z"},
      {"name": "Weekly",  "percent": 0.31, "resets_at": "2026-09-24T00:00:00Z"}
    ],
    "fidelity": "official",
    "status": "ok",
    "activity": "busy",
    "sessions": [{"name": "repo-x", "pid": 12345, "state": "working"}],
    "updated_at": "2026-09-20T17:29:56Z",
    "message": null
  }]
}
```

* `status`: `ok | stale | needs_auth | error`
* `activity`: `idle | busy | waiting`
* `fidelity`: `official | derived | manual`
* `headline_percent` is the most constraining window, and is `null` whenever
  there is nothing real to show.

`updated_at` and `message` exist because a frontend needs the age of a number
to render staleness honestly, and a status is only useful if it can say what
went wrong.

### The rule the code is built around

**A frontend must never show an invented percentage.** Every failure degrades
into a visible status instead of a number:

* a provider that fails with a remembered reading → the real old number, with
  `status` non-`ok` and `updated_at` showing its age;
* a provider that fails with nothing remembered → `headline_percent: null`;
* a reading older than `stale_after_minutes` → `stale`;
* a window past its `resets_at` → dropped entirely, because its percentage now
  describes a period that no longer exists. If that empties a provider, the
  number goes with it.

Readings survive restarts in `~/.local/state/brimlim/readings.json`; sessions
and activity are not persisted, because they would be lies the moment they were
written down.

## Providers

Both v1 providers are `official` — they report numbers the vendor produced, or
no number at all. More providers go behind the same `UsageProvider` trait.

**`claude`** — limits from `https://api.anthropic.com/api/oauth/usage`, asked
for no more often than once every five minutes: usage windows are hours and
days long, so there is nothing to gain from asking more often, and a 429 costs
more than a slightly older number. A rate-limited answer is staleness, not an
error — the last reading stays on screen while the provider backs off (honouring
`Retry-After`, otherwise doubling from two minutes up to twenty). Uses
the OAuth token Claude Code already refreshed into
`~/.claude/.credentials.json`. The daemon never refreshes that token itself;
that would race the CLI for the same file, so an expired token is reported as
`needs_auth`. Sessions come from `~/.claude/sessions/<pid>.json`, the registry
Claude Code keeps of live CLIs.

**`codex`** — limits are read off disk: Codex writes the server's own
`rate_limits` payload into every rollout log under `~/.codex/sessions/`, so
there is no second API call. Sessions come from scanning `/proc`, because Codex
keeps no registry.

### Activity

There is no portable "is the assistant thinking" API, so two honest signals are
combined per session: the process is burning CPU, and its log grew recently.

| | |
|---|---|
| `working` | burning ≥ 0.12 of a core, or wrote to its log in the last 6s |
| `waiting` | alive and used within 30 min, but not computing |
| `idle` | anything else |

A provider is `busy` if any session is working, `waiting` if any is waiting.
Neither signal ever becomes a usage percentage.

## Talking to the daemon from elsewhere

### waybar

```json
"custom/brimlim": {
  "exec": "brimlimd --json",
  "return-type": "json",
  "interval": 5
}
```

waybar wants `text`/`tooltip` keys, so pipe through a formatter of your choice
(`jq`).


## The GNOME Shell extension

`gnome-extension/brimlim@the-wagner-dev.github.io`, GNOME 50, ESM, no panel indicator —
a floating actor added with `Main.layoutManager.addChrome()`.

```bash
cp -r gnome-extension/brimlim@the-wagner-dev.github.io ~/.local/share/gnome-shell/extensions/
glib-compile-schemas ~/.local/share/gnome-shell/extensions/brimlim@the-wagner-dev.github.io/schemas/
gnome-extensions enable brimlim@the-wagner-dev.github.io
```

| file | what it owns |
|------|--------------|
| `extension.js` | lifecycle only: settings, monitors, scale, overview, the daemon connection |
| `lib/client.js` | the bus client; a name watch does the reconnecting |
| `lib/notch.js` | chrome actors, the reveal state machine, who owns which pixels |
| `lib/pill.js` | the pill silhouette, authored once and transformed to four edges |
| `lib/paint.js` | the cell — ring, mark and percentage — as a pure function of a provider |
| `lib/card.js` | the hover card (St widgets + CSS) |
| `lib/announce.js` | when the notch is allowed to interrupt |
| `lib/geometry.js` | logical-pixel metrics and placement |

### Hiding

At rest the notch is collapsed to a 4×80 logical-pixel tongue. The tongue is a
separate actor that is always present and always reactive — a fully hidden
window cannot receive a hover, so "hidden" has to mean "collapsed", never
unmapped. Hovering the tongue slides the pill out over 180 ms (ease-out-cubic);
leaving collapses it after a 400 ms grace period, so clipping the corner of the
pill on the way somewhere else does not dismiss it.

Clicking the pill body pins it open until the next click. Clicking a ring asks
the daemon to refresh that provider and stops there — it never falls through
into the pin.

`auto-hide` (default), `always-visible` and `hidden` are the three modes.

### Who owns which pixels — a GNOME 50 change

The obvious mechanism is `set_input_region` on the actor. **That mechanism no
longer exists in GNOME 50.** `LayoutManager` has dropped `affectsInputRegion`,
and `_updateRegions()` now computes struts only — Mutter derives the input
region from the geometry of reactive actors instead. Passing the old parameter
is not ignored, it throws:

```
Extension brimlim@the-wagner-dev.github.io: Error: Unrecognized parameter "affectsInputRegion"
```

The intent is kept with the mechanism that remains: `reactive` is toggled
across the pill *and its rings* on every state transition, and it is dropped
**before** the collapse animation rather than after it, so the pixels under a
departing pill belong to the window underneath for the whole 160 ms. The rings
have to be switched too — Clutter picks reactive children even under a
non-reactive parent.

### Fullscreen, overview, HiDPI, monitors

* Fullscreen: hidden by default via `trackFullscreen`, with a setting to stay
  on top. Changing it re-registers the chrome actor, since `trackFullscreen` is
  fixed at `addChrome()` time.
* Overview: hidden by default, with a setting. The overview is its own layer,
  so this is an explicit choice rather than a side effect.
* HiDPI: every metric in `lib/geometry.js` is logical; the stage scale is
  learned in one place (`setScale`) and everything else is a pure function of
  it. `tools/test.js` asserts that scale 2 geometry is exactly twice scale 1.
* Monitors: bound to a chosen monitor (or the primary), relaid out on
  `monitors-changed`.

### Reveal on activity

When a session stops working or starts waiting for you, the notch comes out for
five seconds and plays a sound through Meta's sound player (libcanberra
underneath — the only supported way to make a noise from inside the Shell).
Both halves toggle separately, and the first state after startup announces
nothing, so a login does not chime once per open session.

## Tests

```bash
cargo test                  # 27 tests: providers on recorded fixtures, engine, CLI acceptance
gjs -m tools/test.js        # 20 tests: announce policy, HiDPI geometry, formatting, palette
gjs -m tools/render-preview.js out.png 2    # renders the Cairo drawing without a Shell
```

The extension's widgets need a stage, so they are exercised by loading the
extension into a nested headless Shell on a private bus:

```bash
gnome-shell --headless --virtual-monitor 1280x800 --wayland --no-x11
```

Enabling it there and killing the daemon underneath it is how the
"daemon dies, Shell survives" criterion is checked.


## The layer-shell frontend

`crates/brimlim-gtk`, for Hyprland, KWin, sway — anything that implements
wlr-layer-shell. GNOME does not and has said it will not, which is the whole
reason there are two frontends rather than one.

```bash
brimlim-gtk --edge right --mode auto-hide
```

Run it on GNOME and it says so and exits 1, rather than dying inside GDK.

The same shapes, the same colour grade, the same reveal behaviour as the
extension — and two things it can do that GNOME 50 no longer can:

* **A real input region.** `gdk::Surface::set_input_region()` maps straight
  onto `wl_surface.set_input_region`, so the collapsed notch really does hand
  its pixels back. It is recomputed on every state change and surrendered
  before the collapse animation. The card surface keeps an empty region for
  its whole life: it is there to be read, never to be hit.
* **Two surfaces.** The notch and its card are separate layer surfaces, so the
  card is not constrained by the notch's geometry.

`GtkRevealer` does the sliding. Its easing curve is not
configurable in GTK4, so the 180 ms reveal is GTK's ease rather than the
extension's ease-out-cubic.

### Keeping the two ports honest

The drawing logic is written twice, and transliteration rots. So the
extension's own output is recorded as a reference table and checked against
the real Rust functions:

```bash
gjs -m tools/dump-reference.js > crates/brimlim-gtk/fixtures/reference.json
cargo test -p brimlim-gtk
```

101 points of the colour grade and 16 edge/count combinations of the pill,
tongue and ring geometry must match exactly. They currently do.

## Packaging

```bash
./packaging/deb/build.sh        # dist/brimlim_0.1.0_amd64.deb   (4 MB)
./packaging/appimage/build.sh   # dist/brimlimd-x86_64.AppImage  (5 MB)
                                # dist/brimlim-gtk-x86_64.AppImage (27 MB)
```

The `.deb` carries both binaries, the systemd user unit, the D-Bus activation
file, and the GNOME extension with its schema in `/usr/share/glib-2.0/schemas`
(recompiled in `postinst`).

The AppImages are separate on purpose: `brimlimd` bundles to five megabytes
and is what a non-Debian user needs for a waybar module, while the GTK
frontend has to carry GTK4 with it.

There is no auto-updater. Packages are the update mechanism.

**One thing worth knowing if you touch the AppImage build:**
`linuxdeploy-plugin-gtk` exports `GDK_BACKEND=x11` into every AppImage it
builds. For a layer-shell program that is not a preference, it is a guaranteed
failure, so `brimlim-gtk` overrides it whenever `WAYLAND_DISPLAY` is set.

The icon is drawn by `tools/render-icon.js`, using the product's own Cairo
code rather than being authored separately.


## The visual

Matched against the original's own screenshot rather than described from
memory, so the numbers below are measurements, not taste:

| | |
|---|---|
| Pill | pure black, no outline — it should read as bezel that has grown over the screen, not as a panel on top of it |
| Inverted corner | 20 logical px, about a third of the pill's depth, the same ratio the original uses |
| Cell | a 38 px ring with its percentage under it, plus 4 px of room for the waiting pulse that is drawn outside the ring; 46 × 60 px in total |
| Cell spacing | centre to centre is 2.4 ring diameters — the original is much airier than a first pass assumes |
| Ring | a dark disc under the mark, the arc on the outside, rounded caps, twelve o'clock clockwise |
| Grade | green holds to 30%, yellow at 50%, orange at 70%, red at 100% |
| Card | black slab with a tail aimed at the ring, `name` + `Resets …` on one line, then the bar, then `N% Used` |

The provider marks are drawn, not typed: at 38 px a font glyph is at the mercy
of whatever the user has installed. Claude's is its burst of spokes. Codex
gets a filled six-lobed rosette with a hexagonal hole — OpenAI's silhouette,
not its knot. The knot was tried first and abandoned: an outlined knot is
mush at the nineteen pixels the mark is actually drawn at, and a shape that
survives being small is worth more than one that is accurate blown up.

Resets read as a countdown while that means something (`in 51 min`) and as a
time of day once it does not (`Thu 14:00`).

Two things the original does not show, and this one does: the live sessions
with their names, and a visible distinction between an official reading and a
derived one (derived arcs are dashed).

```bash
gjs -m tools/render-preview.js out.png 2                    # the GJS port, four edges
cargo run -p brimlim-gtk --example render -- out.png 2    # the Rust port, notch and card
```

The second one is the only way to look at the layer-shell frontend on a
machine with no layer-shell.


## Working on the extension

```bash
tools/nested-shell.sh
```

Loads the **working tree** into a throwaway GNOME Shell on a private bus and
reports whether it enabled, survived the daemon dying, tore down clean — and
what geometry the Shell actually allocated it:

```
geometry:  pill=70x218 container=70x218 visible=true rings=2
```

That last line exists because "it enabled without errors" and "it drew
something" are different claims, and the difference is a whole class of bug.
Passing `layout_manager: null` to the container removed ClutterActor's default
fixed layout, so its children were never allocated: nothing appeared, no
exception, extension state ACTIVE, log clean. The harness now fails on it.

Two things to know while iterating:

* GNOME 50 imports an extension once per Shell process
  (`await import(extensionJs.get_uri())`, no cache-busting, and the comment
  next to it says so). `ReloadExtension` re-runs enable and disable but keeps
  the cached module, so **changed JS needs a new login** on Wayland.
* `dbus-run-session` does not isolate `gsettings`: dconf is bus-activated and
  inherits the *bus daemon's* environment, so `XDG_CONFIG_HOME` has to be
  exported before `dbus-run-session`, not inside the script it runs. Getting
  this wrong writes to the live session and disables the user's extensions.
