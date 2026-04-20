//! egui layout: sidebars, status bar, central canvas.

use egui::{Color32, RichText};
use youeye_doc::{Document, Node};

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
        doc: Option<&Document>,
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
                match doc {
                    None => {
                        ui.label(RichText::new("No screen open").color(Color32::GRAY));
                    }
                    Some(doc) if doc.children.is_empty() => {
                        ui.label(RichText::new("(empty document)").color(Color32::GRAY));
                    }
                    Some(doc) => {
                        let mut path = Vec::new();
                        for (i, node) in doc.children.iter().enumerate() {
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
                ui.heading("Inspector");
                ui.separator();
                ui.label(RichText::new("Nothing selected").color(Color32::GRAY));
                ui.add_space(12.0);
                ui.collapsing("Tokens", |ui| match doc {
                    Some(d) if !d.tokens.is_empty() => {
                        for (name, value) in &d.tokens.0 {
                            ui.label(format!("--token-{name}: {value}"));
                        }
                    }
                    _ => {
                        ui.label(RichText::new("— none —").color(Color32::GRAY));
                    }
                });
                ui.collapsing("Variables", |ui| match doc {
                    Some(d) if !d.variables.is_empty() => {
                        for (name, value) in &d.variables.0 {
                            ui.label(format!("--var-{name}: {value}"));
                        }
                    }
                    _ => {
                        ui.label(RichText::new("— none —").color(Color32::GRAY));
                    }
                });
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

fn node_label(node: &Node) -> String {
    let base = node.base();
    let kind = match node {
        Node::Group(_) => "Group",
        Node::Frame(_) => "Frame",
        Node::Rect(_) => "Rect",
        Node::Ellipse(_) => "Ellipse",
        Node::Path(_) => "Path",
        Node::Text(_) => "Text",
    };
    match &base.id {
        Some(id) => format!("{kind} · {id}"),
        None => kind.to_string(),
    }
}
