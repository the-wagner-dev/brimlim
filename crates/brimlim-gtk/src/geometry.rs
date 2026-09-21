//! Logical-pixel metrics, transliterated from the extension's
//! `lib/geometry.js`. The two frontends must agree to the pixel, so the
//! constants are kept in the same order and with the same names.

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

impl Edge {
    pub fn is_vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

impl std::str::FromStr for Edge {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "top" => Ok(Edge::Top),
            "right" => Ok(Edge::Right),
            "bottom" => Ok(Edge::Bottom),
            "left" => Ok(Edge::Left),
            other => Err(format!("unknown edge {other}")),
        }
    }
}

pub const RING_SIZE: f64 = 38.0;
pub const RING_STROKE: f64 = 4.0;
/// Room around the ring for the waiting pulse, which is drawn outside it.
/// Without this the animation is clipped by its own cell.
pub const PULSE_ROOM: f64 = 4.0;
/// The percentage sits under its ring, so a provider occupies a cell rather
/// than a circle.
pub const PERCENT_GAP: f64 = 3.0;
pub const PERCENT_HEIGHT: f64 = 15.0;
pub const CELL_GAP: f64 = 34.0;
pub const PILL_PADDING: f64 = 12.0;
pub const PILL_RADIUS: f64 = 24.0;
pub const FLARE: f64 = 20.0;
pub const TONGUE_THICKNESS: f64 = 4.0;
pub const TONGUE_LENGTH: f64 = 80.0;
pub const CARD_WIDTH: f64 = 280.0;
pub const CARD_GAP: f64 = 10.0;
pub const CARD_TAIL: f64 = 9.0;

pub const REVEAL_MS: u32 = 420;
pub const COLLAPSE_MS: u32 = 260;
pub const COLLAPSE_DELAY_MS: u32 = 400;
pub const AUTO_REVEAL_MS: u32 = 5000;

// -- the reveal -----------------------------------------------------------
//
// The notch does not slide out; it grows out. The shape is a pure function
// of one 0..1 progress value, which is what lets both frontends animate
// identically without sharing a line of drawing code. Transliterated from
// `lib/geometry.js`; the reference fixture keeps the two honest.

/// Phase boundaries along that single timeline. They overlap on purpose: the
/// drop is still swelling when it starts to spread, which is what stops the
/// two halves reading as two separate animations.
pub const DROP_END: f64 = 0.40;
pub const STRETCH_FROM: f64 = 0.30;
pub const STRETCH_TO: f64 = 0.88;
pub const CELLS_FROM: f64 = 0.72;
/// How far the drop flattens while it spreads. Surface tension, not a
/// bounce: the shape never grows past the pill's own box.
pub const FLATTEN: f64 = 0.07;

pub fn clamp01(value: f64) -> f64 {
    if value.is_nan() {
        return 0.0;
    }
    value.clamp(0.0, 1.0)
}

pub fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - clamp01(t)).powi(3)
}

/// Slow at both ends. The stretch uses this rather than an ease-out so that
/// the drop is still a drop when it stops swelling.
pub fn ease_in_out_cubic(t: f64) -> f64 {
    let p = clamp01(t);
    if p < 0.5 {
        4.0 * p * p * p
    } else {
        1.0 - (-2.0 * p + 2.0).powi(3) / 2.0
    }
}

/// The pill's drawn — and owned — box at reveal progress `t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RevealShape {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The rounding is part of the morph: a drop is round, a pill is not.
    pub radius: f64,
    pub flare: f64,
    /// The marks arrive last, on a shape that has already settled.
    pub cells: f64,
    /// The tongue is the collapsed state's only visible part, so it goes as
    /// soon as anything else is there to see.
    pub tongue: f64,
}

/// See `revealShape` in the extension: at `t = 0` this is nothing, at `t = 1`
/// it is the whole pill, and in between it leaves the edge as a disc and then
/// stretches along it.
pub fn reveal_shape(edge: Edge, (width, height): (f64, f64), t: f64) -> RevealShape {
    let progress = clamp01(t);
    let vertical = edge.is_vertical();
    let full = if vertical { width } else { height }; // depth, away from the edge
    let span = if vertical { height } else { width }; // length, along the edge

    let swell = ease_out_cubic(progress / DROP_END);
    let stretch = ease_in_out_cubic((progress - STRETCH_FROM) / (STRETCH_TO - STRETCH_FROM));

    // The drop grows out of the tongue's own footprint, and is about as long
    // as it is deep, so it leaves the edge round. Seeding it with the longer
    // of the two is not a detail: a drop shorter than the tongue would leave
    // the pointer that summoned it outside the shape, and the notch would
    // collapse under the very pointer holding it open.
    let seed = full.max(TONGUE_LENGTH);
    let along = span.min(seed + (span - seed) * stretch);
    let depth = full * swell * (1.0 - FLATTEN * (PI * stretch).sin());

    let w = (if vertical { depth } else { along }).round();
    let h = (if vertical { along } else { depth }).round();

    // Half the short side is a disc; the pill's own radius is the end state.
    let half = w.min(h) / 2.0;
    let radius = half.min(half + (PILL_RADIUS - half) * stretch);
    let flare = (FLARE * stretch).min(along / 2.0);

    let (x, y) = match edge {
        Edge::Right => (width - w, ((height - h) / 2.0).round()),
        Edge::Left => (0.0, ((height - h) / 2.0).round()),
        Edge::Top => (((width - w) / 2.0).round(), 0.0),
        Edge::Bottom => (((width - w) / 2.0).round(), height - h),
    };

    RevealShape {
        x,
        y,
        width: w,
        height: h,
        radius,
        flare,
        cells: ease_out_cubic((progress - CELLS_FROM) / (1.0 - CELLS_FROM)),
        tongue: 1.0 - clamp01(progress / STRETCH_FROM),
    }
}

/// A cell is a ring with room for its pulse, and its percentage underneath.
pub fn cell_width() -> f64 {
    RING_SIZE + 2.0 * PULSE_ROOM
}

pub fn cell_size() -> f64 {
    PULSE_ROOM + RING_SIZE + PERCENT_GAP + PERCENT_HEIGHT
}

/// Pill size in logical pixels: `(width, height)` for the given edge.
pub fn pill_size(edge: Edge, count: usize) -> (f64, f64) {
    let (step, across) = if edge.is_vertical() {
        (cell_size(), cell_width())
    } else {
        (cell_width(), cell_size())
    };

    let body = if count > 0 {
        count as f64 * step + (count as f64 - 1.0) * CELL_GAP + 2.0 * PILL_PADDING
    } else {
        step + 2.0 * PILL_PADDING
    };
    let along = body + 2.0 * FLARE;
    let deep = across + 2.0 * PILL_PADDING;

    if edge.is_vertical() {
        (deep, along)
    } else {
        (along, deep)
    }
}

/// Top-left of the cell at `index`, inside the pill.
pub fn ring_origin(edge: Edge, index: usize) -> (f64, f64) {
    let step = if edge.is_vertical() {
        cell_size()
    } else {
        cell_width()
    } + CELL_GAP;
    let offset = FLARE + PILL_PADDING + index as f64 * step;

    if edge.is_vertical() {
        (PILL_PADDING, offset)
    } else {
        (offset, PILL_PADDING)
    }
}

/// The resting tongue inside the pill's box: `(x, y, width, height)`.
pub fn tongue_box(edge: Edge, (width, height): (f64, f64)) -> (f64, f64, f64, f64) {
    let thickness = TONGUE_THICKNESS;
    let length = TONGUE_LENGTH.min(if edge.is_vertical() { height } else { width });

    match edge {
        Edge::Left => (0.0, (height - length) / 2.0, thickness, length),
        Edge::Right => (
            width - thickness,
            (height - length) / 2.0,
            thickness,
            length,
        ),
        Edge::Top => ((width - length) / 2.0, 0.0, length, thickness),
        Edge::Bottom => (
            (width - length) / 2.0,
            height - thickness,
            length,
            thickness,
        ),
    }
}

/// Height of a card with this much in it. Computed rather than measured, so
/// the surface can be sized before anything is drawn into it. The numbers
/// mirror the advances in `paint::draw_card`, and the test below keeps them
/// honest.
pub fn card_height(windows: usize, sessions: usize) -> f64 {
    const PADDING: f64 = 14.0;
    const HEADER: f64 = 26.0;
    /// name + reset, bar, "N% Used"
    const WINDOW_ROW: f64 = 18.0 + 11.0 + 17.0;
    const DIVIDER: f64 = 12.0;
    const SESSIONS_HEADER: f64 = 20.0;
    const SESSION_ROW: f64 = 18.0;

    2.0 * PADDING
        + HEADER
        + windows.max(1) as f64 * WINDOW_ROW
        + DIVIDER
        + SESSIONS_HEADER
        + sessions as f64 * SESSION_ROW
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_grows_along_its_edge_and_not_across_it() {
        let (w1, h1) = pill_size(Edge::Right, 1);
        let (w3, h3) = pill_size(Edge::Right, 3);
        assert_eq!(
            w1, w3,
            "depth is independent of how many providers there are"
        );
        assert!(h3 > h1);

        // A horizontal pill is not the vertical one transposed: a cell is
        // taller than it is wide, because the percentage sits under the ring
        // whichever edge the notch lives on.
        let (tw, th) = pill_size(Edge::Top, 3);
        assert_eq!(
            th,
            cell_size() + 2.0 * PILL_PADDING,
            "depth follows the cell's height"
        );
        assert_eq!(
            tw,
            3.0 * cell_width() + 2.0 * CELL_GAP + 2.0 * PILL_PADDING + 2.0 * FLARE,
            "length follows the cells, laid side by side"
        );
    }

    #[test]
    fn cells_are_laid_out_inside_the_body_clear_of_the_flare() {
        let (_, height) = pill_size(Edge::Right, 2);
        let (_, first) = ring_origin(Edge::Right, 0);
        let (_, second) = ring_origin(Edge::Right, 1);

        assert!(first >= FLARE, "a cell must not sit in the inverted corner");
        assert_eq!(second - first, cell_size() + CELL_GAP);
        assert!(
            second + cell_size() <= height - FLARE,
            "the last percentage must fit inside the body"
        );
    }

    #[test]
    fn the_tongue_keeps_its_logical_size_on_every_edge() {
        for edge in [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left] {
            let size = pill_size(edge, 2);
            let (x, y, w, h) = tongue_box(edge, size);
            assert!(x >= 0.0 && y >= 0.0);
            let (long, short) = if w > h { (w, h) } else { (h, w) };
            assert_eq!(short, TONGUE_THICKNESS);
            assert_eq!(long, TONGUE_LENGTH);
        }
    }
}
