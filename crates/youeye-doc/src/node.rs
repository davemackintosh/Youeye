//! Document node tree.
//!
//! Shape-level SVG elements mirror 1:1 — `Rect`, `Ellipse`, `Path`, `Text`,
//! `Group`. `Frame` is a youeye concept: a group that owns width/height and,
//! later, auto-layout metadata (`youeye:layout="flex"` etc.).
//!
//! `NodeBase` carries the cross-cutting concerns: id, transform, paint, a
//! `youeye:*` attribute bag for our namespaced extensions, and an
//! `extra_attrs` bag that preserves any untyped SVG attributes verbatim so
//! canonical round-trip doesn't lose unknown data.

use std::collections::BTreeMap;

use kurbo::{Affine, BezPath};

use crate::style::{Fill, Stroke};

#[derive(Debug, Clone, Default)]
pub struct NodeBase {
    pub id: Option<String>,
    pub transform: Affine,
    pub fill: Option<Fill>,
    pub stroke: Option<Stroke>,
    /// `youeye:*` attributes, keyed by the bare local name
    /// (e.g. `"layout" -> "flex"` for `youeye:layout="flex"`).
    pub youeye_attrs: BTreeMap<String, String>,
    /// Unmodelled attributes preserved as raw strings. Keyed by the full
    /// attribute name as it appeared in the source (e.g. `"data-foo"`,
    /// `"aria-label"`, `"clip-rule"`).
    pub extra_attrs: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Group(Group),
    Frame(Frame),
    Rect(Rect),
    Ellipse(Ellipse),
    Path(Path),
    Text(Text),
}

impl Node {
    pub fn base(&self) -> &NodeBase {
        match self {
            Node::Group(n) => &n.base,
            Node::Frame(n) => &n.base,
            Node::Rect(n) => &n.base,
            Node::Ellipse(n) => &n.base,
            Node::Path(n) => &n.base,
            Node::Text(n) => &n.base,
        }
    }

    pub fn base_mut(&mut self) -> &mut NodeBase {
        match self {
            Node::Group(n) => &mut n.base,
            Node::Frame(n) => &mut n.base,
            Node::Rect(n) => &mut n.base,
            Node::Ellipse(n) => &mut n.base,
            Node::Path(n) => &mut n.base,
            Node::Text(n) => &mut n.base,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Group {
    pub base: NodeBase,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub base: NodeBase,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Default)]
pub struct Rect {
    pub base: NodeBase,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub rx: f64,
    pub ry: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Ellipse {
    pub base: NodeBase,
    pub cx: f64,
    pub cy: f64,
    pub rx: f64,
    pub ry: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Path {
    pub base: NodeBase,
    pub data: BezPath,
}

#[derive(Debug, Clone, Default)]
pub struct Text {
    pub base: NodeBase,
    pub x: f64,
    pub y: f64,
    pub content: String,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
}
