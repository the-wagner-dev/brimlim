//! The session-bus face of the daemon.
//!
//! One object, one interface, three members — frontends get the whole state
//! as a JSON string rather than a D-Bus struct, so adding a field never
//! breaks a running extension.

use std::sync::Arc;

use zbus::object_server::SignalEmitter;
use zbus::{connection, interface};

use crate::engine::Engine;

pub const BUS_NAME: &str = "org.brimlim.Daemon";
pub const OBJECT_PATH: &str = "/org/brimlim/Daemon";
pub const INTERFACE: &str = "org.brimlim.Daemon";

pub struct DaemonService {
    engine: Arc<Engine>,
}

#[interface(name = "org.brimlim.Daemon")]
impl DaemonService {
    /// The current state as JSON. Always answers, even before the first
    /// successful poll — with providers in a non-`ok` status.
    async fn get_state(&self) -> String {
        self.engine.snapshot_json()
    }

    /// Ask a provider to do real work on the next tick instead of serving a
    /// cached answer. An empty id means all of them.
    async fn refresh(&self, provider_id: String) {
        tracing::debug!(provider = %provider_id, "refresh requested");
        self.engine.request_refresh(&provider_id);
    }

    #[zbus(signal)]
    pub async fn state_changed(emitter: &SignalEmitter<'_>, state: String) -> zbus::Result<()>;
}

pub async fn serve(engine: Arc<Engine>) -> zbus::Result<zbus::Connection> {
    connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, DaemonService { engine })?
        .build()
        .await
}

pub fn emitter(connection: &zbus::Connection) -> zbus::Result<SignalEmitter<'static>> {
    SignalEmitter::new(connection, OBJECT_PATH)
}

/// Ask an already-running daemon for its state. `None` means nobody is
/// serving the name — the caller should poll directly instead.
pub async fn state_from_running_daemon() -> Option<String> {
    let connection = zbus::Connection::session().await.ok()?;
    let reply = connection
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            "GetState",
            &(),
        )
        .await
        .ok()?;
    reply.body().deserialize::<String>().ok()
}
