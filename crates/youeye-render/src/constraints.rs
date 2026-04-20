//! Pin-to-ruler constraint resolution.
//!
//! A shape carrying `youeye:pin-left/right/top/bottom="ruler-id"` gets its
//! corresponding edge snapped to the referenced ruler's position. Ruler scope
//! walks up the tree — the innermost container's rulers shadow outer ones
//! with the same id.
//!
//! Current scope: *single-edge* pins only. If both `pin-left` and
//! `pin-right` are set, `pin-left` wins and the shape is not stretched.
//! Pin-to-ruler-of-the-wrong-orientation silently falls through. Full
//! priority-based resolution (`kasuari`) will land when we need multi-pin
//! stretching or ruler-to-ruler constraints.

use std::collections::BTreeMap;

use kurbo::{Shape, Vec2};

use youeye_doc::{Node, Ruler, RulerOrientation};

pub type RulerScope<'a> = BTreeMap<String, &'a Ruler>;

/// Collect rulers declared directly in `container_children` (non-recursive).
pub fn collect_rulers(container_children: &[Node]) -> Vec<(String, &Ruler)> {
    container_children
        .iter()
        .filter_map(|c| match c {
            Node::Ruler(r) => r.base.id.clone().map(|id| (id, r)),
            _ => None,
        })
        .collect()
}

/// Shadow parent scope with new locals. Inner rulers win when ids collide.
pub fn extend_scope<'a>(
    parent: &RulerScope<'a>,
    locals: Vec<(String, &'a Ruler)>,
) -> RulerScope<'a> {
    let mut out = parent.clone();
    for (id, r) in locals {
        out.insert(id, r);
    }
    out
}

/// Translation to apply to a shape so its pinned edges land on the referenced
/// rulers. Returns `None` when the shape has no pin attrs (or none resolve).
pub fn resolve_pin_translate(shape: &Node, scope: &RulerScope<'_>) -> Option<Vec2> {
    let attrs = &shape.base().youeye_attrs;
    if !["pin-left", "pin-right", "pin-top", "pin-bottom"]
        .iter()
        .any(|k| attrs.contains_key(*k))
    {
        return None;
    }

    let pin_x = pin_position(attrs, "pin-left", scope, RulerOrientation::Vertical).or_else(|| {
        pin_position(attrs, "pin-right", scope, RulerOrientation::Vertical)
            .map(|r| r - authored_width(shape))
    });
    let pin_y = pin_position(attrs, "pin-top", scope, RulerOrientation::Horizontal).or_else(|| {
        pin_position(attrs, "pin-bottom", scope, RulerOrientation::Horizontal)
            .map(|r| r - authored_height(shape))
    });

    if pin_x.is_none() && pin_y.is_none() {
        return None;
    }

    let auth = authored_top_left(shape);
    let target_x = pin_x.unwrap_or(auth.x);
    let target_y = pin_y.unwrap_or(auth.y);
    Some(Vec2::new(target_x - auth.x, target_y - auth.y))
}

fn pin_position(
    attrs: &BTreeMap<String, String>,
    key: &str,
    scope: &RulerScope<'_>,
    required: RulerOrientation,
) -> Option<f64> {
    let id = attrs.get(key)?;
    let r = scope.get(id.as_str())?;
    if r.orientation == required {
        Some(r.position)
    } else {
        None
    }
}

fn authored_top_left(node: &Node) -> Vec2 {
    match node {
        Node::Rect(r) => Vec2::new(r.x, r.y),
        Node::Frame(f) => Vec2::new(f.x, f.y),
        Node::Ellipse(e) => Vec2::new(e.cx - e.rx, e.cy - e.ry),
        Node::Path(p) => {
            let b = p.data.bounding_box();
            Vec2::new(b.x0, b.y0)
        }
        _ => Vec2::ZERO,
    }
}

fn authored_width(node: &Node) -> f64 {
    match node {
        Node::Rect(r) => r.width,
        Node::Frame(f) => f.width,
        Node::Ellipse(e) => e.rx * 2.0,
        Node::Path(p) => p.data.bounding_box().width(),
        _ => 0.0,
    }
}

fn authored_height(node: &Node) -> f64 {
    match node {
        Node::Rect(r) => r.height,
        Node::Frame(f) => f.height,
        Node::Ellipse(e) => e.ry * 2.0,
        Node::Path(p) => p.data.bounding_box().height(),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use youeye_doc::{NodeBase, Rect};

    fn v_ruler(id: &str, x: f64) -> Ruler {
        Ruler {
            base: NodeBase {
                id: Some(id.into()),
                ..Default::default()
            },
            orientation: RulerOrientation::Vertical,
            position: x,
        }
    }

    fn h_ruler(id: &str, y: f64) -> Ruler {
        Ruler {
            base: NodeBase {
                id: Some(id.into()),
                ..Default::default()
            },
            orientation: RulerOrientation::Horizontal,
            position: y,
        }
    }

    fn rect_with_pins(pins: &[(&str, &str)]) -> Node {
        let mut youeye_attrs = BTreeMap::new();
        for (k, v) in pins {
            youeye_attrs.insert((*k).into(), (*v).into());
        }
        Node::Rect(Rect {
            base: NodeBase {
                youeye_attrs,
                ..Default::default()
            },
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            rx: 0.0,
            ry: 0.0,
        })
    }

    #[test]
    fn no_pins_returns_none() {
        let scope = BTreeMap::new();
        assert!(resolve_pin_translate(&rect_with_pins(&[]), &scope).is_none());
    }

    #[test]
    fn pin_left_moves_shape() {
        let r = v_ruler("gutter", 40.0);
        let mut scope = BTreeMap::new();
        scope.insert("gutter".into(), &r);
        let shift =
            resolve_pin_translate(&rect_with_pins(&[("pin-left", "gutter")]), &scope).unwrap();
        assert_eq!(shift, Vec2::new(40.0, 0.0));
    }

    #[test]
    fn pin_right_places_right_edge_at_ruler() {
        let r = v_ruler("safe-right", 320.0);
        let mut scope = BTreeMap::new();
        scope.insert("safe-right".into(), &r);
        let shift =
            resolve_pin_translate(&rect_with_pins(&[("pin-right", "safe-right")]), &scope).unwrap();
        assert_eq!(shift, Vec2::new(220.0, 0.0));
    }

    #[test]
    fn pin_top_moves_vertically() {
        let r = h_ruler("head", 44.0);
        let mut scope = BTreeMap::new();
        scope.insert("head".into(), &r);
        let shift = resolve_pin_translate(&rect_with_pins(&[("pin-top", "head")]), &scope).unwrap();
        assert_eq!(shift, Vec2::new(0.0, 44.0));
    }

    #[test]
    fn pin_orientation_mismatch_is_ignored() {
        // pin-left refers to a horizontal ruler — does nothing.
        let r = h_ruler("wrong", 80.0);
        let mut scope = BTreeMap::new();
        scope.insert("wrong".into(), &r);
        assert!(resolve_pin_translate(&rect_with_pins(&[("pin-left", "wrong")]), &scope).is_none());
    }

    #[test]
    fn inner_ruler_shadows_outer() {
        let outer = v_ruler("edge", 0.0);
        let inner = v_ruler("edge", 32.0);
        let mut outer_scope: RulerScope = BTreeMap::new();
        outer_scope.insert("edge".into(), &outer);
        let inner_scope = extend_scope(&outer_scope, vec![("edge".into(), &inner)]);
        let shift =
            resolve_pin_translate(&rect_with_pins(&[("pin-left", "edge")]), &inner_scope).unwrap();
        assert_eq!(shift.x, 32.0);
    }
}
