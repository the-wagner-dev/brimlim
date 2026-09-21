// Brimlim — a floating usage overlay for AI coding assistants.
//
// This file is lifecycle only: settings, monitors, scale, overview, and the
// daemon connection. The drawing lives in lib/.

import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {transitions} from './lib/announce.js';
import {DaemonClient} from './lib/client.js';
import {setScale} from './lib/geometry.js';
import {Notch} from './lib/notch.js';

const WATCHED_KEYS = [
    'edge',
    'mode',
    'monitor',
    'show-in-fullscreen',
];

export default class BrimlimExtension extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._previousState = null;
        this._settingsIds = [];
        this._overviewIds = [];

        this._themeContext = St.ThemeContext.get_for_stage(global.stage);
        setScale(this._themeContext.scale_factor);

        this._client = new DaemonClient();
        this._notch = new Notch({
            onRefresh: providerId => this._client.refresh(providerId),
        });

        this._applySettings();
        this._notch.setUnavailable('Connecting to brimlimd…');

        this._client.connect('state', (_client, state) => this._onState(state));
        this._client.connect('available', (_client, available) => {
            if (!available) {
                // Forget history so the first state after a reconnect does
                // not replay a burst of chimes for changes we never saw.
                this._previousState = null;
                this._notch.setUnavailable('brimlimd is not running');
            }
        });
        this._client.start();

        for (const key of WATCHED_KEYS) {
            this._settingsIds.push(
                this._settings.connect(`changed::${key}`, () => this._applySettings()));
        }

        this._monitorsId = Main.layoutManager.connect('monitors-changed',
            () => this._notch.relayout());

        this._scaleId = this._themeContext.connect('notify::scale-factor', () => {
            setScale(this._themeContext.scale_factor);
            this._notch.rescale();
        });

        this._overviewIds.push(Main.overview.connect('showing', () => this._syncOverview(true)));
        this._overviewIds.push(Main.overview.connect('hiding', () => this._syncOverview(false)));
        this._syncOverview(Main.overview.visible);
    }

    disable() {
        for (const id of this._settingsIds ?? [])
            this._settings?.disconnect(id);
        this._settingsIds = [];

        for (const id of this._overviewIds ?? [])
            Main.overview.disconnect(id);
        this._overviewIds = [];

        if (this._monitorsId) {
            Main.layoutManager.disconnect(this._monitorsId);
            this._monitorsId = 0;
        }
        if (this._scaleId) {
            this._themeContext?.disconnect(this._scaleId);
            this._scaleId = 0;
        }
        this._themeContext = null;

        this._client?.stop();
        this._client = null;

        this._notch?.destroy();
        this._notch = null;

        this._settings = null;
        this._previousState = null;
    }

    _applySettings() {
        this._notch.configure({
            edge: this._settings.get_string('edge'),
            mode: this._settings.get_string('mode'),
            monitor: this._settings.get_int('monitor'),
            showInFullscreen: this._settings.get_boolean('show-in-fullscreen'),
        });
    }

    _syncOverview(shown) {
        const allowed = this._settings.get_boolean('show-in-overview');
        this._notch.setSuppressed(shown && !allowed);
    }

    _onState(state) {
        const events = transitions(this._previousState, state);
        this._previousState = state;
        this._notch.setProviders(state.providers);

        if (events.length === 0)
            return;

        if (this._settings.get_boolean('reveal-on-activity'))
            this._notch.revealTemporarily();
        if (this._settings.get_boolean('sound-on-activity'))
            this._playChime(events[0]);
    }

    _playChime(event) {
        try {
            // Meta's sound player is libcanberra underneath, and is the only
            // supported way to make a noise from inside the Shell.
            global.display.get_sound_player().play_from_theme(
                event.kind === 'waiting' ? 'dialog-question' : 'complete',
                `Brimlim: ${event.session} ${event.kind}`,
                null);
        } catch (error) {
            logError(error, 'brimlim: could not play a sound');
        }
    }
}
