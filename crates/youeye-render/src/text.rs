//! Text rendering via parley → vello glyph draw.
//!
//! Minimal: system fonts via fontique, honours `Text.font_family` and
//! `font_size`, fills with the node's `fill` paint (defaulting to black).
//! SVG convention places `(text.x, text.y)` at the *baseline* of the first
//! glyph, so we translate the parley layout so that line's baseline lands
//! there.
//!
//! Per-run normalised variable-font coords, line breaking with max-width,
//! embedded fonts, and RTL bidi are intentional deferrals — add when the
//! design tool actually needs them.

use std::cell::RefCell;

use kurbo::{Affine, Vec2};
use std::borrow::Cow;

use parley::{FontContext, FontFamily, LayoutContext, PositionedLayoutItem, StyleProperty};
use vello::Scene;
use vello::peniko::color::{AlphaColor, Srgb};
use vello::peniko::{Brush, Fill as VelloFill};

use youeye_doc::{Document, Text};

thread_local! {
    static FONT_CTX: RefCell<FontContext> = RefCell::new(FontContext::new());
    static LAYOUT_CTX: RefCell<LayoutContext<()>> = RefCell::new(LayoutContext::new());
}

/// Enumerate the font family names known to the local fontique collection.
/// Sorted and de-duplicated. Intended for populating font-picker UI;
/// callers should cache the result — this walks every installed family
/// name from scratch each call.
pub fn list_font_families() -> Vec<String> {
    FONT_CTX.with(|fcx| {
        let mut fcx = fcx.borrow_mut();
        let mut names: Vec<String> = fcx
            .collection
            .family_names()
            .map(|s| s.to_string())
            .collect();
        names.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
        names.dedup();
        names
    })
}

pub fn draw_text(scene: &mut Scene, text: &Text, xform: Affine, doc: &Document) {
    if text.content.is_empty() {
        return;
    }
    let font_size = text.font_size.unwrap_or(16.0) as f32;
    let brush = brush_for_text(text, doc);

    FONT_CTX.with(|fcx| {
        LAYOUT_CTX.with(|lcx| {
            let mut fcx = fcx.borrow_mut();
            let mut lcx = lcx.borrow_mut();
            let mut builder = lcx.ranged_builder(&mut fcx, &text.content, 1.0, true);
            builder.push_default(StyleProperty::FontSize(font_size));
            if let Some(family) = text.font_family.as_deref() {
                builder.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(
                    family.to_string(),
                ))));
            }
            let mut layout: parley::Layout<()> = builder.build(&text.content);
            layout.break_all_lines(None);

            // SVG puts (text.x, text.y) at the first glyph's baseline. Parley
            // positions glyphs with y = baseline within its own layout box,
            // so to move that baseline to text.y we subtract the first
            // line's baseline offset.
            let first_baseline = layout
                .lines()
                .next()
                .map(|l| l.metrics().baseline as f64)
                .unwrap_or(0.0);
            let text_xform = xform * Affine::translate(Vec2::new(text.x, text.y - first_baseline));

            for line in layout.lines() {
                for item in line.items() {
                    let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                        continue;
                    };
                    let run = glyph_run.run();
                    let font = run.font();
                    let size = run.font_size();
                    let coords = run.normalized_coords();
                    scene
                        .draw_glyphs(font)
                        .font_size(size)
                        .brush(&brush)
                        .transform(text_xform)
                        .normalized_coords(coords)
                        .draw(
                            VelloFill::NonZero,
                            glyph_run.positioned_glyphs().map(|g| vello::Glyph {
                                id: g.id,
                                x: g.x,
                                y: g.y,
                            }),
                        );
                }
            }
        });
    });
}

fn brush_for_text(text: &Text, doc: &Document) -> Brush {
    let fill = text.base.fill.as_ref();
    let opacity = fill.and_then(|f| f.opacity);
    let paint = fill.map(|f| &f.paint);

    // Resolve via the shared paint-to-brush path so tokens / variables work
    // the same way here as for shapes. Fall back to solid black so
    // unresolved text stays visible (an invisible glyph run would look
    // like a bug).
    if let Some(p) = paint
        && let Some(brush) = crate::scene::paint_to_brush(p, opacity, doc)
    {
        return brush;
    }
    let default_rgba = (
        0,
        0,
        0,
        (opacity.unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8,
    );
    Brush::Solid(AlphaColor::<Srgb>::from_rgba8(
        default_rgba.0,
        default_rgba.1,
        default_rgba.2,
        default_rgba.3,
    ))
}
