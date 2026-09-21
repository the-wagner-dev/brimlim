//! brimlim-gtk — the layer-shell frontend, for Hyprland and KWin.
//!
//! Same daemon, same Cairo drawing as the GNOME extension, on a compositor
//! that implements wlr-layer-shell. (GNOME does not and will not, which is
//! exactly why there are two frontends.)

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use brimlim_model::State;
use clap::Parser;
use gtk4::glib;
use gtk4::prelude::*;

use brimlim_gtk::announce::{Kind, transitions};
use brimlim_gtk::client::DaemonClient;
use brimlim_gtk::geometry::Edge;
use brimlim_gtk::notch::{Mode, Notch, Options};

const APP_ID: &str = "org.brimlim.Gtk";

#[derive(Parser, Debug, Clone)]
#[command(name = "brimlim-gtk", version, about, long_about = None)]
struct Args {
    /// Screen edge the notch grows from.
    #[arg(long, default_value = "right")]
    edge: Edge,

    /// auto-hide, always-visible or hidden.
    #[arg(long, default_value = "auto-hide")]
    mode: Mode,

    /// Monitor index; defaults to the first one the compositor lists.
    #[arg(long)]
    monitor: Option<usize>,

    /// Show the notch in an ordinary window instead of anchoring it to the
    /// screen edge. For looking at the drawing on a compositor without
    /// layer-shell — GNOME, say. Everything edge-related is inert.
    #[arg(long)]
    windowed: bool,

    /// Do not reveal when a session finishes or starts waiting.
    #[arg(long)]
    no_reveal_on_activity: bool,

    /// Do not play a sound when a session finishes or starts waiting.
    #[arg(long)]
    no_sound_on_activity: bool,
}

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "brimlim_gtk=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    if !args.windowed
        && let Err(message) = demand_wayland()
    {
        eprintln!("{message}");
        return glib::ExitCode::FAILURE;
    }

    let app = gtk4::Application::builder().application_id(APP_ID).build();

    // A refusal to start has to reach the exit status, or a systemd unit
    // would restart-loop a frontend that can never work here.
    let unsupported = Rc::new(Cell::new(false));
    let flag = Rc::clone(&unsupported);
    app.connect_activate(move |app| build(app, &args, &flag));

    // The notch takes no command line of its own; clap has already had it.
    let code = app.run_with_args::<&str>(&[]);
    if unsupported.get() {
        glib::ExitCode::FAILURE
    } else {
        code
    }
}

/// wlr-layer-shell is a Wayland protocol, so GDK must not be allowed to pick
/// X11 — inside an AppImage with a bundled GTK it otherwise can, and the
/// failure surfaces as a GDK assertion rather than as an explanation.
fn demand_wayland() -> Result<(), String> {
    const ADVICE: &str = "brimlim-gtk is a Wayland program: it draws through wlr-layer-shell, \
                          which has no X11 equivalent.";

    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err(ADVICE.to_owned());
    }
    // Overridden rather than defaulted: linuxdeploy's GTK plugin exports
    // GDK_BACKEND=x11 into every AppImage it builds, which for a layer-shell
    // program is not a preference but a guaranteed failure.
    match std::env::var("GDK_BACKEND").as_deref() {
        Ok("wayland") => {}
        Ok(other) => tracing::debug!(backend = other, "overriding GDK_BACKEND with wayland"),
        Err(_) => {}
    }
    // Safe here: single-threaded, and well before GTK reads the environment
    // during initialisation.
    unsafe { std::env::set_var("GDK_BACKEND", "wayland") };
    Ok(())
}

fn build(app: &gtk4::Application, args: &Args, unsupported: &Rc<Cell<bool>>) {
    // Mutter does not implement wlr-layer-shell and has said it will not, so
    // this frontend cannot run on GNOME. Say so plainly rather than crashing
    // inside the first init_layer_shell() call. The check needs GTK to be up,
    // which is why it lives here and not in main().
    if !args.windowed && !gtk4_layer_shell::is_supported() {
        eprintln!(
            "brimlim-gtk needs a compositor with wlr-layer-shell (Hyprland, KWin, sway, …).\n\
             On GNOME, use the Shell extension instead:\n\
             \tgnome-extensions enable brimlim@the-wagner-dev.github.io"
        );
        unsupported.set(true);
        app.quit();
        return;
    }

    let previous: Rc<RefCell<Option<State>>> = Rc::new(RefCell::new(None));
    let notch: Rc<RefCell<Option<Rc<Notch>>>> = Rc::new(RefCell::new(None));

    let client = {
        let state_notch = Rc::clone(&notch);
        let state_previous = Rc::clone(&previous);
        let gone_notch = Rc::clone(&notch);
        let gone_previous = Rc::clone(&previous);
        let args = args.clone();

        DaemonClient::start(
            move |state| {
                let Some(notch) = state_notch.borrow().clone() else {
                    return;
                };
                let events = transitions(state_previous.borrow().as_ref(), &state);
                *state_previous.borrow_mut() = Some(state.clone());
                notch.set_state(state);

                let Some(event) = events.first() else {
                    return;
                };
                if !args.no_reveal_on_activity {
                    notch.reveal_temporarily();
                }
                if !args.no_sound_on_activity {
                    play_chime(&event.kind);
                }
            },
            move |available| {
                if available {
                    return;
                }
                // Forget history so a reconnect does not replay a burst of
                // chimes for changes we never saw.
                *gone_previous.borrow_mut() = None;
                if let Some(notch) = gone_notch.borrow().clone() {
                    notch.set_unavailable("brimlimd is not running");
                }
            },
        )
    };

    let built = Notch::new(
        app,
        Options {
            edge: args.edge,
            mode: args.mode,
            monitor: args.monitor,
            windowed: args.windowed,
        },
        client,
    );
    built.set_unavailable("Connecting to brimlimd…");
    *notch.borrow_mut() = Some(built);
}

/// libcanberra, through the tool it ships, so the frontend takes no link-time
/// dependency on it and stays silent rather than failing when it is absent.
fn play_chime(kind: &Kind) {
    let sound = match kind {
        Kind::Waiting => "dialog-question",
        Kind::Finished => "complete",
    };
    match std::process::Command::new("canberra-gtk-play")
        .arg("-i")
        .arg(sound)
        .spawn()
    {
        Ok(_) => {}
        Err(error) => tracing::debug!(%error, "no canberra-gtk-play; staying silent"),
    }
}
