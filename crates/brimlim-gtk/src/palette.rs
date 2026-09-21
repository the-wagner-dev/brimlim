//! The usage colour grade, transliterated from `lib/palette.js`. Both
//! frontends must reach the same colour for the same percentage.

pub type Rgba = [f64; 4];

const GREEN: [f64; 3] = [0.20, 0.84, 0.29];
const YELLOW: [f64; 3] = [0.91, 0.89, 0.04];
const ORANGE: [f64; 3] = [1.00, 0.45, 0.05];
const RED: [f64; 3] = [1.00, 0.23, 0.11];

const STOPS: [(f64, [f64; 3]); 5] = [
    (0.00, GREEN),
    (0.30, GREEN),
    (0.50, YELLOW),
    (0.70, ORANGE),
    (1.00, RED),
];

pub const TRACK: Rgba = [1.00, 1.00, 1.00, 0.10];
pub const DISC: Rgba = [0.17, 0.17, 0.18, 1.00];
/// Used wherever there is no number — never a usage colour.
pub const UNKNOWN: Rgba = [0.70, 0.73, 0.78, 0.55];
/// "This one is waiting for you" — never a usage colour, for the reason
/// spelled out in the extension's `palette.js`: the pulse is drawn right
/// outside the usage arc, and a session waiting for a reply must not look
/// like a window about to hit its limit.
pub const WAITING: [f64; 3] = [0.42, 0.70, 1.00];

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Green through yellow to red, with the green holding for the first third.
pub fn usage_color(percent: f64) -> [f64; 3] {
    let p = percent.clamp(0.0, 1.0);
    for index in 1..STOPS.len() {
        let (at, color) = STOPS[index];
        if p > at {
            continue;
        }
        let (previous_at, previous_color) = STOPS[index - 1];
        let span = at - previous_at;
        return if span == 0.0 {
            color
        } else {
            lerp(previous_color, color, (p - previous_at) / span)
        };
    }
    RED
}

pub fn set_color(cr: &gtk4::cairo::Context, color: [f64; 3], alpha: f64) {
    cr.set_source_rgba(color[0], color[1], color[2], alpha);
}

pub fn set_rgba(cr: &gtk4::cairo::Context, color: Rgba, alpha: f64) {
    cr.set_source_rgba(color[0], color[1], color[2], color[3] * alpha);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn a_fifth_of_a_window_still_reads_as_fine() {
        assert!(
            close(usage_color(0.21), GREEN),
            "green must hold through the early range"
        );
    }

    #[test]
    fn the_grade_reaches_its_stops() {
        assert!(close(usage_color(0.0), GREEN));
        assert!(close(usage_color(0.5), YELLOW));
        assert!(close(usage_color(0.7), ORANGE));
        assert!(close(usage_color(1.0), RED));
    }

    #[test]
    fn redness_only_ever_increases_with_usage() {
        let mut previous = -1.0;
        let mut p = 0.0;
        while p <= 1.0001 {
            let red = usage_color(p)[0];
            assert!(red >= previous - 1e-9, "red went backwards at {p}");
            previous = red;
            p += 0.05;
        }
    }

    #[test]
    fn out_of_range_input_is_clamped_not_extrapolated() {
        assert_eq!(usage_color(-5.0), usage_color(0.0));
        assert_eq!(usage_color(17.0), usage_color(1.0));
    }
}
