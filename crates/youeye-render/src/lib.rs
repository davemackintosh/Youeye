//! Vello-based canvas renderer.
//!
//! Renders a `youeye-doc` tree into a `vello::Scene`. The app crate owns the
//! wgpu device/queue and hands them to us; we do not create our own.
//!
//! Phase 1: placeholder. The real scene-building lands alongside the document
//! model in phase 2.

pub use kurbo;
pub use vello;
pub use wgpu;
