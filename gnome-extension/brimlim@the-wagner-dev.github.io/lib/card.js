// The hover card: limit windows and live sessions, in words and bars.
//
// Never reactive. It appears under the pointer and must not become something
// the pointer can land on, or hovering a ring would fight with the card that
// hovering produced.
//
// The background — rounded slab plus the tail that points back at its ring —
// is Cairo; everything inside it is St widgets, so the text belongs to the
// theme system and scales with it.

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import {formatAge, formatReset} from './format.js';
import {Edge, Metrics, isVertical, scaled} from './geometry.js';
import {drawMark} from './paint.js';
import {usageColor} from './palette.js';

const STATUS_TEXT = {
    ok: null,
    stale: 'Stale reading',
    needs_auth: 'Needs sign-in',
    error: 'Error',
};

const FIDELITY_TEXT = {
    official: 'Official',
    derived: 'Derived',
    manual: 'Manual',
};

function rgbaString(color, alpha = 1) {
    const [r, g, b] = color;
    return `rgba(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(b * 255)}, ${alpha})`;
}

export const Card = GObject.registerClass(
class Card extends St.Widget {
    _init() {
        super._init({
            style_class: 'brimlim-card',
            layout_manager: new Clutter.BinLayout(),
            reactive: false,
            visible: false,
            width: scaled(Metrics.cardWidth),
        });

        this._edge = Edge.RIGHT;
        this._tailAt = 0;

        this._background = new St.DrawingArea({x_expand: true, y_expand: true});
        this._background.connect('repaint', area => this._repaint(area));
        this.add_child(this._background);

        this._content = new St.BoxLayout({
            style_class: 'brimlim-card-content',
            orientation: Clutter.Orientation.VERTICAL,
            x_expand: true,
        });
        this.add_child(this._content);
    }

    /**
     * Where the tail should point, as an offset along the card's own edge,
     * so it aims at the ring the pointer is on.
     */
    setTail(edge, offset) {
        this._edge = edge;
        this._tailAt = offset;
        this._background.queue_repaint();
        this._content.style = this._contentPadding();
    }

    _contentPadding() {
        const pad = scaled(14);
        const tail = scaled(Metrics.cardTail);
        const extra = {
            [Edge.RIGHT]: `${pad}px ${pad + tail}px ${pad}px ${pad}px`,
            [Edge.LEFT]: `${pad}px ${pad}px ${pad}px ${pad + tail}px`,
            [Edge.TOP]: `${pad + tail}px ${pad}px ${pad}px ${pad}px`,
            [Edge.BOTTOM]: `${pad}px ${pad}px ${pad + tail}px ${pad}px`,
        };
        return `padding: ${extra[this._edge] ?? `${pad}px`};`;
    }

    /** Rebuild for one provider. Cheap enough to do on every hover. */
    show_provider(provider) {
        this._content.destroy_all_children();
        this._content.add_child(this._header(provider));

        if (provider.windows.length > 0) {
            for (const window of provider.windows)
                this._content.add_child(this._window(window));
        } else {
            this._content.add_child(new St.Label({
                style_class: 'brimlim-card-empty',
                text: provider.message ?? 'No reading to show',
            }));
        }

        if (provider.message && provider.windows.length > 0 && provider.status !== 'ok') {
            this._content.add_child(new St.Label({
                style_class: 'brimlim-card-note',
                text: provider.message,
            }));
        }

        this._content.add_child(this._sessions(provider));
        this._content.style = this._contentPadding();
        this.show();
        this._background.queue_repaint();
    }

    _header(provider) {
        const box = new St.BoxLayout({style_class: 'brimlim-card-header'});

        const size = scaled(16);
        const mark = new St.DrawingArea({width: size, height: size, y_align: Clutter.ActorAlign.CENTER});
        mark.connect('repaint', area => {
            const cr = area.get_context();
            drawMark(cr, provider.id, size / 2, size / 2, size / 2, 1);
            cr.$dispose();
        });
        box.add_child(mark);

        box.add_child(new St.Label({
            style_class: 'brimlim-card-title',
            text: `${provider.label} Usage`,
            x_expand: true,
        }));

        const status = STATUS_TEXT[provider.status];
        if (status) {
            box.add_child(new St.Label({
                style_class: `brimlim-chip brimlim-chip-${provider.status.replace('_', '-')}`,
                text: status,
            }));
        } else if (provider.fidelity !== 'official') {
            // "Official" is the norm and needs no badge; anything else has to
            // announce itself.
            box.add_child(new St.Label({
                style_class: 'brimlim-chip brimlim-chip-fidelity',
                text: FIDELITY_TEXT[provider.fidelity] ?? provider.fidelity,
            }));
        }
        return box;
    }

    _window(window) {
        const box = new St.BoxLayout({
            style_class: 'brimlim-window',
            orientation: Clutter.Orientation.VERTICAL,
        });

        const top = new St.BoxLayout();
        top.add_child(new St.Label({
            style_class: 'brimlim-window-name',
            text: window.name,
            x_expand: true,
        }));

        const reset = formatReset(window.resets_at);
        top.add_child(new St.Label({
            style_class: 'brimlim-window-reset',
            text: reset ? `Resets ${reset}` : '',
        }));
        box.add_child(top);

        const hasNumber = typeof window.percent === 'number' && Number.isFinite(window.percent);
        const trackWidth = scaled(Metrics.cardWidth) - scaled(28) - scaled(Metrics.cardTail);
        const track = new St.Widget({style_class: 'brimlim-bar', width: trackWidth});
        if (hasNumber) {
            track.add_child(new St.Widget({
                style_class: 'brimlim-bar-fill',
                width: Math.max(scaled(3), Math.round(trackWidth * Math.min(window.percent, 1))),
                style: `background-color: ${rgbaString(usageColor(window.percent))};`,
            }));
        }
        box.add_child(track);

        box.add_child(new St.Label({
            style_class: 'brimlim-window-used',
            text: hasNumber ? `${Math.round(window.percent * 100)}% Used` : 'No reading',
        }));
        return box;
    }

    _sessions(provider) {
        const box = new St.BoxLayout({
            style_class: 'brimlim-sessions',
            orientation: Clutter.Orientation.VERTICAL,
        });

        const count = provider.sessions.length;
        const heading = count === 1 ? '1 session' : `${count} sessions`;
        const top = new St.BoxLayout();
        top.add_child(new St.Label({
            style_class: 'brimlim-sessions-title',
            text: count > 0 ? heading : 'No live sessions',
            x_expand: true,
        }));
        top.add_child(new St.Label({
            style_class: 'brimlim-sessions-age',
            text: formatAge(provider.updated_at),
        }));
        box.add_child(top);

        for (const session of provider.sessions) {
            const row = new St.BoxLayout({style_class: 'brimlim-session'});
            row.add_child(new St.Widget({
                style_class: `brimlim-dot brimlim-dot-${session.state}`,
            }));
            row.add_child(new St.Label({
                style_class: 'brimlim-session-name',
                text: session.name,
                x_expand: true,
            }));
            row.add_child(new St.Label({
                style_class: 'brimlim-session-state',
                text: session.state,
            }));
            box.add_child(row);
        }
        return box;
    }

    _repaint(area) {
        const [width, height] = area.get_surface_size();
        const cr = area.get_context();
        const radius = scaled(16);
        const tail = scaled(Metrics.cardTail);
        const vertical = isVertical(this._edge);
        const at = this._tailAt;

        // Body, inset by the tail on whichever side the tail sticks out of.
        const x0 = this._edge === Edge.LEFT ? tail : 0;
        const y0 = this._edge === Edge.TOP ? tail : 0;
        const x1 = width - (this._edge === Edge.RIGHT ? tail : 0);
        const y1 = height - (this._edge === Edge.BOTTOM ? tail : 0);

        cr.newPath();
        cr.arc(x0 + radius, y0 + radius, radius, Math.PI, -Math.PI / 2);
        cr.arc(x1 - radius, y0 + radius, radius, -Math.PI / 2, 0);
        cr.arc(x1 - radius, y1 - radius, radius, 0, Math.PI / 2);
        cr.arc(x0 + radius, y1 - radius, radius, Math.PI / 2, Math.PI);
        cr.closePath();
        cr.setSourceRGBA(0, 0, 0, 0.96);
        cr.fill();

        // The tail, pointing back at the ring the pointer is on.
        cr.newPath();
        if (vertical) {
            const tip = Math.max(radius + tail, Math.min(at, height - radius - tail));
            const side = this._edge === Edge.RIGHT ? x1 : x0;
            const point = this._edge === Edge.RIGHT ? width : 0;
            cr.moveTo(side, tip - tail);
            cr.lineTo(point, tip);
            cr.lineTo(side, tip + tail);
        } else {
            const tip = Math.max(radius + tail, Math.min(at, width - radius - tail));
            const side = this._edge === Edge.TOP ? y0 : y1;
            const point = this._edge === Edge.TOP ? 0 : height;
            cr.moveTo(tip - tail, side);
            cr.lineTo(tip, point);
            cr.lineTo(tip + tail, side);
        }
        cr.closePath();
        cr.setSourceRGBA(0, 0, 0, 0.96);
        cr.fill();
        cr.$dispose();
    }

    show_message(title, detail) {
        this.show_provider({
            id: '__daemon__',
            label: title,
            headline_percent: null,
            windows: [],
            fidelity: 'official',
            status: 'error',
            activity: 'idle',
            sessions: [],
            updated_at: null,
            message: detail,
        });
    }
});
