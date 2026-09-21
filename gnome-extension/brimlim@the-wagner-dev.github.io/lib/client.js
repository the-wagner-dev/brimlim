// The session-bus client.
//
// The daemon is allowed to die, restart, or never have been started at all.
// None of that may take the Shell with it, so every call is guarded and the
// name watch does the reconnecting for us.

import Gio from 'gi://Gio';
import GObject from 'gi://GObject';

export const BUS_NAME = 'org.brimlim.Daemon';
export const OBJECT_PATH = '/org/brimlim/Daemon';

const IFACE = `
<node>
  <interface name="org.brimlim.Daemon">
    <method name="GetState">
      <arg type="s" direction="out" name="state"/>
    </method>
    <method name="Refresh">
      <arg type="s" direction="in" name="provider_id"/>
    </method>
    <signal name="StateChanged">
      <arg type="s" name="state"/>
    </signal>
  </interface>
</node>`;

const DaemonProxy = Gio.DBusProxy.makeProxyWrapper(IFACE);

export const DaemonClient = GObject.registerClass({
    Signals: {
        'state': {param_types: [GObject.TYPE_JSOBJECT]},
        // false means the daemon is not on the bus; the UI must say so
        // rather than keep drawing the last numbers as if they were live.
        'available': {param_types: [GObject.TYPE_BOOLEAN]},
    },
}, class DaemonClient extends GObject.Object {
    constructor() {
        super();
        this._watchId = 0;
        this._proxy = null;
        this._signalId = 0;
        this._cancellable = null;
    }

    start() {
        this._cancellable = new Gio.Cancellable();
        this._watchId = Gio.bus_watch_name(
            Gio.BusType.SESSION,
            BUS_NAME,
            Gio.BusNameWatcherFlags.NONE,
            () => this._onAppeared(),
            () => this._onVanished());
    }

    stop() {
        this._cancellable?.cancel();
        this._cancellable = null;
        if (this._watchId) {
            Gio.bus_unwatch_name(this._watchId);
            this._watchId = 0;
        }
        this._dropProxy();
    }

    /** Ask the daemon to do real work now. Silent when it is not around. */
    refresh(providerId = '') {
        try {
            this._proxy?.RefreshAsync(providerId).catch(error => {
                logError(error, 'brimlim: Refresh failed');
            });
        } catch (error) {
            logError(error, 'brimlim: Refresh threw');
        }
    }

    _onAppeared() {
        new DaemonProxy(Gio.DBus.session, BUS_NAME, OBJECT_PATH, (proxy, error) => {
            if (this._cancellable === null)
                return;   // disabled while the proxy was being built
            if (error) {
                logError(error, 'brimlim: could not reach the daemon');
                this.emit('available', false);
                return;
            }

            this._proxy = proxy;
            this._signalId = proxy.connectSignal('StateChanged', (_proxy, _sender, [json]) => {
                this._publish(json);
            });
            this.emit('available', true);
            this._fetchState();
        }, this._cancellable);
    }

    _onVanished() {
        this._dropProxy();
        this.emit('available', false);
    }

    _dropProxy() {
        if (this._proxy && this._signalId) {
            try {
                this._proxy.disconnectSignal(this._signalId);
            } catch {
                // The proxy may already be gone with the connection.
            }
        }
        this._signalId = 0;
        this._proxy = null;
    }

    _fetchState() {
        this._proxy?.GetStateAsync()
            .then(([json]) => this._publish(json))
            .catch(error => {
                if (!error.matches?.(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    logError(error, 'brimlim: GetState failed');
            });
    }

    _publish(json) {
        let state;
        try {
            state = JSON.parse(json);
        } catch (error) {
            logError(error, 'brimlim: daemon sent unparseable state');
            return;
        }
        if (!state || !Array.isArray(state.providers)) {
            log('brimlim: ignoring state without a providers array');
            return;
        }
        this.emit('state', state);
    }
});
