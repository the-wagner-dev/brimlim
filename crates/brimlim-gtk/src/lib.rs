//! The layer-shell frontend, as a library so its pure parts — the geometry
//! and the colour grade that must stay in step with the GNOME extension —
//! can be tested from outside.

pub mod announce;
pub mod client;
pub mod format;
pub mod geometry;
pub mod notch;
pub mod paint;
pub mod palette;
