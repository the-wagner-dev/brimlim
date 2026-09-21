// One provider, drawn as a ring with its percentage under it.
//
// The ring is the only place a number becomes a picture, so it is also where
// the "never invent a percentage" rule has to be enforced visually: a
// provider with no reading gets a muted tick and a dash instead of a number,
// and a derived reading is dashed so it can never be mistaken for an
// official one.

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import {Metrics, cellSize, cellWidth, scaleFactor, scaled} from './geometry.js';
import {paintCell} from './paint.js';

export const Ring = GObject.registerClass({
    Signals: {
        'activated': {},
    },
}, class Ring extends St.Widget {
    _init(provider) {
        super._init({
            style_class: 'brimlim-ring',
            layout_manager: new Clutter.BinLayout(),
            reactive: true,
            track_hover: true,
            can_focus: true,
            width: scaled(cellWidth()),
            height: scaled(cellSize()),
        });

        this._provider = provider;
        this._phase = 0;
        this._pulse = 0;

        this._area = new St.DrawingArea({x_expand: true, y_expand: true});
        this._area.connect('repaint', area => this._repaint(area));
        this.add_child(this._area);

        this._timeline = new Clutter.Timeline({
            actor: this,
            duration: 1400,
            repeat_count: -1,
        });
        this._timeline.connect('new-frame', () => this._onFrame());

        this.connect('destroy', () => this._timeline.run_dispose());
        this.connect('button-press-event', () => {
            // Must not reach the pill underneath, which would toggle the pin.
            this.emit('activated');
            return Clutter.EVENT_STOP;
        });

        this.update(provider);
    }

    get providerId() {
        return this._provider.id;
    }

    get provider() {
        return this._provider;
    }

    update(provider) {
        this._provider = provider;

        const animated = provider.activity === 'busy' || provider.activity === 'waiting';
        if (animated && !this._timeline.is_playing())
            this._timeline.start();
        else if (!animated && this._timeline.is_playing())
            this._timeline.stop();

        if (!animated) {
            this._phase = 0;
            this._pulse = 0;
        }
        this._area.queue_repaint();
    }

    _onFrame() {
        const progress = this._timeline.get_progress();
        this._phase = progress;
        // A sine gives the waiting pulse a breath rather than a blink.
        this._pulse = 0.5 - 0.5 * Math.cos(progress * 2 * Math.PI);
        this._area.queue_repaint();
    }

    _repaint(area) {
        const cr = area.get_context();
        paintCell(cr, {
            ring: scaled(Metrics.ringSize),
            inset: scaled(Metrics.pulseRoom),
            scale: scaleFactor(),
            provider: this._provider,
            phase: this._phase,
            pulse: this._pulse,
        });
        cr.$dispose();
    }
});
