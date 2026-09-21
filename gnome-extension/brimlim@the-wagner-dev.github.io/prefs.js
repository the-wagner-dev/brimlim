import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import Gtk from 'gi://Gtk';

import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensionPreferences.js';

const EDGES = [
    ['right', 'Right'],
    ['left', 'Left'],
    ['top', 'Top'],
    ['bottom', 'Bottom'],
];

const MODES = [
    ['auto-hide', 'Auto-hide'],
    ['always-visible', 'Always visible'],
    ['hidden', 'Hidden'],
];

function comboRow(title, subtitle, options, settings, key) {
    const model = new Gtk.StringList();
    for (const [, label] of options)
        model.append(label);

    const row = new Adw.ComboRow({title, subtitle, model});
    const values = options.map(([value]) => value);
    row.selected = Math.max(0, values.indexOf(settings.get_string(key)));
    row.connect('notify::selected', () => settings.set_string(key, values[row.selected]));
    return row;
}

function switchRow(title, subtitle, settings, key) {
    const row = new Adw.SwitchRow({title, subtitle});
    settings.bind(key, row, 'active', Gio.SettingsBindFlags.DEFAULT);
    return row;
}

export default class BrimlimPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();

        const page = new Adw.PreferencesPage({
            title: 'Brimlim',
            icon_name: 'preferences-desktop-display-symbolic',
        });

        const placement = new Adw.PreferencesGroup({title: 'Placement'});
        placement.add(comboRow('Screen edge', 'Which bezel the notch grows from',
            EDGES, settings, 'edge'));

        const monitor = new Adw.SpinRow({
            title: 'Monitor',
            subtitle: '-1 follows the primary monitor',
            adjustment: new Gtk.Adjustment({lower: -1, upper: 8, step_increment: 1}),
        });
        settings.bind('monitor', monitor, 'value', Gio.SettingsBindFlags.DEFAULT);
        placement.add(monitor);
        page.add(placement);

        const behaviour = new Adw.PreferencesGroup({title: 'Behaviour'});
        behaviour.add(comboRow('Reveal', 'Auto-hide collapses to a thin tongue until hovered',
            MODES, settings, 'mode'));
        behaviour.add(switchRow('Show over fullscreen windows',
            'Off by default, so a fullscreen video is never interrupted',
            settings, 'show-in-fullscreen'));
        behaviour.add(switchRow('Show in the Activities overview',
            'The overview is its own layer; off keeps it uncluttered',
            settings, 'show-in-overview'));
        page.add(behaviour);

        const alerts = new Adw.PreferencesGroup({
            title: 'Alerts',
            description: 'When a session stops working or starts waiting for you',
        });
        alerts.add(switchRow('Reveal for a few seconds', null, settings, 'reveal-on-activity'));
        alerts.add(switchRow('Play a sound', null, settings, 'sound-on-activity'));
        page.add(alerts);

        window.add(page);
    }
}
