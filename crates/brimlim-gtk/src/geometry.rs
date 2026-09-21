//! Logical-pixel metrics, transliterated from the extension's
//! `lib/geometry.js`. The two frontends must agree to the pixel, so the
//! constants are kept in the same order and with the same names.

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

pub const REVEAL_MS: u32 = 180;
pub const COLLAPSE_DELAY_MS: u32 = 400;
pub const AUTO_REVEAL_MS: u32 = 5000;

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
