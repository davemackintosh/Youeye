//! SVG import/export and project folder I/O.
//!
//! - Our own SVGs round-trip canonically (first save normalises; every
//!   subsequent load+save is byte-identical).
//! - Foreign SVG best-effort import via `usvg` is a separate entry point for
//!   a later phase.

pub mod svg;

pub use svg::{from_svg, to_svg};

/// Standard line ending used when serialising SVG on disk.
///
/// Always `\n`, never the platform-native sequence, so files byte-match across
/// Linux / Mac / Windows.
pub const LINE_ENDING: &str = "\n";
