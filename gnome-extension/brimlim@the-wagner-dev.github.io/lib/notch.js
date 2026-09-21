// The notch itself: chrome actors, the reveal state machine, and the input
// region that decides which pixels belong to us.
//
// Two rules shape everything here:
//
//   * Collapsing is never an unmap. A hidden window cannot receive a hover,
//     so "hidden" means collapsed down to a tongue that is always present
//     and always reactive.
//   * The pill does not slide, it grows: its box, its rounding and its
//     marks' opacity are all functions of one 0..1 reveal value, which is
//     `revealShape` in geometry.js and is shared with the GTK port through
//     the reference fixture rather than through code.
//   * Input is surrendered on every state transition, and *before* the
//     collapse animation rather than after it. A click aimed at the window
//     underneath must not be eaten by a pill that is on its way out.
//
// GNOME 50 note: LayoutManager lost `affectsInputRegion`, and _updateRegions()
// now derives struts only — Mutter picks input from the geometry of reactive
// actors instead. So the input region is controlled the only way that still
// exists: by toggling `reactive` across the pill and its rings, and by moving
// the pill out of the container's clip when it is away.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {Card} from './card.js';
import {
    Edge, Metrics, Timing, cellSize, cellWidth, clamp01, isVertical, pillSize, placement,
    revealShape, ringOrigin, scaled, tongueBox,
} from './geometry.js';
import {drawPill, drawTongue} from './pill.js';
import {Ring} from './ring.js';

export const Mode = {
    AUTO_HIDE: 'auto-hide',
    ALWAYS_VISIBLE: 'always-visible',
    HIDDEN: 'hidden',
};

export class Notch {
    constructor({onRefresh}) {
        this._onRefresh = onRefresh;

        this._edge = Edge.RIGHT;
        this._mode = Mode.AUTO_HIDE;
        this._monitorIndex = -1;
        this._showInFullscreen = false;

        this._providers = [];
        this._rings = new Map();
        this._expanded = false;
        this._pinned = false;
        this._suppressed = false;
        this._collapseTimer = 0;
        this._revealTimer = 0;
        this._revealTimeline = null;
        this._reveal = 0;
        this._shape = null;
        this._size = null;
        this._tongueWanted = false;
        this._unavailable = null;

        this._buildActors();
    }

    // -- construction ----------------------------------------------------

    _buildActors() {
        // No layout_manager override: ClutterActor's default is a fixed
        // layout, which is what places the pill and the tongue at the
        // coordinates we compute. Passing null here removes that default and
        // the children are then never allocated — they simply do not appear,
        // with no error anywhere to say why.
        this._container = new St.Widget({name: 'brimlim', reactive: false});

        this._pill = new St.Widget({reactive: true, track_hover: true});
        this._pillBackground = new St.DrawingArea({x_expand: true, y_expand: true});
        this._pillBackground.connect('repaint', area => this._repaintPill(area));
        this._pill.add_child(this._pillBackground);

        // No layout manager: cells are placed at the coordinates both ports
        // agree on, which is also what the GTK frontend hit-tests against.
        this._ringBox = new St.Widget();
        this._pill.add_child(this._ringBox);

        this._tongue = new St.Widget({reactive: true, track_hover: true});
        this._tongueArea = new St.DrawingArea({x_expand: true, y_expand: true});
        this._tongueArea.connect('repaint', area => this._repaintTongue(area));
        this._tongue.add_child(this._tongueArea);

        this._container.add_child(this._pill);
        this._container.add_child(this._tongue);

        this._card = new Card();

        if (GLib.getenv('BRIMLIM_DEBUG')) {
            // Allocation is the thing that silently fails. Report it as the
            // Shell actually computes it, not as we hoped to set it — and
            // report the pill's own visibility with it, because a collapsed
            // notch is now an unmapped pill rather than a small one, and
            // "unmapped" is the claim the harness has to be able to check.
            const report = () => {
                const box = this._pill.get_allocation_box();
                console.log(`BRIMLIM-GEOMETRY pill=${Math.round(box.get_width())}x${Math.round(box.get_height())} ` +
                    `shown=${this._pill.visible} ` +
                    `container=${this._container.width}x${this._container.height} ` +
                    `visible=${this._container.visible} rings=${this._rings.size}`);
            };
            this._pill.connect('notify::allocation', report);
            this._pill.connect('notify::visible', report);
        }

        this._pill.connect('enter-event', () => this._onPointerIn());
        this._pill.connect('leave-event', () => this._onPointerOut());
        this._tongue.connect('enter-event', () => this._onPointerIn());
        this._tongue.connect('leave-event', () => this._onPointerOut());
        this._pill.connect('button-press-event', () => this._togglePin());

        Main.layoutManager.addChrome(this._container, {
            affectsStruts: false,
            trackFullscreen: !this._showInFullscreen,
        });
        Main.layoutManager.addChrome(this._card, {
            affectsStruts: false,
            trackFullscreen: true,
        });
        this._setInteractive(false);
    }

    destroy() {
        this._clearTimer('_collapseTimer');
        this._clearTimer('_revealTimer');
        this._stopReveal();
        this._setInteractive(false);

        Main.layoutManager.removeChrome(this._container);
        Main.layoutManager.removeChrome(this._card);

        this._container.destroy();
        this._card.destroy();
        this._rings.clear();
    }

    // -- settings --------------------------------------------------------

    configure({edge, mode, monitor, showInFullscreen}) {
        const fullscreenChanged = showInFullscreen !== this._showInFullscreen;

        this._edge = edge;
        this._mode = mode;
        this._monitorIndex = monitor;
        this._showInFullscreen = showInFullscreen;

        if (fullscreenChanged) {
            // trackFullscreen is fixed at addChrome time, so the actor has to
            // be re-registered for the change to take.
            Main.layoutManager.removeChrome(this._container);
            Main.layoutManager.addChrome(this._container, {
                affectsStruts: false,
                trackFullscreen: !showInFullscreen,
            });
        }

        this.relayout();
    }

    setSuppressed(suppressed) {
        this._suppressed = suppressed;
        this._applyVisibility();
    }

    // -- state -----------------------------------------------------------

    /** Rebuild everything that baked in the scale factor at construction. */
    rescale() {
        this._ringBox.destroy_all_children();
        this._rings.clear();
        this._card.width = scaled(Metrics.cardWidth);
        this._syncRings();
        this.relayout();
    }

    setProviders(providers) {
        this._unavailable = null;
        this._providers = providers;
        this._syncRings();
        this.relayout();
    }

    setUnavailable(reason) {
        this._unavailable = reason;
        // Rendered through the ordinary provider path rather than a special
        // case: an unreachable daemon is just another thing with a status
        // and no number, and it should look like one.
        this._providers = [{
            id: '__daemon__',
            label: 'Brimlim',
            headline_percent: null,
            windows: [],
            fidelity: 'official',
            status: 'error',
            activity: 'idle',
            sessions: [],
            updated_at: null,
            message: reason,
        }];
        this._syncRings();
        this.relayout();
    }

    _syncRings() {
        const wanted = this._providers.map(p => p.id);
        const current = [...this._rings.keys()];
        const sameSet = wanted.length === current.length &&
            wanted.every((id, i) => id === current[i]);

        if (!sameSet) {
            this._ringBox.destroy_all_children();
            this._rings.clear();
            for (const provider of this._providers) {
                const ring = new Ring(provider);
                if (provider.id !== '__daemon__')
                ring.connect('activated', () => this._onRefresh(provider.id));
                ring.connect('notify::hover', () => this._onRingHover(ring));
                ring.reactive = this._expanded;
                this._ringBox.add_child(ring);
                this._rings.set(provider.id, ring);
            }
            return;
        }

        for (const provider of this._providers)
            this._rings.get(provider.id)?.update(provider);

        // A card left open while its provider changed must show the change.
        if (this._card.visible && this._hoveredRing)
            this._card.show_provider(this._hoveredRing.provider);
    }

    // -- layout ----------------------------------------------------------

    relayout() {
        const monitor = this._monitor();
        if (!monitor) {
            this._container.hide();
            return;
        }

        const vertical = isVertical(this._edge);

        const size = pillSize(this._edge, this._providers.length);
        const [width, height] = size;
        const spot = placement(this._edge, monitor, size);

        this._container.set_position(spot.x, spot.y);
        this._container.set_size(width, height);
        // Keep a pill that has slid away from bleeding onto a neighbouring
        // monitor; the travel happens inside the container's own box.
        this._container.set_clip(0, 0, width, height);

        let index = 0;
        for (const ring of this._rings.values()) {
            const [x, y] = ringOrigin(this._edge, index);
            ring.set_position(x, y);
            ring.set_size(scaled(cellWidth()), scaled(cellSize()));
            index += 1;
        }

        const [tx, ty, tw, th] = tongueBox(this._edge, size);
        this._tongue.set_position(tx, ty);
        this._tongue.set_size(tw, th);
        this._tongueArea.set_size(tw, th);

        this._size = size;
        this._applyVisibility();
        // The pill's position, size and shape are all functions of the
        // reveal, so there is nothing to lay out here beyond handing it the
        // box it now has to grow inside.
        this._setReveal(this._reveal);
        this._tongueArea.queue_repaint();
    }

    _monitor() {
        const monitors = Main.layoutManager.monitors;
        if (monitors.length === 0)
            return null;
        if (this._monitorIndex >= 0 && this._monitorIndex < monitors.length)
            return monitors[this._monitorIndex];
        return Main.layoutManager.primaryMonitor ?? monitors[0];
    }

    _applyVisibility() {
        const visible = !this._suppressed && this._mode !== Mode.HIDDEN;
        this._container.visible = visible;
        // The tongue is the collapsed state made visible. Once the pill is
        // out there is nothing for it to say, so it goes.
        this._tongueWanted = visible && this._mode === Mode.AUTO_HIDE;
        this._tongue.visible = this._tongueWanted && this._reveal < 1;

        if (!visible) {
            this._setInteractive(false);
            this._card.hide();
            return;
        }
        if (this._mode === Mode.ALWAYS_VISIBLE && !this._expanded)
            this._expand(false);
    }

    // -- reveal state machine --------------------------------------------

    _onPointerIn() {
        this._clearTimer('_collapseTimer');
        if (this._mode === Mode.AUTO_HIDE)
            this._expand(true);
        return Clutter.EVENT_PROPAGATE;
    }

    _onPointerOut() {
        if (this._mode !== Mode.AUTO_HIDE || this._pinned)
            return Clutter.EVENT_PROPAGATE;

        this._clearTimer('_collapseTimer');
        // The delay is the whole point: a pointer that clips the corner of
        // the pill on its way somewhere else should not collapse it.
        this._collapseTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, Timing.collapseDelayMs, () => {
            this._collapseTimer = 0;
            if (!this._pill.hover && !this._tongue.hover && !this._pinned)
                this._collapse();
            return GLib.SOURCE_REMOVE;
        });
        return Clutter.EVENT_PROPAGATE;
    }

    _togglePin() {
        this._pinned = !this._pinned;
        this._pill.add_style_class_name('brimlim-pinned');
        if (!this._pinned) {
            this._pill.remove_style_class_name('brimlim-pinned');
            this._onPointerOut();
        }
        return Clutter.EVENT_STOP;
    }

    /** Come out for a moment on an event worth noticing, then go back. */
    revealTemporarily(durationMs = Timing.autoRevealMs) {
        if (this._mode !== Mode.AUTO_HIDE || this._suppressed)
            return;

        this._expand(true);
        this._clearTimer('_revealTimer');
        this._revealTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, durationMs, () => {
            this._revealTimer = 0;
            if (!this._pill.hover && !this._tongue.hover && !this._pinned)
                this._collapse();
            return GLib.SOURCE_REMOVE;
        });
    }

    _expand(animate) {
        if (this._expanded)
            return;
        this._expanded = true;
        this._setInteractive(true);
        this._animateReveal(true, animate);
    }

    _collapse() {
        if (!this._expanded)
            return;
        this._expanded = false;
        // Surrender input first: the pixels under a departing pill belong to
        // the window below from this moment on, including for the whole of
        // the collapse animation.
        this._setInteractive(false);
        this._card.hide();
        this._hoveredRing = null;
        this._animateReveal(false, true);
    }

    /**
     * Drive the reveal from one 0..1 value.
     *
     * The shape, the pill's own box and the marks' opacity all come out of
     * `revealShape`, so the drop that swells out of the edge, the stretch
     * along it and the marks arriving last are one animation rather than
     * three that have to be kept in step.
     */
    _animateReveal(expanded, animate) {
        this._stopReveal();

        const target = expanded ? 1 : 0;
        if (!animate) {
            this._setReveal(target);
            return;
        }

        const from = this._reveal;
        if (from === target)
            return;

        // Reversing mid-flight takes the time it has left, not a full one,
        // or a pointer brushing past would leave the pill drifting out long
        // after the pointer has gone.
        const span = Math.abs(target - from);
        const timeline = new Clutter.Timeline({
            actor: this._container,
            duration: Math.max(1, Math.round(
                (expanded ? Timing.revealMs : Timing.collapseMs) * span)),
        });
        // Linear: every curve in the animation is inside revealShape, where
        // both frontends can share it.
        timeline.connect('new-frame', () => {
            this._setReveal(from + (target - from) * timeline.get_progress());
        });
        timeline.connect('completed', () => {
            this._setReveal(target);
            this._stopReveal();
        });
        this._revealTimeline = timeline;
        timeline.start();
    }

    _stopReveal() {
        if (this._revealTimeline) {
            const timeline = this._revealTimeline;
            this._revealTimeline = null;
            timeline.run_dispose();
        }
    }

    _setReveal(t) {
        this._reveal = clamp01(t);
        if (!this._size)
            return;

        const shape = revealShape(this._edge, this._size, this._reveal);
        this._shape = shape;

        // The pill actor *is* the blob. Mutter derives the input region from
        // the geometry of reactive actors, so a pill that draws small while
        // sitting large would keep swallowing clicks over pixels it no
        // longer covers.
        const empty = shape.width <= 0 || shape.height <= 0;
        this._pill.set_position(shape.x, shape.y);
        this._pill.set_size(shape.width, shape.height);
        // A pill with no size is not a pill drawn very small: it is not
        // there, and an actor that is not visible cannot be picked, which is
        // the clearest way to say the pixels are not ours.
        this._pill.visible = !empty;
        if (!empty) {
            this._pillBackground.set_size(shape.width, shape.height);
            this._pillBackground.queue_repaint();
        }

        // Cells keep the coordinates both ports agree on, in the container's
        // frame, so the box is pushed back by however far the pill is inset.
        this._ringBox.set_position(-shape.x, -shape.y);
        this._ringBox.set_size(this._size[0], this._size[1]);
        this._ringBox.opacity = Math.round(255 * shape.cells);
        // Invisible is also unpickable, which is what keeps a ring from
        // owning pixels the half-grown pill does not cover yet.
        this._ringBox.visible = shape.cells > 0;

        this._tongue.opacity = Math.round(255 * shape.tongue);
        this._tongue.visible = this._tongueWanted && shape.tongue > 0;
    }

    /**
     * Hand the pill's pixels to us, or back to the windows underneath.
     *
     * Clutter picks reactive children even under a non-reactive parent, so
     * the rings have to be switched along with the pill or a ring would keep
     * swallowing clicks after the pill let go.
     */
    _setInteractive(interactive) {
        this._pill.reactive = interactive;
        for (const ring of this._rings.values())
            ring.reactive = interactive;
    }

    _clearTimer(name) {
        if (this[name]) {
            GLib.source_remove(this[name]);
            this[name] = 0;
        }
    }

    // -- card ------------------------------------------------------------

    _onRingHover(ring) {
        if (!ring.hover) {
            if (this._hoveredRing === ring) {
                this._hoveredRing = null;
                this._card.hide();
            }
            return;
        }

        this._hoveredRing = ring;
        // Padding first: it depends on which side the tail sticks out of,
        // and the card's height depends on the padding.
        this._card.setTail(this._edge, 0);
        this._card.show_provider(ring.provider);
        this._placeCard(ring);
    }

    _placeCard(ring) {
        const monitor = this._monitor();
        if (!monitor)
            return;

        const [ringX, ringY] = ring.get_transformed_position();
        // The ring sits inside its cell, inset by the room its pulse needs.
        const inset = scaled(Metrics.pulseRoom);
        const ringSize = scaled(Metrics.ringSize);
        const [ringW, ringH] = [ringSize, ringSize];
        const [cardW, cardH] = this._card.get_preferred_size().slice(2);
        const gap = scaled(Metrics.cardGap);

        let x;
        let y;
        switch (this._edge) {
        case Edge.RIGHT:
            x = this._container.x - cardW - gap;
            y = ringY + ringH / 2 - cardH / 2;
            break;
        case Edge.LEFT:
            x = this._container.x + this._container.width + gap;
            y = ringY + ringH / 2 - cardH / 2;
            break;
        case Edge.TOP:
            x = ringX + ringW / 2 - cardW / 2;
            y = this._container.y + this._container.height + gap;
            break;
        case Edge.BOTTOM:
        default:
            x = ringX + ringW / 2 - cardW / 2;
            y = this._container.y - cardH - gap;
            break;
        }

        const finalX = Math.round(
            Math.max(monitor.x, Math.min(x, monitor.x + monitor.width - cardW)));
        const finalY = Math.round(
            Math.max(monitor.y, Math.min(y, monitor.y + monitor.height - cardH)));
        this._card.set_position(finalX, finalY);

        // Aim the tail at the middle of the ring, in the card's own frame.
        const offset = isVertical(this._edge)
            ? ringY + inset + ringH / 2 - finalY
            : ringX + inset + ringW / 2 - finalX;
        this._card.setTail(this._edge, Math.round(offset));
    }

    // -- painting --------------------------------------------------------

    _repaintPill(area) {
        const [width, height] = area.get_surface_size();
        const shape = this._shape;
        const cr = area.get_context();
        drawPill(cr, this._edge, width, height, {
            flare: shape ? shape.flare : scaled(Metrics.flare),
            radius: shape ? shape.radius : scaled(Metrics.pillRadius),
        });
        cr.$dispose();
    }

    _repaintTongue(area) {
        const [width, height] = area.get_surface_size();
        const cr = area.get_context();
        drawTongue(cr, this._edge, width, height, this._unavailable ? 0.4 : 1);
        cr.$dispose();
    }
}
