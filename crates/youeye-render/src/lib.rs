//! Vello-based canvas renderer.
//!
//! Renders a `youeye-doc` tree into a `vello::Scene`. The app crate owns the
//! wgpu device/queue and hands them to us; we do not create our own.

pub mod constraints;
pub mod layout;
pub mod scene;
pub mod text;

pub use scene::build;

pub use kurbo;
pub use vello;
pub use wgpu;
