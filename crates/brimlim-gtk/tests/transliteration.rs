//! The drawing logic is written twice, once in GJS and once in Rust. That is
//! deliberate, but it is only safe while the two agree — so the extension's
//! own output is recorded as a reference table and checked against the real
//! Rust functions here.
//!
//! Regenerate after changing either port:
//!   gjs -m tools/dump-reference.js > crates/brimlim-gtk/fixtures/reference.json

use brimlim_gtk::geometry::{Edge, pill_size, ring_origin, tongue_box};
use brimlim_gtk::palette::usage_color;
use serde_json::Value;

const REFERENCE: &str = include_str!("../fixtures/reference.json");

fn reference() -> Value {
    serde_json::from_str(REFERENCE).expect("the reference table should be valid JSON")
}

fn numbers(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

#[test]
fn the_colour_grade_matches_the_gnome_extension() {
    let table = reference();
    let rows = table["palette"].as_array().unwrap();
    assert_eq!(rows.len(), 101, "the table should cover the whole range");

    for row in rows {
        let values = numbers(row);
        let percent = values[0];
        let expected = [values[1], values[2], values[3]];
        let actual = usage_color(percent);

        for channel in 0..3 {
            assert!(
                (actual[channel] - expected[channel]).abs() < 1e-9,
                "colour drift at {percent}: rust {actual:?}, extension {expected:?}"
            );
        }
    }
}

#[test]
fn the_pill_geometry_matches_the_gnome_extension() {
    let table = reference();

    for row in table["geometry"].as_array().unwrap() {
        let edge: Edge = row["edge"].as_str().unwrap().parse().unwrap();
        let count = row["count"].as_u64().unwrap() as usize;

        let pill = numbers(&row["pill"]);
        let (w, h) = pill_size(edge, count);
        assert_eq!(
            [w, h].to_vec(),
            pill,
            "pill size drift on {edge:?} with {count}"
        );

        let tongue = numbers(&row["tongue"]);
        let (tx, ty, tw, th) = tongue_box(edge, (w, h));
        assert_eq!(
            [tx, ty, tw, th].to_vec(),
            tongue,
            "tongue drift on {edge:?} with {count}"
        );

        for (index, ring) in row["rings"].as_array().unwrap().iter().enumerate() {
            let (rx, ry) = ring_origin(edge, index);
            assert_eq!(
                [rx, ry].to_vec(),
                numbers(ring),
                "ring {index} drift on {edge:?} with {count}"
            );
        }
    }
}
