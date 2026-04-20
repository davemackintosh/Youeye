//! egui layout: sidebars, status bar, central canvas.

use std::collections::BTreeMap;

use egui::{Color32, RichText};
use youeye_doc::{Color, Fill, Frame, Node, Paint, Ruler, RulerOrientation, Stroke};

use crate::app::DocumentState;
use crate::canvas::Canvas;
use crate::menu::MenuAction;

#[derive(Default)]
pub struct UiState {
    pub selected_tool: Tool,
    /// Path of child indices from the document root to the selected node.
    /// `None` when nothing is selected.
    pub selection: Option<Vec<usize>>,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Rect,
    Ellipse,
    Line,
    Text,
    Pen,
    Frame,
    Hand,
}

impl UiState {
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        _actions: &mut Vec<MenuAction>,
        canvas: &mut Canvas,
        doc_state: Option<&mut DocumentState>,
    ) {
        let mut doc_state = doc_state;

        // Global keyboard shortcuts. Skip when an egui text input has focus
        // so typing "r" in a field doesn't switch tools.
        let input_focused = ctx.memory(|m| m.focused().is_some());
        if !input_focused {
            ctx.input_mut(|i| {
                for (key, tool) in [
                    (egui::Key::V, Tool::Select),
                    (egui::Key::R, Tool::Rect),
                    (egui::Key::O, Tool::Ellipse),
                    (egui::Key::F, Tool::Frame),
                    (egui::Key::L, Tool::Line),
                    (egui::Key::T, Tool::Text),
                    (egui::Key::P, Tool::Pen),
                    (egui::Key::H, Tool::Hand),
                ] {
                    if i.key_pressed(key) {
                        self.selected_tool = tool;
                    }
                }
                // Delete / Backspace removes the current selection.
                if i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace) {
                    if let (Some(path), Some(ds)) =
                        (self.selection.clone(), doc_state.as_deref_mut())
                    {
                        if ds.doc.remove_at(&path) {
                            ds.dirty = true;
                            self.selection = None;
                        }
                    }
                }
            });
        }

        egui::TopBottomPanel::top("toolbar")
            .exact_height(36.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    for (tool, label) in [
                        (Tool::Select, "Select (V)"),
                        (Tool::Frame, "Frame (F)"),
                        (Tool::Rect, "Rect (R)"),
                        (Tool::Ellipse, "Ellipse (O)"),
                        (Tool::Line, "Line (L)"),
                        (Tool::Pen, "Pen (P)"),
                        (Tool::Text, "Text (T)"),
                        (Tool::Hand, "Hand (H)"),
                    ] {
                        if ui
                            .selectable_label(self.selected_tool == tool, label)
                            .clicked()
                        {
                            self.selected_tool = tool;
                        }
                    }
                });
            });

        let mut pending_adds: Vec<Node> = Vec::new();

        egui::SidePanel::left("layers")
            .resizable(true)
            .default_width(240.0)
            .show(ctx, |ui| {
                ui.heading("Layers");
                ui.separator();

                let doc_open = doc_state.is_some();
                if doc_open {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("+Frame").clicked() {
                            pending_adds.push(Node::Frame(default_frame()));
                        }
                        if ui.button("+Rect").clicked() {
                            pending_adds.push(Node::Rect(default_rect()));
                        }
                        if ui.button("+Ellipse").clicked() {
                            pending_adds.push(Node::Ellipse(default_ellipse()));
                        }
                        if ui.button("+Group").clicked() {
                            pending_adds.push(Node::Group(default_group()));
                        }
                    });
                    ui.separator();
                }

                match doc_state.as_deref() {
                    None => {
                        ui.label(RichText::new("No screen open").color(Color32::GRAY));
                    }
                    Some(ds) if ds.doc.children.is_empty() => {
                        ui.label(RichText::new("(empty — add a shape above)").color(Color32::GRAY));
                    }
                    Some(ds) => {
                        let mut path = Vec::new();
                        for (i, node) in ds.doc.children.iter().enumerate() {
                            path.push(i);
                            draw_layer(ui, node, &mut path, &mut self.selection);
                            path.pop();
                        }
                    }
                }
            });

        // Apply pending adds from the layers panel before the inspector
        // captures doc_state. New nodes go into the currently-selected
        // container if it's a Frame or Group, otherwise at document root.
        if !pending_adds.is_empty()
            && let Some(ds) = doc_state.as_deref_mut()
        {
            let container = selected_container_path(&ds.doc, self.selection.as_deref());
            let new_count = pending_adds.len();
            let base_index = insert_into_container(&mut ds.doc, container.as_deref(), pending_adds);
            ds.dirty = true;
            // Select the first newly-added node so the inspector shows it.
            if let Some(base) = base_index {
                let mut new_path = container.unwrap_or_default();
                new_path.push(base);
                self.selection = Some(new_path);
            }
            let _ = new_count;
        }

        egui::SidePanel::right("inspector")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                self.draw_inspector(ui, doc_state.as_deref_mut());
            });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("tool: {:?}", self.selected_tool));
                ui.separator();
                ui.label("100%");
                ui.separator();
                ui.label("offline");
            });
        });

        let tool = self.selected_tool;
        let mut canvas_dirty = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let doc_for_canvas = doc_state.as_deref_mut().map(|s| &mut s.doc);
                if canvas.ui(ui, doc_for_canvas, &mut self.selection, tool) {
                    canvas_dirty = true;
                }
            });
        if canvas_dirty && let Some(ds) = doc_state.as_deref_mut() {
            ds.dirty = true;
        }
    }

    fn draw_inspector(&self, ui: &mut egui::Ui, doc_state: Option<&mut DocumentState>) {
        ui.heading("Inspector");
        ui.separator();

        let Some(ds) = doc_state else {
            ui.label(RichText::new("Nothing selected").color(Color32::GRAY));
            return;
        };

        // Grab rhythm (if any) up-front while borrow is still immutable — the
        // inspector uses it as the default step for gap/padding pickers.
        let rhythm_step = ds
            .doc
            .variables
            .get("rhythm")
            .and_then(|s| {
                let t = s.trim();
                let end = t
                    .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                    .unwrap_or(t.len());
                t[..end].parse::<f64>().ok()
            })
            .unwrap_or(1.0);

        let selection = self.selection.clone();
        let mut became_dirty = false;
        let Some(path) = selection else {
            ui.label(RichText::new("Nothing selected").color(Color32::GRAY));
            became_dirty |= draw_dict_editor(ui, "Tokens", "--token-", &mut ds.doc.tokens.0);
            became_dirty |= draw_dict_editor(ui, "Variables", "--var-", &mut ds.doc.variables.0);
            if became_dirty {
                ds.dirty = true;
            }
            return;
        };

        let token_names: Vec<String> = ds.doc.tokens.0.keys().cloned().collect();
        let rulers_in_scope = collect_rulers_in_scope(&ds.doc, &path);
        let v_ruler_ids: Vec<String> = rulers_in_scope
            .iter()
            .filter(|(_, o)| *o == RulerOrientation::Vertical)
            .map(|(id, _)| id.clone())
            .collect();
        let h_ruler_ids: Vec<String> = rulers_in_scope
            .iter()
            .filter(|(_, o)| *o == RulerOrientation::Horizontal)
            .map(|(id, _)| id.clone())
            .collect();
        match ds.doc.node_at_mut(&path) {
            Some(Node::Frame(frame)) => {
                ui.label(
                    RichText::new(format!(
                        "Frame {}×{}",
                        frame.width as i64, frame.height as i64
                    ))
                    .strong(),
                );
                ui.separator();
                became_dirty |= draw_frame_flex_controls(ui, frame, rhythm_step);
            }
            Some(node) if supports_paint(node) => {
                let kind = node_kind(node);
                let id = node.base().id.clone();
                let id_str = id.as_deref().unwrap_or("(no id)");
                ui.label(RichText::new(format!("{kind} · {id_str}")).strong());
                ui.separator();
                let base = node.base_mut();
                became_dirty |= draw_fill_row(ui, &mut base.fill, &token_names);
                became_dirty |= draw_stroke_row(ui, &mut base.stroke, &token_names);
                became_dirty |=
                    draw_pins_section(ui, &mut base.youeye_attrs, &v_ruler_ids, &h_ruler_ids);
            }
            Some(node) => {
                let id = node.base().id.as_deref().unwrap_or("(no id)");
                ui.label(format!("{} · {id}", node_kind(node)));
                ui.label(RichText::new("No editable properties yet.").color(Color32::GRAY));
            }
            None => {
                ui.label(RichText::new("Selection is stale.").color(Color32::GRAY));
            }
        }
        became_dirty |= draw_dict_editor(ui, "Tokens", "--token-", &mut ds.doc.tokens.0);
        became_dirty |= draw_dict_editor(ui, "Variables", "--var-", &mut ds.doc.variables.0);
        if became_dirty {
            ds.dirty = true;
        }
    }
}

/// Editable rows for a `BTreeMap<String, String>` with "Add" / "Delete" /
/// rename / value-edit. Used for both Tokens and Variables. Returns `true`
/// when the user made any change this frame.
fn draw_dict_editor(
    ui: &mut egui::Ui,
    heading: &str,
    prefix: &str,
    dict: &mut BTreeMap<String, String>,
) -> bool {
    let mut changed = false;
    ui.add_space(12.0);
    ui.collapsing(heading, |ui| {
        let originals: Vec<(String, String)> =
            dict.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        // Collect edits; apply after the iteration so we never mutate the
        // BTreeMap while drawing rows from it.
        let mut edits: Vec<DictEdit> = Vec::new();

        for (orig_name, orig_value) in &originals {
            let mut new_name = orig_name.clone();
            let mut new_value = orig_value.clone();
            let mut delete = false;
            ui.horizontal(|ui| {
                ui.label(prefix);
                ui.add(egui::TextEdit::singleline(&mut new_name).desired_width(100.0));
                ui.label(":");
                ui.add(egui::TextEdit::singleline(&mut new_value).desired_width(120.0));
                if ui.small_button("×").on_hover_text("Delete").clicked() {
                    delete = true;
                }
            });
            if delete {
                edits.push(DictEdit::Delete(orig_name.clone()));
            } else if new_name != *orig_name {
                edits.push(DictEdit::Rename {
                    from: orig_name.clone(),
                    to: new_name,
                    value: new_value,
                });
            } else if new_value != *orig_value {
                edits.push(DictEdit::UpdateValue {
                    name: orig_name.clone(),
                    value: new_value,
                });
            }
        }

        if ui.button("Add").clicked() {
            let base = match prefix {
                "--token-" => "new-token",
                "--var-" => "new-var",
                _ => "new",
            };
            let mut i = 1u32;
            let mut name = base.to_string();
            while dict.contains_key(&name) {
                i += 1;
                name = format!("{base}-{i}");
            }
            dict.insert(name, String::new());
            changed = true;
        }

        if !edits.is_empty() {
            changed = true;
        }
        for edit in edits {
            match edit {
                DictEdit::Delete(name) => {
                    dict.remove(&name);
                }
                DictEdit::UpdateValue { name, value } => {
                    dict.insert(name, value);
                }
                DictEdit::Rename { from, to, value } => {
                    dict.remove(&from);
                    if !to.is_empty() {
                        dict.insert(to, value);
                    }
                }
            }
        }
    });
    changed
}

enum DictEdit {
    Delete(String),
    UpdateValue {
        name: String,
        value: String,
    },
    Rename {
        from: String,
        to: String,
        value: String,
    },
}

/// Renders the flex controls for a Frame. Returns `true` if the user edited
/// any value this frame.
fn draw_frame_flex_controls(ui: &mut egui::Ui, frame: &mut Frame, rhythm_step: f64) -> bool {
    let mut changed = false;

    let is_flex = frame.base.youeye_attrs.get("layout").map(String::as_str) == Some("flex");
    let mut enabled = is_flex;
    if ui.checkbox(&mut enabled, "Auto layout (flex)").changed() {
        if enabled {
            frame
                .base
                .youeye_attrs
                .insert("layout".into(), "flex".into());
        } else {
            frame.base.youeye_attrs.remove("layout");
        }
        changed = true;
    }
    if !enabled {
        changed |= draw_rulers_section(ui, &mut frame.children);
        return changed;
    }

    changed |= combo(
        ui,
        "flex-direction",
        &mut frame.base.youeye_attrs,
        "flex-direction",
        "row",
        &[
            ("row", "Row"),
            ("row-reverse", "Row reverse"),
            ("column", "Column"),
            ("column-reverse", "Column reverse"),
        ],
    );
    changed |= combo(
        ui,
        "justify",
        &mut frame.base.youeye_attrs,
        "justify",
        "start",
        &[
            ("start", "Start"),
            ("center", "Center"),
            ("end", "End"),
            ("space-between", "Space between"),
            ("space-around", "Space around"),
            ("space-evenly", "Space evenly"),
        ],
    );
    changed |= combo(
        ui,
        "align",
        &mut frame.base.youeye_attrs,
        "align",
        "start",
        &[
            ("start", "Start"),
            ("center", "Center"),
            ("end", "End"),
            ("stretch", "Stretch"),
        ],
    );

    changed |= length_drag(ui, "gap", &mut frame.base.youeye_attrs, "gap", rhythm_step);
    changed |= length_drag(
        ui,
        "padding",
        &mut frame.base.youeye_attrs,
        "padding",
        rhythm_step,
    );

    changed |= draw_rulers_section(ui, &mut frame.children);
    changed
}

fn draw_rulers_section(ui: &mut egui::Ui, children: &mut Vec<Node>) -> bool {
    let mut changed = false;
    ui.add_space(12.0);
    ui.collapsing("Rulers", |ui| {
        let mut delete_indices: Vec<usize> = Vec::new();
        for (i, child) in children.iter_mut().enumerate() {
            if let Node::Ruler(r) = child {
                ui.horizontal(|ui| {
                    let label = match r.orientation {
                        RulerOrientation::Horizontal => "H",
                        RulerOrientation::Vertical => "V",
                    };
                    ui.label(label);
                    if ui
                        .add(egui::DragValue::new(&mut r.position).speed(1.0))
                        .changed()
                    {
                        changed = true;
                    }
                    if ui.small_button("×").on_hover_text("Delete").clicked() {
                        delete_indices.push(i);
                    }
                });
            }
        }
        for i in delete_indices.into_iter().rev() {
            children.remove(i);
            changed = true;
        }
        ui.horizontal(|ui| {
            if ui.button("Add horizontal ruler").clicked() {
                children.push(Node::Ruler(Ruler {
                    orientation: RulerOrientation::Horizontal,
                    ..Default::default()
                }));
                changed = true;
            }
            if ui.button("Add vertical ruler").clicked() {
                children.push(Node::Ruler(Ruler {
                    orientation: RulerOrientation::Vertical,
                    ..Default::default()
                }));
                changed = true;
            }
        });
    });
    changed
}

fn combo(
    ui: &mut egui::Ui,
    label: &str,
    attrs: &mut std::collections::BTreeMap<String, String>,
    key: &str,
    default: &str,
    options: &[(&str, &str)],
) -> bool {
    let mut changed = false;
    let current = attrs
        .get(key)
        .cloned()
        .unwrap_or_else(|| default.to_string());
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(label)
            .selected_text(
                options
                    .iter()
                    .find(|(v, _)| *v == current)
                    .map(|(_, t)| *t)
                    .unwrap_or("?"),
            )
            .show_ui(ui, |ui| {
                for (value, text) in options {
                    if ui.selectable_label(current == *value, *text).clicked() {
                        attrs.insert(key.into(), (*value).to_string());
                        changed = true;
                    }
                }
            });
    });
    changed
}

fn length_drag(
    ui: &mut egui::Ui,
    label: &str,
    attrs: &mut std::collections::BTreeMap<String, String>,
    key: &str,
    step: f64,
) -> bool {
    let mut current: f64 = attrs
        .get(key)
        .and_then(|v| {
            let t = v.trim();
            let end = t
                .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                .unwrap_or(t.len());
            t[..end].parse::<f64>().ok()
        })
        .unwrap_or(0.0);
    let before = current;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::DragValue::new(&mut current)
                .speed(step)
                .range(0.0..=f64::MAX),
        );
        off_rhythm_chip(ui, current, step);
    });
    if (current - before).abs() > f64::EPSILON {
        if current == 0.0 {
            attrs.remove(key);
        } else {
            attrs.insert(key.into(), format!("{current}"));
        }
        return true;
    }
    false
}

fn draw_layer(
    ui: &mut egui::Ui,
    node: &Node,
    path: &mut Vec<usize>,
    selection: &mut Option<Vec<usize>>,
) {
    let label = node_label(node);
    let is_selected = selection.as_deref() == Some(path.as_slice());

    let children = match node {
        Node::Group(g) => Some(&g.children),
        Node::Frame(f) => Some(&f.children),
        _ => None,
    };

    if let Some(children) = children {
        egui::CollapsingHeader::new(label.clone())
            .id_salt(path.as_slice())
            .default_open(true)
            .show(ui, |ui| {
                if ui.selectable_label(is_selected, "(this layer)").clicked() {
                    *selection = Some(path.clone());
                }
                for (i, child) in children.iter().enumerate() {
                    path.push(i);
                    draw_layer(ui, child, path, selection);
                    path.pop();
                }
            });
    } else if ui.selectable_label(is_selected, label).clicked() {
        *selection = Some(path.clone());
    }
}

fn node_kind(node: &Node) -> &'static str {
    match node {
        Node::Group(_) => "Group",
        Node::Frame(_) => "Frame",
        Node::Rect(_) => "Rect",
        Node::Ellipse(_) => "Ellipse",
        Node::Path(_) => "Path",
        Node::Text(_) => "Text",
        Node::Ruler(_) => "Ruler",
        Node::Component(_) => "Component",
        Node::Use(_) => "Use",
    }
}

fn node_label(node: &Node) -> String {
    let base = node.base();
    let kind = node_kind(node);
    match &base.id {
        Some(id) => format!("{kind} · {id}"),
        None => kind.to_string(),
    }
}

fn supports_paint(node: &Node) -> bool {
    matches!(node, Node::Rect(_) | Node::Ellipse(_) | Node::Path(_))
}

fn default_frame() -> Frame {
    Frame {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 240.0,
        ..Default::default()
    }
}

fn default_rect() -> youeye_doc::Rect {
    let fill = Fill {
        paint: Paint::Solid(Color {
            r: 0.33,
            g: 0.53,
            b: 0.98,
            a: 1.0,
        }),
        opacity: None,
    };
    youeye_doc::Rect {
        base: youeye_doc::NodeBase {
            fill: Some(fill),
            ..Default::default()
        },
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 80.0,
        rx: 0.0,
        ry: 0.0,
    }
}

fn default_ellipse() -> youeye_doc::Ellipse {
    let fill = Fill {
        paint: Paint::Solid(Color {
            r: 1.0,
            g: 0.55,
            b: 0.2,
            a: 1.0,
        }),
        opacity: None,
    };
    youeye_doc::Ellipse {
        base: youeye_doc::NodeBase {
            fill: Some(fill),
            ..Default::default()
        },
        cx: 50.0,
        cy: 50.0,
        rx: 50.0,
        ry: 50.0,
    }
}

fn default_group() -> youeye_doc::Group {
    youeye_doc::Group::default()
}

/// Walk to the currently-selected node: if it's a container (Frame or Group)
/// return its path so new nodes go inside. Otherwise fall back to the
/// nearest containing ancestor (or `None` for doc root).
fn selected_container_path(
    doc: &youeye_doc::Document,
    selection: Option<&[usize]>,
) -> Option<Vec<usize>> {
    let sel = selection?;
    let mut path = sel.to_vec();
    loop {
        match doc.node_at(&path) {
            Some(Node::Frame(_)) | Some(Node::Group(_)) => return Some(path),
            _ => {
                if path.pop().is_none() {
                    return None;
                }
            }
        }
    }
}

/// Insert `new_nodes` as the last children of the container addressed by
/// `path` (or of the document root when `path` is `None`). Returns the
/// index of the first newly inserted node, or `None` if insertion failed.
fn insert_into_container(
    doc: &mut youeye_doc::Document,
    path: Option<&[usize]>,
    new_nodes: Vec<Node>,
) -> Option<usize> {
    let container_children: &mut Vec<Node> = match path {
        None => &mut doc.children,
        Some(p) => match doc.node_at_mut(p)? {
            Node::Frame(f) => &mut f.children,
            Node::Group(g) => &mut g.children,
            _ => return None,
        },
    };
    let base = container_children.len();
    for n in new_nodes {
        container_children.push(n);
    }
    Some(base)
}

/// Walk from the document root to (but not into) the selected node, picking
/// up every ruler declared as a direct child of any ancestor. Inner
/// declarations shadow outer ones with the same id.
fn collect_rulers_in_scope(
    doc: &youeye_doc::Document,
    path: &[usize],
) -> Vec<(String, RulerOrientation)> {
    let mut out: BTreeMap<String, RulerOrientation> = BTreeMap::new();
    let mut collect = |children: &[Node]| {
        for c in children {
            if let Node::Ruler(r) = c
                && let Some(id) = &r.base.id
            {
                out.insert(id.clone(), r.orientation);
            }
        }
    };

    collect(&doc.children);
    let mut children = &doc.children;
    // Walk ancestors — everything up to but not including the selected node.
    let ancestor_count = path.len().saturating_sub(1);
    for idx in &path[..ancestor_count] {
        let Some(node) = children.get(*idx) else {
            break;
        };
        let next = match node {
            Node::Group(g) => Some(&g.children),
            Node::Frame(f) => Some(&f.children),
            _ => None,
        };
        if let Some(next) = next {
            children = next;
            collect(next);
        } else {
            break;
        }
    }
    out.into_iter().collect()
}

fn draw_pins_section(
    ui: &mut egui::Ui,
    attrs: &mut BTreeMap<String, String>,
    v_ruler_ids: &[String],
    h_ruler_ids: &[String],
) -> bool {
    let mut changed = false;
    if v_ruler_ids.is_empty() && h_ruler_ids.is_empty() {
        return changed;
    }
    ui.add_space(12.0);
    ui.collapsing("Pin to rulers", |ui| {
        changed |= draw_pin_row(ui, "Left", attrs, "pin-left", v_ruler_ids);
        changed |= draw_pin_row(ui, "Right", attrs, "pin-right", v_ruler_ids);
        changed |= draw_pin_row(ui, "Top", attrs, "pin-top", h_ruler_ids);
        changed |= draw_pin_row(ui, "Bottom", attrs, "pin-bottom", h_ruler_ids);
    });
    changed
}

fn draw_pin_row(
    ui: &mut egui::Ui,
    label: &str,
    attrs: &mut BTreeMap<String, String>,
    key: &str,
    ruler_ids: &[String],
) -> bool {
    let mut changed = false;
    let current = attrs.get(key).cloned().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(key)
            .selected_text(if current.is_empty() {
                "(none)".to_string()
            } else {
                current.clone()
            })
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_empty(), "(none)").clicked() {
                    attrs.remove(key);
                    changed = true;
                }
                if ruler_ids.is_empty() {
                    ui.label(RichText::new("no rulers").color(Color32::GRAY));
                }
                for id in ruler_ids {
                    if ui.selectable_label(current == *id, id).clicked() {
                        attrs.insert(key.into(), id.clone());
                        changed = true;
                    }
                }
            });
    });
    changed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaintKind {
    None,
    Color,
    Token,
    Raw,
}

fn draw_fill_row(ui: &mut egui::Ui, fill: &mut Option<Fill>, tokens: &[String]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Fill");
        let paint = fill_paint_mut(fill);
        changed |= paint_picker(ui, "fill", paint, tokens);
        off_token_chip(ui, paint, tokens);
    });
    changed
}

fn draw_stroke_row(ui: &mut egui::Ui, stroke: &mut Option<Stroke>, tokens: &[String]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Stroke");
        let paint = stroke_paint_mut(stroke);
        changed |= paint_picker(ui, "stroke", paint, tokens);
        off_token_chip(ui, paint, tokens);
    });
    // Width control — only meaningful when stroke is not None.
    if let Some(s) = stroke.as_mut()
        && !matches!(s.paint, Paint::None)
    {
        ui.horizontal(|ui| {
            ui.label("  width");
            let mut w = s.width.unwrap_or(1.0);
            if ui
                .add(
                    egui::DragValue::new(&mut w)
                        .speed(0.1)
                        .range(0.0..=f64::MAX),
                )
                .changed()
            {
                s.width = Some(w);
                changed = true;
            }
        });
    }
    changed
}

/// Returns a `&mut Paint` view of a `Fill`, creating a default `Fill` with
/// `Paint::None` if the option was `None`.
fn fill_paint_mut(fill: &mut Option<Fill>) -> &mut Paint {
    if fill.is_none() {
        *fill = Some(Fill::default());
    }
    &mut fill.as_mut().unwrap().paint
}

fn stroke_paint_mut(stroke: &mut Option<Stroke>) -> &mut Paint {
    if stroke.is_none() {
        *stroke = Some(Stroke::default());
    }
    &mut stroke.as_mut().unwrap().paint
}

fn paint_picker(ui: &mut egui::Ui, salt: &str, paint: &mut Paint, tokens: &[String]) -> bool {
    let mut changed = false;

    let current_kind = classify_paint(paint);
    let mut new_kind = current_kind;

    egui::ComboBox::from_id_salt(format!("{salt}-kind"))
        .selected_text(paint_kind_label(current_kind))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut new_kind, PaintKind::None, "none");
            ui.selectable_value(&mut new_kind, PaintKind::Color, "color");
            if !tokens.is_empty() {
                ui.selectable_value(&mut new_kind, PaintKind::Token, "token");
            }
            ui.selectable_value(&mut new_kind, PaintKind::Raw, "raw");
        });

    if new_kind != current_kind {
        *paint = default_paint_for_kind(new_kind, tokens);
        changed = true;
    }

    match paint {
        Paint::None => {}
        Paint::Solid(color) => {
            let mut rgba = [color.r, color.g, color.b, color.a];
            if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
                color.r = rgba[0];
                color.g = rgba[1];
                color.b = rgba[2];
                color.a = rgba[3];
                changed = true;
            }
        }
        Paint::Raw(s) if is_token_ref(s) => {
            let current = extract_token_name(s).unwrap_or_default();
            egui::ComboBox::from_id_salt(format!("{salt}-token"))
                .selected_text(if current.is_empty() {
                    "(pick)".to_string()
                } else {
                    current.clone()
                })
                .show_ui(ui, |ui| {
                    for t in tokens {
                        if ui.selectable_label(current == *t, t).clicked() {
                            *s = format!("var(--token-{t})");
                            changed = true;
                        }
                    }
                });
        }
        Paint::Raw(s) => {
            if ui
                .add(egui::TextEdit::singleline(s).desired_width(140.0))
                .changed()
            {
                changed = true;
            }
        }
    }
    changed
}

fn classify_paint(paint: &Paint) -> PaintKind {
    match paint {
        Paint::None => PaintKind::None,
        Paint::Solid(_) => PaintKind::Color,
        Paint::Raw(s) if is_token_ref(s) => PaintKind::Token,
        Paint::Raw(_) => PaintKind::Raw,
    }
}

fn paint_kind_label(k: PaintKind) -> &'static str {
    match k {
        PaintKind::None => "none",
        PaintKind::Color => "color",
        PaintKind::Token => "token",
        PaintKind::Raw => "raw",
    }
}

fn default_paint_for_kind(kind: PaintKind, tokens: &[String]) -> Paint {
    match kind {
        PaintKind::None => Paint::None,
        PaintKind::Color => Paint::Solid(Color::BLACK),
        PaintKind::Token => match tokens.first() {
            Some(t) => Paint::Raw(format!("var(--token-{t})")),
            None => Paint::None,
        },
        PaintKind::Raw => Paint::Raw(String::new()),
    }
}

fn is_token_ref(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("var(--token-") && t.ends_with(')')
}

/// Show a small amber "off-token" chip when the paint is a raw value and the
/// document declares at least one token. Non-enforcing — purely visual
/// guidance. Empty when there are no tokens yet (the design system hasn't
/// been set up) or the paint is none.
fn off_token_chip(ui: &mut egui::Ui, paint: &Paint, tokens: &[String]) {
    if tokens.is_empty() {
        return;
    }
    let off =
        matches!(paint, Paint::Solid(_)) || matches!(paint, Paint::Raw(s) if !is_token_ref(s));
    if off {
        ui.label(
            RichText::new("off-token")
                .color(Color32::from_rgb(0xdd, 0xa0, 0x30))
                .small(),
        )
        .on_hover_text("Raw value — consider binding this to a token.");
    }
}

/// Off-rhythm chip for gap/padding style lengths. Shown when the document
/// declares `--var-rhythm` and the current value isn't an integer multiple
/// of that rhythm.
fn off_rhythm_chip(ui: &mut egui::Ui, value: f64, rhythm_step: f64) {
    if rhythm_step <= 1.0 {
        return; // No meaningful rhythm to measure against.
    }
    let ratio = value / rhythm_step;
    let snapped = ratio.round() * rhythm_step;
    if (value - snapped).abs() > 0.001 {
        ui.label(
            RichText::new("off-rhythm")
                .color(Color32::from_rgb(0xdd, 0xa0, 0x30))
                .small(),
        )
        .on_hover_text(format!(
            "Not a multiple of --var-rhythm ({rhythm_step}). Nearest: {snapped}."
        ));
    }
}

fn extract_token_name(s: &str) -> Option<String> {
    let t = s.trim();
    let inner = t.strip_prefix("var(--token-")?.strip_suffix(')')?;
    Some(inner.to_string())
}
