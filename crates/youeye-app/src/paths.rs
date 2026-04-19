//! Per-OS data / config / cache directories.
//!
//! Wraps the `directories` crate so the rest of the code never sees
//! `~/.config/…`, `~/Library/Application Support/…`, `%APPDATA%\…` directly.

#![allow(dead_code)] // Used from phase 2 onwards (font library, project history).

use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;

const QUALIFIER: &str = "co";
const ORGANISATION: &str = "dav3";
const APPLICATION: &str = "youeye";

/// Lazy handle to the per-OS directory layout.
pub struct AppDirs {
    inner: ProjectDirs,
}

impl AppDirs {
    pub fn new() -> Result<Self> {
        let inner = ProjectDirs::from(QUALIFIER, ORGANISATION, APPLICATION)
            .context("could not resolve per-OS project directories")?;
        Ok(Self { inner })
    }

    /// Directory for persistent user-library data (imported fonts, preferences, etc).
    pub fn data_dir(&self) -> &std::path::Path {
        self.inner.data_dir()
    }

    /// Directory for regenerable cache (thumbnail cache, font shaping cache, etc).
    pub fn cache_dir(&self) -> &std::path::Path {
        self.inner.cache_dir()
    }

    /// Directory for config files that aren't meant to move between machines.
    pub fn config_dir(&self) -> &std::path::Path {
        self.inner.config_dir()
    }

    /// Font import destination inside the user library.
    pub fn fonts_dir(&self) -> PathBuf {
        self.data_dir().join("fonts")
    }
}
