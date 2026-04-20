//! Fill / stroke style types.
//!
//! Kept deliberately minimal: the renderer only consumes solid colours today,
//! and anything else (gradients, `var(--...)`, `url(#...)`, CSS keywords)
//! travels as a `Raw` string so the SVG round-trips unchanged.

/// sRGB colour, each channel in `[0.0, 1.0]`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const TRANSPARENT: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
}

#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    None,
    Solid(Color),
    /// Anything we don't yet parse — `var(--token-...)`, `url(#grad1)`, CSS
    /// named colours, gradients. Stored verbatim and re-emitted unchanged.
    Raw(String),
}

impl Default for Paint {
    fn default() -> Self {
        Paint::None
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fill {
    pub paint: Paint,
    pub opacity: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stroke {
    pub paint: Paint,
    pub width: Option<f64>,
    pub opacity: Option<f32>,
}
