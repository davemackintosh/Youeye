//! Auto-layout bridge between youeye Frames and the `taffy` flexbox engine.
//!
//! A Frame with `youeye:layout="flex"` becomes a taffy flex container; each
//! direct child becomes a leaf with a fixed size derived from its shape.
//! We compute layout once, then the scene builder uses the returned
//! positions to place each child.
//!
//! Per-child flex overrides (`flex-grow`, `align-self`) are not wired up yet
//! — only frame-level direction / justify / align / gap / padding. Gap and
//! padding that reference `var(--...)` / `calc(...)` resolve to `0` for now;
//! the token resolver for layout lands alongside the inspector in slice C.

use std::collections::BTreeMap;

use kurbo::{Shape, Vec2};
use taffy::prelude::*;
use youeye_doc::{Frame, Node};

/// Computed placement for one flex child, relative to the frame's origin.
#[derive(Debug, Clone, Copy)]
pub struct ChildLayout {
    pub top_left: Vec2,
}

/// Returns `Some(positions)` with one slot per child in `frame.children`
/// order when `frame` carries `youeye:layout="flex"`. Rulers get `None`
/// because they're layout metadata, not flex participants. Returns `None`
/// overall when the frame is not auto-laid-out.
pub fn compute_flex_positions(frame: &Frame) -> Option<Vec<Option<ChildLayout>>> {
    if frame.base.youeye_attrs.get("layout").map(String::as_str) != Some("flex") {
        return None;
    }

    let mut taffy: TaffyTree<()> = TaffyTree::new();

    let attrs = &frame.base.youeye_attrs;
    let gap_val = parse_length(attrs, "gap");
    let padding_val = parse_length(attrs, "padding");

    let root_style = Style {
        display: Display::Flex,
        size: Size {
            width: Dimension::length(frame.width as f32),
            height: Dimension::length(frame.height as f32),
        },
        flex_direction: flex_direction(attrs),
        justify_content: Some(justify_content(attrs)),
        align_items: Some(align_items(attrs)),
        gap: Size {
            width: LengthPercentage::length(gap_val),
            height: LengthPercentage::length(gap_val),
        },
        padding: Rect {
            left: LengthPercentage::length(padding_val),
            right: LengthPercentage::length(padding_val),
            top: LengthPercentage::length(padding_val),
            bottom: LengthPercentage::length(padding_val),
        },
        ..Default::default()
    };

    let mut taffy_ids: Vec<Option<NodeId>> = Vec::with_capacity(frame.children.len());
    let mut flex_children: Vec<NodeId> = Vec::new();
    for child in &frame.children {
        if matches!(child, Node::Ruler(_)) {
            taffy_ids.push(None);
            continue;
        }
        let (w, h) = child_size(child);
        let style = Style {
            size: Size {
                width: Dimension::length(w as f32),
                height: Dimension::length(h as f32),
            },
            ..Default::default()
        };
        let id = taffy.new_leaf(style).ok()?;
        flex_children.push(id);
        taffy_ids.push(Some(id));
    }

    let root = taffy.new_with_children(root_style, &flex_children).ok()?;
    taffy
        .compute_layout(
            root,
            Size {
                width: AvailableSpace::Definite(frame.width as f32),
                height: AvailableSpace::Definite(frame.height as f32),
            },
        )
        .ok()?;

    let mut out = Vec::with_capacity(taffy_ids.len());
    for id in &taffy_ids {
        out.push(match id {
            Some(id) => {
                let layout = taffy.layout(*id).ok()?;
                Some(ChildLayout {
                    top_left: Vec2::new(layout.location.x as f64, layout.location.y as f64),
                })
            }
            None => None,
        });
    }
    Some(out)
}

/// Where a node considers its "origin" to be in its own coordinate space —
/// used by the scene builder to translate children into the positions taffy
/// computes, regardless of where the node was authored.
pub fn authored_top_left(node: &Node) -> Vec2 {
    match node {
        Node::Rect(r) => Vec2::new(r.x, r.y),
        Node::Frame(f) => Vec2::new(f.x, f.y),
        Node::Ellipse(e) => Vec2::new(e.cx - e.rx, e.cy - e.ry),
        Node::Path(p) => {
            let b = p.data.bounding_box();
            Vec2::new(b.x0, b.y0)
        }
        Node::Use(u) => Vec2::new(u.x, u.y),
        Node::Group(_) | Node::Text(_) | Node::Ruler(_) | Node::Component(_) => Vec2::ZERO,
    }
}

fn child_size(node: &Node) -> (f64, f64) {
    match node {
        Node::Rect(r) => (r.width, r.height),
        Node::Frame(f) => (f.width, f.height),
        Node::Ellipse(e) => (e.rx * 2.0, e.ry * 2.0),
        Node::Path(p) => {
            let b = p.data.bounding_box();
            (b.width(), b.height())
        }
        // Groups, text (no parley yet), rulers, and component defs/uses
        // contribute 0x0 to the flex layout. Good enough for now.
        Node::Group(_) | Node::Text(_) | Node::Ruler(_) | Node::Component(_) | Node::Use(_) => {
            (0.0, 0.0)
        }
    }
}

fn flex_direction(attrs: &BTreeMap<String, String>) -> FlexDirection {
    match attrs.get("flex-direction").map(String::as_str) {
        Some("column") => FlexDirection::Column,
        Some("row-reverse") => FlexDirection::RowReverse,
        Some("column-reverse") => FlexDirection::ColumnReverse,
        _ => FlexDirection::Row,
    }
}

fn justify_content(attrs: &BTreeMap<String, String>) -> JustifyContent {
    match attrs.get("justify").map(String::as_str) {
        Some("center") => JustifyContent::Center,
        Some("end") => JustifyContent::End,
        Some("space-between") => JustifyContent::SpaceBetween,
        Some("space-around") => JustifyContent::SpaceAround,
        Some("space-evenly") => JustifyContent::SpaceEvenly,
        _ => JustifyContent::Start,
    }
}

fn align_items(attrs: &BTreeMap<String, String>) -> AlignItems {
    match attrs.get("align").map(String::as_str) {
        Some("center") => AlignItems::Center,
        Some("end") => AlignItems::End,
        Some("stretch") => AlignItems::Stretch,
        _ => AlignItems::Start,
    }
}

fn parse_length(attrs: &BTreeMap<String, String>, key: &str) -> f32 {
    let Some(raw) = attrs.get(key) else {
        return 0.0;
    };
    let trimmed = raw.trim();
    // `var(...)` / `calc(...)` need the token resolver — defer until slice C.
    if trimmed.starts_with("var(") || trimmed.starts_with("calc(") {
        return 0.0;
    }
    let end = trimmed
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(trimmed.len());
    trimmed[..end].parse().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use youeye_doc::{NodeBase, Rect as DocRect};

    fn flex_frame(width: f64, height: f64, attrs: &[(&str, &str)], children: Vec<Node>) -> Frame {
        let mut youeye_attrs = BTreeMap::new();
        youeye_attrs.insert("layout".into(), "flex".into());
        for (k, v) in attrs {
            youeye_attrs.insert((*k).into(), (*v).into());
        }
        Frame {
            base: NodeBase {
                youeye_attrs,
                ..Default::default()
            },
            x: 0.0,
            y: 0.0,
            width,
            height,
            children,
        }
    }

    fn rect(w: f64, h: f64) -> Node {
        Node::Rect(DocRect {
            width: w,
            height: h,
            ..Default::default()
        })
    }

    #[test]
    fn non_flex_frame_returns_none() {
        let frame = Frame {
            base: NodeBase::default(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            children: vec![rect(10.0, 10.0)],
        };
        assert!(compute_flex_positions(&frame).is_none());
    }

    #[test]
    fn row_lays_out_children_side_by_side() {
        let frame = flex_frame(300.0, 100.0, &[], vec![rect(50.0, 50.0), rect(50.0, 50.0)]);
        let positions = compute_flex_positions(&frame).expect("flex positions");
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0].unwrap().top_left, Vec2::new(0.0, 0.0));
        assert_eq!(positions[1].unwrap().top_left, Vec2::new(50.0, 0.0));
    }

    #[test]
    fn row_with_gap() {
        let frame = flex_frame(
            300.0,
            100.0,
            &[("gap", "10")],
            vec![rect(50.0, 50.0), rect(50.0, 50.0)],
        );
        let p = compute_flex_positions(&frame).unwrap();
        assert_eq!(p[0].unwrap().top_left.x, 0.0);
        assert_eq!(p[1].unwrap().top_left.x, 60.0);
    }

    #[test]
    fn column_stacks_children() {
        let frame = flex_frame(
            100.0,
            300.0,
            &[("flex-direction", "column")],
            vec![rect(50.0, 40.0), rect(50.0, 40.0)],
        );
        let p = compute_flex_positions(&frame).unwrap();
        assert_eq!(p[0].unwrap().top_left, Vec2::new(0.0, 0.0));
        assert_eq!(p[1].unwrap().top_left, Vec2::new(0.0, 40.0));
    }

    #[test]
    fn padding_pushes_first_child() {
        let frame = flex_frame(300.0, 100.0, &[("padding", "16")], vec![rect(50.0, 50.0)]);
        let p = compute_flex_positions(&frame).unwrap();
        assert_eq!(p[0].unwrap().top_left, Vec2::new(16.0, 16.0));
    }

    #[test]
    fn justify_center_centers_single_child() {
        let frame = flex_frame(
            200.0,
            100.0,
            &[("justify", "center")],
            vec![rect(50.0, 50.0)],
        );
        let p = compute_flex_positions(&frame).unwrap();
        assert_eq!(p[0].unwrap().top_left.x, 75.0);
    }

    #[test]
    fn justify_space_between_distributes_children() {
        let frame = flex_frame(
            300.0,
            100.0,
            &[("justify", "space-between")],
            vec![rect(50.0, 50.0), rect(50.0, 50.0)],
        );
        let p = compute_flex_positions(&frame).unwrap();
        assert_eq!(p[0].unwrap().top_left.x, 0.0);
        assert_eq!(p[1].unwrap().top_left.x, 250.0);
    }

    #[test]
    fn align_center_centers_children_on_cross_axis() {
        let frame = flex_frame(300.0, 100.0, &[("align", "center")], vec![rect(50.0, 40.0)]);
        let p = compute_flex_positions(&frame).unwrap();
        assert_eq!(p[0].unwrap().top_left.y, 30.0);
    }

    #[test]
    fn var_gap_ignored_for_now() {
        // `var(--var-rhythm)` is valid in the file but we can't resolve it
        // without a token resolver; it should behave like `gap="0"` rather
        // than panicking or producing garbage.
        let frame = flex_frame(
            300.0,
            100.0,
            &[("gap", "var(--var-rhythm)")],
            vec![rect(50.0, 50.0), rect(50.0, 50.0)],
        );
        let p = compute_flex_positions(&frame).unwrap();
        assert_eq!(p[1].unwrap().top_left.x, 50.0);
    }
}
