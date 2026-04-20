//! egui layout: sidebars, status bar, central canvas.

use std::collections::BTreeMap;

use egui::{Color32, RichText};
use youeye_doc::{Frame, Node};

use crate::app::DocumentState;
use crate::canvas::Canvas;
use crate::menu::MenuAction;

#[derive(Default)]
pub struct UiState {
    selected_tool: Tool,
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

        egui::SidePanel::left("layers")
            .resizable(true)
            .default_width(240.0)
            .show(ctx, |ui| {
                ui.heading("Layers");
                ui.separator();
                match doc_state.as_deref() {
                    None => {
                        ui.label(RichText::new("No screen open").color(Color32::GRAY));
                    }
                    Some(ds) if ds.doc.children.is_empty() => {
                        ui.label(RichText::new("(empty document)").color(Color32::GRAY));
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

        egui::SidePanel::right("inspector")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                self.draw_inspector(ui, doc_state);
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

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                canvas.ui(ui);
            });
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
            Some(node) => {
                let id = node.base().id.as_deref().unwrap_or("(no id)");
                ui.label(format!("{} · {id}", node_kind(node)).to_string());
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
