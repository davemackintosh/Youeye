//! Persistent recently-used colour palette.
//!
//! Stored as `RRGGBBAA` hex per line under the per-OS data directory.
//! Text is friendlier than JSON for a 10-line list — human-readable, no
//! serde dep, trivial to parse / write.

use std::fs;
use std::path::PathBuf;

use tracing::warn;

use crate::paths::AppDirs;

const FILE_NAME: &str = "recent-colors.txt";
const MAX_ENTRIES: usize = 10;

pub fn load() -> Vec<[u8; 4]> {
    let Some(path) = path() else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(parse_hex8)
        .take(MAX_ENTRIES)
        .collect()
}

pub fn save(colors: &[[u8; 4]]) {
    let Some(path) = path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        warn!("recent colours: cannot create {parent:?}: {e:?}");
        return;
    }
    let out: String = colors
        .iter()
        .take(MAX_ENTRIES)
        .map(|c| format!("{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]))
        .collect::<Vec<_>>()
        .join("\n");
    if let Err(e) = fs::write(&path, out) {
        warn!("recent colours: cannot write {path:?}: {e:?}");
    }
}

fn path() -> Option<PathBuf> {
    AppDirs::new().ok().map(|d| d.data_dir().join(FILE_NAME))
}

fn parse_hex8(line: &str) -> Option<[u8; 4]> {
    let s = line.trim().strip_prefix('#').unwrap_or(line.trim());
    if s.len() != 8 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    let a = u8::from_str_radix(&s[6..8], 16).ok()?;
    Some([r, g, b, a])
}
