//! youeye document model.
//!
//! Pure, UI-independent. Mirrors SVG structure with `youeye:` namespaced
//! extensions for auto-layout, components, rulers, tokens, and variables.
//!
//! Re-exports `kurbo` for consumers that want `Affine` / `BezPath` without
//! adding it to their own `Cargo.toml`.

use std::collections::BTreeMap;

pub mod node;
pub mod style;
pub mod tokens;

pub use node::{Ellipse, Frame, Group, Node, NodeBase, Path, Rect, Text};
pub use style::{Color, Fill, Paint, Stroke};
pub use tokens::{Tokens, Variables};

pub use kurbo;

/// An SVG viewBox — `(min_x, min_y, width, height)` in user-space units.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ViewBox {
    pub min_x: f64,
    pub min_y: f64,
    pub width: f64,
    pub height: f64,
}

/// One youeye document — corresponds to a single `.svg` file on disk (one
/// screen, or `components.svg`).
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub view_box: Option<ViewBox>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub tokens: Tokens,
    pub variables: Variables,
    pub children: Vec<Node>,
    /// Attributes on the root `<svg>` element we don't explicitly model
    /// (`xmlns`, `xmlns:youeye`, `version`, etc.). Preserved verbatim so
    /// canonical round-trip keeps the file self-describing.
    pub extra_attrs: BTreeMap<String, String>,
    /// Any CSS from the `<style>` block that isn't a top-level `:root`
    /// declaration — `@font-face`, `@media`, class modifiers. Preserved
    /// verbatim across round-trip. Top-level `:root` content is the
    /// business of `tokens` / `variables`, which are authoritative.
    pub raw_style_extra: Option<String>,
}

impl Document {
    /// Walk the node tree by child-index path (as used by the selection
    /// model) and return the node at that location. Returns `None` if the
    /// path runs off the end of a childless node or out of bounds.
    pub fn node_at(&self, path: &[usize]) -> Option<&Node> {
        let (first, rest) = path.split_first()?;
        let mut node = self.children.get(*first)?;
        for idx in rest {
            let children = node_children(node)?;
            node = children.get(*idx)?;
        }
        Some(node)
    }

    /// Mutable twin of [`node_at`].
    pub fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let (first, rest) = path.split_first()?;
        let mut node = self.children.get_mut(*first)?;
        for idx in rest {
            let children = node_children_mut(node)?;
            node = children.get_mut(*idx)?;
        }
        Some(node)
    }
}

fn node_children(node: &Node) -> Option<&Vec<Node>> {
    match node {
        Node::Group(g) => Some(&g.children),
        Node::Frame(f) => Some(&f.children),
        _ => None,
    }
}

fn node_children_mut(node: &mut Node) -> Option<&mut Vec<Node>> {
    match node {
        Node::Group(g) => Some(&mut g.children),
        Node::Frame(f) => Some(&mut f.children),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Affine;

    #[test]
    fn empty_document_defaults() {
        let doc = Document::default();
        assert!(doc.view_box.is_none());
        assert!(doc.children.is_empty());
        assert!(doc.tokens.is_empty());
        assert!(doc.variables.is_empty());
    }

    #[test]
    fn node_base_access_through_enum() {
        let mut rect = Rect::default();
        rect.base.id = Some("r1".into());
        rect.width = 100.0;
        let node = Node::Rect(rect);

        assert_eq!(node.base().id.as_deref(), Some("r1"));
        assert_eq!(node.base().transform, Affine::IDENTITY);
    }

    #[test]
    fn base_mut_through_enum() {
        let mut node = Node::Group(Group::default());
        node.base_mut()
            .youeye_attrs
            .insert("layout".into(), "flex".into());
        assert_eq!(
            node.base().youeye_attrs.get("layout").map(String::as_str),
            Some("flex")
        );
    }

    #[test]
    fn tokens_insert_and_get() {
        let mut t = Tokens::default();
        t.insert("brand-primary", "#0052cc");
        assert_eq!(t.len(), 1);
        assert_eq!(t.get("brand-primary"), Some("#0052cc"));
        assert_eq!(t.get("missing"), None);
    }

    #[test]
    fn variables_insert_and_get() {
        let mut v = Variables::default();
        v.insert("rhythm", "8px");
        v.insert("padding-default", "calc(2 * var(--var-rhythm))");
        assert_eq!(v.len(), 2);
        assert_eq!(v.get("rhythm"), Some("8px"));
    }

    #[test]
    fn paint_default_is_none() {
        assert_eq!(Paint::default(), Paint::None);
    }

    #[test]
    fn paint_raw_round_trips_verbatim() {
        let p = Paint::Raw("var(--token-brand-primary)".into());
        if let Paint::Raw(s) = &p {
            assert_eq!(s, "var(--token-brand-primary)");
        } else {
            panic!("expected Raw");
        }
    }

    #[test]
    fn node_at_returns_root_child() {
        let doc = Document {
            children: vec![
                Node::Rect(Rect::default()),
                Node::Ellipse(Ellipse::default()),
            ],
            ..Default::default()
        };
        assert!(matches!(doc.node_at(&[0]), Some(Node::Rect(_))));
        assert!(matches!(doc.node_at(&[1]), Some(Node::Ellipse(_))));
        assert!(doc.node_at(&[2]).is_none());
        assert!(doc.node_at(&[]).is_none());
    }

    #[test]
    fn node_at_walks_nested_tree() {
        let inner = Node::Rect(Rect {
            base: NodeBase {
                id: Some("inner".into()),
                ..Default::default()
            },
            ..Default::default()
        });
        let frame = Node::Frame(Frame {
            children: vec![inner],
            ..Default::default()
        });
        let group = Node::Group(Group {
            children: vec![frame],
            ..Default::default()
        });
        let doc = Document {
            children: vec![group],
            ..Default::default()
        };
        let found = doc.node_at(&[0, 0, 0]).unwrap();
        assert_eq!(found.base().id.as_deref(), Some("inner"));
    }

    #[test]
    fn node_at_mut_allows_edits() {
        let mut doc = Document {
            children: vec![Node::Frame(Frame::default())],
            ..Default::default()
        };
        let node = doc.node_at_mut(&[0]).unwrap();
        node.base_mut()
            .youeye_attrs
            .insert("layout".into(), "flex".into());
        assert_eq!(
            doc.children[0]
                .base()
                .youeye_attrs
                .get("layout")
                .map(String::as_str),
            Some("flex")
        );
    }

    #[test]
    fn node_at_stops_at_leaf() {
        let doc = Document {
            children: vec![Node::Rect(Rect::default())],
            ..Default::default()
        };
        assert!(doc.node_at(&[0, 0]).is_none());
    }

    #[test]
    fn document_can_nest_groups_and_frames() {
        let mut root = Group::default();
        root.children.push(Node::Rect(Rect {
            base: NodeBase {
                id: Some("bg".into()),
                ..Default::default()
            },
            width: 320.0,
            height: 200.0,
            ..Default::default()
        }));

        let mut frame = Frame::default();
        frame.width = 320.0;
        frame.height = 200.0;
        frame
            .base
            .youeye_attrs
            .insert("layout".into(), "flex".into());
        frame.children.push(Node::Group(root));

        let doc = Document {
            view_box: Some(ViewBox {
                min_x: 0.0,
                min_y: 0.0,
                width: 320.0,
                height: 200.0,
            }),
            width: Some(320.0),
            height: Some(200.0),
            children: vec![Node::Frame(frame)],
            ..Default::default()
        };

        assert_eq!(doc.children.len(), 1);
        match &doc.children[0] {
            Node::Frame(f) => {
                assert_eq!(f.width, 320.0);
                assert_eq!(f.children.len(), 1);
            }
            _ => panic!("expected a Frame"),
        }
    }
}
