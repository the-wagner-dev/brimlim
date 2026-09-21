//! brimlimd — usage limits and agent activity for AI coding assistants.
//!
//! No GUI, no webview, no window. It polls providers, remembers the last good
//! reading across restarts, and publishes state on the session bus.

mod config;
mod dbus;
mod engine;
mod model;
mod providers;
mod store;
mod util;

use std::sync::Arc;

use clap::Parser;

use crate::config::Config;
use crate::engine::Engine;

#[derive(Parser, Debug)]
#[command(name = "brimlimd", version, about, long_about = None)]
struct Args {
    /// Print one JSON snapshot and exit — the waybar/polybar module mode.
    #[arg(long)]
    json: bool,

    /// With --json, poll directly instead of asking a running daemon.
    #[arg(long, requires = "json")]
    no_daemon: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "brimlimd=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    if args.json {
        one_shot(args.no_daemon).await
    } else {
        run_daemon().await
    }
}

/// Single JSON line on stdout. Prefers a running daemon: it already has warm
/// caches and CPU history, so its answer is better than anything a
/// short-lived process can produce.
async fn one_shot(skip_daemon: bool) -> anyhow::Result<()> {
    if !skip_daemon && let Some(state) = dbus::state_from_running_daemon().await {
        println!("{state}");
        return Ok(());
    }

    let engine = Engine::new(Config::load())?;
    let (state, _) = engine.tick().await;
    println!("{}", serde_json::to_string(&state)?);
    Ok(())
}

async fn run_daemon() -> anyhow::Result<()> {
    let config = Config::load();
    let interval = config.tick();
    let engine = Arc::new(Engine::new(config)?);

    // Poll once before taking the bus name. A frontend that connects the
    // instant the name appears would otherwise get an empty provider list
    // back from GetState and have no way to tell "not polled yet" from
    // "nothing installed".
    engine.tick().await;

    let connection = dbus::serve(Arc::clone(&engine)).await?;
    let emitter = dbus::emitter(&connection)?;
    tracing::info!(name = dbus::BUS_NAME, "serving on the session bus");

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let (state, changed) = engine.tick().await;
                if !changed {
                    continue;
                }
                match serde_json::to_string(&state) {
                    Ok(json) => {
                        if let Err(error) =
                            dbus::DaemonService::state_changed(&emitter, json).await
                        {
                            tracing::warn!(%error, "could not emit StateChanged");
                        }
                    }
                    Err(error) => tracing::error!(%error, "state did not serialise"),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("shutting down");
                return Ok(());
            }
        }
    }
}
