//! Walk a [`Document`] and emit vello draw commands into an existing
//! [`Scene`].
//!
//! Scope: solid fills and strokes on `Rect`, `Ellipse`, and `Path`. `Group`
//! recurses. `Frame` recurses with a `(x, y)` translation. `Text` is a no-op
//! until parley integration lands in phase 8.
//!
//! `Paint::Raw` (gradients, `var(--...)`, `url(#id)`, named colours) is
//! silently skipped rather than drawn in a fallback colour — the editor's
//! "off-token" warning UI can surface it, but we don't want misleading
//! visuals on the canvas.

use kurbo::{
    Affine, Ellipse as KEllipse, Line as KLine, Point, Rect as KRect, Shape, Stroke as KStroke,
    Vec2,
};
use vello::Scene;
use vello::peniko::color::{AlphaColor, Srgb};
use vello::peniko::{Brush, Fill as VelloFill};

use youeye_doc::{Color, Document, Node, NodeBase, Paint, Ruler, RulerOrientation};

use crate::constraints::{self, RulerScope};
use crate::layout;

/// Append draw commands for `doc` to `scene`, composed under `root_xform`.
///
/// The caller decides what `root_xform` means — typically a camera transform
/// (translate + scale) composed with any view-level adjustments.
pub fn build(scene: &mut Scene, doc: &Document, root_xform: Affine) {
    let root_bounds = doc
        .view_box
        .map(|vb| {
            KRect::new(
                vb.min_x,
                vb.min_y,
                vb.min_x + vb.width,
                vb.min_y + vb.height,
            )
        })
        .unwrap_or_else(|| KRect::new(-10_000.0, -10_000.0, 10_000.0, 10_000.0));
    let root_scope = constraints::extend_scope(
        &RulerScope::new(),
        constraints::collect_rulers(&doc.children),
    );
    for node in &doc.children {
        render_node(scene, node, root_xform, root_bounds, &root_scope, doc);
    }
}

/// Find a component definition by id anywhere in the doc tree. Walks
/// doc.children and descends into Groups, Frames, and Components (so
/// nested definitions still resolve) but not Uses (no recursion).
fn find_component<'a>(doc: &'a Document, id: &str) -> Option<&'a youeye_doc::Component> {
    fn walk<'a>(children: &'a [Node], id: &str) -> Option<&'a youeye_doc::Component> {
        for child in children {
            match child {
                Node::Component(c) if c.base.id.as_deref() == Some(id) => return Some(c),
                Node::Component(c) => {
                    if let Some(found) = walk(&c.children, id) {
                        return Some(found);
                    }
                }
                Node::Group(g) => {
                    if let Some(found) = walk(&g.children, id) {
                        return Some(found);
                    }
                }
                Node::Frame(f) => {
                    if let Some(found) = walk(&f.children, id) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    walk(&doc.children, id)
}

fn render_node(
    scene: &mut Scene,
    node: &Node,
    parent_xform: Affine,
    parent_bounds: KRect,
    scope: &RulerScope<'_>,
    doc: &Document,
) {
    let base_xform = parent_xform * node.base().transform;
    // Apply any pin-to-ruler translation before drawing. Containers don't
    // currently get pinned (their children do), but it's harmless to try.
    let xform = match constraints::resolve_pin_translate(node, scope) {
        Some(shift) => base_xform * Affine::translate(shift),
        None => base_xform,
    };
    match node {
        Node::Group(g) => {
            let child_scope =
                constraints::extend_scope(scope, constraints::collect_rulers(&g.children));
            for c in &g.children {
                render_node(scene, c, xform, parent_bounds, &child_scope, doc);
            }
        }
        Node::Frame(f) => {
            let local = xform * Affine::translate(Vec2::new(f.x, f.y));
            let bounds = KRect::new(0.0, 0.0, f.width, f.height);
            let child_scope =
                constraints::extend_scope(scope, constraints::collect_rulers(&f.children));
            match layout::compute_flex_positions(f) {
                Some(positions) => {
                    for (child, placed) in f.children.iter().zip(positions.iter()) {
                        match (child, placed) {
                            (Node::Ruler(_), _) => {
                                render_node(scene, child, local, bounds, &child_scope, doc);
                            }
                            (_, Some(layout_pos)) => {
                                let shift = layout_pos.top_left - layout::authored_top_left(child);
                                let child_xform = local * Affine::translate(shift);
                                render_node(scene, child, child_xform, bounds, &child_scope, doc);
                            }
                            (_, None) => {
                                render_node(scene, child, local, bounds, &child_scope, doc);
                            }
                        }
                    }
                }
                None => {
                    for c in &f.children {
                        render_node(scene, c, local, bounds, &child_scope, doc);
                    }
                }
            }
        }
        Node::Rect(r) => {
            let shape = KRect::new(r.x, r.y, r.x + r.width, r.y + r.height);
            paint_shape(scene, &shape, node.base(), xform, doc);
        }
        Node::Ellipse(e) => {
            let shape = KEllipse::new((e.cx, e.cy), (e.rx, e.ry), 0.0);
            paint_shape(scene, &shape, node.base(), xform, doc);
        }
        Node::Path(p) => {
            paint_shape(scene, &p.data, node.base(), xform, doc);
        }
        Node::Text(t) => {
            crate::text::draw_text(scene, t, xform, doc);
        }
        Node::Ruler(r) => {
            render_ruler(scene, r, xform, parent_bounds);
        }
        Node::Component(_) => {
            // Definitions don't draw on their own — only `Use` references do.
        }
        Node::Use(u) => {
            let Some(target) = find_component(doc, &u.href) else {
                return;
            };
            let use_xform = xform * Affine::translate(Vec2::new(u.x, u.y));
            let inner_scope =
                constraints::extend_scope(scope, constraints::collect_rulers(&target.children));
            for c in &target.children {
                render_node(scene, c, use_xform, parent_bounds, &inner_scope, doc);
            }
        }
    }
}

fn render_ruler(scene: &mut Scene, ruler: &Ruler, xform: Affine, bounds: KRect) {
    let brush = Brush::Solid(AlphaColor::<Srgb>::from_rgba8(0xff, 0x57, 0x22, 0xc0));
    let mut stroke = KStroke::new(1.0);
    stroke.dash_pattern.push(6.0);
    stroke.dash_pattern.push(4.0);
    let line = match ruler.orientation {
        RulerOrientation::Horizontal => KLine::new(
            Point::new(bounds.x0, ruler.position),
            Point::new(bounds.x1, ruler.position),
        ),
        RulerOrientation::Vertical => KLine::new(
            Point::new(ruler.position, bounds.y0),
            Point::new(ruler.position, bounds.y1),
        ),
    };
    scene.stroke(&stroke, xform, &brush, None, &line);
}

fn paint_shape(
    scene: &mut Scene,
    shape: &impl Shape,
    base: &NodeBase,
    xform: Affine,
    doc: &Document,
) {
    if let Some(fill) = &base.fill
        && let Some(brush) = paint_to_brush(&fill.paint, fill.opacity, doc)
    {
        scene.fill(VelloFill::NonZero, xform, &brush, None, shape);
    }
    if let Some(stroke) = &base.stroke
        && let Some(brush) = paint_to_brush(&stroke.paint, stroke.opacity, doc)
    {
        let width = stroke.width.unwrap_or(1.0);
        let kstroke = KStroke::new(width);
        scene.stroke(&kstroke, xform, &brush, None, shape);
    }
}

pub(crate) fn paint_to_brush(paint: &Paint, opacity: Option<f32>, doc: &Document) -> Option<Brush> {
    match paint {
        Paint::None => None,
        Paint::Solid(c) => {
            let applied = apply_opacity(*c, opacity);
            Some(Brush::Solid(color_to_vello(applied)))
        }
        Paint::Raw(s) => {
            let resolved = resolve_color_reference(s, doc, 0)?;
            let applied = apply_opacity(resolved, opacity);
            Some(Brush::Solid(color_to_vello(applied)))
        }
    }
}

const MAX_VAR_DEPTH: u32 = 8;

/// Resolve a CSS-ish colour reference like `var(--token-brand)` or
/// `var(--var-accent)` — following chained references up to
/// [`MAX_VAR_DEPTH`] — and parse the terminal value as a colour. Returns
/// `None` on any parse failure, missing token/variable, or cycle.
fn resolve_color_reference(raw: &str, doc: &Document, depth: u32) -> Option<Color> {
    if depth > MAX_VAR_DEPTH {
        return None;
    }
    let t = raw.trim();
    if let Some(name) = var_token_name(t) {
        let value = doc.tokens.get(name)?;
        return parse_color(value).or_else(|| resolve_color_reference(value, doc, depth + 1));
    }
    if let Some(name) = var_var_name(t) {
        let value = doc.variables.get(name)?;
        return parse_color(value).or_else(|| resolve_color_reference(value, doc, depth + 1));
    }
    parse_color(t)
}

fn var_token_name(s: &str) -> Option<&str> {
    s.trim()
        .strip_prefix("var(--token-")
        .and_then(|r| r.strip_suffix(')'))
}

fn var_var_name(s: &str) -> Option<&str> {
    s.trim()
        .strip_prefix("var(--var-")
        .and_then(|r| r.strip_suffix(')'))
}

/// Parse a colour literal: `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(r, g, b)`,
/// `rgba(r, g, b, a)`. `r/g/b` in 0..=255, `a` in 0..=1.
fn parse_color(s: &str) -> Option<Color> {
    let t = s.trim();
    if let Some(c) = parse_hex(t) {
        return Some(c);
    }
    if let Some(c) = parse_rgb_fn(t) {
        return Some(c);
    }
    None
}

fn parse_hex(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#')?;
    let (r, g, b, a) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            (r, g, b, 255u8)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b, 255u8)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    })
}

fn parse_rgb_fn(s: &str) -> Option<Color> {
    let (has_alpha, rest) = if let Some(r) = s.strip_prefix("rgb(") {
        (false, r)
    } else if let Some(r) = s.strip_prefix("rgba(") {
        (true, r)
    } else {
        return None;
    };
    let rest = rest.strip_suffix(')')?;
    let parts: Vec<&str> = rest.split(',').map(str::trim).collect();
    let expected = if has_alpha { 4 } else { 3 };
    if parts.len() != expected {
        return None;
    }
    let r = parts[0].parse::<f32>().ok()? / 255.0;
    let g = parts[1].parse::<f32>().ok()? / 255.0;
    let b = parts[2].parse::<f32>().ok()? / 255.0;
    let a = if has_alpha {
        parts[3].parse::<f32>().ok()?
    } else {
        1.0
    };
    Some(Color { r, g, b, a })
}

fn apply_opacity(c: Color, opacity: Option<f32>) -> Color {
    match opacity {
        Some(o) => Color {
            a: c.a * o.clamp(0.0, 1.0),
            ..c
        },
        None => c,
    }
}

fn color_to_vello(c: Color) -> vello::peniko::Color {
    let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = (c.a.clamp(0.0, 1.0) * 255.0).round() as u8;
    AlphaColor::<Srgb>::from_rgba8(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use youeye_doc::{
        Document, Ellipse as DocEllipse, Fill as DocFill, Group, Node, NodeBase, Paint as DocPaint,
        Path as DocPath, Rect as DocRect,
    };

    #[test]
    fn build_does_not_panic_on_empty_document() {
        let mut scene = Scene::new();
        build(&mut scene, &Document::default(), Affine::IDENTITY);
    }

    #[test]
    fn build_walks_nested_tree() {
        let rect = Node::Rect(DocRect {
            base: NodeBase {
                fill: Some(DocFill {
                    paint: DocPaint::Solid(Color {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    opacity: Some(0.5),
                }),
                ..Default::default()
            },
            width: 100.0,
            height: 100.0,
            ..Default::default()
        });
        let ellipse = Node::Ellipse(DocEllipse {
            cx: 200.0,
            cy: 200.0,
            rx: 50.0,
            ry: 30.0,
            ..Default::default()
        });
        let path = Node::Path(DocPath::default());
        let group = Node::Group(Group {
            children: vec![rect, ellipse, path],
            ..Default::default()
        });
        let doc = Document {
            children: vec![group],
            ..Default::default()
        };

        let mut scene = Scene::new();
        build(&mut scene, &doc, Affine::IDENTITY);
    }

    #[test]
    fn raw_paint_is_skipped() {
        let n = Node::Rect(DocRect {
            base: NodeBase {
                fill: Some(DocFill {
                    paint: DocPaint::Raw("var(--token-brand)".into()),
                    opacity: None,
                }),
                ..Default::default()
            },
            width: 10.0,
            height: 10.0,
            ..Default::default()
        });
        let doc = Document {
            children: vec![n],
            ..Default::default()
        };
        let mut scene = Scene::new();
        // Should not panic — Raw paint is a no-op.
        build(&mut scene, &doc, Affine::IDENTITY);
    }
}
