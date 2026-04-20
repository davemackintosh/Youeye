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
    /// Paths from the document root to each selected node. Empty = nothing
    /// selected. The first entry is the "primary" selection — the one the
    /// inspector treats as the active target. Operations that don't need a
    /// single target (delete, drag-move) act on all entries.
    pub selection: Vec<Vec<usize>>,
    /// System font family names, populated on first Text inspector draw.
    /// Lazy because enumerating fonts hits the OS font DB.
    font_families: Option<Vec<String>>,
    /// When set, the next frame asks egui to focus the Text content field.
    /// Used by the canvas to hand over focus on a double-click.
    pending_text_focus: bool,
    /// If set, the layer at this path is being renamed inline — its
    /// `selectable_label` swaps for a `TextEdit` until commit or cancel.
    renaming: Option<Vec<usize>>,
    /// Scratch buffer for the in-flight rename. Populated when `renaming`
    /// transitions from `None` to `Some`.
    rename_buffer: String,
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
                // Delete / Backspace removes all selected nodes.
                if (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
                    && !self.selection.is_empty()
                    && let Some(ds) = doc_state.as_deref_mut()
                {
                    let paths = std::mem::take(&mut self.selection);
                    if delete_paths(&mut ds.doc, &paths) {
                        ds.dirty = true;
                    }
                }

                // Group selected siblings — ⌘/Ctrl + G.
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::G)
                    && self.selection.len() >= 2
                    && let Some(ds) = doc_state.as_deref_mut()
                    && let Some(new_path) = group_selection(&mut ds.doc, &self.selection)
                {
                    ds.dirty = true;
                    self.selection = vec![new_path];
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
        let mut layer_actions: Vec<LayerAction> = Vec::new();

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
                        if ui.button("+Text").clicked() {
                            pending_adds.push(Node::Text(default_text()));
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
                        let renaming = self.renaming.as_deref();
                        for (i, node) in ds.doc.children.iter().enumerate() {
                            path.push(i);
                            draw_layer(
                                ui,
                                node,
                                &mut path,
                                &mut self.selection,
                                renaming,
                                &mut self.rename_buffer,
                                &mut layer_actions,
                            );
                            path.pop();
                        }
                    }
                }
            });

        // Apply any layer actions (rename / delete) the panel emitted.
        if !layer_actions.is_empty()
            && let Some(ds) = doc_state.as_deref_mut()
        {
            for action in layer_actions {
                match action {
                    LayerAction::StartRename { path, current_id } => {
                        self.renaming = Some(path);
                        self.rename_buffer = current_id;
                    }
                    LayerAction::CommitRename { path, new_id } => {
                        if let Some(node) = ds.doc.node_at_mut(&path) {
                            let trimmed = new_id.trim();
                            node.base_mut().id = if trimmed.is_empty() {
                                None
                            } else {
                                Some(trimmed.to_string())
                            };
                            ds.dirty = true;
                        }
                        self.renaming = None;
                        self.rename_buffer.clear();
                    }
                    LayerAction::CancelRename => {
                        self.renaming = None;
                        self.rename_buffer.clear();
                    }
                    LayerAction::Delete(path) => {
                        if ds.doc.remove_at(&path) {
                            ds.dirty = true;
                            self.selection.retain(|s| !s.starts_with(&path));
                            if self.renaming.as_ref() == Some(&path) {
                                self.renaming = None;
                                self.rename_buffer.clear();
                            }
                        }
                    }
                }
            }
        }

        // Apply pending adds from the layers panel before the inspector
        // captures doc_state. New nodes go into the currently-selected
        // container if it's a Frame or Group, otherwise at document root.
        if !pending_adds.is_empty()
            && let Some(ds) = doc_state.as_deref_mut()
        {
            let primary = self.selection.first().map(|p| p.as_slice());
            let container = selected_container_path(&ds.doc, primary);
            let new_count = pending_adds.len();
            let base_index = insert_into_container(&mut ds.doc, container.as_deref(), pending_adds);
            ds.dirty = true;
            // Select the first newly-added node so the inspector shows it.
            if let Some(base) = base_index {
                let mut new_path = container.unwrap_or_default();
                new_path.push(base);
                self.selection = vec![new_path];
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
        let mut focus_text_content = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let doc_for_canvas = doc_state.as_deref_mut().map(|s| &mut s.doc);
                if canvas.ui(
                    ui,
                    doc_for_canvas,
                    &mut self.selection,
                    tool,
                    &mut focus_text_content,
                ) {
                    canvas_dirty = true;
                }
            });
        if focus_text_content {
            self.pending_text_focus = true;
            ctx.request_repaint();
        }
        if canvas_dirty && let Some(ds) = doc_state.as_deref_mut() {
            ds.dirty = true;
        }
    }

    fn draw_inspector(&mut self, ui: &mut egui::Ui, doc_state: Option<&mut DocumentState>) {
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
        let path: Vec<usize> = match selection.len() {
            0 => {
                ui.label(RichText::new("Nothing selected").color(Color32::GRAY));
                became_dirty |= draw_dict_editor(ui, "Tokens", "--token-", &mut ds.doc.tokens.0);
                became_dirty |=
                    draw_dict_editor(ui, "Variables", "--var-", &mut ds.doc.variables.0);
                if became_dirty {
                    ds.dirty = true;
                }
                return;
            }
            1 => selection.into_iter().next().unwrap(),
            n => {
                ui.label(RichText::new(format!("{n} items selected")).strong());
                ui.label(
                    RichText::new(
                        "Multi-select: drag to move, Delete to remove, ⌘/Ctrl+G to group.",
                    )
                    .color(Color32::GRAY),
                );
                became_dirty |= draw_dict_editor(ui, "Tokens", "--token-", &mut ds.doc.tokens.0);
                became_dirty |=
                    draw_dict_editor(ui, "Variables", "--var-", &mut ds.doc.variables.0);
                if became_dirty {
                    ds.dirty = true;
                }
                return;
            }
        };

        let token_names: Vec<String> = ds.doc.tokens.0.keys().cloned().collect();
        let rulers_in_scope = collect_rulers_in_scope(&ds.doc, &path);
        // Populate the font list lazily — first time we need it.
        if self.font_families.is_none() {
            self.font_families = Some(youeye_render::text::list_font_families());
        }
        let font_families: Vec<String> = self.font_families.clone().unwrap_or_default();
        let focus_text_content = std::mem::take(&mut self.pending_text_focus);
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
            Some(Node::Text(text)) => {
                let id = text.base.id.clone();
                let id_str = id.as_deref().unwrap_or("(no id)");
                ui.label(RichText::new(format!("Text · {id_str}")).strong());
                ui.separator();
                became_dirty |=
                    draw_text_controls(ui, text, &token_names, &font_families, focus_text_content);
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
                draw_value_hint(ui, &new_value);
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

#[derive(Debug, Clone)]
enum LayerAction {
    StartRename {
        path: Vec<usize>,
        current_id: String,
    },
    CommitRename {
        path: Vec<usize>,
        new_id: String,
    },
    CancelRename,
    Delete(Vec<usize>),
}

fn draw_layer(
    ui: &mut egui::Ui,
    node: &Node,
    path: &mut Vec<usize>,
    selection: &mut Vec<Vec<usize>>,
    renaming: Option<&[usize]>,
    rename_buffer: &mut String,
    actions: &mut Vec<LayerAction>,
) {
    let label = node_label(node);
    let is_selected = selection.iter().any(|s| s.as_slice() == path.as_slice());
    let is_renaming = renaming == Some(path.as_slice());
    let shift_held = ui.input(|i| i.modifiers.shift);
    let current_id = node.base().id.clone().unwrap_or_default();

    let toggle = |selection: &mut Vec<Vec<usize>>, path: Vec<usize>| {
        if shift_held {
            if let Some(idx) = selection.iter().position(|s| *s == path) {
                selection.remove(idx);
            } else {
                selection.push(path);
            }
        } else {
            *selection = vec![path];
        }
    };

    let children = match node {
        Node::Group(g) => Some(&g.children),
        Node::Frame(f) => Some(&f.children),
        _ => None,
    };

    let draw_row = |ui: &mut egui::Ui,
                    label_text: &str,
                    is_selected: bool,
                    path: &[usize],
                    selection: &mut Vec<Vec<usize>>,
                    rename_buffer: &mut String,
                    actions: &mut Vec<LayerAction>| {
        if is_renaming {
            let response = ui.add(
                egui::TextEdit::singleline(rename_buffer)
                    .id(egui::Id::new(("layer-rename", path.to_vec())))
                    .desired_width(160.0),
            );
            if !response.has_focus() {
                response.request_focus();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                actions.push(LayerAction::CommitRename {
                    path: path.to_vec(),
                    new_id: rename_buffer.clone(),
                });
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.lost_focus() {
                actions.push(LayerAction::CancelRename);
            }
        } else {
            let response = ui.selectable_label(is_selected, label_text);
            if response.double_clicked() {
                actions.push(LayerAction::StartRename {
                    path: path.to_vec(),
                    current_id: current_id.clone(),
                });
            } else if response.clicked() {
                toggle(selection, path.to_vec());
            }
            response.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    actions.push(LayerAction::StartRename {
                        path: path.to_vec(),
                        current_id: current_id.clone(),
                    });
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    actions.push(LayerAction::Delete(path.to_vec()));
                    ui.close();
                }
            });
        }
    };

    if let Some(children) = children {
        egui::CollapsingHeader::new(label.clone())
            .id_salt(path.as_slice())
            .default_open(true)
            .show(ui, |ui| {
                draw_row(
                    ui,
                    "(this layer)",
                    is_selected,
                    path,
                    selection,
                    rename_buffer,
                    actions,
                );
                for (i, child) in children.iter().enumerate() {
                    path.push(i);
                    draw_layer(ui, child, path, selection, renaming, rename_buffer, actions);
                    path.pop();
                }
            });
    } else {
        draw_row(
            ui,
            &label,
            is_selected,
            path,
            selection,
            rename_buffer,
            actions,
        );
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

/// Next to a token / variable row, render a small visual hint about what
/// kind of value it is: a colour swatch for hex / rgb values, a unit tag
/// for lengths (`12px`, `2em`), an expression tag for `calc(...)` /
/// `var(...)`, nothing for anything else.
fn draw_value_hint(ui: &mut egui::Ui, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Some(swatch) = parse_hint_color(trimmed) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, swatch);
        ui.painter().rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, Color32::BLACK),
            egui::StrokeKind::Inside,
        );
        return;
    }
    if let Some(unit) = parse_length_unit(trimmed) {
        ui.label(RichText::new(unit).small().color(Color32::GRAY));
        return;
    }
    if trimmed.starts_with("var(") {
        ui.label(RichText::new("var").small().color(Color32::GRAY));
        return;
    }
    if trimmed.starts_with("calc(") {
        ui.label(RichText::new("calc").small().color(Color32::GRAY));
    }
}

fn parse_hint_color(s: &str) -> Option<Color32> {
    fn hex_pair(h: &str) -> Option<u8> {
        u8::from_str_radix(h, 16).ok()
    }
    if let Some(hex) = s.strip_prefix('#') {
        let (r, g, b, a) = match hex.len() {
            3 => (
                hex_pair(&hex[0..1].repeat(2))?,
                hex_pair(&hex[1..2].repeat(2))?,
                hex_pair(&hex[2..3].repeat(2))?,
                255u8,
            ),
            6 => (
                hex_pair(&hex[0..2])?,
                hex_pair(&hex[2..4])?,
                hex_pair(&hex[4..6])?,
                255u8,
            ),
            8 => (
                hex_pair(&hex[0..2])?,
                hex_pair(&hex[2..4])?,
                hex_pair(&hex[4..6])?,
                hex_pair(&hex[6..8])?,
            ),
            _ => return None,
        };
        return Some(Color32::from_rgba_unmultiplied(r, g, b, a));
    }
    if let Some(rest) = s.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<&str> = rest.split(',').map(str::trim).collect();
        if parts.len() == 3
            && let (Ok(r), Ok(g), Ok(b)) = (
                parts[0].parse::<u8>(),
                parts[1].parse::<u8>(),
                parts[2].parse::<u8>(),
            )
        {
            return Some(Color32::from_rgb(r, g, b));
        }
    }
    if let Some(rest) = s.strip_prefix("rgba(").and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<&str> = rest.split(',').map(str::trim).collect();
        if parts.len() == 4
            && let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                parts[0].parse::<u8>(),
                parts[1].parse::<u8>(),
                parts[2].parse::<u8>(),
                parts[3].parse::<f32>(),
            )
        {
            let a8 = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
            return Some(Color32::from_rgba_unmultiplied(r, g, b, a8));
        }
    }
    None
}

fn parse_length_unit(s: &str) -> Option<&'static str> {
    // First char must be numeric / sign / dot.
    let first = s.chars().next()?;
    if !(first.is_ascii_digit() || first == '-' || first == '+' || first == '.') {
        return None;
    }
    // Find where the number ends.
    let end = s
        .find(|c: char| {
            !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')
        })
        .unwrap_or(s.len());
    let number = &s[..end];
    if number.parse::<f64>().is_err() {
        return None;
    }
    let unit = s[end..].trim();
    match unit {
        "" => Some("num"),
        "px" => Some("px"),
        "em" => Some("em"),
        "rem" => Some("rem"),
        "%" => Some("%"),
        "pt" => Some("pt"),
        "vh" => Some("vh"),
        "vw" => Some("vw"),
        "s" => Some("s"),
        "ms" => Some("ms"),
        _ => None,
    }
}

/// Delete every selected node. Sort by path length descending then
/// lexicographically so earlier removals don't invalidate later paths.
/// Returns true if any node was removed.
fn delete_paths(doc: &mut youeye_doc::Document, paths: &[Vec<usize>]) -> bool {
    // Skip paths whose ancestor is also in the set — the ancestor's removal
    // takes the descendant with it.
    let mut targets: Vec<Vec<usize>> = paths
        .iter()
        .filter(|p| {
            !paths
                .iter()
                .any(|other| other.len() < p.len() && p.starts_with(other))
        })
        .cloned()
        .collect();
    // Sort so siblings with a higher index get removed first (stable indices).
    targets.sort_by(|a, b| b.cmp(a));
    let mut any = false;
    for path in targets {
        if doc.remove_at(&path) {
            any = true;
        }
    }
    any
}

/// Group selected sibling nodes into a new `Group`. All selected paths
/// must share a parent container. On success, returns the path to the
/// new Group and writes it into the doc. Non-sibling selections are a
/// no-op and return `None`.
fn group_selection(doc: &mut youeye_doc::Document, selection: &[Vec<usize>]) -> Option<Vec<usize>> {
    if selection.len() < 2 {
        return None;
    }
    let (last0, parent_path) = selection[0].split_last()?;
    let parent_path = parent_path.to_vec();
    let mut indices: Vec<usize> = Vec::with_capacity(selection.len());
    indices.push(*last0);
    for p in &selection[1..] {
        let (last, this_parent) = p.split_last()?;
        if this_parent != parent_path.as_slice() {
            return None;
        }
        indices.push(*last);
    }
    indices.sort();
    indices.dedup();

    let parent_children = doc.container_children_mut(&parent_path)?;
    let mut taken: Vec<Node> = Vec::with_capacity(indices.len());
    for &i in indices.iter().rev() {
        if i >= parent_children.len() {
            return None;
        }
        taken.push(parent_children.remove(i));
    }
    taken.reverse();
    let insert_at = indices[0];
    let new_group = Node::Group(youeye_doc::Group {
        base: youeye_doc::NodeBase::default(),
        children: taken,
    });
    parent_children.insert(insert_at, new_group);

    let mut new_path = parent_path;
    new_path.push(insert_at);
    Some(new_path)
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

fn draw_text_controls(
    ui: &mut egui::Ui,
    text: &mut youeye_doc::Text,
    token_names: &[String],
    font_families: &[String],
    focus_content: bool,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label("Content");
    });
    let content_id = egui::Id::new("inspector-text-content");
    let content_response = ui.add(
        egui::TextEdit::multiline(&mut text.content)
            .id(content_id)
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
    if content_response.changed() {
        changed = true;
    }
    if focus_content {
        content_response.request_focus();
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("x");
        changed |= ui
            .add(egui::DragValue::new(&mut text.x).speed(1.0))
            .changed();
        ui.label("y");
        changed |= ui
            .add(egui::DragValue::new(&mut text.y).speed(1.0))
            .changed();
    });

    ui.horizontal(|ui| {
        ui.label("Size");
        let mut size = text.font_size.unwrap_or(16.0);
        if ui
            .add(
                egui::DragValue::new(&mut size)
                    .speed(0.5)
                    .range(1.0..=512.0),
            )
            .changed()
        {
            text.font_size = Some(size);
            changed = true;
        }
    });

    ui.horizontal(|ui| {
        ui.label("Family");
        let current = text.font_family.clone().unwrap_or_default();
        let label = if current.is_empty() {
            "(system default)".to_string()
        } else {
            current.clone()
        };
        egui::ComboBox::from_id_salt("text-font-family")
            .selected_text(label)
            .width(160.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current.is_empty(), "(system default)")
                    .clicked()
                {
                    text.font_family = None;
                    changed = true;
                }
                for family in font_families {
                    if ui.selectable_label(current == *family, family).clicked() {
                        text.font_family = Some(family.clone());
                        changed = true;
                    }
                }
            });
    });
    // Free-text fallback — handy for fonts the system hasn't scanned or
    // when the user wants a specific CSS-style stack string.
    ui.horizontal(|ui| {
        ui.label("  custom");
        let mut family = text.font_family.clone().unwrap_or_default();
        let response = ui.add(egui::TextEdit::singleline(&mut family).desired_width(140.0));
        if response.changed() {
            text.font_family = if family.trim().is_empty() {
                None
            } else {
                Some(family)
            };
            changed = true;
        }
    });

    ui.add_space(6.0);
    changed |= draw_fill_row(ui, &mut text.base.fill, token_names);

    changed
}

fn default_text() -> youeye_doc::Text {
    let fill = Fill {
        paint: Paint::Solid(Color {
            r: 0.92,
            g: 0.92,
            b: 0.95,
            a: 1.0,
        }),
        opacity: None,
    };
    youeye_doc::Text {
        base: youeye_doc::NodeBase {
            fill: Some(fill),
            ..Default::default()
        },
        x: 40.0,
        y: 40.0,
        content: "Text".into(),
        font_family: None,
        font_size: Some(24.0),
    }
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
