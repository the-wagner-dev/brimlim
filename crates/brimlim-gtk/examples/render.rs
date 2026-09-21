//! Renders the Rust port's own drawing to a PNG, with no compositor and no
//! window — the only way to look at the layer-shell frontend's output on a
//! machine that has no layer-shell.
//!
//!   cargo run -p brimlim-gtk --example render -- out.png [scale]

use brimlim_gtk::geometry::{
    CARD_WIDTH, Edge, FLARE, PILL_RADIUS, PULSE_ROOM, RING_SIZE, card_height, pill_size,
    ring_origin,
};
use brimlim_gtk::paint;
use brimlim_model::{Activity, Fidelity, Provider, Session, SessionState, Status, Window};
use chrono::{Duration, Utc};
use gtk4::cairo::{Context, Format, ImageSurface};

fn provider(
    id: &str,
    label: &str,
    percent: Option<f64>,
    fidelity: Fidelity,
    status: Status,
    activity: Activity,
) -> Provider {
    let now = Utc::now();
    Provider {
        id: id.to_owned(),
        label: label.to_owned(),
        headline_percent: percent,
        windows: vec![
            Window {
                name: "Current session".to_owned(),
                percent: percent.unwrap_or(0.0),
                resets_at: Some(now + Duration::minutes(51)),
            },
            Window {
                name: "All models".to_owned(),
                percent: 0.07,
                resets_at: Some(now + Duration::hours(30)),
            },
        ],
        fidelity,
        status,
        activity,
        sessions: vec![
            Session {
                name: "brimlim".to_owned(),
                pid: 1,
                state: SessionState::Working,
            },
            Session {
                name: "airtrace".to_owned(),
                pid: 2,
                state: SessionState::Waiting,
            },
            Session {
                name: "geodash".to_owned(),
                pid: 3,
                state: SessionState::Idle,
            },
        ],
        updated_at: Some(now - Duration::minutes(3)),
        message: None,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "render.png".to_owned());
    let scale: f64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(2.0);

    let providers = [
        provider(
            "claude",
            "Claude",
            Some(0.73),
            Fidelity::Official,
            Status::Ok,
            Activity::Busy,
        ),
        provider(
            "codex",
            "Codex",
            Some(0.21),
            Fidelity::Official,
            Status::Ok,
            Activity::Idle,
        ),
        provider(
            "gemini",
            "Gemini",
            Some(0.52),
            Fidelity::Official,
            Status::Ok,
            Activity::Waiting,
        ),
        provider(
            "manual",
            "Manual",
            None,
            Fidelity::Official,
            Status::NeedsAuth,
            Activity::Idle,
        ),
    ];

    let edge = Edge::Right;
    let (pill_w, pill_h) = pill_size(edge, providers.len());
    let card_h = card_height(providers[0].windows.len(), providers[0].sessions.len());

    let margin = 30.0;
    let width = ((CARD_WIDTH + margin * 3.0 + pill_w) * scale) as i32;
    let height = ((pill_h.max(card_h) + margin * 2.0) * scale) as i32;

    let surface = ImageSurface::create(Format::ARgb32, width, height).expect("surface");
    let cr = Context::new(&surface).expect("context");
    cr.scale(scale, scale);

    // A light desktop behind it, to check the pill reads on a real background.
    cr.set_source_rgb(0.83, 0.85, 0.88);
    let _ = cr.paint();

    // The notch, flush against the right-hand wall the way it sits on a
    // monitor edge.
    let pill_x = f64::from(width) / scale - pill_w;
    let pill_y = (f64::from(height) / scale - pill_h) / 2.0;
    let _ = cr.save();
    cr.translate(pill_x, pill_y);
    paint::draw_pill(&cr, edge, pill_w, pill_h, FLARE, PILL_RADIUS);
    for (index, provider) in providers.iter().enumerate() {
        let (x, y) = ring_origin(edge, index);
        let _ = cr.save();
        cr.translate(x, y);
        paint::paint_cell(&cr, RING_SIZE, PULSE_ROOM, 1.0, provider, 0.3, 0.8);
        let _ = cr.restore();
    }
    let _ = cr.restore();

    // The card, where hovering the first ring would put it.
    let card_x = pill_x - CARD_WIDTH - margin;
    let (_, first_ring_y) = ring_origin(edge, 0);
    let ring_centre = pill_y + first_ring_y + PULSE_ROOM + RING_SIZE / 2.0;
    // Clamped the way place_card clamps to the monitor, so the header is
    // never drawn off the top.
    let card_y = (ring_centre - card_h / 2.0).max(margin);
    let _ = cr.save();
    cr.translate(card_x, card_y);
    paint::draw_card(
        &cr,
        edge,
        CARD_WIDTH,
        card_h,
        1.0,
        ring_centre - card_y,
        &providers[0],
    );
    let _ = cr.restore();

    drop(cr);
    let mut file = std::fs::File::create(&out).expect("create output");
    surface.write_to_png(&mut file).expect("write png");
    println!("wrote {out} ({width}x{height}, scale {scale})");
}
