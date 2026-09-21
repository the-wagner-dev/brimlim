//! The Rust port's reveal, frame by frame, next to nothing but a PNG.
//!
//! `tools/render-reveal.js` draws the same strip from the GJS port. The
//! transliteration test proves the two agree on the numbers; putting the two
//! images side by side is how you find out whether they agree on the picture,
//! which is a different question and the one that matters here.
//!
//!   cargo run -p brimlim-gtk --example reveal -- out.png [scale]

use brimlim_gtk::geometry::{
    Edge, PULSE_ROOM, RING_SIZE, TONGUE_LENGTH, TONGUE_THICKNESS, pill_size, reveal_shape,
    ring_origin,
};
use brimlim_gtk::paint;
use brimlim_model::{Activity, Fidelity, Provider, Status};
use gtk4::cairo::{Context, Format, ImageSurface};

const STEPS: [f64; 8] = [0.0, 0.15, 0.3, 0.42, 0.55, 0.7, 0.85, 1.0];

fn provider(id: &str, percent: f64, activity: Activity) -> Provider {
    Provider {
        id: id.to_owned(),
        label: id.to_owned(),
        headline_percent: Some(percent),
        windows: Vec::new(),
        fidelity: Fidelity::Official,
        status: Status::Ok,
        activity,
        sessions: Vec::new(),
        updated_at: None,
        message: None,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "reveal.png".to_owned());
    let scale: f64 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0);

    let providers = [
        provider("claude", 0.73, Activity::Busy),
        provider("codex", 0.21, Activity::Idle),
        provider("claude", 0.52, Activity::Waiting),
    ];

    let edge = Edge::Right;
    let size = pill_size(edge, providers.len());
    let gutter = 30.0;
    let frame_w = size.0 + gutter;
    let frame_h = size.1 + gutter;
    let mark_row = 110.0;

    let width = frame_w * STEPS.len() as f64;
    let height = frame_h + mark_row;

    let surface = ImageSurface::create(
        Format::ARgb32,
        (width * scale) as i32,
        (height * scale) as i32,
    )
    .expect("surface");
    let cr = Context::new(&surface).expect("context");
    cr.scale(scale, scale);
    cr.set_source_rgb(0.83, 0.85, 0.88);
    let _ = cr.paint();

    for (index, &t) in STEPS.iter().enumerate() {
        let shape = reveal_shape(edge, size, t);

        let _ = cr.save();
        cr.translate(index as f64 * frame_w + gutter / 2.0, gutter / 2.0);

        // The box the notch is allowed to grow inside, so a frame that
        // overflows it is visible rather than merely wrong in a test.
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.12);
        cr.set_line_width(1.0);
        cr.rectangle(0.5, 0.5, size.0 - 1.0, size.1 - 1.0);
        let _ = cr.stroke();

        if shape.tongue > 0.0 {
            let _ = cr.save();
            cr.translate(
                size.0 - TONGUE_THICKNESS,
                ((size.1 - TONGUE_LENGTH) / 2.0).round(),
            );
            paint::draw_tongue(&cr, TONGUE_THICKNESS, TONGUE_LENGTH, shape.tongue);
            let _ = cr.restore();
        }

        if shape.width >= 1.0 && shape.height >= 1.0 {
            let _ = cr.save();
            cr.translate(shape.x, shape.y);
            paint::draw_pill(
                &cr,
                edge,
                shape.width,
                shape.height,
                shape.flare,
                shape.radius,
            );
            let _ = cr.restore();
        }

        if shape.cells > 0.0 {
            cr.push_group();
            for (cell, provider) in providers.iter().enumerate() {
                let (x, y) = ring_origin(edge, cell);
                let _ = cr.save();
                cr.translate(x, y);
                paint::paint_cell(&cr, RING_SIZE, PULSE_ROOM, 1.0, provider, 0.3, 0.8);
                let _ = cr.restore();
            }
            let _ = cr.pop_group_to_source();
            let _ = cr.paint_with_alpha(shape.cells);
        }

        let _ = cr.restore();
    }

    // The mark's own animation, blown up: at cell size the wave is a
    // shimmer, and a shimmer is hard to review one frame at a time.
    let radius = 38.0;
    for (index, &phase) in STEPS.iter().enumerate() {
        let _ = cr.save();
        cr.translate(
            index as f64 * frame_w + frame_w / 2.0,
            frame_h + mark_row / 2.0,
        );
        cr.set_source_rgb(0.0, 0.0, 0.0);
        cr.arc(0.0, 0.0, radius * 1.4, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
        paint::draw_mark(&cr, "claude", 0.0, 0.0, radius, 1.0, Some(phase));
        let _ = cr.restore();
    }

    drop(cr);
    let mut file = std::fs::File::create(&out).expect("create output");
    surface.write_to_png(&mut file).expect("write png");
    println!("wrote {out} ({width}x{height}, scale {scale})");
}
