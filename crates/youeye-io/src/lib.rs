//! SVG import/export and project folder I/O.
//!
//! - Our own SVGs round-trip perfectly (all `youeye:*` preserved).
//! - Foreign SVGs: best-effort via `usvg`, flattened into raw shapes.
//!
//! Phase 1: placeholder.

/// Standard line ending used when serialising SVG on disk.
///
/// Always `\n`, never the platform-native sequence, so files byte-match across
/// Linux / Mac / Windows.
pub const LINE_ENDING: &str = "\n";
