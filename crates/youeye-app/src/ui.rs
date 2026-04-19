//! egui layout: sidebars, status bar, central canvas placeholder.

use egui::{Color32, RichText};

use crate::menu::MenuAction;

#[derive(Default)]
pub struct UiState {
    selected_tool: Tool,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
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
    pub fn draw(&mut self, ctx: &egui::Context, _actions: &mut Vec<MenuAction>) {
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
                ui.label(RichText::new("No screen open").color(Color32::GRAY));
            });

        egui::SidePanel::right("inspector")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                ui.heading("Inspector");
                ui.separator();
                ui.label(RichText::new("Nothing selected").color(Color32::GRAY));
                ui.add_space(12.0);
                ui.collapsing("Tokens", |ui| {
                    ui.label(RichText::new("— not wired yet —").color(Color32::GRAY));
                });
                ui.collapsing("Variables", |ui| {
                    ui.label(RichText::new("— not wired yet —").color(Color32::GRAY));
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

        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            ui.painter().rect_filled(rect, 0.0, Color32::from_gray(24));
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("canvas — vello integration next")
                        .color(Color32::from_gray(140))
                        .size(14.0),
                );
            });
        });
    }
}
