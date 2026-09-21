//! The layer-shell notch: two surfaces, a reveal state machine, and the
//! input region that decides which pixels are ours.
//!
//! Unlike the GNOME 50 port, this frontend really can set an input region —
//! `gdk::Surface::set_input_region()` maps straight onto
//! `wl_surface.set_input_region`. It is recomputed on every state change, and
//! surrendered *before* the collapse animation, so a click aimed at the
//! window underneath is never eaten by a pill on its way out. The card
//! surface keeps an empty region for its whole life: it is there to be read,
//! never to be hit.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use brimlim_model::{Activity, Provider, State};
use gtk4::prelude::*;
use gtk4::{cairo, gdk, glib};
use gtk4_layer_shell::{Edge as LayerEdge, KeyboardMode, Layer, LayerShell};

use crate::client::DaemonClient;
use crate::geometry::{
    AUTO_REVEAL_MS, CARD_GAP, CARD_WIDTH, COLLAPSE_DELAY_MS, Edge, FLARE, PILL_RADIUS, PULSE_ROOM,
    REVEAL_MS, RING_SIZE, card_height, cell_size, cell_width, pill_size, ring_origin, tongue_box,
};
use crate::paint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    AutoHide,
    AlwaysVisible,
    Hidden,
}

impl std::str::FromStr for Mode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto-hide" => Ok(Mode::AutoHide),
            "always-visible" => Ok(Mode::AlwaysVisible),
            "hidden" => Ok(Mode::Hidden),
            other => Err(format!("unknown mode {other}")),
        }
    }
}

pub struct Options {
    pub edge: Edge,
    pub mode: Mode,
    pub monitor: Option<usize>,
    /// Ordinary window rather than a layer surface: a way to look at the
    /// drawing where layer-shell does not exist. Anchoring, the input region
    /// and the edge-hugging reveal are all inert in this mode.
    pub windowed: bool,
}

struct Inner {
    providers: Vec<Provider>,
    expanded: bool,
    pinned: bool,
    hovered: Option<usize>,
    phase: f64,
    pulse: f64,
    /// Where the card's tail should point, along the card's own edge.
    tail_at: f64,
    collapse_source: Option<glib::SourceId>,
    reveal_source: Option<glib::SourceId>,
}

pub struct Notch {
    window: gtk4::ApplicationWindow,
    card_window: gtk4::ApplicationWindow,
    card_area: gtk4::DrawingArea,
    pill: gtk4::DrawingArea,
    tongue: gtk4::DrawingArea,
    revealer: gtk4::Revealer,
    frame: gtk4::Fixed,
    options: Options,
    client: DaemonClient,
    inner: RefCell<Inner>,
}

impl Notch {
    pub fn new(app: &gtk4::Application, options: Options, client: DaemonClient) -> Rc<Self> {
        let window = gtk4::ApplicationWindow::builder().application(app).build();
        let card_window = gtk4::ApplicationWindow::builder().application(app).build();

        let pill = gtk4::DrawingArea::new();
        let tongue = gtk4::DrawingArea::new();
        let card_area = gtk4::DrawingArea::new();

        let revealer = gtk4::Revealer::builder()
            .transition_duration(REVEAL_MS)
            .transition_type(reveal_transition(options.edge))
            .reveal_child(options.mode == Mode::AlwaysVisible)
            .child(&pill)
            .build();

        let frame = gtk4::Fixed::new();
        let overlay = gtk4::Overlay::builder().child(&frame).build();
        overlay.add_overlay(&revealer);
        overlay.add_overlay(&tongue);
        window.set_child(Some(&overlay));
        card_window.set_child(Some(&card_area));

        let notch = Rc::new(Self {
            window,
            card_window,
            card_area,
            pill,
            tongue,
            revealer,
            frame,
            options,
            client,
            inner: RefCell::new(Inner {
                providers: Vec::new(),
                expanded: false,
                pinned: false,
                hovered: None,
                phase: 0.0,
                pulse: 0.0,
                tail_at: 0.0,
                collapse_source: None,
                reveal_source: None,
            }),
        });

        notch.setup_layer_shell();
        notch.setup_drawing();
        notch.setup_input();
        notch.relayout();

        notch.window.present();
        notch
    }

    // -- construction ----------------------------------------------------

    fn setup_layer_shell(self: &Rc<Self>) {
        if self.options.windowed {
            self.window.set_title(Some("Brimlim (windowed preview)"));
            self.card_window.set_title(Some("Brimlim card"));
            return;
        }

        for window in [&self.window, &self.card_window] {
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            window.set_keyboard_mode(KeyboardMode::None);
            // No strut: this is an overlay, not a dock.
            window.set_exclusive_zone(0);
            if let Some(monitor) = self.monitor() {
                window.set_monitor(Some(&monitor));
            }
        }

        self.window.set_anchor(layer_edge(self.options.edge), true);
        self.card_window
            .set_anchor(layer_edge(self.options.edge), true);
        self.card_window
            .set_anchor(cross_anchor(self.options.edge), true);
    }

    fn setup_drawing(self: &Rc<Self>) {
        let notch = Rc::clone(self);
        self.pill.set_draw_func(move |_area, cr, width, height| {
            notch.draw_pill(cr, f64::from(width), f64::from(height));
        });

        let notch = Rc::clone(self);
        self.tongue.set_draw_func(move |_area, cr, width, height| {
            let alpha = if notch.inner.borrow().providers.is_empty() {
                0.4
            } else {
                1.0
            };
            paint::draw_tongue(cr, f64::from(width), f64::from(height), alpha);
        });

        let notch = Rc::clone(self);
        self.card_area
            .set_draw_func(move |_area, cr, width, height| {
                let inner = notch.inner.borrow();
                let Some(provider) = inner.hovered.and_then(|i| inner.providers.get(i)) else {
                    return;
                };
                paint::draw_card(
                    cr,
                    notch.options.edge,
                    f64::from(width),
                    f64::from(height),
                    notch.scale(),
                    inner.tail_at,
                    provider,
                );
            });
    }

    fn setup_input(self: &Rc<Self>) {
        let motion = gtk4::EventControllerMotion::new();

        let notch = Rc::clone(self);
        motion.connect_enter(move |_controller, _x, _y| notch.on_pointer_in());

        let notch = Rc::clone(self);
        motion.connect_leave(move |_controller| notch.on_pointer_out());

        let notch = Rc::clone(self);
        motion.connect_motion(move |_controller, x, y| notch.on_motion(x, y));
        self.window.add_controller(motion);

        let click = gtk4::GestureClick::new();
        let notch = Rc::clone(self);
        click.connect_pressed(move |_gesture, _n, x, y| notch.on_click(x, y));
        self.window.add_controller(click);

        // The surface only exists once mapped; the first input region has to
        // wait for it.
        let notch = Rc::clone(self);
        self.window
            .connect_map(move |_window| notch.apply_input_region());

        let notch = Rc::clone(self);
        self.card_window
            .connect_map(move |_window| notch.clear_card_input_region());
    }

    // -- state -----------------------------------------------------------

    pub fn set_state(self: &Rc<Self>, state: State) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.providers = state.providers;
            if inner.hovered.is_some_and(|i| i >= inner.providers.len()) {
                inner.hovered = None;
            }
        }
        self.relayout();
        self.sync_animation();
        self.pill.queue_draw();
        self.tongue.queue_draw();
        self.card_area.queue_draw();
    }

    pub fn set_unavailable(self: &Rc<Self>, reason: &str) {
        // Rendered through the ordinary provider path: an unreachable daemon
        // is just another thing with a status and no number.
        let placeholder = Provider {
            id: "__daemon__".to_owned(),
            label: "Brimlim".to_owned(),
            headline_percent: None,
            windows: Vec::new(),
            fidelity: brimlim_model::Fidelity::Official,
            status: brimlim_model::Status::Error,
            activity: Activity::Idle,
            sessions: Vec::new(),
            updated_at: None,
            message: Some(reason.to_owned()),
        };
        self.set_state(State::new(vec![placeholder]));
    }

    fn relayout(self: &Rc<Self>) {
        let count = self.inner.borrow().providers.len();
        let (width, height) = pill_size(self.options.edge, count);

        self.frame.set_size_request(width as i32, height as i32);
        self.pill.set_content_width(width as i32);
        self.pill.set_content_height(height as i32);
        self.window.set_default_size(width as i32, height as i32);

        let (tx, ty, tw, th) = tongue_box(self.options.edge, (width, height));
        self.tongue.set_content_width(tw as i32);
        self.tongue.set_content_height(th as i32);
        self.tongue
            .set_halign(halign_for(self.options.edge, tx, width));
        self.tongue
            .set_valign(valign_for(self.options.edge, ty, height));

        self.revealer.set_halign(pill_halign(self.options.edge));
        self.revealer.set_valign(pill_valign(self.options.edge));

        self.apply_input_region();
    }

    // -- reveal ----------------------------------------------------------

    fn on_pointer_in(self: &Rc<Self>) {
        self.cancel(Timer::Collapse);
        if self.options.mode == Mode::AutoHide {
            self.expand();
        }
    }

    fn on_pointer_out(self: &Rc<Self>) {
        self.hide_card();
        if self.options.mode != Mode::AutoHide || self.inner.borrow().pinned {
            return;
        }
        self.cancel(Timer::Collapse);

        // The delay is the whole point: a pointer clipping the corner of the
        // pill on its way elsewhere should not dismiss it.
        let notch = Rc::clone(self);
        let source = glib::timeout_add_local_once(
            Duration::from_millis(u64::from(COLLAPSE_DELAY_MS)),
            move || {
                notch.inner.borrow_mut().collapse_source = None;
                if !notch.inner.borrow().pinned {
                    notch.collapse();
                }
            },
        );
        self.inner.borrow_mut().collapse_source = Some(source);
    }

    fn on_motion(self: &Rc<Self>, x: f64, y: f64) {
        if !self.inner.borrow().expanded {
            return;
        }
        match self.ring_at(x, y) {
            Some(index) => self.show_card(index),
            None => self.hide_card(),
        }
    }

    fn on_click(self: &Rc<Self>, x: f64, y: f64) {
        if self.inner.borrow().expanded
            && let Some(index) = self.ring_at(x, y)
        {
            // A ring click is a refresh and stops there; it must never fall
            // through into the pin.
            let id = self.inner.borrow().providers[index].id.clone();
            if id != "__daemon__" {
                self.client.refresh(&id);
            }
            return;
        }

        let pinned = {
            let mut inner = self.inner.borrow_mut();
            inner.pinned = !inner.pinned;
            inner.pinned
        };
        if !pinned {
            self.on_pointer_out();
        }
    }

    /// Come out for a moment on something worth noticing, then go back.
    pub fn reveal_temporarily(self: &Rc<Self>) {
        if self.options.mode != Mode::AutoHide {
            return;
        }
        self.expand();
        self.cancel(Timer::Reveal);

        let notch = Rc::clone(self);
        let source = glib::timeout_add_local_once(
            Duration::from_millis(u64::from(AUTO_REVEAL_MS)),
            move || {
                notch.inner.borrow_mut().reveal_source = None;
                if !notch.inner.borrow().pinned {
                    notch.collapse();
                }
            },
        );
        self.inner.borrow_mut().reveal_source = Some(source);
    }

    fn expand(self: &Rc<Self>) {
        if self.inner.borrow().expanded {
            return;
        }
        self.inner.borrow_mut().expanded = true;
        self.revealer.set_reveal_child(true);
        self.apply_input_region();
    }

    fn collapse(self: &Rc<Self>) {
        if !self.inner.borrow().expanded {
            return;
        }
        self.inner.borrow_mut().expanded = false;
        // Surrender input first: the pixels under a departing pill belong to
        // the window below for the whole of the animation.
        self.apply_input_region();
        self.hide_card();
        self.revealer.set_reveal_child(false);
    }

    fn cancel(self: &Rc<Self>, timer: Timer) {
        let mut inner = self.inner.borrow_mut();
        let slot = match timer {
            Timer::Collapse => &mut inner.collapse_source,
            Timer::Reveal => &mut inner.reveal_source,
        };
        if let Some(source) = slot.take() {
            source.remove();
        }
    }

    // -- input region ----------------------------------------------------

    fn apply_input_region(self: &Rc<Self>) {
        if self.options.windowed {
            // An ordinary window owns all of its pixels; carving an input
            // region out of one would just make it unclickable.
            return;
        }
        let Some(surface) = self.window.surface() else {
            return;
        };
        let count = self.inner.borrow().providers.len();
        let (width, height) = pill_size(self.options.edge, count);

        let rect = if self.inner.borrow().expanded {
            cairo::RectangleInt::new(0, 0, width as i32, height as i32)
        } else {
            let (x, y, w, h) = tongue_box(self.options.edge, (width, height));
            cairo::RectangleInt::new(x as i32, y as i32, w as i32, h as i32)
        };
        surface.set_input_region(Some(&cairo::Region::create_rectangle(&rect)));
    }

    fn clear_card_input_region(self: &Rc<Self>) {
        if self.options.windowed {
            return;
        }
        if let Some(surface) = self.card_window.surface() {
            // The card is for reading, never for hitting.
            surface.set_input_region(Some(&cairo::Region::create()));
        }
    }

    // -- card ------------------------------------------------------------

    /// Which cell the pointer is over. The whole cell counts, percentage
    /// included: the label belongs to its ring.
    fn ring_at(self: &Rc<Self>, x: f64, y: f64) -> Option<usize> {
        let count = self.inner.borrow().providers.len();
        // A cell is ring-wide and cell-tall on every edge: the percentage
        // sits under its ring whichever way the pill runs.
        (0..count).find(|&index| {
            let (rx, ry) = ring_origin(self.options.edge, index);
            x >= rx && x <= rx + cell_width() && y >= ry && y <= ry + cell_size()
        })
    }

    fn show_card(self: &Rc<Self>, index: usize) {
        if self.inner.borrow().hovered == Some(index) {
            return;
        }
        self.inner.borrow_mut().hovered = Some(index);

        let inner = self.inner.borrow();
        let Some(provider) = inner.providers.get(index) else {
            return;
        };
        let height = card_height(provider.windows.len(), provider.sessions.len());
        drop(inner);

        self.card_area.set_content_width(CARD_WIDTH as i32);
        self.card_area.set_content_height(height as i32);
        self.card_window
            .set_default_size(CARD_WIDTH as i32, height as i32);
        self.place_card(index, height);
        self.card_area.queue_draw();
        self.card_window.present();
    }

    fn hide_card(self: &Rc<Self>) {
        if self.inner.borrow().hovered.is_none() {
            return;
        }
        self.inner.borrow_mut().hovered = None;
        self.card_window.set_visible(false);
    }

    /// Anchor the card beside its ring, in monitor coordinates.
    fn place_card(self: &Rc<Self>, index: usize, height: f64) {
        if self.options.windowed {
            return; // the card is just another window here
        }
        let Some(monitor) = self.monitor() else {
            return;
        };
        let area = monitor.geometry();
        let count = self.inner.borrow().providers.len();
        let (pill_w, pill_h) = pill_size(self.options.edge, count);
        let (ring_x, ring_y) = ring_origin(self.options.edge, index);
        let edge = self.options.edge;

        let depth = if edge.is_vertical() { pill_w } else { pill_h };
        self.card_window
            .set_margin(layer_edge(edge), (depth + CARD_GAP) as i32);

        if edge.is_vertical() {
            // The notch is centred on the cross axis, so the ring's screen
            // position follows from the pill's own height.
            let top = (f64::from(area.height()) - pill_h) / 2.0;
            let centre = top + ring_y + PULSE_ROOM + RING_SIZE / 2.0;
            let margin = (centre - height / 2.0).max(0.0);
            self.card_window.set_margin(LayerEdge::Top, margin as i32);
            // Aim the tail at the middle of the ring, in the card's own frame.
            self.inner.borrow_mut().tail_at = centre - margin;
        } else {
            let left = (f64::from(area.width()) - pill_w) / 2.0;
            let centre = left + ring_x + PULSE_ROOM + RING_SIZE / 2.0;
            let margin = (centre - CARD_WIDTH / 2.0).max(0.0);
            self.card_window.set_margin(LayerEdge::Left, margin as i32);
            self.inner.borrow_mut().tail_at = centre - margin;
        }
    }

    // -- painting and animation ------------------------------------------

    fn draw_pill(&self, cr: &cairo::Context, width: f64, height: f64) {
        let scale = self.scale();
        paint::draw_pill(
            cr,
            self.options.edge,
            width,
            height,
            FLARE * scale,
            PILL_RADIUS * scale,
        );

        let inner = self.inner.borrow();
        for (index, provider) in inner.providers.iter().enumerate() {
            let (x, y) = ring_origin(self.options.edge, index);
            let _ = cr.save();
            cr.translate(x, y);
            paint::paint_cell(
                cr,
                RING_SIZE,
                PULSE_ROOM,
                scale,
                provider,
                inner.phase,
                inner.pulse,
            );
            let _ = cr.restore();
        }
    }

    /// The spinner and the waiting pulse only cost a frame clock while
    /// something is actually happening.
    fn sync_animation(self: &Rc<Self>) {
        let animated = self
            .inner
            .borrow()
            .providers
            .iter()
            .any(|p| matches!(p.activity, Activity::Busy | Activity::Waiting));

        if !animated {
            let mut inner = self.inner.borrow_mut();
            inner.phase = 0.0;
            inner.pulse = 0.0;
            return;
        }
        if self.inner.borrow().phase != 0.0 {
            return; // a tick callback is already running
        }

        let notch = Rc::clone(self);
        self.window.add_tick_callback(move |_widget, clock| {
            let animated = notch
                .inner
                .borrow()
                .providers
                .iter()
                .any(|p| matches!(p.activity, Activity::Busy | Activity::Waiting));
            if !animated {
                return glib::ControlFlow::Break;
            }

            let micros = clock.frame_time();
            let cycle = 1_400_000.0;
            let phase = (micros as f64 % cycle) / cycle;
            {
                let mut inner = notch.inner.borrow_mut();
                inner.phase = phase;
                inner.pulse = 0.5 - 0.5 * (phase * std::f64::consts::TAU).cos();
            }
            notch.pill.queue_draw();
            glib::ControlFlow::Continue
        });
    }

    fn monitor(&self) -> Option<gdk::Monitor> {
        let display = gdk::Display::default()?;
        let monitors = display.monitors();
        let index = self.options.monitor.unwrap_or(0);
        monitors
            .item(index as u32)
            .and_then(|m| m.downcast::<gdk::Monitor>().ok())
    }

    fn scale(&self) -> f64 {
        // GTK gives logical pixels to the draw function and scales the
        // surface itself, so the drawing stays in logical units. The factor
        // is still needed for the hairlines that must not vanish.
        f64::from(self.window.scale_factor().max(1))
    }
}

enum Timer {
    Collapse,
    Reveal,
}

fn layer_edge(edge: Edge) -> LayerEdge {
    match edge {
        Edge::Top => LayerEdge::Top,
        Edge::Right => LayerEdge::Right,
        Edge::Bottom => LayerEdge::Bottom,
        Edge::Left => LayerEdge::Left,
    }
}

/// The axis the card is positioned along for a given notch edge.
fn cross_anchor(edge: Edge) -> LayerEdge {
    if edge.is_vertical() {
        LayerEdge::Top
    } else {
        LayerEdge::Left
    }
}

fn reveal_transition(edge: Edge) -> gtk4::RevealerTransitionType {
    match edge {
        Edge::Right => gtk4::RevealerTransitionType::SlideLeft,
        Edge::Left => gtk4::RevealerTransitionType::SlideRight,
        Edge::Top => gtk4::RevealerTransitionType::SlideDown,
        Edge::Bottom => gtk4::RevealerTransitionType::SlideUp,
    }
}

fn pill_halign(edge: Edge) -> gtk4::Align {
    match edge {
        Edge::Right => gtk4::Align::End,
        Edge::Left => gtk4::Align::Start,
        _ => gtk4::Align::Fill,
    }
}

fn pill_valign(edge: Edge) -> gtk4::Align {
    match edge {
        Edge::Top => gtk4::Align::Start,
        Edge::Bottom => gtk4::Align::End,
        _ => gtk4::Align::Fill,
    }
}

fn halign_for(edge: Edge, _x: f64, _width: f64) -> gtk4::Align {
    match edge {
        Edge::Right => gtk4::Align::End,
        Edge::Left => gtk4::Align::Start,
        _ => gtk4::Align::Center,
    }
}

fn valign_for(edge: Edge, _y: f64, _height: f64) -> gtk4::Align {
    match edge {
        Edge::Top => gtk4::Align::Start,
        Edge::Bottom => gtk4::Align::End,
        _ => gtk4::Align::Center,
    }
}
