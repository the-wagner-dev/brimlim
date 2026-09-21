//! The session-bus client, on GLib's main loop.
//!
//! Deliberately not zbus: the frontend already runs a GLib main context, and
//! gio's bus API lives on it natively — one loop, no bridge, and the same
//! name-watch reconnection the GNOME extension gets.

use std::cell::RefCell;
use std::rc::Rc;

use brimlim_model::State;
use gtk4::gio;
use gtk4::prelude::*;

pub const BUS_NAME: &str = "org.brimlim.Daemon";
pub const OBJECT_PATH: &str = "/org/brimlim/Daemon";
pub const INTERFACE: &str = "org.brimlim.Daemon";

#[derive(Clone)]
pub struct DaemonClient {
    connection: Rc<RefCell<Option<gio::DBusConnection>>>,
    /// Held, not dropped: the subscription unsubscribes itself when it goes.
    subscription: Rc<RefCell<Option<gio::SignalSubscription>>>,
}

impl DaemonClient {
    /// Starts watching for the daemon. `on_state` fires for every reading,
    /// `on_available(false)` when the daemon leaves the bus.
    pub fn start(
        on_state: impl Fn(State) + 'static,
        on_available: impl Fn(bool) + 'static,
    ) -> Self {
        let client = Self {
            connection: Rc::new(RefCell::new(None)),
            subscription: Rc::new(RefCell::new(None)),
        };

        let on_state = Rc::new(on_state);
        let on_available = Rc::new(on_available);

        let appeared = {
            let client = client.clone();
            let on_state = Rc::clone(&on_state);
            let on_available = Rc::clone(&on_available);
            move |connection: gio::DBusConnection, _name: &str, _owner: &str| {
                *client.connection.borrow_mut() = Some(connection.clone());

                let handler = Rc::clone(&on_state);
                let subscription = connection.subscribe_to_signal(
                    Some(BUS_NAME),
                    Some(INTERFACE),
                    Some("StateChanged"),
                    Some(OBJECT_PATH),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |signal| {
                        if let Some(json) = signal.parameters.child_value(0).str() {
                            publish(json, handler.as_ref());
                        }
                    },
                );
                *client.subscription.borrow_mut() = Some(subscription);

                on_available(true);
                client.fetch_state(Rc::clone(&on_state));
            }
        };

        let vanished = {
            let client = client.clone();
            let on_available = Rc::clone(&on_available);
            move |_connection: gio::DBusConnection, _name: &str| {
                client.subscription.borrow_mut().take();
                *client.connection.borrow_mut() = None;
                on_available(false);
            }
        };

        gio::bus_watch_name(
            gio::BusType::Session,
            BUS_NAME,
            gio::BusNameWatcherFlags::NONE,
            appeared,
            vanished,
        );

        client
    }

    /// Ask the daemon to do real work now. Silent when it is not around.
    pub fn refresh(&self, provider_id: &str) {
        let Some(connection) = self.connection.borrow().clone() else {
            return;
        };
        connection.call(
            Some(BUS_NAME),
            OBJECT_PATH,
            INTERFACE,
            "Refresh",
            Some(&(provider_id.to_owned(),).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            2_000,
            gio::Cancellable::NONE,
            |result| {
                if let Err(error) = result {
                    tracing::warn!(%error, "Refresh failed");
                }
            },
        );
    }

    fn fetch_state(&self, on_state: Rc<impl Fn(State) + 'static>) {
        let Some(connection) = self.connection.borrow().clone() else {
            return;
        };
        connection.call(
            Some(BUS_NAME),
            OBJECT_PATH,
            INTERFACE,
            "GetState",
            None,
            None,
            gio::DBusCallFlags::NONE,
            5_000,
            gio::Cancellable::NONE,
            move |result| match result {
                Ok(reply) => {
                    if let Some(json) = reply.child_value(0).str() {
                        publish(json, on_state.as_ref());
                    }
                }
                Err(error) => tracing::warn!(%error, "GetState failed"),
            },
        );
    }
}

fn publish(json: &str, on_state: &impl Fn(State)) {
    match serde_json::from_str::<State>(json) {
        Ok(state) => on_state(state),
        Err(error) => tracing::warn!(%error, "daemon sent unparseable state"),
    }
}
