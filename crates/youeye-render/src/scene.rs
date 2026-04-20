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

use kurbo::{Affine, Ellipse as KEllipse, Rect as KRect, Shape, Stroke as KStroke, Vec2};
use vello::Scene;
use vello::peniko::color::{AlphaColor, Srgb};
use vello::peniko::{Brush, Fill as VelloFill};

use youeye_doc::{Color, Document, Node, NodeBase, Paint};

use crate::layout;

/// Append draw commands for `doc` to `scene`, composed under `root_xform`.
///
/// The caller decides what `root_xform` means — typically a camera transform
/// (translate + scale) composed with any view-level adjustments.
pub fn build(scene: &mut Scene, doc: &Document, root_xform: Affine) {
    for node in &doc.children {
        render_node(scene, node, root_xform);
    }
}

fn render_node(scene: &mut Scene, node: &Node, parent_xform: Affine) {
    let xform = parent_xform * node.base().transform;
    match node {
        Node::Group(g) => {
            for c in &g.children {
                render_node(scene, c, xform);
            }
        }
        Node::Frame(f) => {
            let local = xform * Affine::translate(Vec2::new(f.x, f.y));
            match layout::compute_flex_positions(f) {
                Some(positions) => {
                    for (child, placed) in f.children.iter().zip(positions.iter()) {
                        let shift = placed.top_left - layout::authored_top_left(child);
                        let child_xform = local * Affine::translate(shift);
                        render_node(scene, child, child_xform);
                    }
                }
                None => {
                    for c in &f.children {
                        render_node(scene, c, local);
                    }
                }
            }
        }
        Node::Rect(r) => {
            let shape = KRect::new(r.x, r.y, r.x + r.width, r.y + r.height);
            paint_shape(scene, &shape, node.base(), xform);
        }
        Node::Ellipse(e) => {
            let shape = KEllipse::new((e.cx, e.cy), (e.rx, e.ry), 0.0);
            paint_shape(scene, &shape, node.base(), xform);
        }
        Node::Path(p) => {
            paint_shape(scene, &p.data, node.base(), xform);
        }
        Node::Text(_) => {
            // Deferred to phase 8 (parley).
        }
    }
}

fn paint_shape(scene: &mut Scene, shape: &impl Shape, base: &NodeBase, xform: Affine) {
    if let Some(fill) = &base.fill
        && let Some(brush) = paint_to_brush(&fill.paint, fill.opacity)
    {
        scene.fill(VelloFill::NonZero, xform, &brush, None, shape);
    }
    if let Some(stroke) = &base.stroke
        && let Some(brush) = paint_to_brush(&stroke.paint, stroke.opacity)
    {
        let width = stroke.width.unwrap_or(1.0);
        let kstroke = KStroke::new(width);
        scene.stroke(&kstroke, xform, &brush, None, shape);
    }
}

fn paint_to_brush(paint: &Paint, opacity: Option<f32>) -> Option<Brush> {
    match paint {
        Paint::None => None,
        Paint::Solid(c) => {
            let applied = apply_opacity(*c, opacity);
            Some(Brush::Solid(color_to_vello(applied)))
        }
        Paint::Raw(_) => None,
    }
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
