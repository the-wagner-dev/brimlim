//! Cairo drawing, transliterated from the extension's `lib/pill.js` and
//! `lib/paint.js`. Same shapes, same colours, same rule: a provider without
//! a reading gets a muted tick and a dash, never an arc that could be read
//! as a number.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use brimlim_model::{Activity, Fidelity, Provider, SessionState, Status};
use chrono::Utc;
use gtk4::cairo::Context;

use crate::format::{format_age, format_reset};
use crate::geometry::{CARD_TAIL, Edge, RING_STROKE};
use crate::palette::{self, DISC, TRACK, UNKNOWN, WAITING, usage_color};

const START_ANGLE: f64 = -FRAC_PI_2;

fn stroke(cr: &Context) {
    if let Err(error) = cr.stroke() {
        tracing::warn!(%error, "cairo stroke failed");
    }
}

fn fill(cr: &Context) {
    if let Err(error) = cr.fill() {
        tracing::warn!(%error, "cairo fill failed");
    }
}

// -- the pill ----------------------------------------------------------------

/// Right-edge pill, flush against `x = w`. `flare` is the inverted corner at
/// the bezel; `radius` the ordinary rounding on the three free sides.
fn right_edge_path(cr: &Context, w: f64, h: f64, flare: f64, radius: f64) {
    let f = flare.min(h / 2.0);
    let r = radius.min(w).min((h - 2.0 * f) / 2.0);

    cr.new_path();
    cr.move_to(w, 0.0);
    cr.arc(w - f, 0.0, f, 0.0, FRAC_PI_2);
    cr.line_to(r, f);
    cr.arc_negative(r, f + r, r, -FRAC_PI_2, PI);
    cr.line_to(0.0, h - f - r);
    cr.arc_negative(r, h - f - r, r, PI, FRAC_PI_2);
    cr.line_to(w - f, h - f);
    cr.arc(w - f, h, f, -FRAC_PI_2, 0.0);
    cr.close_path();
}

/// Run `draw` in a frame where the screen edge is always on the right.
fn with_edge_frame<F: FnOnce(f64, f64)>(
    cr: &Context,
    edge: Edge,
    width: f64,
    height: f64,
    draw: F,
) {
    let _ = cr.save();
    match edge {
        Edge::Right => draw(width, height),
        Edge::Left => {
            cr.translate(width, 0.0);
            cr.scale(-1.0, 1.0);
            draw(width, height);
        }
        Edge::Top => {
            cr.translate(0.0, height);
            cr.rotate(-FRAC_PI_2);
            draw(height, width);
        }
        Edge::Bottom => {
            cr.translate(width, 0.0);
            cr.rotate(FRAC_PI_2);
            draw(height, width);
        }
    }
    let _ = cr.restore();
}

pub fn draw_pill(cr: &Context, edge: Edge, width: f64, height: f64, flare: f64, radius: f64) {
    with_edge_frame(cr, edge, width, height, |w, h| {
        right_edge_path(cr, w, h, flare, radius);
        // Black, and no outline: the pill is meant to read as a piece of the
        // bezel that has grown out over the screen, not as a panel sitting
        // on top of it.
        cr.set_source_rgba(0.0, 0.0, 0.0, 1.0);
        fill(cr);
    });
}

pub fn draw_tongue(cr: &Context, width: f64, height: f64, alpha: f64) {
    let radius = width.min(height) / 2.0;
    cr.new_path();
    if width >= height {
        cr.arc(radius, radius, radius, FRAC_PI_2, -FRAC_PI_2);
        cr.arc(width - radius, radius, radius, -FRAC_PI_2, FRAC_PI_2);
    } else {
        cr.arc(radius, radius, radius, PI, 0.0);
        cr.arc(radius, height - radius, radius, 0.0, PI);
    }
    cr.close_path();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.38 * alpha);
    fill(cr);
}

// -- text --------------------------------------------------------------------

fn layout(cr: &Context, text: &str, size: f64, bold: bool) -> pangocairo::pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    let mut description = pangocairo::pango::FontDescription::from_string("Sans");
    description.set_absolute_size(size * f64::from(pangocairo::pango::SCALE));
    if bold {
        description.set_weight(pangocairo::pango::Weight::Bold);
    }
    layout.set_font_description(Some(&description));
    layout.set_text(text);
    layout
}

fn draw_text(cr: &Context, text: &str, x: f64, y: f64, size: f64, bold: bool, color: [f64; 4]) {
    let layout = layout(cr, text, size, bold);
    cr.set_source_rgba(color[0], color[1], color[2], color[3]);
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
    cr.new_path();
}

fn draw_text_right(
    cr: &Context,
    text: &str,
    right: f64,
    y: f64,
    size: f64,
    bold: bool,
    color: [f64; 4],
) {
    let layout = layout(cr, text, size, bold);
    cr.set_source_rgba(color[0], color[1], color[2], color[3]);
    cr.move_to(right - f64::from(layout.pixel_size().0), y);
    pangocairo::functions::show_layout(cr, &layout);
    cr.new_path();
}

fn draw_text_centered(
    cr: &Context,
    text: &str,
    cx: f64,
    y: f64,
    size: f64,
    bold: bool,
    color: [f64; 4],
) {
    let layout = layout(cr, text, size, bold);
    cr.set_source_rgba(color[0], color[1], color[2], color[3]);
    cr.move_to(
        (cx - f64::from(layout.pixel_size().0) / 2.0).round(),
        y.round(),
    );
    pangocairo::functions::show_layout(cr, &layout);
    cr.new_path();
}

// -- the cell ----------------------------------------------------------------

/// The provider's mark. Drawn rather than set as text: at 38 logical pixels a
/// font glyph is at the mercy of whatever the user has installed, and these
/// shapes are simple enough to own.
pub fn draw_mark(
    cr: &Context,
    id: &str,
    cx: f64,
    cy: f64,
    radius: f64,
    alpha: f64,
    phase: Option<f64>,
) {
    cr.set_source_rgba(1.0, 1.0, 1.0, alpha);
    cr.set_line_cap(gtk4::cairo::LineCap::Round);

    if id == "codex" {
        // A filled shape cannot shimmer spoke by spoke, so it breathes.
        let breath = match phase {
            Some(phase) => 0.93 + 0.07 * (0.5 + 0.5 * (TAU * phase).cos()),
            None => 1.0,
        };
        draw_rosette(cr, cx, cy, radius * breath);
        return;
    }

    // A burst of spokes, the Claude mark's silhouette. Eleven is what makes
    // it read as a burst rather than as an asterisk.
    let spokes = 11;
    cr.set_line_width((radius * 0.17).max(1.0));
    for i in 0..spokes {
        let angle = f64::from(i) / f64::from(spokes) * TAU - FRAC_PI_2;
        let inner = radius * 0.16;

        // At rest the spokes alternate long and short. While the agent is
        // working that alternation becomes a wave running round the mark.
        let (outer, lit) = match phase {
            Some(phase) => {
                let wave = 0.5 + 0.5 * (TAU * (phase - f64::from(i) / f64::from(spokes))).cos();
                (radius * (0.74 + 0.3 * wave), 0.4 + 0.6 * wave)
            }
            None => (radius * if i % 2 == 0 { 1.0 } else { 0.82 }, 1.0),
        };

        cr.set_source_rgba(1.0, 1.0, 1.0, alpha * lit);
        cr.new_path();
        cr.move_to(cx + angle.cos() * inner, cy + angle.sin() * inner);
        cr.line_to(cx + angle.cos() * outer, cy + angle.sin() * outer);
        stroke(cr);
    }
}

/// A six-lobed rosette with a hexagonal hole — the OpenAI mark's silhouette,
/// not its knot. Filled rather than stroked on purpose: at the nineteen
/// pixels this is actually drawn at, an outlined knot turns to mush, and a
/// shape that survives being small is worth more than one that is accurate
/// when blown up.
fn draw_rosette(cr: &Context, cx: f64, cy: f64, radius: f64) {
    const HOLE: f64 = 0.46;
    const BULGE: f64 = 1.32;

    let ring = |r: f64| {
        for i in 0..6 {
            let angle = f64::from(i) / 6.0 * TAU - FRAC_PI_2;
            let next = f64::from(i + 1) / 6.0 * TAU - FRAC_PI_2;
            let mid = (angle + next) / 2.0;
            let (ctrl_x, ctrl_y) = (cx + mid.cos() * r * BULGE, cy + mid.sin() * r * BULGE);

            if i == 0 {
                cr.move_to(cx + angle.cos() * r, cy + angle.sin() * r);
            }
            cr.curve_to(
                ctrl_x,
                ctrl_y,
                ctrl_x,
                ctrl_y,
                cx + next.cos() * r,
                cy + next.sin() * r,
            );
        }
        cr.close_path();
    };

    cr.set_fill_rule(gtk4::cairo::FillRule::EvenOdd);
    cr.new_path();
    ring(radius);
    ring(radius * HOLE);
    fill(cr);
    cr.set_fill_rule(gtk4::cairo::FillRule::Winding);
}

/// One provider's ring, its mark, its activity animation and its percentage.
///
/// `inset` is the room the waiting pulse needs outside the ring; the cell is
/// that much wider than the ring on every side but the bottom, where the
/// percentage goes.
pub fn paint_cell(
    cr: &Context,
    ring: f64,
    inset: f64,
    scale: f64,
    provider: &Provider,
    phase: f64,
    pulse: f64,
) {
    let stroke_width = (RING_STROKE * scale).round();
    let center = inset + ring / 2.0;
    let radius = ring / 2.0 - stroke_width / 2.0;

    let dimmed = if provider.status == Status::Stale {
        0.55
    } else {
        1.0
    };
    let known = matches!(provider.status, Status::Ok | Status::Stale);

    // The disc the mark sits on, a shade above the pill so the ring reads as
    // a control rather than as a hole.
    palette::set_rgba(cr, DISC, 1.0);
    cr.new_path();
    cr.arc(
        center,
        center,
        radius - stroke_width / 2.0 - (2.0 * scale).round(),
        0.0,
        TAU,
    );
    fill(cr);

    cr.set_line_cap(gtk4::cairo::LineCap::Round);
    cr.set_line_width(stroke_width);

    palette::set_rgba(cr, TRACK, 1.0);
    cr.new_path();
    cr.arc(center, center, radius, 0.0, TAU);
    stroke(cr);

    let percent = provider.headline_percent.filter(|p| p.is_finite());
    match percent {
        Some(percent) => {
            match provider.fidelity {
                Fidelity::Derived => {
                    cr.set_dash(&[(3.0 * scale).round(), (3.0 * scale).round()], 0.0)
                }
                Fidelity::Manual => {
                    cr.set_dash(&[(1.0 * scale).round(), (4.0 * scale).round()], 0.0)
                }
                Fidelity::Official => {}
            }
            palette::set_color(cr, usage_color(percent), dimmed);
            cr.new_path();
            cr.arc(
                center,
                center,
                radius,
                START_ANGLE,
                START_ANGLE + TAU * percent.min(1.0),
            );
            stroke(cr);
            cr.set_dash(&[], 0.0);
        }
        None => {
            // No reading: one muted tick at twelve o'clock. An empty ring
            // would read as zero, which is a number nobody measured.
            palette::set_rgba(cr, UNKNOWN, 1.0);
            cr.set_line_width((2.0 * scale).round());
            cr.new_path();
            cr.arc(
                center,
                center,
                radius,
                START_ANGLE - 0.12,
                START_ANGLE + 0.12,
            );
            stroke(cr);
            cr.set_line_width(stroke_width);
        }
    }

    draw_mark(
        cr,
        &provider.id,
        center,
        center,
        radius * 0.48,
        if known { 1.0 } else { 0.5 },
        (provider.activity == Activity::Busy).then_some(phase),
    );

    if provider.activity == Activity::Waiting {
        palette::set_color(cr, WAITING, 0.15 + 0.5 * pulse);
        cr.set_line_width((2.0 * scale).round());
        cr.new_path();
        cr.arc(center, center, radius + (3.0 * scale).round(), 0.0, TAU);
        stroke(cr);
    }

    let (text, color) = match percent {
        Some(percent) => (
            format!("{}%", (percent * 100.0).round() as i64),
            [1.0, 1.0, 1.0, dimmed],
        ),
        None => ("—".to_owned(), UNKNOWN),
    };
    draw_text_centered(
        cr,
        &text,
        center,
        ring + (3.0 * scale).round(),
        13.0 * scale,
        false,
        color,
    );
}

// -- the card ----------------------------------------------------------------

const TEXT: [f64; 4] = [0.906, 0.914, 0.933, 1.0];
const TEXT_DIM: [f64; 4] = [0.906, 0.914, 0.933, 0.55];
const WHITE: [f64; 4] = [1.0, 1.0, 1.0, 1.0];

fn rounded_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_path();
    cr.arc(x + r, y + r, r, PI, -FRAC_PI_2);
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.close_path();
}

/// The card's slab, with the tail that points back at its ring. `tail_at` is
/// an offset along the card's own edge.
fn draw_card_slab(cr: &Context, edge: Edge, width: f64, height: f64, scale: f64, tail_at: f64) {
    let radius = 16.0 * scale;
    let tail = CARD_TAIL * scale;

    let x0 = if edge == Edge::Left { tail } else { 0.0 };
    let y0 = if edge == Edge::Top { tail } else { 0.0 };
    let x1 = width - if edge == Edge::Right { tail } else { 0.0 };
    let y1 = height - if edge == Edge::Bottom { tail } else { 0.0 };

    rounded_rect(cr, x0, y0, x1 - x0, y1 - y0, radius);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.96);
    fill(cr);

    cr.new_path();
    if edge.is_vertical() {
        let tip = tail_at.clamp(radius + tail, height - radius - tail);
        let side = if edge == Edge::Right { x1 } else { x0 };
        let point = if edge == Edge::Right { width } else { 0.0 };
        cr.move_to(side, tip - tail);
        cr.line_to(point, tip);
        cr.line_to(side, tip + tail);
    } else {
        let tip = tail_at.clamp(radius + tail, width - radius - tail);
        let side = if edge == Edge::Top { y0 } else { y1 };
        let point = if edge == Edge::Top { 0.0 } else { height };
        cr.move_to(tip - tail, side);
        cr.line_to(tip, point);
        cr.line_to(tip + tail, side);
    }
    cr.close_path();
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.96);
    fill(cr);
}

/// The hover card: windows with bars and resets, then live sessions.
pub fn draw_card(
    cr: &Context,
    edge: Edge,
    width: f64,
    height: f64,
    scale: f64,
    tail_at: f64,
    provider: &Provider,
) {
    let now = Utc::now();
    let pad = 14.0 * scale;
    let tail = CARD_TAIL * scale;

    draw_card_slab(cr, edge, width, height, scale, tail_at);

    let left = pad + if edge == Edge::Left { tail } else { 0.0 };
    let right = width - pad - if edge == Edge::Right { tail } else { 0.0 };
    let mut y = pad + if edge == Edge::Top { tail } else { 0.0 };

    draw_mark(
        cr,
        &provider.id,
        left + 8.0 * scale,
        y + 8.0 * scale,
        8.0 * scale,
        1.0,
        None,
    );
    draw_text(
        cr,
        &format!("{} Usage", provider.label),
        left + 22.0 * scale,
        y,
        13.0 * scale,
        true,
        WHITE,
    );

    let badge = match provider.status {
        Status::Ok => match provider.fidelity {
            // "Official" is the norm and needs no badge.
            Fidelity::Official => "",
            Fidelity::Derived => "Derived",
            Fidelity::Manual => "Manual",
        },
        Status::Stale => "Stale reading",
        Status::NeedsAuth => "Needs sign-in",
        Status::Error => "Error",
    };
    if !badge.is_empty() {
        draw_text_right(cr, badge, right, y + scale, 10.0 * scale, true, TEXT_DIM);
    }
    y += 26.0 * scale;

    if provider.windows.is_empty() {
        let message = provider.message.as_deref().unwrap_or("No reading to show");
        draw_text(cr, message, left, y, 11.0 * scale, false, TEXT_DIM);
        y += 46.0 * scale;
    }

    for window in &provider.windows {
        draw_text(cr, &window.name, left, y, 12.0 * scale, false, TEXT);
        if let Some(reset) = format_reset(window.resets_at, now) {
            let reset = format!("Resets {reset}");
            draw_text_right(cr, &reset, right, y, 11.0 * scale, false, TEXT_DIM);
        }
        y += 18.0 * scale;

        let track = right - left;
        rounded_rect(cr, left, y, track, 6.0 * scale, 3.0 * scale);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
        fill(cr);

        let filled = (track * window.percent.min(1.0)).max(3.0 * scale);
        rounded_rect(cr, left, y, filled, 6.0 * scale, 3.0 * scale);
        palette::set_color(cr, usage_color(window.percent), 1.0);
        fill(cr);
        y += 11.0 * scale;

        draw_text(
            cr,
            &format!("{}% Used", (window.percent * 100.0).round() as i64),
            left,
            y,
            11.0 * scale,
            false,
            TEXT,
        );
        y += 17.0 * scale;
    }

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
    cr.set_line_width(scale);
    cr.move_to(left, y + 4.0 * scale);
    cr.line_to(right, y + 4.0 * scale);
    stroke(cr);
    y += 12.0 * scale;

    let heading = match provider.sessions.len() {
        0 => "No live sessions".to_owned(),
        1 => "1 session".to_owned(),
        n => format!("{n} sessions"),
    };
    draw_text(cr, &heading, left, y, 11.0 * scale, true, TEXT);
    draw_text_right(
        cr,
        &format_age(provider.updated_at, now),
        right,
        y,
        10.0 * scale,
        false,
        TEXT_DIM,
    );
    y += 20.0 * scale;

    for session in &provider.sessions {
        let (color, alpha) = match session.state {
            SessionState::Working => ([0.20, 0.84, 0.29], 1.0),
            SessionState::Waiting => (WAITING, 1.0),
            SessionState::Idle => ([1.0, 1.0, 1.0], 0.25),
        };
        cr.new_path();
        cr.arc(left + 3.5 * scale, y + 7.0 * scale, 3.5 * scale, 0.0, TAU);
        palette::set_color(cr, color, alpha);
        fill(cr);

        draw_text(
            cr,
            &session.name,
            left + 14.0 * scale,
            y,
            11.0 * scale,
            false,
            TEXT,
        );
        draw_text_right(
            cr,
            &format!("{:?}", session.state).to_lowercase(),
            right,
            y,
            10.0 * scale,
            false,
            TEXT_DIM,
        );
        y += 18.0 * scale;
    }
}
